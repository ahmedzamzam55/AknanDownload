// Shared mock background environment for background.sim.js and background-streams.sim.js.
const assert = require("assert");
const { createSimEnv } = require("./sim-env.js");

function createBgEnv() {
  const sent = [];
  const ytCalls = [];
  const suggested = [];
  const net = { pingExt: null, appDown: false };
  const env = createSimEnv({
    version: "1.0.4",
    userAgent: "TestUA",
    onFetch: async (url, opts = {}) => {
      if (url.endsWith("/ping") && net.pingExt) return { ok: true, status: 200, json: async () => ({ ext: net.pingExt }) };
      if (url.endsWith("/add")) {
        if (net.appDown) throw new TypeError("Failed to fetch");
        sent.push(JSON.parse(opts.body));
      }
      if (url.endsWith("/resolve") && JSON.parse(opts.body).url.includes("unsupported.example")) {
        return { ok: true, status: 200, json: async () => ({ ok: false, error: "Unsupported URL: https://unsupported.example/watch" }) };
      }
      if (url.endsWith("/resolve")) {
        ytCalls.push(JSON.parse(opts.body));
        await new Promise((r) => setTimeout(r, 30)); // yt-dlp takes a moment
        return {
          ok: true,
          status: 200,
          json: async () => ({
            ok: true,
            title: "Real: Title",
            options: [
              { key: "1080p|MP4", label: "1080p · MP4", sub: "94 MB", quality: "1080p", videoUrl: "https://gv/137", audioUrl: "https://gv/140", ext: "mp4", size: 94e6, headers: [["User-Agent", "YtDlpUA"]] },
              { key: "audio|M4A", label: "Audio only · M4A 130k", sub: "4 MB", quality: "audio", videoUrl: "https://gv/140", audioUrl: null, ext: "m4a", size: 4e6, headers: [] },
            ],
            subtitles: [
              { lang: "ar", name: "Arabic", url: "https://sub.example/ar.vtt", ext: "vtt", auto: false },
              { lang: "en", name: "English", url: "https://sub.example/en.json3", ext: "json3", auto: true },
            ],
          }),
        };
      }
      if (url.includes("/pl/master")) {
        return {
          ok: true,
          status: 200,
          text: async () => "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n360/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720\nhttps://cdn2.example/720/index.m3u8\n",
        };
      }
      if (url.includes("/pl/") && url.endsWith("index.m3u8")) {
        return { ok: true, status: 200, text: async () => "#EXTM3U\n#EXT-X-TARGETDURATION:6\n#EXTINF:6,\na.ts\n" };
      }
      return { ok: true, status: 200 };
    },
  });
  const tick = () => new Promise((r) => setTimeout(r, 20));
  const now = new Date().toISOString();
  const dl = async (o) => {
    const item = { id: Math.random(), state: "in_progress", startTime: now, ...o };
    await env.listeners.dl(item);
    const waits = env.listeners.det(item, () => suggested.push(item.id));
    assert.strictEqual(waits, true, "the browser waits for our answer");
    for (let i = 0; i < 20; i++) await tick();
    const kept = suggested.includes(item.id);
    const taken = env.cancelled.includes(item.id);
    assert(kept !== taken, "answered exactly once: left to the browser or taken by the app");
  };
  return { ...env, sent, ytCalls, suggested, net, tick, dl };
}

module.exports = { createBgEnv };
