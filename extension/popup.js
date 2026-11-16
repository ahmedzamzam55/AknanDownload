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

function initStatusSection() {
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
}

function renderMediaList(tab, items) {
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
}

async function init() {
  await aknanI18n.ready;
  aknanPopupControls.applyStaticI18n();
  // Ask background to refresh config (also refreshes appLang if app language changed recently)
  try {
    chrome.runtime.sendMessage({ type: "swiftget-config" }, () => {});
  } catch {}
  const { refreshOn } = aknanPopupControls.initControls();
  chrome.storage.onChanged.addListener((ch, area) => {
    if ((!area || area === "local") && ch && ch.appLang) {
      aknanPopupControls.applyStaticI18n();
      refreshOn();
    }
  });
  initStatusSection();

  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  chrome.runtime.sendMessage({ type: "swiftget-get-media", tabId: tab.id }, (items) => {
    if (!items || !items.length) return;
    renderMediaList(tab, items);
  });
}

init();
