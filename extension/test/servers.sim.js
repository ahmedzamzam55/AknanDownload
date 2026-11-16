// Embedded multi-server switching and protected HLS header preservation tests.
// node extension/test/servers.sim.js
const assert = require("assert");
const { createBgEnv } = require("./bg-env.js");

(async () => {
  const s = createBgEnv();
  const { all, listeners, sent, ytCalls, tabMsgs, tick } = s;
  const tabId = 91;
  const outerUrl = "https://laroza1.website/play.php?vid=abc";
  const m3u8Hdr = [{ name: "Content-Type", value: "application/vnd.apple.mpegurl" }];

  // Server 1 iframe requests a protected master.m3u8 (extension fetch gets 403 in mock because URL != /pl/master, falls back to /resolve)
  const srv1Stream = "https://okprimeone.site/hls2/01/master.m3u8?t=s1";
  all.before.find((f) => f.name === "onSubFrameBeforeNavigate")({ tabId, frameId: 5, url: "https://okprimeone.site/embed-1.html" });
  await tick();
  listeners.sendHeaders({
    tabId,
    frameId: 5,
    requestId: "req1",
    url: srv1Stream,
    requestHeaders: [
      { name: "Referer", value: "https://okprimeone.site/embed-1.html" },
      { name: "Origin", value: "https://okprimeone.site" },
      { name: "User-Agent", value: "PlayerUA" },
    ],
  });
  listeners.headers({
    tabId,
    frameId: 5,
    requestId: "req1",
    url: srv1Stream,
    statusCode: 200,
    responseHeaders: m3u8Hdr,
    documentUrl: "https://okprimeone.site/embed-1.html",
    initiator: "https://okprimeone.site",
  });
  for (let i = 0; i < 6; i++) await tick();

  // Verify /resolve received real iframe Referer + Origin headers instead of laroza1.website
  const call1 = ytCalls[ytCalls.length - 1];
  assert.strictEqual(call1.url, srv1Stream);
  assert.strictEqual(call1.referer, "https://okprimeone.site/embed-1.html");
  assert(call1.headers.some(([k, v]) => k === "Origin" && v === "https://okprimeone.site"));

  // Verify click inside frame 5 downloads with real embed Referer + Origin even when msg carries outerUrl
  const items1 = await new Promise((r) => listeners.msg({ type: "swiftget-resolve-video", frame: true }, { tab: { id: tabId, url: outerUrl }, frameId: 5, url: "https://okprimeone.site/embed-1.html" }, r));
  assert(items1.items && items1.items.length, "resolved qualities returned");
  const dlRes = await new Promise((r) =>
    listeners.msg({ type: "swiftget-download", item: items1.items[0], title: "Ep 01", referrer: outerUrl }, { tab: { id: tabId, url: outerUrl }, frameId: 5 }, r)
  );
  assert.strictEqual(dlRes.ok, true);
  const sent1 = sent[sent.length - 1];
  assert.strictEqual(sent1.referrer, "https://okprimeone.site/embed-1.html", "iframe Referer must not be overwritten by wrapper page");
  assert(sent1.headers.some(([k, v]) => k === "Origin" && v === "https://okprimeone.site"), "Origin must be forwarded");

  // Switching to Server 2 on same frame clears Server 1's stream and picks up Server 2
  const srv2Mp4 = "https://uqload.cx/v/video.mp4";
  await all.before.find((f) => f.name === "onSubFrameBeforeNavigate")({ tabId, frameId: 5, url: "https://uqload.cx/embed-2.html" });
  await tick();
  listeners.sendHeaders({
    tabId,
    frameId: 5,
    requestId: "req2",
    url: srv2Mp4,
    requestHeaders: [{ name: "Referer", value: "https://uqload.cx/embed-2.html" }],
  });
  listeners.headers({
    tabId,
    frameId: 5,
    requestId: "req2",
    url: srv2Mp4,
    statusCode: 200,
    responseHeaders: [{ name: "Content-Type", value: "video/mp4" }, { name: "Content-Length", value: "5000000" }],
  });
  for (let i = 0; i < 4; i++) await tick();
  const afterSwitch = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(afterSwitch.length, 1);
  assert.strictEqual(afterSwitch[0].url, srv2Mp4);
  assert.strictEqual(afterSwitch[0].reqHeaders[0][1], "https://uqload.cx/embed-2.html");

  console.log("servers.sim: multi-server Origin/Referer & switching checks passed");
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
