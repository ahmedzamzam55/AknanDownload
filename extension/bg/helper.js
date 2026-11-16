// Options, endpoint helpers, and native messaging helper bridge.
const DEFAULTS = { port: 9614, intercept: true, minSizeKB: 0, enabled: true };

async function getOpts() {
  return Object.assign({}, DEFAULTS, await chrome.storage.local.get(DEFAULTS));
}

function apiUrl(port, path) {
  return `http://127.0.0.1:${port}${path}`;
}

// which add-in asks (its version and browser): the program tells the user
// when the Firefox add-on is older than the program and must be updated
// (Firefox keeps its own copy of the add-on)
const IS_FIREFOX = typeof navigator !== "undefined" && /Firefox\//.test(navigator.userAgent || "");
const ADDIN_HEADERS = { "X-Aknan-Ext": chrome.runtime.getManifest().version, "X-Aknan-Browser": IS_FIREFOX ? "firefox" : "chromium" };
// who this add-in is: the program lets it start its helper (see below)
if (!IS_FIREFOX && chrome.runtime.id) ADDIN_HEADERS["X-Aknan-Id"] = chrome.runtime.id;

// how the connection to the program is (for the add-in's window)
var linkState = "unknown"; // "ok" | "nokey" | "down"

async function plainFetch(url, opts = {}, ms = 1500) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), ms);
  try {
    if (/^http:\/\/127\.0\.0\.1:/.test(url)) opts = { ...opts, headers: Object.assign({}, ADDIN_HEADERS, opts.headers || {}) };
    return await fetch(url, { ...opts, signal: ctrl.signal });
  } finally {
    clearTimeout(timer);
  }
}

// ---------------------------------------------------------------- the program's helper
// The program puts a small helper of its own on this computer and tells the
// browser about it ("native messaging"): the browser starts it and the add-in
// talks to it through the browser itself, not over the network. Nothing the
// browser does about pages reaching this computer, and no port kept by
// another program or by Windows, stands in the way. Without the helper (an
// older program, Firefox before the user allowed it) the port is used, as
// before; when the helper can't reach the program, the port is tried too.
const HOST = "com.aknan.download";
// asked for by the user (a download, the program's window): the helper may
// open the program when it is closed
const START_PATHS = new Set(["/add", "/links", "/playlist", "/show", "/open-torrent"]);
let helper = null; // { port, pending, heard }
let helperOffUntil = 0;
let helperSeq = 0;
let helperQuiet = null;

async function helperAllowed() {
  if (Date.now() < helperOffUntil) return false;
  if (IS_FIREFOX) {
    // Firefox: only once the user said yes in the add-in's window
    try {
      if (!(await chrome.permissions.contains({ permissions: ["nativeMessaging"] }))) return false;
    } catch {
      return false;
    }
  }
  return typeof chrome.runtime.connectNative === "function";
}

function helperOpen() {
  if (helper) return helper;
  let port;
  try {
    port = chrome.runtime.connectNative(HOST);
  } catch {
    helperOffUntil = Date.now() + 60000;
    return null;
  }
  const me = { port, pending: new Map(), heard: false };
  port.onMessage.addListener((m) => {
    me.heard = true;
    const p = m && me.pending.get(m.id);
    if (!p) return;
    me.pending.delete(m.id);
    clearTimeout(p.timer);
    p.resolve(m);
  });
  port.onDisconnect.addListener(() => {
    void (chrome.runtime && chrome.runtime.lastError);
    if (helper === me) helper = null;
    // never said a word: there is no helper on this computer (yet)
    if (!me.heard) helperOffUntil = Date.now() + 60000;
    for (const p of me.pending.values()) {
      clearTimeout(p.timer);
      p.resolve(null);
    }
    me.pending.clear();
  });
  helper = me;
  return me;
}

/** After a quiet while the helper is let go (it ends; the browser may rest). */
function helperRestLater() {
  clearTimeout(helperQuiet);
  helperQuiet = setTimeout(() => {
    if (!helper) return;
    if (helper.pending.size) return helperRestLater();
    try {
      helper.port.disconnect();
    } catch {
      /* gone already */
    }
    helper = null;
  }, 120000);
}

function helperAsk(msg, wait) {
  return new Promise((resolve) => {
    const h = helperOpen();
    if (!h) return resolve(null);
    const id = ++helperSeq;
    const timer = setTimeout(() => {
      h.pending.delete(id);
      resolve({ id, error: "timeout" });
    }, wait + 2000);
    h.pending.set(id, { resolve, timer });
    try {
      h.port.postMessage({ ...msg, id });
    } catch {
      h.pending.delete(id);
      clearTimeout(timer);
      resolve(null);
    }
    helperRestLater();
  });
}

function helperResponse(status, body) {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => JSON.parse(body || "null"),
    text: async () => body || "",
  };
}

/** A request through the helper; null = not this way (the port is tried). */
async function viaHelper(path, opts, ms) {
  if (path === "/pair" || path === "/quit" || !(await helperAllowed())) return null;
  const start = START_PATHS.has(path) || !!opts.startApp;
  const m = await helperAsk(
    {
      method: (opts.method || "GET").toUpperCase(),
      path,
      headers: Object.assign({}, ADDIN_HEADERS, opts.headers || {}),
      body: typeof opts.body === "string" ? opts.body : null,
      ms,
      start,
    },
    // opening the program takes a few seconds
    start ? Math.max(ms, 30000) : ms
  );
  if (!m) return null;
  // the program said no to the helper's key: the add-in's own way is tried
  // (never a dead end because of the helper)
  if (m.status === 401) return null;
  if (typeof m.status === "number") {
    linkState = "ok";
    return helperResponse(m.status, m.body);
  }
  if (m.error === "timeout") {
    const e = new Error("Aknan Download took too long to answer");
    e.name = "AbortError";
    throw e;
  }
  // "down" (the program is closed), "big" (an answer too big for the helper)
  return null;
}
