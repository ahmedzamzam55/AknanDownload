// Shared content-script state, site preferences, and formatting/DOM helpers.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  CS.TOP = window.top === window;
  CS.SITE = location.hostname.toLowerCase();
  CS.PRIVATE = !!(chrome.extension && chrome.extension.inIncognitoContext);
  CS.OLD_SIZES = { small: 0.85, normal: 1, large: 1.25 };
  CS.STYLES = ["compact", "blue", "dark", "light", "green", "icon"];

  CS.items = [];
  CS.panel = { enabled: true, skipSites: [] };
  CS.switchedOn = true;
  CS.showInFullscreen = false;
  CS.noButtonHere = false;
  CS.sitePlace = null;
  CS.look = "compact";
  CS.zoom = 1;
  CS.solid = 0.9;
  CS.bubbles = new Map(); // video -> bubble
  CS.lastPage = "";

  CS.hostMatches = (host, p) => {
    p = String(p).replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
    if (!p) return false;
    if (p.includes("*")) return new RegExp("^" + p.replace(/[.]/g, "\\.").replace(/\*/g, ".*") + "$").test(host);
    return host === p || host.endsWith("." + p);
  };

  CS.panelAllowed = () =>
    CS.switchedOn &&
    !CS.noButtonHere &&
    CS.panel.enabled !== false &&
    !(CS.panel.skipSites || []).some((s) => CS.hostMatches(CS.SITE, s));

  // a press of the user (or the same press passed on by early.js), never one a
  // page's script made up
  CS.userPress = (e) => !!e && (e.isTrusted || !!(globalThis.__aknanReplayed && globalThis.__aknanReplayed.has(e)));

  CS.removeAll = () => {
    for (const b of CS.bubbles.values()) b.host.remove();
    CS.bubbles.clear();
  };

  CS.keepPlace = (p) => {
    CS.sitePlace = p;
    // never in a private window (it would keep the site's name)
    if (CS.PRIVATE) return;
    try {
      chrome.storage.local.get({ buttonPlaces: {} }, (o) => {
        const all = (o && o.buttonPlaces) || {};
        delete all[CS.SITE];
        all[CS.SITE] = p;
        const sites = Object.keys(all);
        for (const s of sites.slice(0, Math.max(0, sites.length - 200))) delete all[s];
        chrome.storage.local.set({ buttonPlaces: all });
      });
    } catch (_) {
      /* not kept: it stays there on this page */
    }
  };

  CS.noButtonOnThisSite = () => {
    CS.noButtonHere = true;
    CS.removeAll();
    try {
      chrome.storage.local.get({ noButtonSites: [] }, (o) => {
        const list = ((o && o.noButtonSites) || []).filter((h) => h !== CS.SITE);
        list.push(CS.SITE);
        chrome.storage.local.set({ noButtonSites: list.slice(-500) });
      });
    } catch (_) {
      /* only for this page then */
    }
  };

  CS.fmtSize = (n) => {
    if (!n) return "";
    const u = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) {
      n /= 1024;
      i++;
    }
    return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
  };

  CS.fmtLen = (s) => {
    s = Math.round(Number(s) || 0);
    if (!s) return "";
    const two = (n) => String(n).padStart(2, "0");
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
  };

  // "1:02:05", "02:05", "125", "2:05.5" -> seconds (null when not a time)
  CS.parseClock = (text) => {
    const t = String(text || "").trim().replace(/[٠-٩]/g, (c) => String(c.charCodeAt(0) - 0x660)).replace(",", ".");
    const m = /^(?:(\d{1,2}):)?(?:(\d{1,3}):)?(\d{1,6})(?:\.(\d{1,3}))?$/.exec(t);
    if (!m) return null;
    const parts = t.split(".")[0].split(":").map(Number);
    if (parts.slice(1).some((n) => n >= 60)) return null;
    let secs = parts.reduce((a, n) => a * 60 + n, 0);
    if (m[4]) secs += Number("0." + m[4]);
    return secs;
  };

  CS.clock = (s) => {
    s = Math.max(0, Math.floor(Number(s) || 0));
    const two = (n) => String(n).padStart(2, "0");
    return s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${two(Math.floor(s / 60))}:${two(s % 60)}`;
  };

  // the resolution in plain words (the app sends it; streams seen in the
  // player only have the height)
  CS.tagOf = (h) => (!h ? "" : h >= 4320 ? "8K" : h >= 2160 ? "4K" : h >= 1440 ? "2K" : h >= 1080 ? "Full HD" : h >= 720 ? "HD" : "SD");

  CS.cleanTitle = () =>
    (document.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").trim().slice(0, 120) || "video";

  CS.clampZoom = (z) => Math.min(2, Math.max(0.5, Number(z) || 1));
  CS.clampSolid = (o) => Math.min(1, Math.max(0.2, Number(o) || 0.9));

  CS.el = (tag, props = {}, ...kids) => {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(props)) {
      if (k === "text") e.textContent = v;
      else e.setAttribute(k, v);
    }
    e.append(...kids);
    return e;
  };
})();
