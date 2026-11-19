// ---------------------------------------------------------------- a clean start & self update
// After every update (and every reload) the add-in forgets what the old
// version kept: as if it was removed and added again. The user's choices
// (button style, size...) and a key given by pairing stay.
const OLD_KEYS = ["loadedStamp", "reloadedFor", "reloadedForStamp", "seenRepair"];

async function cleanOnInstalled(d) {
  if (d.reason !== "install" && d.reason !== "update") return;
  try {
    await chrome.storage.session.clear();
    await chrome.storage.local.remove(OLD_KEYS.filter((k) => k !== "seenRepair"));
  } catch {
    /* nothing kept */
  }
  link = null;
  await rememberLoadedFiles();
}

// The add-in is loaded from the program's own folder. When the program
// updates itself it brings newer add-in files along; the browser only reads
// them after a reload, so the add-in reloads itself once per new version.
//
// The version alone is not enough (a new setup of the same version brings
// changed files too): the files on the disk are compared with the files the
// browser loaded, and when they differ the add-in reloads (once for them).
const OWN_FILES = [
  "manifest.json",
  "early.js",
  "yt-itags.js",
  "yt-core.js",
  "i18n/ar.js",
  "i18n/fr.js",
  "i18n/es.js",
  "i18n/de.js",
  "i18n/pt.js",
  "i18n/tr.js",
  "i18n/ru.js",
  "i18n.js",
  "bg/helper.js",
  "bg/auth.js",
  "bg/selfupdate.js",
  "bg/context.js",
  "bg/requests.js",
  "bg/yt-subs.js",
  "bg/sub-fetch.js",
  "bg/dash-subs.js",
  "bg/rules.js",
  "bg/downloads.js",
  "bg/takeover.js",
  "bg/intercept.js",
  "bg/tab-media.js",
  "bg/hls-sniff.js",
  "bg/sniffer.js",
  "bg/resolve.js",
  "bg/menus.js",
  "bg/messages.js",
  "background.js",
  "content/state.js",
  "content/videos.js",
  "content/styles.js",
  "content/bubble-dom.js",
  "content/bubble-actions.js",
  "content/subs-tab.js",
  "content/bubble-render.js",
  "content/clip-tab.js",
  "content/place.js",
  "content/links.js",
  "content.js",
  "popup.html",
  "popup-controls.js",
  "popup.js",
  "options.html",
  "options.js",
];

async function filesStamp() {
  let all = "";
  for (const name of OWN_FILES) {
    try {
      const r = await fetch(chrome.runtime.getURL(name), { cache: "no-store" });
      all += `${name}\n${await r.text()}\n`;
    } catch {
      all += `${name}\n-\n`;
    }
  }
  const sum = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(all));
  return [...new Uint8Array(sum)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// what the browser loaded: the files as they are when it (re)loads the add-in
async function rememberLoadedFiles() {
  try {
    await chrome.storage.local.set({ loadedStamp: await filesStamp() });
  } catch {
    /* nothing to compare with: the version check still works */
  }
}

async function followOwnFiles() {
  const { loadedStamp, reloadedForStamp } = await chrome.storage.local.get(["loadedStamp", "reloadedForStamp"]);
  if (!loadedStamp) return false;
  const now = await filesStamp();
  if (now === loadedStamp || now === reloadedForStamp) return false;
  await chrome.storage.local.set({ reloadedForStamp: now });
  chrome.runtime.reload();
  return true;
}

async function followAppVersion() {
  const last = (await chrome.storage.session.get("versionCheckAt")).versionCheckAt || 0;
  if (Date.now() - last < 60000) return;
  await chrome.storage.session.set({ versionCheckAt: Date.now() });
  try {
    if (await followOwnFiles()) return;
  } catch {
    /* fetch / crypto missing: the version check below */
  }
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/ping"), { headers: { "X-SwiftGet": "1" } }, 800);
    if (!r.ok || typeof r.json !== "function") return;
    const ping = await r.json();
    // "Repair the browser add-in" in the program: start again, clean
    const repair = Number(ping.repair) || 0;
    if (repair) {
      const { seenRepair } = await chrome.storage.local.get("seenRepair");
      if (seenRepair !== repair) {
        await chrome.storage.local.set({ seenRepair: repair });
        if (seenRepair !== undefined) {
          await chrome.storage.session.clear();
          chrome.runtime.reload();
          return;
        }
      }
    }
    const want = ping.ext;
    const have = chrome.runtime.getManifest().version;
    if (!want || want === have) return;
    // only once per version: if the files on disk did not change (the add-in
    // was loaded from somewhere else) it must not keep reloading
    const done = (await chrome.storage.local.get("reloadedFor")).reloadedFor;
    if (done === want) return;
    await chrome.storage.local.set({ reloadedFor: want });
    chrome.runtime.reload();
  } catch {
    /* the program is not running */
  }
}

// "are you there": on the known port, else on the program's own port (the
// known one may be kept by another program or by Windows); a busy computer
// may take a moment, so it is asked twice before the program is "not running"
async function appAlive() {
  const { port } = await getOpts();
  const ask = async (p, ms) => {
    try {
      const r = await plainFetch(apiUrl(p, "/ping"), { headers: { "X-SwiftGet": "1" } }, ms);
      return r.ok;
    } catch {
      return false;
    }
  };
  try {
    const h = await viaHelper("/ping", { headers: { "X-SwiftGet": "1" } }, 3000);
    if (h && h.ok) {
      followAppVersion();
      return true;
    }
  } catch {
    /* the port is asked below */
  }
  for (const ms of [1500, 4000]) {
    if (await ask(port, ms)) {
      followAppVersion();
      return true;
    }
    const l = await programLink(true);
    if (l && l.port && l.port !== port && (await ask(l.port, ms))) return true;
  }
  return false;
}
