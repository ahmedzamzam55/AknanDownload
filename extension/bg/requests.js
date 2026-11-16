// ---------------------------------------------------------------- the browser's own request & forms
// What the browser sent for a file a moment ago (some sites let only exactly
// that request in: the right Accept-Language, Sec-Fetch-*, client hints...).
// The download is handed over with the same headers. Kept in memory for two
// minutes: the download starts right after its request.
const SENT_HEADERS = new Map();
const KEEP_HEADERS = /^(accept-language|sec-fetch-(site|mode|dest|user)|sec-ch-ua(-mobile|-platform)?|origin|referer|user-agent|cookie|x-requested-with)$/i;

function rememberRequest(d) {
  if (!d.requestHeaders || !/^https?:/i.test(d.url)) return;
  const kept = d.requestHeaders.filter((h) => KEEP_HEADERS.test(h.name) && typeof h.value === "string");
  if (!kept.length) return;
  SENT_HEADERS.set(d.url, { at: Date.now(), headers: kept });
  if (SENT_HEADERS.size > 300) {
    const old = Date.now() - 120000;
    for (const [k, v] of SENT_HEADERS) if (v.at < old || SENT_HEADERS.size > 300) SENT_HEADERS.delete(k);
  }
}

// ---------------------------------------------------------------- forms
// A file a form's button gives (POST): the app asks for it the same way, with
// the form's data. What it sent is kept for two minutes. A form with a file in
// it (an upload) can't be sent again: its download stays in the browser.
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

/**
 * The form posted to `url` a moment ago, as the app sends it again:
 * undefined = no form; null = one that can't be sent again (a file in it).
 */
function formFor(url) {
  const f = url && FORMS.get(url);
  if (!f || Date.now() - f.at > 120000) return undefined;
  // multipart: the browser gives its fields without its files
  if (f.body === null || (f.fields && /multipart/i.test(f.type || ""))) return null;
  return { post: b64(f.body), postType: f.fields ? "application/x-www-form-urlencoded" : f.type || "" };
}

/** What the browser sent for `url` (or the first of `urls` it knows). */
function browserRequest(...urls) {
  for (const u of urls) {
    const r = u && SENT_HEADERS.get(u);
    if (r && Date.now() - r.at < 120000) return r.headers;
  }
  return null;
}

async function sendToApp({ url, audioUrl, referrer, fileName, headers, sourcePage, sourceKey, alsoUrl, clip, store, form }) {
  const { port } = await getOpts();
  // the browser's own request for this file: its headers go along (the app
  // keeps its own Accept / Range); its cookie is the one the site really got
  const sent = headers ? null : browserRequest(url, alsoUrl);
  const sentCookie = sent && sent.find((h) => /^cookie$/i.test(h.name));
  const sentReferer = sent && sent.find((h) => /^referer$/i.test(h.name));
  const sentUa = sent && sent.find((h) => /^user-agent$/i.test(h.name));
  const passOn = sent ? sent.filter((h) => !/^(cookie|referer|user-agent)$/i.test(h.name)).map((h) => [h.name, h.value]) : null;
  const body = {
    url,
    audioUrl: audioUrl || null,
    headers: headers || (passOn && passOn.length ? passOn : null),
    sourcePage: sourcePage || null,
    sourceKey: sourceKey || null,
    referrer: referrer || (sentReferer && sentReferer.value) || null,
    cookies: (sentCookie && sentCookie.value) || (await cookieHeader(url, store)),
    userAgent: (sentUa && sentUa.value) || navigator.userAgent,
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
