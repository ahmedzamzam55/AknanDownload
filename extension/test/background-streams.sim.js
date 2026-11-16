// Forms, private cookies, self-update, framed players, switch, DASH & HLS checks.
const assert = require("assert");

async function runStreamChecks(s) {
  const { ctx, all, listeners, session, localStore, sent, ytCalls, tabMsgs, state, net, tick, dl } = s;

  // a form's download: sent with what the form sent; one with a file in it stays
  {
    const url = "https://forms.example/export";
    for (const f of all.before) f({ method: "POST", url, requestId: "r1", type: "main_frame", requestBody: { formData: { id: ["7"], name: ["a b"] } } });
    for (const f of all.beforeSend) f({ method: "POST", url, requestId: "r1", requestHeaders: [{ name: "Content-Type", value: "application/x-www-form-urlencoded" }] });
    const n = sent.length;
    await dl({ url, finalUrl: "https://files.example/report.csv", filename: "C:\\x\\report.csv" });
    const req = sent[n];
    assert(req, "the form's download went to the app");
    assert.strictEqual(req.url, url, "from the form's own address");
    assert.strictEqual(Buffer.from(req.post, "base64").toString(), "id=7&name=a+b");
    assert.strictEqual(req.postType, "application/x-www-form-urlencoded");
    const up = "https://forms.example/upload";
    for (const f of all.before) f({ method: "POST", url: up, requestId: "r2", type: "main_frame", requestBody: { formData: { f: ["x"] } } });
    for (const f of all.beforeSend) f({ method: "POST", url: up, requestId: "r2", requestHeaders: [{ name: "Content-Type", value: "multipart/form-data; boundary=x" }] });
    await dl({ url: up, filename: "C:\\x\\out.zip" });
    assert.strictEqual(sent.length, n + 1, "a form with a file in it stays in the browser");
    assert.strictEqual(localStore.lastLeft.why, "form");
    console.log("forms: the app posts the same data; an upload form stays in the browser");
  }

  // a private window: its own cookies go along
  {
    const n = sent.length;
    await dl({ url: "https://example.com/private.zip", incognito: true });
    assert.strictEqual(sent[n].cookies, "private=yes", "the private window's cookies");
    console.log("private window: its own cookies");
  }

  // add-in reloads itself once when the program brings a newer one
  net.pingExt = "1.0.5"; delete session.versionCheckAt;
  await ctx.followAppVersion(); await tick();
  assert.strictEqual(state.reloads, 1, "reloads for the new version");
  delete session.versionCheckAt;
  await ctx.followAppVersion(); await tick();
  assert.strictEqual(state.reloads, 1, "only once per version");
  net.pingExt = "1.0.4"; delete session.versionCheckAt;
  await ctx.followAppVersion();
  assert.strictEqual(state.reloads, 1, "same version: nothing");
  net.pingExt = null;
  console.log("self update: reloads once when the program has a newer add-in");

  // a player inside a frame (Dailymotion's "geo.dailymotion.com/player/…"):
  // the page around it is looked up, its qualities are offered
  {
    const page = "https://www.dailymotion.com/video/xac78kk";
    const n = ytCalls.length;
    const res = await new Promise((resolve) =>
      listeners.msg({ type: "swiftget-resolve-video", page: "", src: "", frame: true }, { tab: { id: 21, url: page }, frameId: 4 }, resolve));
    assert(res.items && res.items.length >= 2, "qualities of the page: " + JSON.stringify(res));
    assert.strictEqual(ytCalls[n].url, page, "the page, not the player frame");
    assert(!ytCalls.slice(n).some((c) => /geo\.dailymotion\.com/.test(c.url)));
    console.log("player in a frame: uses the page's qualities");
  }

  // the on / off switch of the add-in's window
  {
    localStore.enabled = false;
    listeners.store({ enabled: { newValue: false } }, "local");
    const before = sent.length;
    await dl({ url: "https://example.com/while-off.zip" });
    assert.strictEqual(sent.length, before, "off: the browser keeps its downloads");
    const cfg = await new Promise((r) => listeners.msg({ type: "swiftget-config" }, { tab: { id: 7, url: state.tabUrl } }, r));
    assert.strictEqual(cfg.enabled, false, "off: no buttons on videos");
    const n = tabMsgs.length;
    await ctx.pushToTab(7);
    assert.strictEqual(tabMsgs.slice(n).pop().items.length, 0, "off: nothing offered on the page");
    localStore.enabled = true;
    await dl({ url: "https://example.com/while-on.zip" });
    assert.strictEqual(sent.length, before + 1, "on again: back to the app");
    console.log("on / off switch: off leaves everything to the browser");
  }

  // a DASH stream (.mpd): offered, its qualities come from the app
  {
    listeners.headers({ tabId: 31, url: "https://cdn.example/v/manifest.mpd", statusCode: 200, responseHeaders: [{ name: "Content-Type", value: "application/dash+xml" }] });
    await tick(); await tick();
    const items = await new Promise((r) => listeners.msg({ type: "swiftget-get-media", tabId: 31 }, {}, r));
    const dash = items.find((i) => i.resolve);
    assert(dash && dash.resolve.endsWith(".mpd"), "DASH stream offered: " + JSON.stringify(items));
    const n = ytCalls.length;
    const res = await new Promise((r) => listeners.msg({ type: "swiftget-resolve-video", resolve: dash.resolve }, { tab: { id: 31, url: "https://site.example/watch/1" } }, r));
    assert(res.items && res.items.length, "its qualities");
    assert.strictEqual(ytCalls[n].url, dash.resolve);
    assert.strictEqual(ytCalls[n].referer, "https://site.example/watch/1", "the page goes along as referrer");
    console.log("DASH stream: offered, qualities from the app with the page as referrer");
  }

  // an HLS stream with several qualities: each one by its name (from the
  // playlist), the lists the player opened itself are not repeated
  {
    const hdr = [{ name: "Content-Type", value: "application/vnd.apple.mpegurl" }];
    listeners.headers({ tabId: 41, url: "https://cdn.example/pl/master", statusCode: 200, responseHeaders: hdr });
    await tick(); await tick(); await tick();
    listeners.headers({ tabId: 41, url: "https://cdn.example/pl/1080/index.m3u8", statusCode: 200, responseHeaders: hdr });
    for (let i = 0; i < 6; i++) await tick();
    const items = await new Promise((r) => listeners.msg({ type: "swiftget-get-media", tabId: 41 }, {}, r));
    const labels = items.map((i) => i.label);
    assert.strictEqual(JSON.stringify(labels), JSON.stringify(["1080p · MP4 · stream", "720p · MP4 · stream", "360p · MP4 · stream"]));
    assert.strictEqual(items[0].url, "https://cdn.example/pl/1080/index.m3u8");
    assert.strictEqual(items[1].url, "https://cdn2.example/720/index.m3u8");
    // the button of that player: the page is nothing the app knows -> its streams
    const res = await new Promise((r) =>
      listeners.msg({ type: "swiftget-resolve-video", page: "", src: "", frame: false }, { tab: { id: 41, url: "https://unsupported.example/watch" } }, r));
    assert(res.items && res.items.length === 3, "the streams instead of an error: " + JSON.stringify(res));
    console.log("HLS qualities:", labels.join(" | "));
  }
}

if (require.main === module) {
  const { createBgEnv } = require("./bg-env.js");
  runStreamChecks(createBgEnv())
    .then(() => console.log("background-streams: all checks passed"))
    .catch((e) => { console.error(e); process.exit(1); });
}

module.exports = { runStreamChecks };
