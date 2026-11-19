// ---------------------------------------------------------------- 2. media & YouTube stream sniffer

const MEDIA_TYPE = /^(video|audio)\//i;
const HLS_TYPE = /mpegurl/i;
const DASH_TYPE = /dash\+xml/i;
const DISGUISED_HLS = /\/hls\d*\/.*\.(txt|m3u)(\?|$)/i;
const AD_HOSTS = /(^|\.)(doubleclick\.net|googlesyndication\.com|imasdk\.googleapis\.com|ahmybid\.net|vidwestxyz\.com|exoclick\.com|juicyads\.com|popads\.net|adnxs\.com|tsyndicate\.com|hilltopads\.net)$/i;

function isAdMediaUrl(u) {
  try {
    return AD_HOSTS.test(new URL(u).hostname);
  } catch {
    return false;
  }
}

function headerMap(list) {
  const out = {};
  for (const h of list || []) out[h.name.toLowerCase()] = h.value || "";
  return out;
}

async function addMedia(tabId, item) {
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  const prev = current.find((m) => m.url === item.url);
  if (prev) {
    let changed = false;
    if (item.reqHeaders && item.reqHeaders.length) {
      prev.reqHeaders = item.reqHeaders;
      changed = true;
    }
    if (item.frameId) prev.frameId = item.frameId;
    if (item.frameOrigin) prev.frameOrigin = item.frameOrigin;
    if (item.seenAt) prev.seenAt = item.seenAt;
    if (changed) await chrome.storage.session.set({ [key]: current });
    return;
  }
  if (item.type === "HLS stream") {
    const isChild = current.some(
      (m) =>
        (m.master && m.master.variants.some((v) => sameStreamPath(v.url, item.url))) ||
        (m.resolvedOptions && m.resolvedOptions.options && m.resolvedOptions.options.some((o) => sameStreamPath(o.videoUrl, item.url)))
    );
    if (isChild) item.child = true;
  }
  current.push(item);
  while (current.length > 20) current.shift();
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
  if (item.type === "HLS stream" && !item.child) readHls(tabId, item.url).catch(() => {});
}

async function addSubTrack(tabId, track) {
  const key = `subs_${tabId}`;
  const list = (await chrome.storage.session.get(key))[key] || [];
  const norm = streamPathKey(track.url);
  if (list.some((s) => s.url === track.url || (s.lang === track.lang && !!s.auto === !!track.auto && streamPathKey(s.url) === norm))) return;
  const next = [...list, track];
  if (next.length > 40) next.shift();
  await chrome.storage.session.set({ [key]: next });
  pushToTab(tabId);
}

const activeHlsHosts = new Map();

function markHlsHost(tabId, url) {
  try {
    const h = new URL(url).hostname.toLowerCase();
    let set = activeHlsHosts.get(tabId);
    if (!set) activeHlsHosts.set(tabId, (set = new Set()));
    set.add(h);
  } catch {}
}

function hasHlsHost(tabId, url) {
  try {
    const set = activeHlsHosts.get(tabId);
    return !!(set && set.has(new URL(url).hostname.toLowerCase()));
  } catch {
    return false;
  }
}

function onMediaHeadersReceived(d) {
  if (d.tabId < 0 || ((d.statusCode < 200 || d.statusCode >= 300) && d.statusCode !== 304)) return;
  const url = d.url;
  if (!/^https?:/i.test(url)) return;
  if (/googlevideo\.com/i.test(url)) return;
  if (isTelegramStream(url) || isAdMediaUrl(url)) return;
  const h = headerMap(d.responseHeaders);
  const type = (h["content-type"] || "").toLowerCase();
  const reqHeaders = captureRequestHeaders(d);
  const frameId = d.frameId || 0;
  const frameOrigin = originOf(d.documentUrl || d.initiator || "");
  const seenAt = Date.now();
  const subTrack = guessSubTrack(url, type);
  if (subTrack) {
    addSubTrack(d.tabId, { ...subTrack, reqHeaders, frameId, frameOrigin, seenAt });
    return;
  }
  const isHls = HLS_TYPE.test(type) || /\.m3u8(\?|$)/i.test(url) || DISGUISED_HLS.test(url);
  const isDash = DASH_TYPE.test(type) || /\.mpd(\?|$)/i.test(url);
  const isTs = type.includes("mp2t") || /\.ts(\?|$)/i.test(url);
  let size = 0;
  const cr = /\/(\d+)\s*$/.exec(h["content-range"] || "");
  if (cr) size = Number(cr[1]);
  else if (h["content-length"]) size = Number(h["content-length"]);
  const isStandaloneTs = isTs && !isHls && size >= 5 * 1024 * 1024 && !hasHlsHost(d.tabId, url);
  const isMedia = (MEDIA_TYPE.test(type) && !type.includes("mp2t")) || isStandaloneTs;
  if (!isHls && !isDash && !isMedia) return;
  if (isHls) markHlsHost(d.tabId, url);
  if (isDash) {
    addMedia(d.tabId, { url, type: "DASH stream", size: 0, ext: "mp4", dash: true, reqHeaders, frameId, frameOrigin, seenAt });
    sniffDashSubs(d.tabId, url, reqHeaders).catch(() => {});
    return;
  }
  if (/[?&](range|bytestart)=/i.test(url)) return;
  if (isMedia && size > 0 && size < 300 * 1024) return;

  addMedia(d.tabId, {
    url,
    type: isHls ? "HLS stream" : isStandaloneTs ? "video/mp2t" : type.split(";")[0],
    size,
    ext: isHls || isStandaloneTs ? "ts" : (type.split("/")[1] || "mp4").split(";")[0].replace("x-matroska", "mkv").replace("mpeg", "mp3"),
    reqHeaders,
    frameId,
    frameOrigin,
    seenAt,
  });
}

// ---------------------------------------------------------------- 3. YouTube

const seenStreams = new Map();

async function addYoutubeStream(tabId, stream) {
  const key = `yt_${tabId}`;
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  const videoId = tab ? videoIdOf(tab.url) : null;
  const state = (await chrome.storage.session.get(key))[key] || { videoId, sabr: false, streams: {} };
  if (videoId && state.videoId && state.videoId !== videoId) {
    state.videoId = videoId;
    state.streams = {};
    state.sabr = false;
    seenStreams.delete(tabId);
  }
  state.videoId = state.videoId || videoId;
  if (stream.sabr) {
    if (state.sabr) return;
    state.sabr = true;
  } else {
    const sk = `${stream.itag}_${stream.dur}${stream.lang ? "_" + stream.lang : ""}`;
    state.streams[sk] = stream;
    const ids = Object.keys(state.streams);
    if (ids.length > 60) delete state.streams[ids[0]];
  }
  await chrome.storage.session.set({ [key]: state });
  pushToTab(tabId);
}

function onYoutubeBeforeRequest(d) {
  if (d.tabId < 0) return;
  const stream = parseStream(d.url);
  if (!stream) return;
  const sk = stream.sabr ? "sabr" : `${stream.itag}_${stream.dur}${stream.lang ? "_" + stream.lang : ""}`;
  let seen = seenStreams.get(d.tabId);
  if (!seen) seenStreams.set(d.tabId, (seen = new Set()));
  if (seen.has(sk)) return;
  seen.add(sk);
  addYoutubeStream(d.tabId, stream);
}
