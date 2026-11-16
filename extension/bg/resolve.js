// Video download, playlist forwarding, and yt-dlp quality resolution.

// a part of a video as the page sent it: two times in seconds, the end
// after the start (else nothing)
function partOf(c) {
  if (!c || typeof c !== "object") return null;
  const start = Number(c.start);
  const end = Number(c.end);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start || end > 7 * 24 * 3600) return null;
  return { start, end, exact: c.exact === true };
}

async function downloadVideo(option, pageTitle, pageUrl, clip, store) {
  const title = cleanYoutubeTitle(option.title || pageTitle);
  const name = option.quality === "mp3" ? `${title}.mp3` : `${title} [${option.quality}].${option.ext}`;
  await sendToApp({
    url: option.videoUrl,
    audioUrl: option.audioUrl,
    referrer: option.source && !isYoutube(option.source) ? option.source : pageUrl || "https://www.youtube.com/",
    fileName: name,
    headers: option.headers,
    sourcePage: option.source,
    sourceKey: option.key,
    clip,
    store,
  });
}

async function sendPlaylist(url) {
  if (!/^https?:/i.test(url || "")) throw new Error("bad address");
  const { port } = await getOpts();
  const r = await fetchWithTimeout(
    apiUrl(port, "/playlist"),
    { method: "POST", headers: { "Content-Type": "application/json", "X-SwiftGet": "1" }, body: JSON.stringify({ url }) },
    5000
  );
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
}

// Ask the app (yt-dlp) for the qualities of a video page.
const resolving = new Map(); // tabId -> page being resolved

async function askApp(page, referer, startApp = false) {
  const { port } = await getOpts();
  const r = await fetchWithTimeout(
    apiUrl(port, "/resolve"),
    {
      startApp,
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify({ url: page, cookieList: await cookieList(referer || page), referer: referer || null }),
    },
    90000
  );
  const data = await r.json();
  return data.ok
    ? { resolved: { title: data.title, site: data.site, options: data.options } }
    : { resolveError: data.error || "unknown error" };
}

async function patchYt(tabId, videoId, change) {
  const key = `yt_${tabId}`;
  let state = (await chrome.storage.session.get(key))[key];
  if (!state || state.videoId !== videoId) state = { videoId, sabr: false, streams: {} };
  Object.assign(state, change);
  await chrome.storage.session.set({ [key]: state });
  pushToTab(tabId);
}

async function resolveYoutube(tabId, pageUrl) {
  const videoId = videoIdOf(pageUrl);
  if (!videoId || !/(^|\.)youtube\.com$/i.test(new URL(pageUrl).hostname)) return;
  const key = `yt_${tabId}`;
  const state = (await chrome.storage.session.get(key))[key];
  if (state && state.videoId === videoId && (state.resolved || state.resolveError)) return;
  if (resolving.get(tabId) === videoId) return;
  resolving.set(tabId, videoId);
  await patchYt(tabId, videoId, { resolving: true, resolveError: null });
  let change;
  try {
    change = await askApp(`https://www.youtube.com/watch?v=${videoId}`);
  } catch (e) {
    change = { resolveError: (await appAlive()) ? String((e && e.message) || e) : aknanI18n.t("Aknan Download is not running") };
  } finally {
    if (resolving.get(tabId) === videoId) resolving.delete(tabId);
  }
  // the user may have moved on to another video meanwhile
  const now = await chrome.tabs.get(tabId).catch(() => null);
  if (now && videoIdOf(now.url) === videoId) await patchYt(tabId, videoId, { resolving: false, ...change });
}

/**
 * Any other site: the page (or the post / reel / video link next to the
 * video the user is watching) is handed to yt-dlp, which knows Facebook,
 * Instagram, TikTok, X, Vimeo, Dailymotion and ~1800 more sites.
 */
async function resolvePage(tabId, page, embedded = false) {
  // YouTube's own pages have their own way (resolveYoutube); a YouTube
  // player embedded in another site goes this way
  if (!page || !/^https?:/i.test(page) || (isYoutube(page) && !embedded)) return;
  const key = `res_${tabId}`;
  const state = (await chrome.storage.session.get(key))[key];
  if (state && state.page === page && (state.resolved || state.resolving || state.resolveError)) return;
  if (resolving.get(tabId) === page) return;
  resolving.set(tabId, page);
  await chrome.storage.session.set({ [key]: { page, resolving: true } });
  pushToTab(tabId);
  let change;
  try {
    change = await askApp(page);
  } catch (e) {
    change = { resolveError: (await appAlive()) ? String((e && e.message) || e) : aknanI18n.t("Aknan Download is not running") };
  } finally {
    if (resolving.get(tabId) === page) resolving.delete(tabId);
  }
  const now = (await chrome.storage.session.get(key))[key];
  if (now && now.page === page) {
    await chrome.storage.session.set({ [key]: { page, resolving: false, ...change } });
    pushToTab(tabId);
  }
}

/** The bubble of a video that is not the page's main one was clicked. */
async function resolveOneVideo(msg, tab) {
  if (isTelegramStream(msg.src) || isTelegramStream(msg.resolve)) return { error: telegramText() };
  // a DASH stream of the page: its own qualities
  if (msg.resolve && /^https?:/i.test(msg.resolve)) {
    try {
      // the user asked (a player's own menu): a closed program is opened
      const r = await askApp(msg.resolve, tab && tab.url, true);
      if (r.resolved && r.resolved.options.length) return { items: optionItems(r.resolved, msg.resolve) };
      return { error: r.resolveError || aknanI18n.t("Can't get this video") };
    } catch (e) {
      if (await appAlive()) return { error: String((e && e.message) || e) };
      return { error: aknanI18n.t("Aknan Download is not running"), notRunning: true };
    }
  }
  // a player in a frame (Dailymotion, news sites...): what was found for the
  // whole page (its qualities, its streams) is what this player plays
  if (msg.frame && tab) {
    if (!msg.page && !isYoutube(tab.url)) {
      await resolvePage(tab.id, tab.url).catch(() => {});
      // it may have been started already (the player was seen): wait for it
      for (let i = 0; i < 180; i++) {
        const { res } = await readTab(tab.id);
        if (!res || !res.resolving) break;
        await new Promise((r) => setTimeout(r, 500));
      }
    }
    const found = (await itemsForTab(tab.id)).filter((i) => !i.disabled);
    if (found.length) return { items: found };
  }
  const items = [];
  if (msg.feed && !msg.page) {
    // Facebook feed and no link next to this video: the page's own streams,
    // else how to get the video's own page
    const found = tab ? (await itemsForTab(tab.id)).filter((i) => !i.disabled && !i.yt) : [];
    if (found.length) return { items: found };
    return { error: aknanI18n.t("Open the video first (click the time next to the name above the video), then press the button again.") };
  }
  if (msg.src && /^https?:/i.test(msg.src)) {
    items.push({ label: aknanI18n.t("Video file"), sub: msg.src, url: msg.src, ext: (/\.(\w{2,4})(\?|$)/.exec(msg.src) || [])[1] || "mp4" });
  }
  const page = msg.page || (msg.src && !msg.frame ? "" : tab && tab.url);
  let problem = "";
  if (page && /^https?:/i.test(page)) {
    try {
      const r = await askApp(page);
      if (r.resolved && r.resolved.options.length) items.unshift(...optionItems(r.resolved, page));
      else problem = r.resolveError || aknanI18n.t("Can't get this video");
    } catch (e) {
      if (!(await appAlive())) return { error: aknanI18n.t("Aknan Download is not running"), notRunning: true };
      problem = String((e && e.message) || e);
    }
  }
  if (items.length) return { items };
  // the page is nothing the app knows (a player of its own): the streams the
  // player opened are this video
  const found = tab ? (await itemsForTab(tab.id)).filter((i) => !i.disabled && !i.yt) : [];
  if (found.length) return { items: found };
  if (/unsupported url/i.test(problem)) return { error: aknanI18n.t("Play the video for a moment, then press the button again.") };
  if (isDrm(problem)) return { error: drmText(), drm: true };
  return { error: problem || aknanI18n.t("Can't get this video") };
}
