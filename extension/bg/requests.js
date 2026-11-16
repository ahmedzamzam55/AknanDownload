// ---------------------------------------------------------------- the browser's own request & forms
// What the browser sent for a file (Host-less header capture): kept 30 min so
// a stream loaded before clicking the bubble still has its Referer and Origin.
const SENT_HEADERS = new Map();
const SENT_BY_ID = new Map();
const SENT_TTL = 1800000;
const KEEP_HEADERS = /^(accept-language|origin|referer|user-agent|cookie|x-requested-with|sec-fetch-(site|mode|dest|user)|sec-ch-ua(-mobile|-platform)?)$/i;

function trimSent(map, limit) {
  if (map.size <= limit) return;
  const old = Date.now() - SENT_TTL;
  for (const [k, v] of map) if (v.at < old || map.size > limit) map.delete(k);
}

function rememberRequest(d) {
  if (!d.requestHeaders || !/^https?:/i.test(d.url)) return;
  const kept = d.requestHeaders.filter((h) => KEEP_HEADERS.test(h.name) && typeof h.value === "string");
  if (!kept.length) return;
  const entry = { at: Date.now(), headers: kept };
  SENT_HEADERS.set(d.url, entry);
  if (d.requestId) SENT_BY_ID.set(d.requestId, entry);
  trimSent(SENT_HEADERS, 500);
  trimSent(SENT_BY_ID, 500);
}

/** Pairs [[name, value], ...] captured for `d`, filling missing Referer / Origin. */
function captureRequestHeaders(d) {
  const byId = d.requestId && SENT_BY_ID.get(d.requestId);
  const raw = (byId && byId.headers) || browserRequest(d.url) || [];
  const pairs = raw.map((h) => [h.name, h.value]);
  const has = (k) => pairs.some(([n, v]) => n.toLowerCase() === k && v);
  const ref = /^https?:/i.test(d.documentUrl || "") ? d.documentUrl : /^https?:/i.test(d.originUrl || "") ? d.originUrl : /^https?:/i.test(d.initiator || "") ? `${d.initiator}/` : "";
  if (!has("referer") && ref) pairs.push(["Referer", ref]);
  if (!has("origin") && /^https?:/i.test(d.initiator || "")) pairs.push(["Origin", d.initiator]);
  return pairs;
}

// ---------------------------------------------------------------- forms
const FORMS = new Map();
const FORM_LIMIT = 64 * 1024;

function forgetOld(map, ms) {
  const old = Date.now() - ms;
  for (const [k, v] of map) if (v.at < old) map.delete(k);
}

function formOf(d) {
  const rb = d.requestBody;
  if (!rb) return { body: null };
  if (rb.formData) {
    const p = new URLSearchParams();
    for (const [k, values] of Object.entries(rb.formData)) for (const v of values) p.append(k, String(v));
    const body = p.toString();
    return body.length > FORM_LIMIT ? { body: null } : { body, fields: true };
  }
  if (Array.isArray(rb.raw) && rb.raw.length && rb.raw.every((x) => x.bytes)) {
    const size = rb.raw.reduce((n, x) => n + x.bytes.byteLength, 0);
    if (size > FORM_LIMIT) return { body: null };
    const all = new Uint8Array(size);
    let at = 0;
    for (const x of rb.raw) {
      all.set(new Uint8Array(x.bytes), at);
      at += x.bytes.byteLength;
    }
    try {
      return { body: new TextDecoder("utf-8", { fatal: true }).decode(all) };
    } catch {
      return { body: null };
    }
  }
  return { body: null };
}

function onFormBeforeRequest(d) {
  if (d.method !== "POST") return;
  forgetOld(FORMS, 120000);
  FORMS.set(d.url, { ...formOf(d), at: Date.now(), id: d.requestId });
}

function onFormBeforeSendHeaders(d) {
  if (d.method !== "POST") return;
  const f = FORMS.get(d.url);
  const ct = (d.requestHeaders || []).find((h) => h.name.toLowerCase() === "content-type");
  if (f && f.id === d.requestId && ct && ct.value) f.type = ct.value;
}

const b64 = (text) => {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
};

function formFor(url) {
  const f = url && FORMS.get(url);
  if (!f || Date.now() - f.at > 120000) return undefined;
  if (f.body === null || (f.fields && /multipart/i.test(f.type || ""))) return null;
  return { post: b64(f.body), postType: f.fields ? "application/x-www-form-urlencoded" : f.type || "" };
}

/** What the browser sent for `url` (or the first of `urls` it knows). */
function browserRequest(...urls) {
  for (const u of urls) {
    const r = u && SENT_HEADERS.get(u);
    if (r && Date.now() - r.at < SENT_TTL) return r.headers;
  }
  return null;
}

function findHeader(pairs, name) {
  const m = (pairs || []).find(([k]) => String(k).toLowerCase() === name);
  return m ? m[1] : "";
}

function mergePairs(primary, secondary) {
  const out = [];
  const seen = new Set();
  for (const [k, v] of [...(primary || []), ...(secondary || [])]) {
    if (!k || v == null) continue;
    const low = String(k).toLowerCase();
    if (seen.has(low)) continue;
    seen.add(low);
    out.push([k, v]);
  }
  return out;
}

async function sendToApp({ url, audioUrl, referrer, fileName, headers, reqHeaders, sourcePage, sourceKey, alsoUrl, clip, store, form }) {
  const { port } = await getOpts();
  const captured = reqHeaders && reqHeaders.length ? reqHeaders : (browserRequest(url, alsoUrl) || []).map((h) => [h.name, h.value]);
  const combined = mergePairs(headers, captured);
  const sentCookie = findHeader(combined, "cookie");
  const sentReferer = findHeader(combined, "referer");
  const sentUa = findHeader(combined, "user-agent");
  const passOn = combined.filter(([k]) => !/^(cookie|referer|user-agent)$/i.test(k));
  const outHeaders = headers && headers.length ? mergePairs(headers, passOn) : passOn.length ? passOn : null;
  const body = {
    url,
    audioUrl: audioUrl || null,
    headers: outHeaders,
    sourcePage: sourcePage || null,
    sourceKey: sourceKey || null,
    // Prefer the real player Referer over outer wrapper pages (e.g. laroza1 -> okprimeone iframe)
    referrer: sentReferer || referrer || null,
    cookies: sentCookie || (await cookieHeader(url, store)),
    userAgent: sentUa || navigator.userAgent,
    fileName: fileName || null,
    clip: clip || null,
    ...(form ? form : {}),
  };
  const r = await fetchWithTimeout(
    apiUrl(port, "/add"),
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify(body),
    },
    3000
  );
  if (!r.ok) throw new Error(`Aknan Download answered HTTP ${r.status}`);
}
