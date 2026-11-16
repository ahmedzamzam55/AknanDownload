// ---------------------------------------------------------------- 1. downloads decision logic

// Remembered in session storage, not in memory: the browser stops this
// service worker whenever it likes, and a lost "let the browser keep it"
// mark made the same file come back over and over.
const BYPASS_KEY = "bypassUrls";
const RECENT_KEY = "recentSends";

async function takeBypass(url) {
  const list = (await chrome.storage.session.get(BYPASS_KEY))[BYPASS_KEY] || [];
  const now = Date.now();
  const fresh = list.filter((b) => now - b.at < 120000);
  const hit = fresh.some((b) => b.url === url);
  await chrome.storage.session.set({ [BYPASS_KEY]: fresh.filter((b) => b.url !== url) });
  return hit;
}

async function addBypass(url) {
  const list = (await chrome.storage.session.get(BYPASS_KEY))[BYPASS_KEY] || [];
  list.push({ url, at: Date.now() });
  await chrome.storage.session.set({ [BYPASS_KEY]: list.slice(-50) });
}

/** The same address handed to the app a moment ago (pages and the browser can fire twice). */
async function sentRecently(url) {
  const list = (await chrome.storage.session.get(RECENT_KEY))[RECENT_KEY] || [];
  const now = Date.now();
  const fresh = list.filter((r) => now - r.at < 15000);
  const hit = fresh.some((r) => r.url === url);
  if (!hit) fresh.push({ url, at: now });
  await chrome.storage.session.set({ [RECENT_KEY]: fresh.slice(-50) });
  return hit;
}

/** The app's "Download with the browser" button: leave this one to the browser. */
async function appSaysBrowser(urls) {
  const { port } = await getOpts();
  try {
    const r = await fetchWithTimeout(
      apiUrl(port, "/browser"),
      { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify({ urls }) },
      800
    );
    if (!r.ok || typeof r.json !== "function") return false;
    return !!(await r.json()).browser;
  } catch {
    return false;
  }
}

/** Only downloads the user starts now: not ones the browser restores or resumes at start-up. */
function isNewDownload(item) {
  if (item.state && item.state !== "in_progress") return false;
  // ours (given back to the browser) and other extensions' downloads stay in the browser
  if (item.byExtensionId) return false;
  const started = Date.parse(item.startTime || "");
  if (!Number.isNaN(started) && Date.now() - started > 30000) return false;
  return true;
}

// Why the last download stayed with the browser (the toolbar window says it).
async function leftToBrowser(why, item) {
  try {
    const name = baseName(item && item.filename) || ((item && (item.finalUrl || item.url)) || "").split(/[?#]/)[0].split("/").pop() || "";
    await chrome.storage.local.set({ lastLeft: { why, name: name.slice(0, 120), at: Date.now() } });
  } catch {
    /* nothing to say it with */
  }
}

/**
 * Should Aknan Download take this browser download? `{ take, why }`: why it
 * stays with the browser (said in the toolbar window), or "" when nothing
 * needs saying (sending is off, not a new download...).
 */
async function decide(item) {
  const no = (why) => ({ take: false, why });
  const opts = await getOpts();
  if (!opts.enabled || !opts.intercept) return no("");
  const url = item.finalUrl || item.url;
  if (!url || !/^https?:/i.test(url)) return no(""); // blob:, data:, file: stay in the browser
  // Telegram's own download of a video: only its page can give the file
  if (isTelegramStream(url) || isTelegramStream(item.url)) return no("");
  if (!isNewDownload(item)) return no("");
  if (await takeBypass(url)) return no("");
  // the browser's PDF viewer saving the PDF it shows: the browser asks where
  // to save it anyway (that window can't be stopped), so it keeps it rather
  // than two windows asking at once
  if (await shownInPdfViewer(item)) return no("");
  // already handed to the app by the click on its link: the browser's copy goes
  if ((await justTaken(item.url)) || (await justTaken(url))) return { take: true, why: "", already: true };
  if (opts.minSizeKB > 0 && item.totalBytes > 0 && item.totalBytes < opts.minSizeKB * 1024) return no("small");
  // a form with a file in it can't be sent again
  const form = formFor(item.url);
  if (form === null) return no("form");
  if (!(await appAlive())) return no("not-running"); // app not running: let the browser handle it
  const cfg = await getConfig();
  if (!wanted(cfg, url, baseName(item.filename), item.mime, currentKeys())) return no("rules");
  if (await appSaysBrowser([item.url, url])) return no(""); // the user chose the browser for it
  // a link that works once only (the browser just used it) or only with
  // the page's own sign-in: it stays in the browser
  if (!(await appCanFetch(form ? item.url : url, item, form))) return no("cant-fetch");
  return { take: true, why: "", form };
}

/** Can the program get this file with the browser's sign-in? (a short look; when it can't tell, yes) */
async function appCanFetch(url, item, form) {
  const { port } = await getOpts();
  try {
    const sent = browserRequest(url, item.url);
    const pick = (n) => sent && sent.find((h) => h.name.toLowerCase() === n);
    const passOn = sent ? sent.filter((h) => !/^(cookie|referer|user-agent|range)$/i.test(h.name)).map((h) => [h.name, h.value]) : null;
    const body = {
      url,
      referrer: item.referrer || (pick("referer") && pick("referer").value) || null,
      cookies: (pick("cookie") && pick("cookie").value) || (await cookieHeader(url, await storeOf(item))),
      userAgent: (pick("user-agent") && pick("user-agent").value) || navigator.userAgent,
      headers: passOn && passOn.length ? passOn : null,
      ...(form ? form : {}),
    };
    const r = await fetchWithTimeout(
      apiUrl(port, "/check"),
      { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify(body) },
      3200
    );
    if (!r.ok || typeof r.json !== "function") return true;
    return (await r.json()).ok !== false;
  } catch {
    return true;
  }
}

/** A PDF that a tab shows in the browser's own viewer right now. */
async function shownInPdfViewer(item) {
  const pdf = /pdf/i.test(item.mime || "") || /\.pdf($|[?#])/i.test(item.finalUrl || item.url || "");
  if (!pdf) return false;
  try {
    const tabs = await chrome.tabs.query({});
    const same = (a, b) => !!a && !!b && a.split("#")[0] === b.split("#")[0];
    return tabs.some((t) => same(t.url, item.url) || same(t.url, item.finalUrl));
  } catch {
    return false;
  }
}
