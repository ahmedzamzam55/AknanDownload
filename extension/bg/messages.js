// ---------------------------------------------------------------- messages from page/popup

function reply(promise, sendResponse) {
  promise
    .then(() => sendResponse({ ok: true }))
    .catch((e) => sendResponse({ ok: false, error: String((e && e.message) || e) }));
  return true;
}

function onRuntimeMessage(msg, sender, sendResponse) {
  switch (msg.type) {
    case "swiftget-download": {
      const item = msg.item || {};
      // a video inside a frame (embedded player): that frame's page
      const referrer = sender.frameId && msg.referrer ? msg.referrer : sender.tab ? sender.tab.url : msg.referrer;
      const tabPage = sender.tab && /^https?:/i.test(sender.tab.url || "") && !isManifestUrl(sender.tab.url) ? sender.tab.url : "";
      // a part only from the "✂ Part" tab (the whole video never carries one)
      const clip = msg.part ? partOf(msg.clip) : null;
      if (msg.part && !clip) return reply(Promise.reject(new Error("the part is not right")), sendResponse);
      return reply(
        storeOf(sender.tab).then(async (store) => {
          let sub = msg.subtitle;
          if (sub && sub.url && /^https?:/i.test(sub.url)) {
            const [body, cookies] = await Promise.all([fetchSubBody(sub.url), cookieHeader(sub.url, store)]);
            sub = { ...sub, ...(body ? { body } : {}), ...(cookies ? { cookies } : {}) };
          }
          return item.yt
            ? downloadVideo(item.yt, msg.title, referrer, clip, store, item.reqHeaders, sub, tabPage)
            : sendToApp({ url: item.url, referrer, reqHeaders: item.reqHeaders, fileName: msg.fileName, sourcePage: tabPage || null, clip, store, direct: true, subtitle: sub });
        }),
        sendResponse
      );
    }
    case "swiftget-download-subs": {
      const tracks = Array.isArray(msg.tracks) ? msg.tracks.filter((t) => t && /^https?:/i.test(t.url || "")) : [];
      if (!tracks.length) return reply(Promise.reject(new Error("no subtitles selected")), sendResponse);
      const fmt = /^(srt|vtt|txt)$/i.test(msg.format || "") ? msg.format.toLowerCase() : "srt";
      const base = cleanYoutubeTitle(msg.title || "video");
      const ref = sender.tab ? sender.tab.url : msg.referrer || "";
      const group = Date.now();
      return reply(
        storeOf(sender.tab).then(async (store) => {
          for (let i = 0; i < tracks.length; i += 1) {
            if (i > 0) await new Promise((r) => setTimeout(r, 250));
            const t = tracks[i];
            const lang = String(t.lang || "sub").replace(/[^a-zA-Z0-9._-]/g, "_") || "sub";
            const subBody = await fetchSubBody(t.url);
            await sendToApp({
              url: t.url,
              referrer: ref,
              sourcePage: ref,
              fileName: `${base}.${lang}.${fmt}`,
              subConvert: fmt,
              subBody: subBody || undefined,
              group,
              direct: true,
              store,
            });
          }
        }),
        sendResponse
      );
    }
    case "swiftget-get-media":
      itemsForTab(msg.tabId).then(sendResponse);
      return true;
    case "swiftget-media-request":
      if (!sender.tab) return false;
      if (sender.tab.url && videoIdOf(sender.tab.url)) resolveYoutube(sender.tab.id, sender.tab.url);
      itemsForTab(sender.tab.id).then(sendResponse);
      return true;
    case "swiftget-video-seen":
      // the page shows a video: find its qualities
      if (!sender.tab) return false;
      getOpts().then((o) => {
        if (!o.enabled) return;
        if (isYoutube(sender.tab.url)) resolveYoutube(sender.tab.id, sender.tab.url);
        // a Facebook feed: its address is no video (the video's own post
        // link is looked up when its button is clicked)
        else if (!msg.feed) resolvePage(sender.tab.id, msg.page || sender.tab.url, !!msg.frame);
      });
      return false;
    case "swiftget-resolve-video":
      // one of several videos on the page: its own file, or its own link
      resolveOneVideo(msg, sender).then(sendResponse);
      return true;
    case "swiftget-link-rules":
      // what a click on a file link in the page should do (decided in the
      // page at once, so the browser never starts its own download)
      Promise.all([getOpts(), getConfig()]).then(([o, cfg]) => {
        if (!o.enabled || !o.intercept || !cfg) return sendResponse(null);
        sendResponse({
          exts: cfg.captureExts || [],
          skipSites: cfg.skipSites || [],
          skipAddresses: cfg.skipAddresses || [],
          skipKey: cfg.skipKey || "none",
          forceKey: cfg.forceKey || "none",
        });
      });
      return true;
    case "swiftget-link":
      // a file link clicked in a page: straight to the app
      return reply(
        storeOf(sender.tab).then((store) => takeLink(msg.url, sender.tab ? sender.tab.url : "", store)),
        sendResponse
      );
    case "swiftget-magnet":
      // a torrent's magnet link clicked on a page
      return reply(
        getOpts().then(async (o) => {
          if (!o.enabled || !o.intercept) throw new Error("sending is off");
          // Options > Torrents: Aknan Download is not the main torrent program
          const cfg = await getConfig();
          if (cfg && cfg.torrents === false) throw new Error("torrents are not taken");
          return sendToApp({ url: msg.url, referrer: sender.tab ? sender.tab.url : "" });
        }),
        sendResponse
      );
    case "swiftget-playlist":
      // "Download all" on a playlist page: the app's playlist window
      return reply(sendPlaylist(msg.url), sendResponse);
    case "swiftget-status":
      (async () => {
        const ok = await appAlive();
        if (!ok) return sendResponse({ ok: false, state: "down" });
        // the program runs: is the add-in connected to it (its key)?
        const { port } = await getOpts();
        let linked = false;
        try {
          linked = (await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 2500)).ok;
        } catch {
          linked = false;
        }
        sendResponse({ ok: linked, state: linked ? "ok" : "nokey" });
      })();
      return true;
    case "swiftget-reconnect":
      reconnect().then((ok) => sendResponse({ ok }));
      return true;
    case "swiftget-keys":
      lastKeys = { alt: !!msg.alt, ctrl: !!msg.ctrl, shift: !!msg.shift, at: Date.now() };
      return false;
    case "swiftget-config":
      Promise.all([getConfig(), getOpts()]).then(([cfg, o]) => {
        const panel = (cfg && cfg.panel) || { enabled: true, skipSites: [] };
        sendResponse(o.enabled ? panel : { ...panel, enabled: false });
      });
      return true;
    default:
      return false;
  }
}

var __aknanBgModulesLoaded = true;
