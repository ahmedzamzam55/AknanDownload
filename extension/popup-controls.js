// Popup static i18n and control bindings (switch, size, style, opacity, fullscreen, site status).
var aknanPopupControls = (() => {
  function applyStaticI18n() {
    document.documentElement.lang = aknanI18n.lang();
    document.documentElement.dir = aknanI18n.isRtl() ? "rtl" : "ltr";
    const set = (id, key) => {
      const el = document.getElementById(id);
      if (el) el.textContent = aknanI18n.t(key);
    };
    const setAttr = (id, attr, key) => {
      const el = document.getElementById(id);
      if (el) el.setAttribute(attr, aknanI18n.t(key));
    };
    set("lblTitle", "Aknan Download");
    set("status", "checking…");
    setAttr("settings", "title", "Settings");
    setAttr("switchLabel", "title", "Turn the add-in on or off");
    setAttr("enabled", "aria-label", "Aknan Download on / off");
    set("lblSize", "Download button size");
    setAttr("scale", "aria-label", "Download button size");
    set("scaleReset", "Reset");
    setAttr("scaleReset", "title", "Back to normal size");
    set("lblStyle", "Button style");
    setAttr("look", "aria-label", "Button style");
    set("optCompact", "Small (arrow + ✕)");
    set("optBlue", "Blue");
    set("optDark", "Dark");
    set("optLight", "Light");
    set("optGreen", "Green");
    set("optIcon", "Icon only");
    set("lblSolid", "Button see-through");
    setAttr("solid", "aria-label", "How solid the download button is");
    set("lblFullscreen", "Show the video button in full screen too");
    set("lblMediaHead", "Media on this page");
    set("lblEmpty", "Nothing detected yet. Start playing the video.");
  }

  function initControls() {
    document.getElementById("settings").addEventListener("click", () => {
      chrome.runtime.openOptionsPage();
      window.close();
    });

    // the on / off switch: off = the browser keeps its downloads, no buttons
    // on videos, nothing is looked up (the add-in stays installed)
    const on = document.getElementById("enabled");
    const showOn = (v) => {
      on.checked = v;
      document.body.classList.toggle("off", !v);
      document.getElementById("onText").textContent = aknanI18n.t(v ? "Aknan Download is on" : "Aknan Download is paused");
      document.getElementById("onHint").textContent = aknanI18n.t(
        v
          ? "Downloads and videos of this browser go to Aknan Download"
          : "The browser keeps its downloads, no buttons on videos"
      );
    };
    chrome.storage.local.get({ enabled: true }, (o) => showOn(o.enabled !== false));
    on.addEventListener("change", () => {
      chrome.storage.local.set({ enabled: on.checked });
      showOn(on.checked);
    });

    const look = document.getElementById("look");
    chrome.storage.local.get({ bubbleStyle: "compact" }, (o) => (look.value = o.bubbleStyle || "compact"));
    look.addEventListener("change", () => chrome.storage.local.set({ bubbleStyle: look.value }));

    // how solid the button is on the video (the mouse on it: always solid)
    const solid = document.getElementById("solid");
    const solidText = document.getElementById("solidText");
    const showSolid = (v) => {
      solid.value = String(v);
      solidText.textContent = `${v}%`;
    };
    chrome.storage.local.get({ bubbleOpacity: 0.9 }, (o) => showSolid(Math.round((Number(o.bubbleOpacity) || 0.9) * 100)));
    solid.addEventListener("input", () => {
      showSolid(Number(solid.value));
      chrome.storage.local.set({ bubbleOpacity: Number(solid.value) / 100 });
    });

    // full screen: the button stays out of the way unless the user wants it
    const fullscreen = document.getElementById("fullscreen");
    chrome.storage.local.get({ bubbleFullscreen: false }, (o) => (fullscreen.checked = !!o.bubbleFullscreen));
    fullscreen.addEventListener("change", () => chrome.storage.local.set({ bubbleFullscreen: fullscreen.checked }));

    // size of the "Download this video" button on pages (also in the settings page)
    const scale = document.getElementById("scale");
    const scaleText = document.getElementById("scaleText");
    const OLD = { small: 85, normal: 100, large: 125 };
    const show = (v) => {
      scale.value = String(v);
      scaleText.textContent = `${v}%`;
    };
    chrome.storage.local.get({ bubbleScale: null, bubbleSize: "normal" }, (o) =>
      show(o.bubbleScale != null ? Math.round(o.bubbleScale * 100) : OLD[o.bubbleSize] || 100)
    );
    const save = (v) => {
      show(v);
      chrome.storage.local.set({ bubbleScale: v / 100 });
    };
    scale.addEventListener("input", () => save(Number(scale.value)));
    document.getElementById("scaleReset").addEventListener("click", () => save(100));

    // why the last download stayed in the browser (the last ten minutes)
    const WHY = {
      "not-running": aknanI18n.t("Aknan Download was not running or did not answer."),
      small: aknanI18n.t("it is smaller than the size set in the add-in's settings."),
      rules: aknanI18n.t("Aknan Download's Options leave this kind of file, or this site, to the browser."),
      "cant-fetch": aknanI18n.t("the site gives this file to the browser only (a sign-in of the page, or a link that works once)."),
      form: aknanI18n.t("it comes from a form that sends a file, which can't be sent again."),
    };
    chrome.storage.local.get({ lastLeft: null }, ({ lastLeft }) => {
      const box = document.getElementById("left");
      if (!lastLeft || !WHY[lastLeft.why] || Date.now() - lastLeft.at > 10 * 60 * 1000) return;
      const name = lastLeft.name ? `"${lastLeft.name}"` : aknanI18n.t("The last download");
      box.textContent = aknanI18n.t("{} stayed in the browser: {}", name, WHY[lastLeft.why]);
      box.hidden = false;
    });

    // the button was turned off on this site (from its own help): back on
    chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
      let site = "";
      try {
        site = new URL((tabs && tabs[0] && tabs[0].url) || "").hostname.toLowerCase();
      } catch {
        return;
      }
      chrome.storage.local.get({ noButtonSites: [] }, ({ noButtonSites }) => {
        if (!site || !(noButtonSites || []).includes(site)) return;
        const again = document.createElement("button");
        again.className = "act";
        again.textContent = aknanI18n.t("Show the video button on {} again", site);
        again.addEventListener("click", () => {
          chrome.storage.local.set({ noButtonSites: noButtonSites.filter((h) => h !== site) }, () => {
            again.textContent = aknanI18n.t("Done - reload the page");
            again.disabled = true;
          });
        });
        document.querySelector("section").appendChild(again);
      });
    });

    return { refreshOn: () => showOn(on.checked) };
  }

  return { applyStaticI18n, initControls };
})();
