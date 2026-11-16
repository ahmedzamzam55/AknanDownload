// Floating "Download this video" button coordinator & page event wiring.
(() => {
  if (window.__aknanDownload) return;
  window.__aknanDownload = true;
  const CS = globalThis.__aknanCS;

  try {
    chrome.storage.local.get({ noButtonSites: [], buttonPlaces: {} }, (o) => {
      CS.noButtonHere = !!(o && (o.noButtonSites || []).includes(CS.SITE));
      CS.sitePlace = (o && o.buttonPlaces && o.buttonPlaces[CS.SITE]) || null;
      if (CS.noButtonHere) CS.removeAll();
    });
  } catch (_) {
    /* storage not reachable: shown, at its usual place */
  }

  try {
    chrome.storage.local.get({ enabled: true }, (o) => {
      CS.switchedOn = !o || o.enabled !== false;
      if (!CS.switchedOn) CS.removeAll();
    });
    chrome.storage.onChanged.addListener((ch) => {
      if (!ch.enabled) return;
      CS.switchedOn = ch.enabled.newValue !== false;
      if (!CS.switchedOn) CS.removeAll();
      else chrome.runtime.sendMessage({ type: "swiftget-media-request" }, (res) => !chrome.runtime.lastError && CS.update(res));
    });
  } catch (_) {
    // storage not reachable (add-in reloading): stays on
  }

  try {
    chrome.storage.local.get({ bubbleFullscreen: false }, (o) => (CS.showInFullscreen = !!(o && o.bubbleFullscreen)));
    chrome.storage.onChanged.addListener((ch) => {
      if (ch.bubbleFullscreen) {
        CS.showInFullscreen = !!ch.bubbleFullscreen.newValue;
        CS.bubbles.forEach((b) => CS.placeOne(b));
      }
      if (ch.appLang) {
        const dir = aknanI18n.isRtl() ? "rtl" : "ltr";
        CS.bubbles.forEach((b) => {
          const w = b.root && b.root.querySelector(".wrap");
          if (w) w.setAttribute("dir", dir);
          CS.renderOne(b);
        });
      }
    });
    chrome.storage.local.get({ bubbleScale: null, bubbleSize: "normal", bubbleStyle: "compact", bubbleOpacity: 0.9 }, (o) => {
      CS.zoom = CS.clampZoom(o && o.bubbleScale != null ? o.bubbleScale : CS.OLD_SIZES[o && o.bubbleSize]);
      CS.solid = CS.clampSolid(o && o.bubbleOpacity);
      CS.look = CS.STYLES.includes(o && o.bubbleStyle) ? o.bubbleStyle : "compact";
      CS.applyZoom();
      CS.applyLook();
    });
    chrome.storage.onChanged.addListener((ch) => {
      if (ch.bubbleStyle) {
        CS.look = CS.STYLES.includes(ch.bubbleStyle.newValue) ? ch.bubbleStyle.newValue : "compact";
        CS.applyLook();
        return;
      }
      if (ch.bubbleOpacity) CS.solid = CS.clampSolid(ch.bubbleOpacity.newValue);
      else if (ch.bubbleScale) CS.zoom = CS.clampZoom(ch.bubbleScale.newValue);
      else if (ch.bubbleSize) CS.zoom = CS.clampZoom(CS.OLD_SIZES[ch.bubbleSize.newValue]);
      else return;
      CS.applyZoom();
    });
  } catch (_) {
    // storage not reachable (add-in reloading): default size
  }

  CS.initLinks();

  // a click anywhere else closes an open menu
  addEventListener(
    "mousedown",
    (e) => {
      const path = e.composedPath ? e.composedPath() : [];
      for (const b of CS.bubbles.values()) {
        if (b.open && !path.includes(b.host)) {
          b.open = false;
          b.help = false;
          CS.renderOne(b);
        }
      }
    },
    true
  );
  addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    for (const b of CS.bubbles.values()) {
      if (b.open) {
        b.open = false;
        CS.renderOne(b);
      }
    }
  });
  for (const ev of ["fullscreenchange", "webkitfullscreenchange"]) {
    document.addEventListener(ev, () => setTimeout(() => CS.bubbles.forEach((b) => CS.placeOne(b)), 50));
  }

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg.type === "swiftget-media") CS.update(msg.items);
    if (msg.type === "swiftget-collect-links") {
      if (!CS.TOP) return false; // the page itself answers
      sendResponse(CS.collectLinks(!!msg.selection));
      return true;
    }
    if (msg.type === "swiftget-open-panel") {
      const v = CS.biggestVideo();
      if (!v) return false;
      let b = CS.bubbles.get(v);
      if (!b) b = CS.makeBubble(v);
      b.dismissed = false;
      CS.layer().appendChild(b.host);
      b.open = true;
      if (!CS.itemsOf(b).length) CS.lookUp(b);
      else CS.renderOne(b);
      CS.placeOne(b);
    }
    return false;
  });

  chrome.runtime.sendMessage({ type: "swiftget-config" }, (res) => {
    if (!chrome.runtime.lastError && res) {
      CS.panel = res;
      if (!CS.panelAllowed()) CS.removeAll();
    }
  });
  if (CS.TOP) {
    // keys held while clicking a link: Alt = let the browser download it, etc.
    addEventListener(
      "mousedown",
      (e) => {
        chrome.runtime.sendMessage({ type: "swiftget-keys", alt: e.altKey, ctrl: e.ctrlKey, shift: e.shiftKey }).catch?.(() => {});
      },
      true
    );
    // media detected before this script loaded
    chrome.runtime.sendMessage({ type: "swiftget-media-request" }, (res) => {
      if (!chrome.runtime.lastError) CS.update(res);
    });
  }

  addEventListener("scroll", CS.place, { passive: true, capture: true });
  addEventListener("resize", CS.place, { passive: true });
  setInterval(CS.place, 700);
  setInterval(CS.watchVideos, 1500);
  document.addEventListener("play", () => setTimeout(() => (CS.watchVideos(), CS.place()), 300), true);
  addEventListener("popstate", () => setTimeout(CS.watchVideos, 500));
})();
