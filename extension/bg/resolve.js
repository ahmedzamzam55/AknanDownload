// Video download, playlist forwarding, and yt-dlp / HLS quality resolution.

function partOf(c) {
  if (!c || typeof c !== "object") return null;
  const start = Number(c.start);
  const end = Number(c.end);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start || end > 7 * 24 * 3600) return null;
  return { start, end, exact: c.exact === true };
}

function isManifestUrl(u) {
  return /\.(m3u8|m3u|mpd)(\?|$)|\/hls\d*\/.*\.txt(\?|$)/i.test(u || "");
}

async function downloadVideo(option, pageTitle, pageUrl, clip, store, reqHeaders, subtitle, fallbackPage) {
  const title = cleanYoutubeTitle(option.title || pageTitle);
  const name = option.quality === "mp3" ? `${title}.mp3` : `${title} [${option.quality}].${option.ext}`;
  const srcRef = option.source && !isYoutube(option.source) && !isManifestUrl(option.source) ? option.source : "";
  const sourcePage = option.source || (!isManifestUrl(fallbackPage) ? fallbackPage : "") || null;
  await sendToApp({
    url: option.videoUrl,
    audioUrl: option.audioUrl,
    referrer: srcRef || pageUrl || "https://www.youtube.com/",
    fileName: name,
    headers: option.headers,
    reqHeaders,
    sourcePage,
    sourceKey: option.key,
    clip,
    store,
    direct: true,
    subtitle: subtitle || undefined,
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

const resolving = new Map();

async function askApp(page, referer, startApp = false, reqHeaders = null) {
  const { port } = await getOpts();
  const pairs = reqHeaders && reqHeaders.length ? reqHeaders : (browserRequest(page) || []).map((h) => [h.name, h.value]);
  const sentReferer = findHeader(pairs, "referer");
  const effRef = sentReferer || referer || null;
  const c1 = await cookieList(effRef || page);
  let cookies = c1;
  if (effRef && effRef !== page) {
    const c2 = await cookieList(page);
    if (c2.length) {
      const seen = new Set(c1.map((c) => `${c.domain}|${c.name}|${c.path}`));
      cookies = [...c1, ...c2.filter((c) => !seen.has(`${c.domain}|${c.name}|${c.path}`))];
    }
  }
  const r = await fetchWithTimeout(
    apiUrl(port, "/resolve"),
    {
      startApp,
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify({ url: page, cookieList: cookies, referer: effRef, headers: pairs }),
    },
    90000
  );
  const data = await r.json();
  return data.ok
    ? { resolved: { title: data.title, site: data.site, options: data.options, subtitles: data.subtitles || [] } }
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
  const now = await chrome.tabs.get(tabId).catch(() => null);
  if (now && videoIdOf(now.url) === videoId) await patchYt(tabId, videoId, { resolving: false, ...change });
}

async function resolvePage(tabId, page, embedded = false) {
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

async function resolveOneVideo(msg, sender) {
  const tab = sender && sender.tab;
  const frameId = (sender && sender.frameId) || 0;
  if (isTelegramStream(msg.src) || isTelegramStream(msg.resolve)) return { error: telegramText() };
  if (msg.resolve && /^https?:/i.test(msg.resolve)) {
    try {
      let rh = msg.reqHeaders;
      if ((!rh || !rh.length) && tab) {
        const { media } = await readTab(tab.id);
        const match = media.find((m) => m.url === msg.resolve);
        if (match) rh = match.reqHeaders;
      }
      const ref = (frameId && sender && sender.url) || (tab && tab.url);
      const r = await askApp(msg.resolve, ref, true, rh);
      const src = isManifestUrl(msg.resolve) ? "" : msg.resolve;
      if (r.resolved && r.resolved.options.length) return { items: optionItems(r.resolved, src, rh) };
      return { error: r.resolveError || aknanI18n.t("Can't get this video") };
    } catch (e) {
      if (await appAlive()) return { error: String((e && e.message) || e) };
      return { error: aknanI18n.t("Aknan Download is not running"), notRunning: true };
    }
  }
  if (msg.frame && tab) {
    if (frameId > 0) {
      let { media } = await readTab(tab.id);
      const hls = media.filter((m) => m.frameId === frameId && m.type === "HLS stream" && !m.master && !m.media && !m.resolvedOptions);
      if (hls.length) {
        for (const m of hls.slice(0, 3)) await readHls(tab.id, m.url).catch(() => {});
        ({ media } = await readTab(tab.id));
      }
      const mine = media.filter((m) => m.frameId === frameId);
      if (mine.length) {
        const frameItems = mediaItems(mine).filter((i) => !i.disabled);
        if (frameItems.length) return { items: frameItems };
      }
    }
    if (!msg.page && !isYoutube(tab.url)) {
      await resolvePage(tab.id, tab.url).catch(() => {});
      for (let i = 0; i < 180; i++) {
        const { res } = await readTab(tab.id);
        if (!res || !res.resolving) break;
        await new Promise((r) => setTimeout(r, 500));
      }
    }
    const { res, media } = await readTab(tab.id);
    if (!(res && res.resolved && res.resolved.options.length)) {
      const hls = media.filter((m) => m.type === "HLS stream" && !m.master && !m.media && !m.resolvedOptions && (!frameId || !m.frameId || m.frameId === frameId));
      for (const m of hls.slice(0, 3)) await readHls(tab.id, m.url).catch(() => {});
    }
    let found = (await itemsForTab(tab.id, frameId)).filter((i) => !i.disabled);
    if (found.length) return { items: found };
    const fUrl = sender && sender.url;
    if (fUrl && /^https?:/i.test(fUrl) && fUrl !== tab.url && !/geo\.dailymotion\.com\/player/i.test(fUrl)) {
      try {
        const r = await askApp(fUrl, tab.url, true);
        if (r.resolved && r.resolved.options.length) return { items: optionItems(r.resolved, fUrl) };
      } catch {}
    }
    found = (await itemsForTab(tab.id, frameId)).filter((i) => !i.disabled);
    if (found.length) return { items: found };
  }
  const items = [];
  if (msg.feed && !msg.page) {
    const found = tab ? (await itemsForTab(tab.id, frameId)).filter((i) => !i.disabled && !i.yt) : [];
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
  const found = tab ? (await itemsForTab(tab.id, frameId)).filter((i) => !i.disabled && !i.yt) : [];
  if (found.length) return { items: found };
  if (/unsupported url/i.test(problem)) return { error: aknanI18n.t("Play the video for a moment, then press the button again.") };
  if (isDrm(problem)) return { error: drmText(), drm: true };
  return { error: problem || aknanI18n.t("Can't get this video") };
}
