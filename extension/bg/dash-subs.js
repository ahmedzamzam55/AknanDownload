// Extract subtitle AdaptationSets from DASH (.mpd) manifests.

function xmlAttr(tag, name) {
  const m = new RegExp(`\\b${name}=(["'])([^"']*)\\1`, "i").exec(tag);
  return m ? m[2].trim() : "";
}

function parseDashSubs(mpdText, baseUrl) {
  const text = String(mpdText || "").replace(/^\uFEFF/, "");
  if (!/<MPD\b/i.test(text)) return [];
  const out = [];
  const sets = text.match(/<AdaptationSet\b[^>]*>[\s\S]*?<\/AdaptationSet>/gi) || [];
  for (const block of sets) {
    const head = (block.match(/<AdaptationSet\b[^>]*>/i) || [""])[0];
    const ct = xmlAttr(head, "contentType").toLowerCase();
    const mime = (xmlAttr(head, "mimeType") || xmlAttr(block, "mimeType")).toLowerCase();
    const isText = ct === "text" || /(?:vtt|ttml|subrip|srt|stpp|wvtt)/i.test(mime);
    if (!isText) continue;
    const mBase = /<BaseURL[^>]*>([^<]+)<\/BaseURL>/i.exec(block);
    if (!mBase) continue;
    let url = "";
    try {
      url = new URL(mBase[1].trim().replace(/&amp;/g, "&"), baseUrl).toString();
    } catch {
      continue;
    }
    const lang = xmlAttr(head, "lang") || "und";
    const label = xmlAttr(head, "label") || subNameFor(lang);
    const ext = mime.includes("ttml") ? "ttml" : mime.includes("srt") || mime.includes("subrip") ? "srt" : "vtt";
    out.push({ lang, name: label, url, ext, auto: false });
  }
  return out;
}

async function sniffDashSubs(tabId, url, reqHeaders) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 8000);
  try {
    const r = await fetch(url, { credentials: "include", signal: ctl.signal });
    if (!r || !r.ok) return;
    const text = (await r.text()).slice(0, 512 * 1024);
    const tracks = parseDashSubs(text, url);
    for (const t of tracks) {
      await addSubTrack(tabId, { ...t, reqHeaders, seenAt: Date.now() });
    }
  } catch {
    /* ignore */
  } finally {
    clearTimeout(timer);
  }
}
