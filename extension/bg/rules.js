// ---------------------------------------------------------------- app options & capture rules

let appConfig = null;
let configAt = 0;

/** Options > File Types / General of the app (cached for a minute). */
async function getConfig() {
  if (appConfig && Date.now() - configAt < 60000) return appConfig;
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 1000);
    if (r.ok) {
      appConfig = await r.json();
      configAt = Date.now();
      const nextLang = appConfig && appConfig.language ? String(appConfig.language).slice(0, 2).toLowerCase() : "";
      if (nextLang && aknanI18n.LANGS.includes(nextLang)) {
        const prevLang = aknanI18n.lang();
        aknanI18n.setLang(nextLang);
        try {
          Promise.resolve(chrome.storage.local.set({ appLang: nextLang })).catch(() => {});
        } catch {}
        if (nextLang !== prevLang) makeMenus();
      }
    }
  } catch {
    /* app not running or older version: keep the last one */
  }
  return appConfig;
}

/** "*" matches anything, "?" one character (case-insensitive). */
function wildcard(pattern, text) {
  const re = new RegExp(
    "^" + pattern.toLowerCase().replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$"
  );
  return re.test(text.toLowerCase());
}

function hostMatches(host, pattern) {
  const p = pattern.replace(/^https?:\/\//i, "").split("/")[0].toLowerCase();
  if (!p) return false;
  if (p.includes("*")) return wildcard(p, host);
  return host === p || host.endsWith("." + p);
}

function extensionOf(name) {
  const clean = (name || "").split(/[?#]/)[0];
  const last = clean.split(/[\\/]/).pop() || "";
  const dot = last.lastIndexOf(".");
  return dot > 0 ? last.slice(dot + 1).toLowerCase() : "";
}

/** Should Aknan Download take this browser download? (Options > File Types rules) */
function wanted(cfg, url, fileName, mime, keys) {
  if (!cfg) return true; // old app: take everything like before
  if (cfg.skipKey && cfg.skipKey !== "none" && keys[cfg.skipKey]) return false;
  if (cfg.forceKey && cfg.forceKey !== "none" && keys[cfg.forceKey]) return true;
  let host = "";
  try {
    host = new URL(url).hostname.toLowerCase();
  } catch {
    return false;
  }
  if ((cfg.skipSites || []).some((s) => hostMatches(host, s))) return false;
  if ((cfg.skipAddresses || []).some((a) => wildcard(a, url))) return false;
  // a .torrent file: only when Aknan Download is the main torrent program
  const isTorrent = extensionOf(fileName) === "torrent" || extensionOf(new URL(url).pathname) === "torrent" || /bittorrent/i.test(mime || "");
  if (isTorrent) return cfg.torrents !== false;
  const exts = cfg.captureExts || [];
  if (!exts.length) return true;
  let ext = extensionOf(fileName) || extensionOf(new URL(url).pathname);
  // script addresses (download.php?id=…) say nothing about the file
  if (/^(php\d?|aspx?|jsp|cgi|pl|cfm|do|action|html?)$/.test(ext) && !extensionOf(fileName)) ext = "";
  if (!ext) {
    // no extension (download.php?id=…): go by the type the server sends
    return !!mime && !/^(text\/html|image\/|text\/plain)/i.test(mime);
  }
  if (exts.some((e) => wildcard(e, ext))) return true;
  // an extension not in the list, but the server says it is a file to
  // download (not a page, picture or text the browser shows)
  return !!mime && /^(application\/(octet-stream|x-|zip|vnd\.|pdf|java-archive|gzip)|video\/|audio\/)/i.test(mime) && !/^(html?|xhtml|php\d?|aspx?|jsp|json|xml|js|css|txt)$/.test(ext);
}

// keys held when the user clicked the link (sent by the page)
let lastKeys = { alt: false, ctrl: false, shift: false, at: 0 };
function currentKeys() {
  return Date.now() - lastKeys.at < 4000 ? lastKeys : { alt: false, ctrl: false, shift: false };
}

function baseName(path) {
  if (!path) return null;
  const name = path.split(/[\\/]/).pop();
  return name || null;
}
