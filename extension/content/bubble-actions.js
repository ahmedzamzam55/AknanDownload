// Bubble interaction actions: resolve per-video qualities, download, playlist, app launch retry.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  // which video a player shows now: its own link, else its own file (a
  // blob: source says nothing: the site makes a new one for the same video)
  CS.videoKey = (video) => CS.ownLink(video) || CS.directSrc(video) || "";

  // what was found (or is being looked up) is for another video now
  CS.stale = (b) => {
    const key = CS.videoKey(b.video);
    if (b.own) return b.ownFor !== key;
    if (b.loading) return b.asked !== key;
    return false;
  };

  // what was found belongs to another video of this player now
  CS.forget = (b) => {
    if (!b.own && !b.loading) return;
    b.own = null;
    b.ownFor = "";
    b.loading = false;
    b.open = false;
    b.tab = "whole";
    b.clipMode = false;
    CS.renderOne(b);
  };

  CS.itemsOf = (b) => {
    if (b.own && b.ownFor !== CS.videoKey(b.video)) {
      b.own = null;
      b.ownFor = "";
    }
    if (b.own) return b.own;
    // the page's own video only: a player that shows another video (a
    // preview on a home page) is looked up for that video
    if (!CS.isMain(b.video)) return [];
    const mine = CS.ownLink(b.video);
    return !mine || CS.isVideoPage(location.href) || mine === CS.cleanLink(location.href) ? CS.items : [];
  };

  CS.lookUp = (b, stream, reqHeaders) => {
    // another video on the page (or nothing found for it yet): look it up now
    b.loading = true;
    b.open = true;
    const asked = CS.videoKey(b.video);
    b.asked = asked;
    CS.renderOne(b, aknanI18n.t("Looking for the video qualities…"));
    chrome.runtime.sendMessage(
      // its own post / video link, or its own file; the page only when
      // neither is there
      {
        type: "swiftget-resolve-video",
        resolve: stream || "",
        reqHeaders: reqHeaders || undefined,
        // in a frame that is only a player: the add-in uses the page around it
        page: CS.ownLink(b.video) || (CS.directSrc(b.video) || !CS.TOP || CS.inFeed() ? "" : location.href),
        feed: CS.TOP && CS.inFeed(),
        src: CS.directSrc(b.video),
        frame: !CS.TOP,
      },
      (res) => {
        b.loading = false;
        if (chrome.runtime.lastError || !res) return CS.notRunning(b, () => CS.lookUp(b, stream, reqHeaders));
        if (res.error) {
          if (res.notRunning || /not running/i.test(res.error)) return CS.notRunning(b, () => CS.lookUp(b, stream, reqHeaders));
          return CS.renderOne(b, res.error, true);
        }
        // the player went on to another video meanwhile: not its qualities
        if (CS.videoKey(b.video) !== asked) {
          b.open = false;
          return CS.renderOne(b);
        }
        b.own = res.items || [];
        b.ownFor = asked;
        CS.renderOne(b);
      }
    );
  };

  CS.clicked = (b) => {
    const list = CS.itemsOf(b);
    if (!list.length && !b.loading) return CS.lookUp(b);
    b.help = false;
    b.open = !b.open;
    CS.renderOne(b);
  };

  // The app is closed: offer to open it, then do what the user wanted.
  CS.notRunning = (b, retry) => {
    b.open = true;
    CS.renderOne(b, aknanI18n.t("Aknan Download is not running."), true, {
      text: aknanI18n.t("Open Aknan Download"),
      run: () => {
        // the program registers "aknan-download:" when it is installed
        const a = document.createElement("a");
        a.href = "aknan-download://open";
        a.style.display = "none";
        document.documentElement.appendChild(a);
        a.click();
        a.remove();
        CS.renderOne(b, aknanI18n.t("Opening Aknan Download…"));
        let tries = 0;
        const wait = setInterval(() => {
          tries++;
          chrome.runtime.sendMessage({ type: "swiftget-status" }, (res) => {
            if (chrome.runtime.lastError) return;
            if (res && res.ok) {
              clearInterval(wait);
              retry();
            } else if (tries >= 25) {
              clearInterval(wait);
              CS.renderOne(b, aknanI18n.t("Aknan Download did not start. Open it from the Start menu and try again."), true);
            }
          });
        }, 1000);
      },
    });
  };

  function sendPlaylist(b) {
    chrome.runtime.sendMessage({ type: "swiftget-playlist", url: location.href }, (res) => {
      if (chrome.runtime.lastError || !res || !res.ok) return CS.notRunning(b, () => sendPlaylist(b));
      CS.flashOne(b, aknanI18n.t("The playlist is open in Aknan Download ✔"), false);
    });
  }

  // "Download all": the whole playlist (YouTube list), or raw multi-file captures
  CS.downloadAllOf = (b, usable) => {
    if (CS.TOP && /youtube(kids)?\.com/i.test(location.hostname) && /[?&]list=/.test(location.search)) {
      return { text: aknanI18n.t("Download all videos of this playlist"), run: () => sendPlaylist(b) };
    }
    if (usable.some((m) => m.yt)) return null;
    const files = usable.filter((m) => !m.yt && m.url);
    if (files.length >= 2) {
      return {
        text: aknanI18n.t("Download all ({} files)", files.length),
        run: () => files.forEach((m, i) => setTimeout(() => CS.download(b, m), i * 300)),
      };
    }
    return null;
  };

  CS.flashOne = (b, text, isError) => {
    b.open = true;
    CS.renderOne(b, text, isError);
    setTimeout(() => {
      b.open = false;
      CS.renderOne(b);
    }, 2500);
  };

  CS.download = (b, m, subtitle) => {
    // a stream whose qualities are not known yet: ask the app for them
    if (m.resolve) return CS.lookUp(b, m.resolve, m.reqHeaders);
    const fileName = `${CS.cleanTitle()}.${m.ext || "mp4"}`;
    chrome.runtime.sendMessage(
      { type: "swiftget-download", item: m, title: document.title, fileName, referrer: location.href, subtitle: subtitle || undefined },
      (res) => {
        if (chrome.runtime.lastError || !res || !res.ok) CS.notRunning(b, () => CS.download(b, m, subtitle));
        else CS.flashOne(b, aknanI18n.t("Sent to Aknan Download ✔"), false);
      }
    );
  };

  CS.update = (newItems) => {
    if (!CS.TOP) return; // frames look up their own videos
    CS.items = newItems || [];
    if (!CS.items.length) {
      // new page / new video: forget the bubbles and "hidden"
      for (const b of CS.bubbles.values()) b.host.remove();
      CS.bubbles.clear();
      return;
    }
    if (!CS.panelAllowed()) return;
    CS.place();
    const topSubs = CS.subsOf ? CS.subsOf(null, CS.items) : [];
    for (const b of CS.bubbles.values()) {
      if (b.own && topSubs.length && b.own[0] && !b.loading) {
        b.own[0].subs = [...(b.own[0].subs || []), ...topSubs];
        CS.renderOne(b);
      } else if (!b.own && !b.loading) CS.renderOne(b);
    }
  };
})();
