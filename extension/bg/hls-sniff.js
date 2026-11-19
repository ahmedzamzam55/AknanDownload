// HLS playlist parsing, protected fallback resolution via desktop app, and iframe navigation cleanup.

function parseHlsMaster(text, base) {
  text = String(text || "").replace(/^\uFEFF/, "");
  if (!/#EXT-X-STREAM-INF/i.test(text)) return null;
  const lines = text.split(/\r?\n/).map((l) => l.trim());
  const variants = [];
  const separateAudio = lines.some((l) => /^#EXT-X-MEDIA:/i.test(l) && /\bTYPE=AUDIO\b/i.test(l) && /\bURI=/i.test(l));
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
  variants.sort((a, b) => b.height - a.height || b.bw - a.bw);
  const seen = new Set();
  const unique = variants.filter((v) => {
    const k = v.height ? `h${v.height}` : `b${v.bw}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
  const subs = [];
  for (const l of lines) {
    if (!/^#EXT-X-MEDIA:/i.test(l) || !/TYPE=SUBTITLES/i.test(l)) continue;
    const u = /URI="([^"]+)"/i.exec(l);
    if (!u) continue;
    let subUrl = "";
    try { subUrl = new URL(u[1], base).toString(); } catch { continue; }
    const lm = /LANGUAGE="([^"]+)"/i.exec(l);
    const nm = /NAME="([^"]+)"/i.exec(l);
    const lang = (lm && lm[1]) || "und";
    subs.push({ lang, name: (nm && nm[1]) || subNameFor(lang), url: subUrl, ext: "vtt", auto: false });
  }
  return { variants: unique, separateAudio, subs };
}

function hlsLabel(v) {
  if (v.height) return `${v.height}p`;
  if (v.bw) return `${Math.round(v.bw / 1000)} kbps`;
  return "Video";
}

function originOf(u) {
  try {
    return new URL(u).origin;
  } catch {
    return "";
  }
}

/** Clear stale streams when an embedded server iframe switches to another URL/server. */
async function onSubFrameBeforeNavigate(d) {
  if (d.tabId < 0 || !(d.frameId > 0) || !/^https?:/i.test(d.url || "")) return;
  const key = `media_${d.tabId}`;
  const subKey = `subs_${d.tabId}`;
  const got = await chrome.storage.session.get([key, subKey]);
  const current = got[key] || [];
  const curSubs = got[subKey] || [];
  const nextOrigin = originOf(d.url);
  const keepFn = (m) => m.frameId !== d.frameId || (m.frameUrl && m.frameUrl === d.url && m.frameOrigin === nextOrigin);
  const kept = current.filter(keepFn);
  const keptSubs = curSubs.filter(keepFn);
  if (kept.length === current.length && keptSubs.length === curSubs.length) return;
  await chrome.storage.session.set({ [key]: kept, [subKey]: keptSubs });
  pushToTab(d.tabId);
}

async function resolveHlsThroughApp(tabId, url) {
  if (!(await appAlive())) return;
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  const me = current.find((m) => m.url === url);
  if (!me || me.resolvedOptions) return;
  const r = await askApp(url, null, false, me.reqHeaders).catch(() => null);
  if (!r || !r.resolved || !r.resolved.options || !r.resolved.options.length) return;
  const fresh = (await chrome.storage.session.get(key))[key] || [];
  const target = fresh.find((m) => m.url === url);
  if (!target) return;
  target.resolvedOptions = r.resolved;
  const inside = r.resolved.options.map((o) => o.videoUrl).filter(Boolean);
  for (const m of fresh) if (m !== target && inside.some((u) => sameStreamPath(u, m.url))) m.child = true;
  await chrome.storage.session.set({ [key]: fresh });
  pushToTab(tabId);
}

async function readHls(tabId, url) {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 8000);
  let text = "";
  let ok = false;
  try {
    const r = await fetch(url, { credentials: "include", signal: ctl.signal });
    ok = r.ok;
    if (ok) text = (await r.text()).slice(0, 512 * 1024);
  } catch {
    ok = false;
  } finally {
    clearTimeout(timer);
  }
  if (!ok || !/^\s*#EXTM3U/.test(text.replace(/^\uFEFF/, ""))) {
    await resolveHlsThroughApp(tabId, url);
    return;
  }
  const master = parseHlsMaster(text, url);
  const key = `media_${tabId}`;
  const current = (await chrome.storage.session.get(key))[key] || [];
  const me = current.find((m) => m.url === url);
  if (!me) return;
  if (master) {
    me.master = master;
    const inside = master.variants.map((v) => v.url);
    for (const m of current) if (m !== me && inside.some((u) => sameStreamPath(u, m.url))) m.child = true;
  } else {
    me.media = true;
    if (current.some((m) => (m.master && m.master.variants.some((v) => sameStreamPath(v.url, url))) || (m.resolvedOptions && m.resolvedOptions.options && m.resolvedOptions.options.some((o) => sameStreamPath(o.videoUrl, url))))) {
      me.child = true;
    }
  }
  await chrome.storage.session.set({ [key]: current });
  pushToTab(tabId);
}
