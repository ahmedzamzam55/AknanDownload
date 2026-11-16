// Runs background.js with a fake `chrome` API and a fake SwiftGet app.
// node extension/test/background.sim.js
const assert = require("assert");
const { createBgEnv } = require("./bg-env.js");
const { runStreamChecks } = require("./background-streams.sim.js");

(async () => {
  const s = createBgEnv();
  const { listeners, tabMsgs, sent, ytCalls, net, state, cancelled, browserDl, localStore, tick, dl } = s;
  const gv = (itag, mime, clen, dur, r) => `https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&itag=${itag}&mime=${encodeURIComponent(mime)}&clen=${clen}&dur=${dur}&pot=T&range=${r}&rn=1`;

  // player loads an ad, then the real video in many pieces
  listeners.before({ tabId: 7, url: gv(136, "video/mp4", 5e6, "15.0", "0-1000") });
  await tick();
  for (const r of ["0-1000", "1001-2000", "2001-3000"]) {
    listeners.before({ tabId: 7, url: gv(137, "video/mp4", 150e6, "300.5", r) }); await tick();
    listeners.before({ tabId: 7, url: gv(140, "audio/mp4", 4e6, "300.5", r) }); await tick();
  }
  const last = tabMsgs[tabMsgs.length - 1];
  assert.strictEqual(last.type, "swiftget-media");
  assert.strictEqual(last.items[0].label, "1080p Full HD · MP4");
  assert.strictEqual(last.items[0].yt.tag, "Full HD");
  console.log("button shows:", last.items.map((i) => i.label).join(" | "));

  // user clicks 1080p on the page
  const res = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: last.items[0], title: "(2) Cool video - YouTube" }, { tab: { id: 7, url: state.tabUrl } }, resolve));
  assert.strictEqual(res.ok, true);
  const req = sent[0];
  assert(req.url.includes("itag=137") && !req.url.includes("range="));
  assert(req.audioUrl.includes("itag=140"));
  assert.strictEqual(req.fileName, "Cool video [1080p].mp4");
  assert.strictEqual(req.userAgent, "TestUA");
  console.log("sent to app:", req.fileName, "| audio:", !!req.audioUrl);

  // navigating to another video clears the list
  state.tabUrl = "https://www.youtube.com/watch?v=BBBBBBBBBBB";
  await listeners.upd(7, { url: state.tabUrl }, { id: 7, url: state.tabUrl }); await tick();
  const afterNav = tabMsgs[tabMsgs.length - 1].items;
  assert(!afterNav.some((i) => i.label === "1080p · MP4" && i.yt && i.yt.videoUrl.includes("itag=137")), "old video's items gone");

  // ---- SABR video: no usable stream URL, the app (yt-dlp) gives the qualities
  listeners.before({ tabId: 7, url: "https://rr1---sn-x.googlevideo.com/videoplayback?expire=9&sabr=1&ump=1" });
  await listeners.upd(7, { url: state.tabUrl, status: "complete" }, { id: 7, url: state.tabUrl });
  await new Promise((r) => setTimeout(r, 150));
  assert.strictEqual(ytCalls.length, 1, "asked the app once");
  assert.strictEqual(ytCalls[0].url, "https://www.youtube.com/watch?v=BBBBBBBBBBB");
  assert(Array.isArray(ytCalls[0].cookieList), "sends the site's cookies as a list");
  const labels = tabMsgs.map((m) => (m.items[0] || {}).label);
  assert(labels.includes("Getting video qualities…"), "shows progress while resolving");
  const final = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(final[0].label, "1080p · MP4");
  const res2 = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: final[0], title: "ignored - YouTube" }, { tab: { id: 7, url: state.tabUrl } }, resolve));
  assert.strictEqual(res2.ok, true);
  const req2 = sent[sent.length - 1];
  assert.strictEqual(req2.fileName, "Real_ Title [1080p].mp4");
  assert.strictEqual(req2.sourcePage, "https://www.youtube.com/watch?v=BBBBBBBBBBB");
  assert.strictEqual(req2.sourceKey, "1080p|MP4");
  assert.strictEqual(req2.audioUrl, "https://gv/140");
  assert.strictEqual(req2.headers[0][1], "YtDlpUA");
  console.log("SABR video via app:", req2.fileName, "| key:", req2.sourceKey);

  // ---- any other site: a reel in an Instagram feed
  state.tabUrl = "https://www.instagram.com/";
  await listeners.upd(8, { url: state.tabUrl, status: "loading" }, { id: 8, url: state.tabUrl });
  tabMsgs.length = 0;
  listeners.msg({ type: "swiftget-video-seen", page: "https://www.instagram.com/reel/C1abcDEF/" }, { tab: { id: 8, url: state.tabUrl } }, () => {});
  await new Promise((r) => setTimeout(r, 150));
  const igCall = ytCalls[ytCalls.length - 1];
  assert.strictEqual(igCall.url, "https://www.instagram.com/reel/C1abcDEF/");
  const igItems = tabMsgs[tabMsgs.length - 1].items;
  assert.strictEqual(igItems[0].label, "1080p · MP4");
  const res3 = await new Promise((resolve) =>
    listeners.msg({ type: "swiftget-download", item: igItems[0], title: "x" }, { tab: { id: 8, url: state.tabUrl } }, resolve));
  assert.strictEqual(res3.ok, true);
  const req3 = sent[sent.length - 1];
  assert.strictEqual(req3.sourcePage, "https://www.instagram.com/reel/C1abcDEF/");
  assert.strictEqual(req3.referrer, "https://www.instagram.com/reel/C1abcDEF/");
  console.log("Instagram reel via app:", req3.fileName);

  // browser downloads: only new ones, each once, no loop when the app fails
  const before = sent.length;
  await dl({ url: "https://example.com/old.pdf", startTime: new Date(Date.now() - 3600e3).toISOString() });
  await dl({ url: "https://example.com/done.pdf", state: "complete" });
  await dl({ url: "https://example.com/ext.pdf", byExtensionId: "other" });
  assert.strictEqual(sent.length, before, "restored / finished / extension downloads are left alone");
  await dl({ url: "https://example.com/new.pdf" });
  await dl({ url: "https://example.com/new.pdf" });
  assert.strictEqual(sent.length, before + 1, "a new download goes to the app once");
  net.appDown = true;
  {
    const n = cancelled.length;
    await dl({ url: "https://example.com/fail.pdf" });
    assert.strictEqual(cancelled.length, n, "the app did not take it: the browser's own download goes on");
    assert.deepStrictEqual(browserDl, [], "never asked for a second time");
    assert.strictEqual(localStore.lastLeft.why, "not-running", "why it stayed is kept for the toolbar window");
  }
  net.appDown = false;
  console.log("browser downloads: restored ones ignored, no repeats, no loop");

  await runStreamChecks(s);
  console.log("background simulation: all checks passed");
})().catch((e) => { console.error(e); process.exit(1); });
