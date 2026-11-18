// End-to-end simulation of YouTube instant captions, network/HLS subtitle sniffing, and 429 fallback.
// node extension/test/subtitles.sim.js
const assert = require("assert");
const { createSimEnv } = require("./sim-env.js");

(async () => {
  const sent = [];
  let ytResolveDelay = 200;
  let resolveCount = 0;

  const env = createSimEnv({
    version: "1.0.12",
    userAgent: "TestUA",
    onFetch: async (url, opts = {}) => {
      if (url.endsWith("/add")) {
        sent.push(JSON.parse(opts.body));
        return { ok: true, status: 200 };
      }
      if (url.includes("youtube.com/youtubei/v1/player")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            captions: {
              playerCaptionsTracklistRenderer: {
                captionTracks: [
                  { baseUrl: "https://www.youtube.com/api/timedtext?v=SUBTESTVID1&lang=ar&fmt=srv3", languageCode: "ar", name: { simpleText: "Arabic" }, kind: "asr", isTranslatable: true },
                ],
              },
            },
          }),
        };
      }
      if (url.includes("youtube.com/api/timedtext") && url.includes("tlang=en")) {
        return { ok: false, status: 429, text: async () => "<html>Too Many Requests</html>" };
      }
      if (url.includes("youtube.com/api/timedtext")) {
        return {
          ok: true,
          status: 200,
          text: async () => JSON.stringify({ events: [{ tStartMs: 1000, dDurationMs: 2500, segs: [{ utf8: "مرحبا بكم" }] }] }),
        };
      }
      if (url.includes("translate.googleapis.com/translate_a/single")) {
        return { ok: true, status: 200, json: async () => [[["Welcome everyone\n", "مرحبا بكم\n"]]] };
      }
      if (url.endsWith("/resolve")) {
        resolveCount += 1;
        await new Promise((r) => setTimeout(r, ytResolveDelay));
        return {
          ok: true,
          status: 200,
          json: async () => ({
            ok: true,
            title: "Sub Test Video",
            options: [{ key: "1080p|MP4", label: "1080p · MP4", sub: "50 MB", quality: "1080p", videoUrl: "https://gv/137", audioUrl: "https://gv/140", ext: "mp4", size: 50e6, headers: [] }],
            subtitles: [{ lang: "ar", name: "Arabic", url: "https://www.youtube.com/api/timedtext?v=SUBTESTVID1&lang=ar", ext: "vtt", auto: true }],
          }),
        };
      }
      if (url === "https://hls.example/master.m3u8") {
        return {
          ok: true,
          status: 200,
          text: async () => '#EXTM3U\n#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="French",LANGUAGE="fr",URI="subs/fr.vtt"\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\n720.m3u8\n',
        };
      }
      if (url === "https://sub.example/ar.vtt") {
        return { ok: true, status: 200, text: async () => "WEBVTT\n\n00:00:01.000 --> 00:00:03.000\nترجمة تجريبية\n" };
      }
      return { ok: true, status: 200, text: async () => "" };
    },
  });

  const { listeners, tabMsgs, state } = env;
  const tick = (ms = 40) => new Promise((r) => setTimeout(r, ms));

  // 1. YouTube instant subtitles appear BEFORE /resolve finishes
  state.tabUrl = "https://www.youtube.com/watch?v=SUBTESTVID1";
  listeners.before({ tabId: 7, url: "https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&itag=137&mime=video%2Fmp4&clen=50000000&dur=120.0&pot=T&range=0-1000&rn=1" });
  listeners.before({ tabId: 7, url: "https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&itag=140&mime=audio%2Fmp4&clen=2000000&dur=120.0&pot=T&range=0-1000&rn=1" });
  await listeners.upd(7, { url: state.tabUrl, status: "complete" }, { id: 7, url: state.tabUrl });
  await tick(60);
  const earlyPush = tabMsgs[tabMsgs.length - 1];
  assert(earlyPush && earlyPush.items[0] && Array.isArray(earlyPush.items[0].subs), "YouTube subs available immediately");
  assert(earlyPush.items[0].subs.some((s) => s.lang === "ar") && earlyPush.items[0].subs.some((s) => s.lang === "en"), "includes ar & translated en");
  await tick(220);
  assert.strictEqual(resolveCount, 1);

  // 2. Generic .vtt sniffer attaches real captions and ignores storyboard/thumbnail .vtt
  state.tabUrl = "https://site.example/watch/1";
  await listeners.upd(7, { url: state.tabUrl }, { id: 7, url: state.tabUrl });
  await tick(20);
  listeners.headers({ tabId: 7, statusCode: 200, url: "https://cdn.example/movie.mp4", responseHeaders: [{ name: "Content-Type", value: "video/mp4" }, { name: "Content-Length", value: "5000000" }] });
  listeners.headers({ tabId: 7, statusCode: 200, url: "https://cdn.example/thumbs/storyboard.vtt", responseHeaders: [{ name: "Content-Type", value: "text/vtt" }] });
  listeners.headers({ tabId: 7, statusCode: 200, url: "https://sub.example/ar.vtt", responseHeaders: [{ name: "Content-Type", value: "text/vtt" }] });
  await tick(40);
  const genPush = tabMsgs[tabMsgs.length - 1].items;
  assert(genPush[0].subs && genPush[0].subs.length === 1, "expected 1 real subtitle track");
  assert.strictEqual(genPush[0].subs[0].url, "https://sub.example/ar.vtt");

  // 3. HLS #EXT-X-MEDIA:TYPE=SUBTITLES populates items[0].subs
  state.tabUrl = "https://hls.example/page";
  await listeners.upd(7, { url: state.tabUrl }, { id: 7, url: state.tabUrl });
  await tick(20);
  listeners.headers({ tabId: 7, statusCode: 200, url: "https://hls.example/master.m3u8", responseHeaders: [{ name: "Content-Type", value: "application/vnd.apple.mpegurl" }] });
  await tick(60);
  const hlsPush = tabMsgs[tabMsgs.length - 1].items;
  assert(hlsPush[0].subs && hlsPush[0].subs.some((s) => s.lang === "fr" && s.url === "https://hls.example/subs/fr.vtt"), "HLS subtitle parsed");

  // 4. swiftget-download with subtitle pre-fetches body + cookies
  sent.length = 0;
  const r1 = await new Promise((res) => listeners.msg({
    type: "swiftget-download",
    item: { url: "https://cdn.example/movie.mp4", reqHeaders: [] },
    title: "Test Movie",
    subtitle: { lang: "ar", url: "https://sub.example/ar.vtt", burn: true },
  }, { tab: { id: 9, incognito: true, url: "https://site.example/watch/1" } }, res));
  assert.strictEqual(r1.ok, true);
  assert(sent[0].subtitle && sent[0].subtitle.body.includes("WEBVTT"), "subtitle body pre-fetched");
  assert.strictEqual(sent[0].subtitle.cookies, "private=yes");

  // 5. swiftget-download-subs batch + tlang=en 429 translation fallback
  sent.length = 0;
  const tracks = earlyPush.items[0].subs.filter((s) => s.lang === "ar" || s.lang === "en");
  const r2 = await new Promise((res) => listeners.msg({
    type: "swiftget-download-subs",
    title: "Sub Test Video",
    format: "srt",
    tracks,
  }, { tab: { id: 7, url: "https://www.youtube.com/watch?v=SUBTESTVID1" } }, res));
  assert.strictEqual(r2.ok, true);
  assert.strictEqual(sent.length, 2);
  assert(sent[0].subBody.includes("مرحبا بكم"), "Arabic json3 body forwarded");
  assert(sent[1].subBody.includes("Welcome everyone"), "HTTP 429 translated via GTX fallback");
  assert.strictEqual(sent[1].subConvert, "srt");
  assert.strictEqual(sent[0].group, sent[1].group);

  console.log("subtitles.sim: all checks passed");
})();
