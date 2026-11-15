const fmtSize = (n) => {
  if (!n) return "";
  const u = ["B", "KB", "MB", "GB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i ? 1 : 0)} ${u[i]}`;
};

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

async function init() {
  await aknanI18n.ready;
  applyStaticI18n();
  // Ask background to refresh config (also refreshes appLang if app language changed recently)
  try {
    chrome.runtime.sendMessage({ type: "swiftget-config" }, () => {});
  } catch {}
  chrome.storage.onChanged.addListener((ch, area) => {
    if ((!area || area === "local") && ch && ch.appLang) {
      applyStaticI18n();
      showOn(on.checked);
    }
  });
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

  chrome.runtime.sendMessage({ type: "swiftget-status" }, (res) => {
    const el = document.getElementById("status");
    const ok = res && res.ok;
    const nokey = res && res.state === "nokey";
    el.textContent = aknanI18n.t(ok ? "connected" : nokey ? "not connected" : "app not running");
    el.classList.toggle("on", !!ok);
    if (nokey) {
      // the program runs but does not know this add-in (yet): connect again
      const again = document.createElement("button");
      again.className = "act";
      again.textContent = aknanI18n.t("Connect again");
      again.addEventListener("click", () => {
        again.textContent = aknanI18n.t("Connecting… (answer in Aknan Download if it asks)");
        again.disabled = true;
        chrome.runtime.sendMessage({ type: "swiftget-reconnect" }, (r) => {
          again.disabled = false;
          again.textContent = aknanI18n.t(r && r.ok ? "Connected ✔" : "Not connected - try again");
          if (r && r.ok) {
            el.textContent = aknanI18n.t("connected");
            el.classList.add("on");
          }
        });
      });
      document.querySelector("section").appendChild(again);
    } else if (!ok) {
      // the program registers "aknan-download:" when it is installed
      const open = document.createElement("a");
      open.href = "aknan-download://open";
      open.className = "open";
      open.textContent = aknanI18n.t("Open Aknan Download");
      open.addEventListener("click", () => setTimeout(() => window.close(), 300));
      document.querySelector("section").appendChild(open);
    }
  });

  // Firefox: the add-in talks to the program through the program's helper
  // only once the user allows it (once); until then it uses the port
  if (/Firefox\//.test(navigator.userAgent) && chrome.permissions) {
    chrome.permissions.contains({ permissions: ["nativeMessaging"] }).then((has) => {
      if (has) return;
      const direct = document.createElement("button");
      direct.className = "act";
      direct.textContent = aknanI18n.t("Connect directly (recommended)");
      direct.title = aknanI18n.t("Firefox asks once: the add-in then talks to Aknan Download without a port");
      direct.addEventListener("click", () => {
        chrome.permissions.request({ permissions: ["nativeMessaging"] }).then((yes) => {
          direct.textContent = aknanI18n.t(yes ? "Connected directly ✔" : "Not allowed");
          direct.disabled = !!yes;
        });
      });
      document.querySelector("section").appendChild(direct);
    }, () => {});
  }

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  chrome.runtime.sendMessage({ type: "swiftget-get-media", tabId: tab.id }, (items) => {
    if (!items || !items.length) return;
    const box = document.getElementById("media");
    box.replaceChildren();
    const title = (tab.title || "video").replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").slice(0, 120);
    // a YouTube playlist: all of its videos in the app's playlist window
    if (/youtube\.com/i.test(tab.url || "") && /[?&]list=/.test(tab.url || "")) {
      const row = document.createElement("div");
      row.className = "media";
      const info = document.createElement("div");
      info.textContent = aknanI18n.t("All videos of this playlist");
      const btn = document.createElement("button");
      btn.className = "all";
      btn.textContent = aknanI18n.t("Download all");
      btn.addEventListener("click", () =>
        chrome.runtime.sendMessage({ type: "swiftget-playlist", url: tab.url }, (res) => {
          btn.textContent = aknanI18n.t(res && res.ok ? "Opened ✔" : "App not running");
        })
      );
      row.append(info, btn);
      box.appendChild(row);
    }
    items.forEach((m) => {
      const row = document.createElement("div");
      row.className = "media";
      const info = document.createElement("div");
      info.textContent = m.label;
      const small = document.createElement("small");
      small.textContent = m.sub || "";
      info.appendChild(small);
      const btn = document.createElement("button");
      btn.textContent = aknanI18n.t("Download");
      btn.disabled = !!m.disabled;
      btn.addEventListener("click", () => {
        if (m.resolve) {
          // a stream: its qualities come from the app, shown in its place
          btn.textContent = "…";
          btn.disabled = true;
          chrome.runtime.sendMessage({ type: "swiftget-resolve-video", resolve: m.resolve }, (res) => {
            if (!res || res.error || !res.items) {
              btn.textContent = (res && res.error) || aknanI18n.t("App not running");
              return;
            }
            const rows = res.items.map((it) => {
              const r = document.createElement("div");
              r.className = "media";
              const inf = document.createElement("div");
              inf.textContent = it.label;
              const go = document.createElement("button");
              go.textContent = aknanI18n.t("Download");
              go.addEventListener("click", () =>
                chrome.runtime.sendMessage({ type: "swiftget-download", item: it, title: tab.title, referrer: tab.url, fileName: `${title}.${it.ext || "mp4"}` }, (r2) => {
                  go.textContent = aknanI18n.t(r2 && r2.ok ? "Sent ✔" : "App not running");
                })
              );
              r.append(inf, go);
              return r;
            });
            row.replaceWith(...rows);
          });
          return;
        }
        chrome.runtime.sendMessage(
          { type: "swiftget-download", item: m, title: tab.title, referrer: tab.url, fileName: `${title}.${m.ext || "mp4"}` },
          (res) => {
            btn.textContent = aknanI18n.t(res && res.ok ? "Sent ✔" : "App not running");
          }
        );
      });
      row.append(info, btn);
      box.appendChild(row);
    });
  });
}

init();
