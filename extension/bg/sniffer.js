// ---------------------------------------------------------------- 2. media & YouTube stream sniffer

const MEDIA_TYPE = /^(video|audio)\//i;
const HLS_TYPE = /mpegurl/i;
const DASH_TYPE = /dash\+xml/i;

function headerMap(list) {
  const out = {};
  for (const h of list || []) out[h.name.toLowerCase()] = h.value || "";
  return out;
}

async function addMedia(tabId, item) {
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  if (current.some((m) => m.url === item.url)) return;
  current.push(item);
  while (current.length > 15) current.shift();
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
  if (item.type === "HLS stream") readHls(tabId, item.url).catch(() => {});
}

/**
 * An HLS stream: its playlist says which qualities it has (a "master" list
 * of one list per quality). Read once, so the button shows "1080p", "720p"...
 * instead of anonymous streams.
 */
function parseHlsMaster(text, base) {
  if (!/#EXT-X-STREAM-INF/i.test(text)) return null;
  const lines = text.split(/\r?\n/).map((l) => l.trim());
  const variants = [];
  // separate sound lists: a quality list alone would be without sound
  const separateAudio = lines.some((l) => /^#EXT-X-MEDIA:.*TYPE=AUDIO.*URI=/i.test(l));
  for (let i = 0; i < lines.length; i++) {
    if (!/^#EXT-X-STREAM-INF:/i.test(lines[i])) continue;
    let uri = "";
    for (let j = i + 1; j < lines.length; j++) {
      if (lines[j] && !lines[j].startsWith("#")) {
        uri = lines[j];
        break;
      }
    }
    if (!uri) continue;
    let url = "";
    try {
      url = new URL(uri, base).toString();
    } catch {
      continue;
    }
    const res = /RESOLUTION=(\d+)x(\d+)/i.exec(lines[i]);
    const bw = /[^-]BANDWIDTH=(\d+)/i.exec("," + lines[i]);
    variants.push({ url, height: res ? Number(res[2]) : 0, width: res ? Number(res[1]) : 0, bw: bw ? Number(bw[1]) : 0 });
  }
  if (!variants.length) return null;
  // one entry per quality, the best first
  variants.sort((a, b) => b.height - a.height || b.bw - a.bw);
  const seen = new Set();
  const unique = variants.filter((v) => {
    const k = v.height ? `h${v.height}` : `b${v.bw}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
  return { variants: unique, separateAudio };
}

async function readHls(tabId, url) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 8000);
  let text = "";
  try {
    const r = await fetch(url, { credentials: "include", signal: ctl.signal });
    if (!r.ok) return;
    text = (await r.text()).slice(0, 512 * 1024);
  } finally {
    clearTimeout(timer);
  }
  if (!/^\s*#EXTM3U/.test(text)) return;
  const master = parseHlsMaster(text, url);
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  const me = current.find((m) => m.url === url);
  if (!me) return;
  if (master) {
    me.master = master;
    // the quality lists the player opened by itself: shown under the master
    const inside = new Set(master.variants.map((v) => v.url));
    for (const m of current) if (m !== me && inside.has(m.url)) m.child = true;
  } else {
    me.media = true;
    if (current.some((m) => m.master && m.master.variants.some((v) => v.url === url))) me.child = true;
  }
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
}

/** "1080p" (or the bit rate when the list does not say the size). */
function hlsLabel(v) {
  if (v.height) return `${v.height}p`;
  if (v.bw) return `${Math.round(v.bw / 1000)} kbps`;
  return "Video";
}

function onMediaHeadersReceived(d) {
  if (d.tabId < 0 || d.statusCode < 200 || d.statusCode >= 300) return;
  const url = d.url;
  if (!/^https?:/i.test(url)) return;
  if (/googlevideo\.com/i.test(url)) return; // YouTube has its own handler below
  if (isTelegramStream(url)) return; // only Telegram's own page can play it
  const h = headerMap(d.responseHeaders);
  const type = (h["content-type"] || "").toLowerCase();
  const isHls = HLS_TYPE.test(type) || /\.m3u8(\?|$)/i.test(url);
  // DASH: a list of the parts of a video in several qualities (.mpd)
  const isDash = DASH_TYPE.test(type) || /\.mpd(\?|$)/i.test(url);
  const isMedia = MEDIA_TYPE.test(type) && !type.includes("mp2t"); // skip HLS chunks
  if (!isHls && !isDash && !isMedia) return;
  if (isDash) {
    addMedia(d.tabId, { url, type: "DASH stream", size: 0, ext: "mp4", dash: true });
    return;
  }
  // byte-range pieces of a bigger stream can't be saved as a file
  if (/[?&](range|bytestart)=/i.test(url)) return;

  let size = 0;
  const cr = /\/(\d+)\s*$/.exec(h["content-range"] || "");
  if (cr) size = Number(cr[1]);
  else if (h["content-length"]) size = Number(h["content-length"]);
  if (isMedia && size > 0 && size < 300 * 1024) return; // tiny clips, ads, sounds

  addMedia(d.tabId, {
    url,
    type: isHls ? "HLS stream" : type.split(";")[0],
    size,
    ext: isHls ? "ts" : (type.split("/")[1] || "mp4").split(";")[0].replace("x-matroska", "mkv").replace("mpeg", "mp3"),
  });
}

// ---------------------------------------------------------------- 3. YouTube

// The player asks for many small pieces of the same stream; we only need to
// store each stream once per tab.
const seenStreams = new Map(); // tabId -> Set("<itag>_<dur>")

async function addYoutubeStream(tabId, stream) {
  const key = `yt_${tabId}`;
  const tab = await chrome.tabs.get(tabId).catch(() => null);
  const videoId = tab ? videoIdOf(tab.url) : null;
  const state = (await chrome.storage.session.get(key))[key] || { videoId, sabr: false, streams: {} };
  if (videoId && state.videoId && state.videoId !== videoId) {
    // the user moved to another video
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
    // keep the newest URL of each stream (the old one may expire)
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
