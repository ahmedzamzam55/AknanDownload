// Instant YouTube & YouTube Kids caption + streamingData quality extraction.

const SUB_COMMON_LANGS = ["ar", "en", "fr", "de", "es", "tr", "pt", "ru", "it", "ja", "ko", "zh-Hans"];
const SUB_FALLBACK_NAMES = {
  ar: "Arabic", en: "English", "en-orig": "English", fr: "French", de: "German",
  es: "Spanish", tr: "Turkish", pt: "Portuguese", ru: "Russian", it: "Italian",
  ja: "Japanese", ko: "Korean", zh: "Chinese", "zh-Hans": "Chinese (Simplified)",
  "zh-Hant": "Chinese (Traditional)", hi: "Hindi", id: "Indonesian", und: "Subtitles",
};
const YT_SUB_CACHE = new Map();
const YT_SUB_PENDING = new Map();

function setSubFmt(baseUrl, fmt, tlang) {
  let out = String(baseUrl || "").replace(/([?&])fmt=[^&]*/g, "$1").replace(/[?&]+$/, "").replace(/\?&/, "?");
  out += `${out.includes("?") ? "&" : "?"}fmt=${encodeURIComponent(fmt || "json3")}`;
  if (tlang) out += `&tlang=${encodeURIComponent(tlang)}`;
  return out;
}

function extractBalancedJson(text, marker) {
  const idx = text.indexOf(marker);
  if (idx < 0) return null;
  let i = idx + marker.length;
  while (i < text.length && /\s/.test(text[i])) i += 1;
  const open = text[i], close = open === "[" ? "]" : open === "{" ? "}" : "";
  if (!close) return null;
  let depth = 0, inStr = false, esc = false;
  for (let j = i; j < text.length; j += 1) {
    const ch = text[j];
    if (inStr) {
      if (esc) esc = false;
      else if (ch === "\\") esc = true;
      else if (ch === "\"") inStr = false;
    } else if (ch === "\"") inStr = true;
    else if (ch === open) depth += 1;
    else if (ch === close && --depth === 0) {
      try { return JSON.parse(text.slice(i, j + 1)); } catch { return null; }
    }
  }
  return null;
}

function tracksFromRenderer(tracks, transLangs, fmt) {
  if (!Array.isArray(tracks) || !tracks.length) return [];
  const out = [], seen = new Set();
  let pivot = null;
  for (const t of tracks) {
    if (!t || !t.baseUrl || !t.languageCode) continue;
    const lang = String(t.languageCode);
    const label = (t.name && (t.name.simpleText || (Array.isArray(t.name.runs) && t.name.runs.map((r) => r.text).join("")))) || subNameFor(lang);
    const isAuto = t.kind === "asr", key = `${lang.toLowerCase()}|${isAuto ? 1 : 0}`;
    if (!seen.has(key)) {
      seen.add(key);
      out.push({
        lang,
        name: String(label).replace(/\s*\(auto-generated\)\s*/i, "").trim() || subNameFor(lang),
        url: setSubFmt(t.baseUrl, fmt),
        ext: fmt,
        auto: isAuto,
      });
    }
    if (!pivot && t.isTranslatable !== false) pivot = t;
    if (t.isTranslatable && (!pivot || !isAuto)) pivot = t;
  }
  if (pivot) {
    const avail = Array.isArray(transLangs) && transLangs.length ? new Set(transLangs.map((x) => x && x.languageCode).filter(Boolean)) : null;
    const navLang = typeof navigator !== "undefined" && navigator.language ? navigator.language.split("-")[0] : "";
    for (const code of [navLang, ...SUB_COMMON_LANGS].filter(Boolean)) {
      const k = `${code.toLowerCase()}|1`;
      if (seen.has(`${code.toLowerCase()}|0`) || seen.has(k) || (avail && !avail.has(code))) continue;
      seen.add(k);
      out.push({ lang: code, name: subNameFor(code), url: setSubFmt(pivot.baseUrl, fmt, code), ext: fmt, auto: true });
    }
  }
  return out;
}

function parseYoutubeWatchSubs(html) {
  return tracksFromRenderer(extractBalancedJson(html, "\"captionTracks\":"), extractBalancedJson(html, "\"translationLanguages\":"), "json3");
}

function extractQuickOptions(playerJson) {
  const sd = playerJson && playerJson.streamingData;
  if (!sd) return [];
  const dur = Number((playerJson.videoDetails && playerJson.videoDetails.lengthSeconds) || 0);
  const all = [...(sd.formats || []), ...(sd.adaptiveFormats || [])];
  const streams = {};
  for (const f of all) {
    if (!f || !f.url) continue;
    const itag = Number(f.itag);
    if (!itag) continue;
    const parsed = parseStream(f.url);
    if (parsed && !parsed.sabr) {
      streams[`${itag}_${parsed.dur || dur}`] = parsed;
      continue;
    }
    const mime = String(f.mimeType || "").toLowerCase();
    const isAudio = mime.startsWith("audio/");
    const ext = mime.includes("webm") ? (isAudio ? "webm" : "webm") : (isAudio ? "m4a" : "mp4");
    const kind = isAudio ? "a" : Number(f.audioChannels) > 0 ? "av" : "v";
    streams[`${itag}_q`] = {
      itag,
      url: f.url,
      mime,
      kind,
      h: Number(f.height) || 0,
      fps: Number(f.fps) || 30,
      abr: Math.round((Number(f.bitrate) || 0) / 1000),
      size: Number(f.contentLength) || 0,
      dur: Number(f.approxDurationMs ? Number(f.approxDurationMs) / 1000 : dur) || 0,
      ext,
    };
  }
  return buildOptions(streams, appConfig && appConfig.mp3Kbps);
}

async function fetchYoutubePlayer(videoId, pageUrl) {
  const id = String(videoId || "").trim();
  if (!id) return { subs: [], quickOptions: [], title: "" };
  const cached = YT_SUB_CACHE.get(id);
  if (cached && Date.now() - cached.at < 300_000) return cached;
  if (YT_SUB_PENDING.has(id)) return YT_SUB_PENDING.get(id);
  const host = /youtubekids\.com/i.test(pageUrl || "") ? "www.youtubekids.com" : "www.youtube.com";
  const run = (async () => {
    try {
      let subs = [], quickOptions = [], title = "";
      const apiResp = await plainFetch(`https://${host}/youtubei/v1/player?prettyPrint=false`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ videoId: id, context: { client: { clientName: "ANDROID", clientVersion: "20.10.38", androidSdkVersion: 34, hl: "en", gl: "US" } } }),
      }, 8000).catch(() => null);
      if (apiResp && apiResp.ok) {
        const j = await apiResp.json().catch(() => null);
        const rnd = j && j.captions && j.captions.playerCaptionsTracklistRenderer;
        if (rnd) subs = tracksFromRenderer(rnd.captionTracks, rnd.translationLanguages, "json3");
        quickOptions = extractQuickOptions(j);
        title = (j && j.videoDetails && j.videoDetails.title) || "";
      }
      if (!subs.length) {
        const r = await plainFetch(`https://${host}/watch?v=${encodeURIComponent(id)}&hl=en`, { credentials: "include" }, 10000).catch(() => null);
        if (r && r.ok) subs = parseYoutubeWatchSubs(await r.text());
      }
      const res = { at: Date.now(), subs, quickOptions, title };
      if (subs.length || quickOptions.length) YT_SUB_CACHE.set(id, res);
      return res;
    } catch {
      return { subs: [], quickOptions: [], title: "" };
    } finally {
      YT_SUB_PENDING.delete(id);
    }
  })();
  YT_SUB_PENDING.set(id, run);
  return run;
}

async function fetchYoutubeSubs(videoId, pageUrl) {
  const r = await fetchYoutubePlayer(videoId, pageUrl);
  return (r && r.subs) || [];
}
