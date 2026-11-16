// Bubble positioning, edge snapping, overlap avoidance, and covered-iframe detection.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  // a free place (fractions of the video, after a drag) put on its nearest
  // edge; near a corner, in the corner
  CS.snapToEdge = (at) => {
    const d = { left: at.x, right: 1 - at.x, top: at.y, bottom: 1 - at.y };
    const edge = Object.keys(d).reduce((a, k) => (d[a] <= d[k] ? a : k));
    const c = (n) => (n < 0.08 ? 0 : n > 0.92 ? 1 : n);
    if (edge === "left") return { x: 0, y: c(at.y) };
    if (edge === "right") return { x: 1, y: c(at.y) };
    if (edge === "top") return { x: c(at.x), y: 0 };
    return { x: c(at.x), y: 1 };
  };

  // is one of the page's own buttons (much smaller than the video) under this spot?
  function onPageControl(b, left, top, w, h, r) {
    const area = (x) => Math.max(0, x.width) * Math.max(0, x.height);
    const small = (e) => area(e.getBoundingClientRect()) < area(r) / 5;
    const points = [[left + w / 2, top + h / 2], [left + 3, top + 3], [left + w - 3, top + 3]];
    for (const [px, py] of points) {
      let list = [];
      try {
        list = document.elementsFromPoint(px, py);
      } catch (_) {
        return false;
      }
      for (const e of list) {
        if (e === b.host) continue;
        if (e === b.video || e.contains(b.video)) break;
        const c = e.closest && e.closest("button, [role=button], a[href], input, select");
        if (c && !c.contains(b.video) && small(c)) return true;
      }
    }
    return false;
  }

  // where the button sits on its video: the user's place on this site, else
  // the top right corner, or the top left one when a button of the page's
  // own is there (kept a moment, so it does not jump about)
  function spotFor(b, r, w, h) {
    if (b.place) return b.place;
    const now = Date.now();
    if (b.auto && b.auto.v === b.video && now - b.auto.t < 600) return b.auto.at;
    let at = { x: 1, y: 0 };
    for (const c of [{ x: 1, y: 0 }, { x: 0, y: 0 }]) {
      const left = r.left + 8 + c.x * Math.max(0, r.width - w - 16);
      const top = r.top + 8 + c.y * Math.max(0, r.height - h - 16);
      if (!onPageControl(b, left, top, w, h, r)) {
        at = c;
        break;
      }
    }
    b.auto = { v: b.video, at, t: now };
    return at;
  }

  CS.placeOne = (b) => {
    const fs = document.fullscreenElement || document.webkitFullscreenElement;
    const r = b.video.getBoundingClientRect();
    // closed with its X: it stays closed (also after full screen, a new size…)
    // until the page shows another video; in full screen only when the user
    // asked for it (the add-in's window)
    const onScreen =
      !b.dismissed && (!fs || CS.showInFullscreen) &&
      r.bottom > 0 && r.top < innerHeight && r.width >= 200 && r.height >= 100 && b.video.isConnected &&
      !(fs && fs.tagName === "VIDEO") && (!fs || fs.contains(b.video) || fs.shadowRoot);
    b.host.style.display = onScreen ? "block" : "none";
    if (!onScreen) return;
    if (b.host.parentNode !== CS.layer()) CS.layer().appendChild(b.host);
    // the size of the button itself (the user can make it smaller or bigger);
    // its list floats and never moves it
    const mainRect = b.root.getElementById("main").getBoundingClientRect();
    const w = mainRect.width || 62;
    const h = mainRect.height || 22;
    let x;
    let y;
    if (b.dx != null) {
      // being dragged
      x = r.left + b.dx;
      y = r.top + b.dy;
    } else {
      const at = spotFor(b, r, w, h);
      x = r.left + 8 + at.x * Math.max(0, r.width - w - 16);
      y = r.top + 8 + at.y * Math.max(0, r.height - h - 16);
    }
    x = Math.max(r.left, Math.min(x, r.right - 60));
    y = Math.max(r.top, Math.min(y, r.bottom - h));
    const left = Math.max(4, Math.min(x, innerWidth - w));
    const top = Math.max(4, Math.min(y, innerHeight - h - 4));
    b.host.style.left = `${left}px`;
    b.host.style.top = `${top}px`;
    // where the list opens: away from the near edge of the screen
    const wrap = b.root.querySelector(".wrap");
    if (wrap) {
      const listW = 330 * CS.zoom;
      const listH = Math.min(380, (b.root.getElementById("list") || {}).scrollHeight || 380) * CS.zoom;
      wrap.classList.toggle("toright", left + w - listW < 4);
      wrap.classList.toggle("up", top + h + listH > innerHeight - 4 && top - listH > 4);
    }
  };

  // big frames whose top right corner (where the button is) the page covers
  function coveredPlayers() {
    const out = [];
    for (const f of document.querySelectorAll("iframe")) {
      const r = f.getBoundingClientRect();
      if (r.width < 300 || r.height < 170 || r.bottom < 60 || r.top > innerHeight - 60) continue;
      const x = Math.min(r.right - 40, innerWidth - 5);
      const y = Math.max(r.top + 16, 5);
      const top = document.elementFromPoint(x, y);
      if (top && top !== f && !(CS.bubbles.get(f) && top === CS.bubbles.get(f).host)) out.push(f);
      if (out.length >= 2) break;
    }
    return out;
  }

  CS.place = () => {
    if (!CS.panelAllowed()) return;
    const vids = CS.allVideos().filter((v) => {
      const r = v.getBoundingClientRect();
      return r.width >= 200 && r.height >= 100;
    });
    // bubbles of videos that went away
    for (const [v, b] of CS.bubbles) {
      if (!v.isConnected) {
        b.host.remove();
        CS.bubbles.delete(v);
      }
    }
    // the main video once the add-in found media or it plays (then it shows
    // "looking for qualities"), any other one as soon as it is a real player
    for (const v of vids.slice(0, 12)) {
      let b = CS.bubbles.get(v);
      if (!b) {
        const show = CS.isMain(v) ? CS.items.length || CS.playing(v) : !(v.paused && !(v.duration > 3) && !CS.directSrc(v));
        if (!show) continue;
        b = CS.makeBubble(v);
      }
      if (!b.dismissed) CS.placeOne(b);
    }
    // the video plays in a frame the page lays a see-through layer over
    // (Google Drive): the frame's own button can be seen but never pressed,
    // so this page shows one over the frame (with what was found for it)
    if (CS.TOP && CS.items.length && !vids.length) {
      for (const f of coveredPlayers()) {
        const b = CS.bubbles.get(f) || CS.makeBubble(f);
        if (!b.dismissed) CS.placeOne(b);
      }
    }
  };
})();
