// Page link click interception (files & magnet links) and link collector.
(() => {
  if (window.__aknanDownload) return;
  const CS = (globalThis.__aknanCS = globalThis.__aknanCS || {});

  const wildcard = (pattern, text) =>
    new RegExp(
      "^" + String(pattern).toLowerCase().replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$"
    ).test(String(text).toLowerCase());

  let linkRules = null;
  const refreshLinkRules = () =>
    chrome.runtime.sendMessage({ type: "swiftget-link-rules" }, (res) => {
      linkRules = chrome.runtime.lastError ? null : res || null;
    });

  // shown by the browser itself, not downloaded: never taken from a click
  // pages and pictures open in the browser; a PDF too, unless PDF is one of
  // the file types the app takes (then a click on its link goes to the app:
  // once it is open in the browser's PDF viewer, the viewer's own download
  // button always asks where to save, and that can't be stopped)
  const VIEWED = /^(html?|xhtml|php\d?|aspx?|jsp|cgi|pl|cfm|do|action|json|xml|txt|css|js|jpe?g|png|gif|webp|svg|bmp|ico|avif)$/;
  const keyHeld = (e, k) => (k === "alt" ? e.altKey : k === "ctrl" ? e.ctrlKey : k === "shift" ? e.shiftKey : false);

  function wantedLink(e, href) {
    const r = linkRules;
    if (!r || !CS.switchedOn) return false;
    let u;
    try {
      u = new URL(href);
    } catch {
      return false;
    }
    if (!/^https?:$/.test(u.protocol)) return false;
    if (r.skipKey !== "none" && keyHeld(e, r.skipKey)) return false;
    const forced = r.forceKey !== "none" && keyHeld(e, r.forceKey);
    // Ctrl / Shift / Meta + click: a new tab or window, unless that is the "take it" key
    if (!forced && (e.ctrlKey || e.shiftKey || e.metaKey || e.altKey)) return false;
    const host = u.hostname.toLowerCase();
    if ((r.skipSites || []).some((s) => CS.hostMatches(host, s))) return false;
    if ((r.skipAddresses || []).some((a) => wildcard(a, href))) return false;
    const last = decodeURIComponent(u.pathname.split("/").pop() || "");
    const dot = last.lastIndexOf(".");
    const ext = dot > 0 ? last.slice(dot + 1).toLowerCase() : "";
    if (!ext || VIEWED.test(ext)) return false;
    if (ext === "pdf" && !(r.exts || []).some((x) => wildcard(x, ext))) return false;
    // a .torrent file opens in the program's torrent window
    if (forced || ext === "torrent") return true;
    return (r.exts || []).some((x) => wildcard(x, ext));
  }

  CS.initLinks = () => {
    refreshLinkRules();
    setInterval(refreshLinkRules, 60000);

    // Heard after the page's own click code (not before it, and the click is
    // not stopped): file sites often make their link work in that code (a
    // request saying "the download starts", a cookie); stopped, the link stayed
    // "403 Forbidden" for the program while the browser and other programs got
    // the file. Only the browser's own download is held back.
    addEventListener(
      "click",
      (e) => {
        if (e.button !== 0 || e.defaultPrevented) return;
        const a = e.composedPath ? e.composedPath().find((n) => n && n.tagName === "A" && n.href) : null;
        if (!a || a.dataset.aknanPass) return;
        const href = String(a.href);
        if (!wantedLink(e, href)) return;
        e.preventDefault();
        chrome.runtime.sendMessage({ type: "swiftget-link", url: href }, (res) => {
          // not taken (the app is closed, the user chose the browser): the browser's way
          if (chrome.runtime.lastError || !res || !res.ok) {
            const tmp = document.createElement("a");
            tmp.href = href;
            if (a.hasAttribute("download")) tmp.setAttribute("download", a.getAttribute("download") || "");
            tmp.dataset.aknanPass = "1";
            document.documentElement.appendChild(tmp);
            tmp.click();
            tmp.remove();
          }
        });
      },
      false
    );

    // magnet links of torrents go to Aknan Download (Alt + click: to the browser)
    addEventListener(
      "click",
      (e) => {
        if (e.button !== 0 || e.altKey || e.defaultPrevented || !CS.switchedOn) return;
        const a = e.composedPath ? e.composedPath().find((n) => n && n.tagName === "A" && n.href) : null;
        const href = a ? String(a.href) : "";
        if (!/^magnet:\?/i.test(href) || !/xt=urn:bt[im]h:/i.test(href)) return;
        e.preventDefault();
        e.stopPropagation();
        chrome.runtime.sendMessage({ type: "swiftget-magnet", url: href }, (res) => {
          // not taken (the app is closed, or sending is off): the browser's way
          if (chrome.runtime.lastError || !res || !res.ok) location.href = href;
        });
      },
      true
    );
  };

  // every link of the page (or the selection) for "Download all links"
  CS.collectLinks = (selectionOnly) => {
    const sel = selectionOnly ? getSelection() : null;
    const inSel = (el) => !sel || (sel.rangeCount > 0 && sel.containsNode(el, true));
    const out = [];
    const seen = new Set();
    const add = (url, text) => {
      if (!url || !/^https?:/i.test(url)) return;
      url = url.split("#")[0];
      if (seen.has(url)) return;
      seen.add(url);
      out.push({ url, text: (text || "").replace(/\s+/g, " ").trim().slice(0, 200) });
    };
    // the page itself and the parts built as web components
    const roots = [document];
    try {
      for (const el of document.querySelectorAll("*")) if (el.shadowRoot) roots.push(el.shadowRoot);
    } catch (_) {
      /* a strange page */
    }
    for (const root of roots) {
      for (const a of root.querySelectorAll("a[href]")) if (inSel(a)) add(a.href, a.innerText || a.title);
      for (const el of root.querySelectorAll("img[src], video[src], audio[src], source[src], embed[src]")) {
        if (inSel(el)) add(el.src, el.alt || el.title || "");
      }
      // buttons and boxes that keep their file's address in data-* (many
      // download pages): made absolute against the page
      for (const el of root.querySelectorAll("[data-href], [data-url], [data-download], [data-file], [data-link]")) {
        if (!inSel(el)) continue;
        for (const k of ["href", "url", "download", "file", "link"]) {
          const v = el.dataset && el.dataset[k];
          if (v && !/^(javascript|data):/i.test(v)) {
            try {
              add(new URL(v, location.href).href, el.innerText || el.title || "");
            } catch (_) {
              /* not an address */
            }
          }
        }
      }
    }
    // addresses written as plain text (pages that show a list of links, a
    // note or a paste): what the user sees is what is collected
    const text = sel ? String(sel) : (document.body && document.body.innerText) || "";
    for (const m of text.slice(0, 2000000).matchAll(/\bhttps?:\/\/[^\s"'<>()\[\]{}]+/gi)) {
      add(m[0].replace(/[.,;:!?]+$/, ""), "");
      if (out.length > 5000) break;
    }
    return { page: location.href, links: out };
  };
})();
