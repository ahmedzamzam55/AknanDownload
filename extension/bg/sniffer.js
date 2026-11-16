// ---------------------------------------------------------------- 2. media & YouTube stream sniffer

const MEDIA_TYPE = /^(video|audio)\//i;
const HLS_TYPE = /mpegurl/i;
const DASH_TYPE = /dash\+xml/i;
const DISGUISED_HLS = /\/hls\d*\/.*\.(txt|m3u)(\?|$)/i;

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
  current.push(item);
  while (current.length > 20) current.shift();
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
  if (item.type === "HLS stream") readHls(tabId, item.url).catch(() => {});
}

function onMediaHeadersReceived(d) {
  if (d.tabId < 0 || d.statusCode < 200 || d.statusCode >= 300) return;
  const url = d.url;
  if (!/^https?:/i.test(url)) return;
  if (/googlevideo\.com/i.test(url)) return;
  if (isTelegramStream(url)) return;
  const h = headerMap(d.responseHeaders);
  const type = (h["content-type"] || "").toLowerCase();
  const isHls = HLS_TYPE.test(type) || /\.m3u8(\?|$)/i.test(url) || DISGUISED_HLS.test(url);
  const isDash = DASH_TYPE.test(type) || /\.mpd(\?|$)/i.test(url);
  const isMedia = MEDIA_TYPE.test(type) && !type.includes("mp2t");
  if (!isHls && !isDash && !isMedia) return;
  const reqHeaders = captureRequestHeaders(d);
  const frameId = d.frameId || 0;
  const frameOrigin = originOf(d.documentUrl || d.initiator || "");
  const seenAt = Date.now();
  if (isDash) {
    addMedia(d.tabId, { url, type: "DASH stream", size: 0, ext: "mp4", dash: true, reqHeaders, frameId, frameOrigin, seenAt });
    return;
  }
  if (/[?&](range|bytestart)=/i.test(url)) return;

  let size = 0;
  const cr = /\/(\d+)\s*$/.exec(h["content-range"] || "");
  if (cr) size = Number(cr[1]);
  else if (h["content-length"]) size = Number(h["content-length"]);
  if (isMedia && size > 0 && size < 300 * 1024) return;

  addMedia(d.tabId, {
    url,
    type: isHls ? "HLS stream" : type.split(";")[0],
    size,
    ext: isHls ? "ts" : (type.split("/")[1] || "mp4").split(";")[0].replace("x-matroska", "mkv").replace("mpeg", "mp3"),
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
