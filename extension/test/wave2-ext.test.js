// Unit + integration tests for Wave 2 extension upgrades:
// YouTube Kids, DASH subtitles, tab-media fallback, quickOptions, and variant-preserving subs.
const assert = require("assert");
const { createSimEnv } = require("./sim-env.js");

(async () => {
  let resolveCount = 0;
  const env = createSimEnv({
    version: "1.0.12",
    onFetch: async (url, opts = {}) => {
      if (url.includes("youtube.com/youtubei/v1/player") || url.includes("youtubekids.com/youtubei/v1/player")) {
        return {
          ok: true,
          status: 200,
          json: async () => ({
            videoDetails: { title: "Kids Rhyme", lengthSeconds: "65" },
            streamingData: {
              formats: [{ itag: 18, url: "https://gv/kids18", mimeType: 'video/mp4; codecs="avc1, mp4a"', height: 360, fps: 30, audioChannels: 2, bitrate: 500000, contentLength: "4000000" }],
            },
            captions: {
              playerCaptionsTracklistRenderer: {
                captionTracks: [{ baseUrl: "https://www.youtube.com/api/timedtext?v=KID12345678&lang=ar", languageCode: "ar", name: { simpleText: "Arabic" } }],
              },
            },
          }),
        };
      }
      if (url.endsWith("/resolve")) {
        resolveCount += 1;
        await new Promise((r) => setTimeout(r, 80));
        return {
          ok: true,
          status: 200,
          json: async () => ({
            ok: true,
            title: "Kids Rhyme",
            site: "Youtube",
            options: [{ key: "720p|MP4", label: "720p · MP4", sub: "12 MB", quality: "720p", videoUrl: "https://gv/kids720", audioUrl: "https://gv/kids140", ext: "mp4", size: 12e6, headers: [] }],
            subtitles: [{ lang: "ar", name: "Arabic (auto)", url: "https://sub.example/ar-auto.vtt", ext: "vtt", auto: true }],
          }),
        };
      }
      if (url === "https://dash.example/stream.mpd") {
        return {
          ok: true,
          status: 200,
          text: async () => `<MPD><Period><AdaptationSet contentType="text" lang="ar" label="Arabic"><BaseURL>subs/ar.vtt</BaseURL></AdaptationSet></Period></MPD>`,
        };
      }
      return { ok: true, status: 200, text: async () => "" };
    },
  });

  const { ctx, listeners, tabMsgs, state } = env;
  const tick = (ms = 35) => new Promise((r) => setTimeout(r, ms));

  // 1. parseDashSubs & sniffDashSubs
  const parsedDash = ctx.parseDashSubs('<MPD><Period><AdaptationSet mimeType="text/vtt" lang="es"><BaseURL>es.vtt</BaseURL></AdaptationSet></Period></MPD>', "https://cdn.example/v/manifest.mpd");
  assert.strictEqual(parsedDash.length, 1);
  assert.strictEqual(parsedDash[0].url, "https://cdn.example/v/es.vtt");

  listeners.headers({ tabId: 33, statusCode: 304, url: "https://dash.example/stream.mpd", responseHeaders: [{ name: "Content-Type", value: "application/dash+xml" }] });
  await tick(50);
  const dashItems = await new Promise((r) => listeners.msg({ type: "swiftget-get-media", tabId: 33 }, {}, r));
  assert(dashItems[0].subs && dashItems[0].subs.some((s) => s.lang === "ar" && s.url === "https://dash.example/subs/ar.vtt"), "DASH text AdaptationSet attached on HTTP 304");

  // 2. YouTube Kids: instant quickOptions + variant-preserving subs + /resolve
  assert.strictEqual(ctx.isYoutube("https://www.youtubekids.com/watch?v=KID12345678"), true);
  state.tabUrl = "https://www.youtubekids.com/watch?v=KID12345678";
  await listeners.upd(8, { url: state.tabUrl, status: "complete" }, { id: 8, url: state.tabUrl });
  await tick(35);
  const earlyKids = await new Promise((r) => listeners.msg({ type: "swiftget-get-media", tabId: 8 }, {}, r));
  assert(earlyKids.some((i) => i.yt && i.yt.quality === "360p"), "instant streamingData quickOptions shown before /resolve finishes");
  await tick(100);
  assert.strictEqual(resolveCount, 1);
  const finalKids = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(finalKids[0].yt.quality, "720p");
  assert(finalKids[0].subs.filter((s) => s.lang === "ar").length >= 2, "manual ar and auto ar variants both preserved");

  console.log("wave2-ext.test: all checks passed");
})().catch((e) => { console.error(e); process.exit(1); });
