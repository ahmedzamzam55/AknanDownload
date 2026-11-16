// ---------------------------------------------------------------- the program's key
// The program listens on two ports: the known one (Options, 9614) answers
// only "are you there" and pairing; the real one is chosen at random on
// this computer. Every request to it is signed with the program's secret
// key (the key itself is never sent, and a signed request can't be sent
// again). Loaded from the program's folder, the add-in finds the key and the
// port there (token.json); otherwise (Firefox, another folder) it asks for
// them once and the user says yes in a window of the program.
let link = null; // { token, port }
const FRONT_PATHS = new Set(["/ping", "/pair", "/show", "/quit"]);

async function fromFolder() {
  try {
    const r = await fetch(chrome.runtime.getURL("token.json"), { cache: "no-store" });
    if (!r.ok) return { has: false };
    const j = await r.json();
    return { has: true, token: j.token || "", port: Number(j.port) || 0 };
  } catch {
    return { has: false };
  }
}

/** The key and the real port, or null. */
async function programLink(fresh) {
  if (link && !fresh) return link;
  const f = await fromFolder();
  if (f.has && f.token && f.port) return (link = { token: f.token, port: f.port, folder: true });
  const { appToken, appPort } = await chrome.storage.local.get(["appToken", "appPort"]);
  if (appToken && appPort) return (link = { token: appToken, port: Number(appPort), folder: false });
  link = null;
  return null;
}

let pairing = null;
async function askForKey(frontPort) {
  if (!pairing) {
    pairing = (async () => {
      try {
        // the user has up to two minutes to answer in the program
        const r = await plainFetch(apiUrl(frontPort, "/pair"), { method: "POST", headers: { "X-SwiftGet": "1" } }, 125000);
        if (!r.ok) return null;
        const j = await r.json();
        if (j.token && j.port) {
          await chrome.storage.local.set({ appToken: j.token, appPort: j.port });
          link = { token: j.token, port: Number(j.port), folder: false };
          return link;
        }
        return null;
      } catch {
        return null;
      } finally {
        setTimeout(() => (pairing = null), 1000);
      }
    })();
  }
  return pairing;
}

const enc = new TextEncoder();
const hex = (buf) => [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, "0")).join("");
async function signHeaders(token, method, path) {
  const time = String(Date.now());
  const nonce = hex(crypto.getRandomValues(new Uint8Array(16)));
  const key = await crypto.subtle.importKey("raw", enc.encode(token), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = hex(await crypto.subtle.sign("HMAC", key, enc.encode(`${method}\n${path}\n${time}\n${nonce}`)));
  return { "X-Aknan-Time": time, "X-Aknan-Nonce": nonce, "X-Aknan-Sig": sig };
}

/** The real port moved (it was taken at the program's start): ask where. */
async function whereNow(frontPort, l) {
  try {
    const r = await plainFetch(apiUrl(frontPort, "/where"), { headers: { "X-SwiftGet": "1", ...(await signHeaders(l.token, "GET", "/where")) } }, 1500);
    if (!r.ok) return null;
    const port = Number((await r.json()).port) || 0;
    if (!port) return null;
    if (!l.folder) await chrome.storage.local.set({ appPort: port });
    return (link = { ...l, port });
  } catch {
    return null;
  }
}

/** A request; to the program it goes to the real port, signed. */
async function fetchWithTimeout(url, opts = {}, ms = 1500) {
  const m = /^http:\/\/127\.0\.0\.1:(\d+)(\/[^?#]*)/.exec(url);
  if (!m) return plainFetch(url, opts, ms);
  const frontPort = Number(m[1]);
  const path = m[2];
  const helped = await viaHelper(path, opts, ms);
  if (helped) return helped;
  if (FRONT_PATHS.has(path)) return plainFetch(url, opts, ms);
  const method = (opts.method || "GET").toUpperCase();
  const send = async (l) => {
    const headers = Object.assign({}, opts.headers || {}, await signHeaders(l.token, method, path));
    return plainFetch(`http://127.0.0.1:${l.port}${path}`, { ...opts, headers }, ms);
  };
  let l = await programLink();
  if (!l) {
    // from the program's folder, the key may only be a moment late
    const f = await fromFolder();
    if (f.has) {
      await new Promise((r) => setTimeout(r, 1500));
      l = await programLink(true);
    }
    if (!l && !f.has) l = await askForKey(frontPort);
    if (!l) {
      linkState = "nokey";
      throw new TypeError("not connected to Aknan Download");
    }
  }
  let r;
  try {
    r = await send(l);
  } catch (e) {
    if (e && e.name === "AbortError") throw e;
    // the real port did not answer: it may have moved
    const again = (await programLink(true)) || l;
    const moved = again.port !== l.port ? again : await whereNow(frontPort, again);
    if (!moved) {
      linkState = "down";
      throw e;
    }
    r = await send(moved);
  }
  if (r.status === 401) {
    // a new key (the program was set up again): read it again, else ask
    const f = await fromFolder();
    let again = await programLink(true);
    if (!again || (again.token === l.token && !f.has)) {
      await chrome.storage.local.remove(["appToken", "appPort"]);
      again = f.has ? null : await askForKey(frontPort);
    }
    if (again && again.token !== l.token) r = await send(again);
  }
  linkState = r.status === 401 ? "nokey" : "ok";
  return r;
}

/** The add-in's window: "Connect again" (forget the key and the port, look again). */
async function reconnect() {
  link = null;
  linkState = "unknown";
  const f = await fromFolder();
  if (!f.has) await chrome.storage.local.remove(["appToken", "appPort"]);
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(apiUrl(port, "/config"), { headers: { "X-SwiftGet": "1" } }, 3000);
    return r.ok;
  } catch {
    return false;
  }
}
