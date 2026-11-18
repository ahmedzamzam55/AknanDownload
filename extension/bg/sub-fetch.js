// Instant YouTube subtitle extraction, network/HLS subtitle heuristics, and 429-resilient browser fetching.

const SUB_COMMON_LANGS = ["ar", "en", "fr", "de", "es", "tr", "pt", "ru", "it", "ja", "ko", "zh-Hans"];
const SUB_FALLBACK_NAMES = {
  ar: "Arabic", en: "English", "en-orig": "English", fr: "French", de: "German",
  es: "Spanish", tr: "Turkish", pt: "Portuguese", ru: "Russian", it: "Italian",
  ja: "Japanese", ko: "Korean", zh: "Chinese", "zh-Hans": "Chinese (Simplified)",
  "zh-Hant": "Chinese (Traditional)", hi: "Hindi", id: "Indonesian", und: "Subtitles",
};
const YT_SUB_CACHE = new Map();
const YT_SUB_PENDING = new Map();
const RAW_SUB_CACHE = new Map();

function subNameFor(code) {
  const c = String(code || "").trim();
  if (!c || c === "und") return "Subtitles";
  const base = c.split(/[-_]/)[0].toLowerCase();
  if (SUB_FALLBACK_NAMES[c]) return SUB_FALLBACK_NAMES[c];
  try {
    if (typeof Intl !== "undefined" && Intl.DisplayNames) {
      const label = new Intl.DisplayNames(["en"], { type: "language" }).of(c) || new Intl.DisplayNames(["en"], { type: "language" }).of(base);
      if (label && label.toLowerCase() !== c.toLowerCase()) return label;
    }
  } catch {}
  return SUB_FALLBACK_NAMES[base] || c.toUpperCase();
}

function isSpriteOrThumbUrl(url) {
  return /(?:^|[\/_?&.-])(thumbnails?|storyboards?|sprites?|preview|mosaic|thum|bif|seekbar)(?:[\/_?&.-]|$)/i.test(String(url || ""));
}

function guessSubTrack(url, contentType) {
  const u = String(url || "");
  if (!u.startsWith("http") || u.includes("127.0.0.1") || u.includes("localhost") || isSpriteOrThumbUrl(u)) return null;
  const ct = String(contentType || "").toLowerCase();
  if (ct.includes("text/html") || ct.includes("image/") || ct.includes("javascript")) return null;
  let ext = "";
  const mExt = u.match(/\.(vtt|srt|ass|ssa|ttml|dfxp)(?:$|[?#])/i);
  if (mExt) ext = mExt[1].toLowerCase();
  else if (ct.includes("text/vtt")) ext = "vtt";
  else if (ct.includes("subrip")) ext = "srt";
  else if (ct.includes("ttml")) ext = "ttml";
  else if (u.includes("/api/timedtext")) ext = "vtt";
  if (!ext) return null;
  let lang = "und";
  try {
    const parsed = new URL(u);
    lang = parsed.searchParams.get("tlang") || parsed.searchParams.get("lang") || lang;
    if (lang === "und") {
      const hint = parsed.pathname.match(/[._/-]([a-z]{2}(?:-[A-Za-z]{2,4})?)(?:[._/-]|\.(?:vtt|srt|ass|ssa|ttml))/i);
      if (hint && !/^(sub|subs|captions?|tracks?|cc|vtt|srt)$/i.test(hint[1])) lang = hint[1];
    }
  } catch {}
  return { lang, name: subNameFor(lang), url: u, ext, auto: u.includes("kind=asr") || u.includes("tlang=") };
}

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
    const isAuto = t.kind === "asr", key = lang.toLowerCase();
    if (!seen.has(key)) {
      seen.add(key);
      out.push({ lang, name: String(label).replace(/\s*\(auto-generated\)\s*/i, "").trim() || subNameFor(lang), url: setSubFmt(t.baseUrl, fmt), ext: fmt, auto: isAuto });
    }
    if (!pivot && t.isTranslatable !== false) pivot = t;
    if (t.isTranslatable && (!pivot || !isAuto)) pivot = t;
  }
  if (pivot) {
    const avail = Array.isArray(transLangs) && transLangs.length ? new Set(transLangs.map((x) => x && x.languageCode).filter(Boolean)) : null;
    const navLang = typeof navigator !== "undefined" && navigator.language ? navigator.language.split("-")[0] : "";
    for (const code of [navLang, ...SUB_COMMON_LANGS].filter(Boolean)) {
      if (seen.has(code.toLowerCase()) || (avail && !avail.has(code))) continue;
      seen.add(code.toLowerCase());
      out.push({ lang: code, name: subNameFor(code), url: setSubFmt(pivot.baseUrl, fmt, code), ext: fmt, auto: true });
    }
  }
  return out;
}

function parseYoutubeWatchSubs(html) {
  return tracksFromRenderer(extractBalancedJson(html, "\"captionTracks\":"), extractBalancedJson(html, "\"translationLanguages\":"), "json3");
}

async function fetchYoutubeSubs(videoId) {
  const id = String(videoId || "").trim();
  if (!id) return [];
  const cached = YT_SUB_CACHE.get(id);
  if (cached && Date.now() - cached.at < 300_000) return cached.subs;
  if (YT_SUB_PENDING.has(id)) return YT_SUB_PENDING.get(id);
  const run = (async () => {
    try {
      let subs = [];
      const apiResp = await plainFetch("https://www.youtube.com/youtubei/v1/player?prettyPrint=false", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ videoId: id, context: { client: { clientName: "ANDROID", clientVersion: "20.10.38", androidSdkVersion: 34, hl: "en", gl: "US" } } }),
      }, 8000).catch(() => null);
      if (apiResp && apiResp.ok) {
        const j = await apiResp.json().catch(() => null);
        const rnd = j && j.captions && j.captions.playerCaptionsTracklistRenderer;
        if (rnd) subs = tracksFromRenderer(rnd.captionTracks, rnd.translationLanguages, "json3");
      }
      if (!subs.length) {
        const r = await plainFetch(`https://www.youtube.com/watch?v=${encodeURIComponent(id)}&hl=en`, { credentials: "include" }, 10000).catch(() => null);
        if (r && r.ok) subs = parseYoutubeWatchSubs(await r.text());
      }
      if (subs.length) YT_SUB_CACHE.set(id, { at: Date.now(), subs });
      return subs;
    } catch { return []; } finally { YT_SUB_PENDING.delete(id); }
  })();
  YT_SUB_PENDING.set(id, run);
  return run;
}

function mergeSubLists(...lists) {
  const map = new Map();
  for (const list of lists) {
    if (!Array.isArray(list)) continue;
    for (const s of list) {
      if (!s || !s.url) continue;
      const lang = String(s.lang || "und").trim() || "und";
      const key = lang.toLowerCase() === "und" ? `und|${s.name || s.url}` : lang.toLowerCase();
      const prev = map.get(key);
      if (!prev || (prev.auto && !s.auto)) map.set(key, { ...s, lang, name: s.name || subNameFor(lang), ext: s.ext || "vtt" });
    }
  }
  return Array.from(map.values());
}

function fmtVttMs(ms) {
  const n = Math.max(0, Math.round(ms)), h = Math.floor(n / 3600000), m = Math.floor((n % 3600000) / 60000), s = Math.floor((n % 60000) / 1000), r = n % 1000;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(r).padStart(3, "0")}`;
}

function parseRawCues(raw) {
  const trimmed = String(raw || "").trim();
  const cues = [];
  if (trimmed.startsWith("{")) {
    try {
      for (const ev of JSON.parse(trimmed).events || []) {
        if (ev.tStartMs == null || !Array.isArray(ev.segs)) continue;
        const text = ev.segs.map((s) => s.utf8 || "").join("").replace(/\s+/g, " ").trim();
        if (text) cues.push({ start: fmtVttMs(ev.tStartMs), end: fmtVttMs(ev.tStartMs + (ev.dDurationMs || 2000)), text, endMs: ev.tStartMs + (ev.dDurationMs || 2000), startMs: ev.tStartMs });
      }
      for (let i = 0; i + 1 < cues.length; i += 1) {
        if (cues[i + 1].startMs > cues[i].startMs && cues[i].endMs > cues[i + 1].startMs) cues[i].end = cues[i + 1].start;
      }
    } catch {}
    return cues;
  }
  for (const blk of trimmed.replace(/\r\n?/g, "\n").split(/\n{2,}/)) {
    const ls = blk.split("\n").map((l) => l.trim()).filter(Boolean);
    const ti = ls.findIndex((l) => l.includes("-->"));
    if (ti < 0) continue;
    const [a, b] = ls[ti].split("-->");
    const text = ls.slice(ti + 1).join(" ").replace(/<[^>]+>/g, "").trim();
    if (text) cues.push({ start: a.trim().split(/\s+/)[0].replace(",", "."), end: b.trim().split(/\s+/)[0].replace(",", "."), text });
  }
  return cues;
}

async function translateFallbackVtt(srcUrl, targetLang) {
  let raw = RAW_SUB_CACHE.get(srcUrl);
  if (!raw) {
    const r = await plainFetch(srcUrl, { credentials: "include" }, 12000).catch(() => null);
    if (!r || !r.ok) return "";
    raw = await r.text();
    if (!raw || /^\s*<!doctype|^\s*<html/i.test(raw)) return "";
    if (RAW_SUB_CACHE.size > 16) RAW_SUB_CACHE.clear();
    RAW_SUB_CACHE.set(srcUrl, raw);
  }
  const cues = parseRawCues(raw);
  if (!cues.length) return "";
  const out = new Array(cues.length);
  for (let i = 0; i < cues.length; ) {
    const batch = [];
    let chars = 0;
    while (i + batch.length < cues.length && batch.length < 40 && chars < 2400) {
      const line = cues[i + batch.length].text.replace(/\n+/g, " ");
      batch.push(line);
      chars += line.length + 1;
    }
    const u = `https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl=${encodeURIComponent(targetLang)}&dt=t&q=${encodeURIComponent(batch.join("\n"))}`;
    const tr = await plainFetch(u, {}, 10000).catch(() => null);
    if (!tr || !tr.ok) return "";
    const data = await tr.json().catch(() => null);
    const lines = Array.isArray(data && data[0]) ? data[0].map((s) => (s && s[0]) || "").join("").split("\n") : [];
    for (let k = 0; k < batch.length; k += 1) out[i + k] = (lines[k] || batch[k]).trim();
    i += batch.length;
  }
  return `WEBVTT\n\n${cues.map((c, idx) => `${c.start} --> ${c.end}\n${out[idx] || c.text}\n`).join("\n")}`;
}

async function fetchSubBody(url) {
  if (!url || !String(url).startsWith("http")) return "";
  for (let i = 0; i < 2; i += 1) {
    try {
      const r = await plainFetch(url, { credentials: "include" }, 12000);
      if (r && r.ok) {
        const text = await r.text();
        if (text && text.trim() && !/^\s*<!doctype|^\s*<html/i.test(text)) return text;
      }
      if (!r || (r.status !== 429 && r.status !== 503)) break;
    } catch { break; }
    await new Promise((res) => setTimeout(res, 600));
  }
  try {
    const u = new URL(url), tl = u.searchParams.get("tlang");
    if (tl && u.hostname.endsWith("youtube.com") && u.pathname.includes("timedtext")) { u.searchParams.delete("tlang"); return await translateFallbackVtt(u.toString(), tl); }
  } catch {}
  return "";
}
