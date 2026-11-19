// Universal subtitle track normalization, merging, 429/503 retries, and translation fallback.

const RAW_SUB_CACHE = new Map();

function subNameFor(code) {
  const c = String(code || "").trim();
  if (!c || c === "und") return "Subtitles";
  const base = c.split(/[-_]/)[0].toLowerCase();
  if (typeof SUB_FALLBACK_NAMES !== "undefined" && SUB_FALLBACK_NAMES[c]) return SUB_FALLBACK_NAMES[c];
  try {
    if (typeof Intl !== "undefined" && Intl.DisplayNames) {
      const label = new Intl.DisplayNames(["en"], { type: "language" }).of(c) || new Intl.DisplayNames(["en"], { type: "language" }).of(base);
      if (label && label.toLowerCase() !== c.toLowerCase()) return label;
    }
  } catch {}
  return (typeof SUB_FALLBACK_NAMES !== "undefined" && SUB_FALLBACK_NAMES[base]) || c.toUpperCase();
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

function mergeSubLists(...lists) {
  const map = new Map();
  for (const list of lists) {
    if (!Array.isArray(list)) continue;
    for (const s of list) {
      if (!s || (!s.url && !s.body)) continue;
      const lang = String(s.lang || "und").trim() || "und";
      const low = lang.toLowerCase();
      const variant = s.variant || (low === "und" ? s.name || s.url : "");
      const key = `${low}|${s.auto ? 1 : 0}|${String(variant).toLowerCase()}`;
      if (!map.has(key)) {
        map.set(key, { ...s, lang, name: s.name || subNameFor(lang), ext: s.ext || "vtt" });
      }
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

function decodeSubDataUrl(url) {
  const m = /^data:([^,]*),(.*)$/s.exec(String(url || ""));
  if (!m) return "";
  try {
    if (/;base64/i.test(m[1])) return new TextDecoder().decode(Uint8Array.from(atob(m[2]), (c) => c.charCodeAt(0)));
    return decodeURIComponent(m[2]);
  } catch {
    return "";
  }
}

function retryAfterMs(r, fallbackMs) {
  const raw = r && r.headers && typeof r.headers.get === "function" ? r.headers.get("retry-after") : "";
  if (!raw) return fallbackMs;
  const sec = Number(raw);
  if (Number.isFinite(sec) && sec > 0) return Math.min(15000, Math.max(400, sec * 1000));
  const at = Date.parse(raw);
  return Number.isFinite(at) ? Math.min(15000, Math.max(400, at - Date.now())) : fallbackMs;
}

async function fetchSubBody(url, reqHeaders) {
  if (!url) return "";
  if (String(url).startsWith("data:")) return decodeSubDataUrl(url);
  if (!String(url).startsWith("http")) return "";
  const headers = {};
  for (const [k, v] of reqHeaders || []) {
    if (!/^(cookie|referer|origin|host|user-agent|sec-)/i.test(k) && v) headers[k] = v;
  }
  const init = Object.keys(headers).length ? { credentials: "include", headers } : { credentials: "include" };
  const isYtTlang = /youtube\.com\/api\/timedtext/i.test(url) && /[?&]tlang=/.test(url);
  const maxTries = isYtTlang ? 2 : 4;
  for (let i = 0; i < maxTries; i += 1) {
    try {
      const r = await plainFetch(url, init, 12000);
      if (r && r.ok) {
        const text = await r.text();
        if (text && text.trim() && !/^\s*<!doctype|^\s*<html/i.test(text)) return text;
      }
      if (!r || ![429, 502, 503, 504].includes(r.status)) break;
      if (i + 1 < maxTries) await new Promise((res) => setTimeout(res, retryAfterMs(r, isYtTlang ? 350 : 1200 * (i + 1))));
    } catch {
      if (i + 1 >= maxTries) break;
      await new Promise((res) => setTimeout(res, 800 * (i + 1)));
    }
  }
  try {
    const u = new URL(url), tl = u.searchParams.get("tlang");
    if (tl && u.hostname.endsWith("youtube.com") && u.pathname.includes("timedtext")) {
      u.searchParams.delete("tlang");
      return await translateFallbackVtt(u.toString(), tl);
    }
  } catch {}
  return "";
}
