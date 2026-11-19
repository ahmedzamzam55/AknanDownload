// Video element discovery (including shadow DOM), page/feed link matching, and watch probe.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  let shadowVideos = [];
  let deepRound = 0;

  function collectShadow(root, out, depth) {
    if (depth > 6) return;
    for (const el of root.querySelectorAll("*")) {
      const sr = el.shadowRoot;
      if (!sr) continue;
      out.push(...sr.querySelectorAll("video"));
      collectShadow(sr, out, depth + 1);
    }
  }

  CS.allVideos = () => {
    const list = [...document.querySelectorAll("video")];
    if (deepRound++ % 4 === 0) {
      const found = [];
      try {
        collectShadow(document, found, 0);
      } catch (_) {
        /* a strange page: keep what we have */
      }
      shadowVideos = found;
    }
    for (const v of shadowVideos) if (v.isConnected && !list.includes(v)) list.push(v);
    return list;
  };

  // The video the user is watching: a playing one first, else the biggest
  // one on screen.
  CS.biggestVideo = () => {
    let best = null;
    let score = 0;
    for (const v of CS.allVideos()) {
      const r = v.getBoundingClientRect();
      if (r.width < 200 || r.height < 100) continue;
      const visible = Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0));
      if (visible < 60) continue;
      const s = r.width * visible * (!v.paused && !v.ended ? 4 : 1);
      if (s > score) {
        best = v;
        score = s;
      }
    }
    return best;
  };

  CS.playing = (v) => !v.paused && !v.ended && v.duration > 3;

  // Pages that are one video: their own address is the best link for it.
  const VIDEO_PAGE = [
    /youtube(kids)?\.com\/(watch|shorts\/|live\/|embed\/)/i,
    /youtube-nocookie\.com\/embed\//i,
    /tiktok\.com\/@[^/]+\/(video|photo)\//i,
    /instagram\.com\/(reels?|p|tv)\//i,
    /facebook\.com\/(.+\/videos\/|watch|reel\/|share\/[vr]\/|plugins\/video)/i,
    /fb\.watch\//i,
    /(twitter|x)\.com\/[^/]+\/status\//i,
    /vimeo\.com\/\d+/i,
    /player\.vimeo\.com\/video\//i,
    // (not the "geo.dailymotion.com/player/…" frame: that is only the player)
    /dailymotion\.com\/(video\/|embed\/video\/)/i,
    /dai\.ly\//i,
    /reddit\.com\/r\/[^/]+\/comments\//i,
    /twitch\.tv\/(videos\/|[^/]+\/clip\/)/i,
    /clips\.twitch\.tv\//i,
    /pinterest\.[a-z.]+\/pin\//i,
    /snapchat\.com\/(spotlight|add)\//i,
    /linkedin\.com\/(posts|feed\/update)\//i,
  ];
  CS.isVideoPage = (u) => VIDEO_PAGE.some((r) => r.test(u));

  // Facebook feeds: the post a video is in is also a link the app can read
  // (the feed's own address - facebook.com/ - is not)
  const FB_POST = [
    /facebook\.com\/[^/?#]+\/posts\//i,
    /facebook\.com\/permalink\.php\?[^#]*story_fbid=/i,
    /facebook\.com\/story\.php\?[^#]*story_fbid=/i,
    /facebook\.com\/groups\/[^/]+\/(permalink|posts)\//i,
    /facebook\.com\/photo(\.php)?\/?\?[^#]*fbid=/i,
  ];
  const onFacebook = /(^|\.)facebook\.com$/i.test(location.hostname);
  const isPostLink = (u) => onFacebook && FB_POST.some((r) => r.test(u));
  // a Facebook feed (home, a page's wall, groups...): many videos, and its
  // own address is none of them
  CS.inFeed = () => onFacebook && !CS.isVideoPage(location.href) && !isPostLink(location.href);

  // what is left of a Facebook link once its tracking part is cut off
  CS.cleanLink = (u) => {
    try {
      const x = new URL(u);
      for (const k of [...x.searchParams.keys()]) if (/^(__cft__|__tn__|_fb_noscript|ref|rdid|mibextid)/i.test(k)) x.searchParams.delete(k);
      x.hash = "";
      return x.toString();
    } catch {
      return u.split("#")[0];
    }
  };

  // In feeds (Facebook, Instagram, X...) the page shows many videos: look for
  // the post / reel link around the video being watched.
  function linkFor(video) {
    if (CS.isVideoPage(location.href) || isPostLink(location.href)) return CS.cleanLink(location.href);
    // a Facebook feed / home page is no video: nothing rather than the feed
    return CS.ownLink(video) || (onFacebook ? "" : location.href);
  }

  // the post / video link next to this very video ("" = none found)
  CS.ownLink = (video) => {
    let el = video;
    let post = "";
    for (let depth = 0; el && depth < 25; depth++) {
      const links = el.querySelectorAll ? el.querySelectorAll("a[href]") : [];
      for (const a of links) {
        if (CS.isVideoPage(a.href)) return CS.cleanLink(a.href);
        if (!post && isPostLink(a.href)) post = CS.cleanLink(a.href);
      }
      // the end of this post (Facebook marks its posts, it has no <article>)
      if (el.tagName === "ARTICLE" || (el.getAttribute && el.getAttribute("role") === "article")) break;
      // out of a shadow root: on to the element that holds it
      el = el.parentElement || (el.parentNode && el.parentNode.host) || null;
    }
    if (post) return post;
    // an embedded player: the frame's own address is the video
    if (!CS.TOP && CS.isVideoPage(location.href)) return location.href;
    return "";
  };

  CS.watchVideos = () => {
    const v = CS.biggestVideo();
    if (!v) return;
    // ignore tiny muted background loops that never play for long
    if (v.paused && !(v.duration > 3) && !v.currentSrc) return;
    // in a frame (an embedded player): its own address when it is a video
    // page (YouTube / Vimeo embeds), otherwise the page around it (the
    // add-in knows that one)
    const page = CS.TOP ? linkFor(v) : CS.isVideoPage(location.href) ? location.href : "";
    if (page !== CS.lastPage || (!CS.TOP && !CS.lastPage)) {
      if (!CS.TOP && CS.lastPage === "frame:" + page) return;
      CS.lastPage = CS.TOP ? page : "frame:" + page;
      chrome.runtime.sendMessage({ type: "swiftget-video-seen", page, frame: !CS.TOP, feed: CS.TOP && !page && CS.inFeed() }).catch?.(() => {});
    }
  };

  CS.isMain = (video) => CS.TOP && (video.tagName === "IFRAME" || video === CS.biggestVideo());

  CS.directSrc = (video) => {
    if (video.tagName === "IFRAME") return "";
    const src = video.currentSrc || video.src || "";
    return /^https?:/i.test(src) && !/\.m3u8|\.mpd/i.test(src) ? src : "";
  };

  // where the bubbles live: in full screen the player's own element (the
  // only thing the browser shows then)
  CS.layer = () => {
    const f = document.fullscreenElement || document.webkitFullscreenElement;
    return f && f.tagName !== "VIDEO" ? f : document.documentElement;
  };

  CS.heightOf = (q) => {
    const m = /^(\d{3,4})p/.exec(q || "");
    return m ? Number(m[1]) : 0;
  };
})();
