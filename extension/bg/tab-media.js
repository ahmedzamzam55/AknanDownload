// ---------------------------------------------------------------- per-tab state
// media_<tab>: generic files found on the page
// yt_<tab>:    { videoId, sabr, streams: { "<itag>_<dur>": stream }, resolved... }
// res_<tab>:   any other site: { page, resolving, resolved, resolveError }

async function readTab(tabId) {
  const keys = [`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`, `subs_${tabId}`];
  const r = await chrome.storage.session.get(keys);
  return { media: r[keys[0]] || [], yt: r[keys[1]] || null, res: r[keys[2]] || null, subs: r[keys[3]] || [] };
}

function streamPathKey(u) {
  try {
    const p = new URL(u);
    return p.origin + p.pathname;
  } catch {
    return u || "";
  }
}

function sameStreamPath(a, b) {
  return !!a && !!b && (a === b || streamPathKey(a) === streamPathKey(b));
}

function optionItems(resolved, source, reqHeaders) {
  const subs = resolved.subtitles && resolved.subtitles.length ? resolved.subtitles : undefined;
  return resolved.options.map((o, idx) => ({
    label: o.label,
    sub: o.sub,
    reqHeaders: reqHeaders && reqHeaders.length ? reqHeaders : undefined,
    subs: idx === 0 ? subs : undefined,
    site: idx === 0 ? resolved.site || "" : undefined,
    yt: { ...o, title: resolved.title, source },
  }));
}

function orderMedia(media, preferFrameId) {
  if (preferFrameId > 0) {
    const mine = media.filter((m) => m.frameId === preferFrameId);
    if (mine.length) return mine;
  }
  let activeFrame = 0;
  let maxSeen = -1;
  for (const m of media) {
    if (m.frameId > 0 && (m.seenAt || 0) >= maxSeen) {
      maxSeen = m.seenAt || 0;
      activeFrame = m.frameId;
    }
  }
  if (!activeFrame) return media;
  const head = media.filter((m) => m.frameId === activeFrame);
  const tail = media.filter((m) => m.frameId !== activeFrame);
  return [...head, ...tail];
}

/** Everything the page button and the popup show, in one list. */
async function itemsForTab(tabId, preferFrameId = 0) {
  const { media, yt, res, subs } = await readTab(tabId);
  const items = [];
  if (yt) {
    const ytSource = `https://${yt.host || "www.youtube.com"}/watch?v=${yt.videoId}`;
    if (yt.resolved && yt.resolved.options.length) {
      items.push(...optionItems(yt.resolved, ytSource));
    } else {
      for (const o of buildOptions(yt.streams, appConfig && appConfig.mp3Kbps)) items.push({ label: o.label, sub: o.sub, yt: o });
      if (!items.length && yt.quickOptions && yt.quickOptions.length) {
        items.push(...optionItems({ title: yt.title || "", site: "YouTube", options: yt.quickOptions, subtitles: yt.subs || [] }, ytSource));
      }
      if (!items.length && !(res && res.resolved && res.resolved.options.length)) {
        if (yt.resolving) {
          items.push({ label: aknanI18n.t("Getting video qualities…"), sub: aknanI18n.t("This takes a few seconds"), disabled: true, busy: true });
        } else if (yt.resolveError) {
          items.push({ label: aknanI18n.t("Can't get this video"), sub: yt.resolveError, disabled: true });
        } else if (yt.sabr) {
          items.push({
            label: aknanI18n.t("This video needs Aknan Download"),
            sub: aknanI18n.t("Open Aknan Download (with yt-dlp.exe next to it) and reload the page"),
            disabled: true,
          });
        }
      }
    }
  }
  const ytHasQualities = items.some((i) => !i.disabled && i.yt);
  if (res && (!yt || !ytHasQualities)) {
    if (res.resolved && res.resolved.options.length) {
      if (!ytHasQualities) items.length = 0;
      items.push(...optionItems(res.resolved, res.page));
    } else if (!yt && res.resolving) {
      items.push({ label: aknanI18n.t("Getting video qualities…"), sub: aknanI18n.t("This takes a few seconds"), disabled: true, busy: true });
    } else if (!yt && res.resolveError && !media.length) {
      const unknown = /unsupported url|no video formats|unable to extract/i.test(res.resolveError);
      items.push(
        isDrm(res.resolveError)
          ? { label: drmText(), sub: "", disabled: true, drm: true }
          : unknown
            ? { label: aknanI18n.t("Play the video for a moment"), sub: aknanI18n.t("Its qualities appear here as soon as it plays"), disabled: true }
            : { label: aknanI18n.t("Can't get this video"), sub: res.resolveError, disabled: true }
      );
    }
  }
  const resolvedFound = items.some((i) => i.yt);
  const visible = orderMedia(media, preferFrameId).filter((m) => !(resolvedFound && /HLS|DASH|video/i.test(m.type)));
  items.push(...mediaItems(visible));
  const usableFirst = items.find((i) => !i.disabled);
  if (usableFirst) {
    const hlsSubs = visible.flatMap((m) => (m.master && m.master.subs) || []);
    const merged = mergeSubLists(usableFirst.subs, yt && yt.subs, yt && yt.resolved && yt.resolved.subtitles, res && res.resolved && res.resolved.subtitles, hlsSubs, orderMedia(subs, preferFrameId));
    if (merged.length) usableFirst.subs = merged;
  }
  return items;
}

function mediaItems(list) {
  const items = [];
  let hlsAsked = false;
  const mainHls = list.find((m) => !m.child && (m.resolvedOptions || m.master));
  for (const m of list) {
    if (mainHls && m.type !== "HLS stream" && !m.dash && m.frameId !== mainHls.frameId && m.size > 0 && m.size < 15 * 1024 * 1024) {
      continue;
    }
    const rh = m.reqHeaders && m.reqHeaders.length ? m.reqHeaders : undefined;
    if (m.dash) {
      items.push({ label: aknanI18n.t("Video stream (DASH) · choose a quality"), sub: m.url, resolve: m.url, reqHeaders: rh, ext: "mp4" });
      continue;
    }
    if (m.type === "HLS stream") {
      if (m.child) continue;
      if (m.resolvedOptions && m.resolvedOptions.options && m.resolvedOptions.options.length) {
        items.push(...optionItems(m.resolvedOptions, "", rh));
        continue;
      }
      if (m.master && !m.master.separateAudio) {
        for (const v of m.master.variants) {
          items.push({ label: `${hlsLabel(v)} · MP4 · ${aknanI18n.t("stream")}`, sub: v.url, url: v.url, reqHeaders: rh, ext: "ts" });
        }
        continue;
      }
      if (m.master) {
        items.push({
          label: `${m.master.variants.map(hlsLabel).slice(0, 4).join(" / ")} · ${aknanI18n.t("choose a quality")}`,
          sub: m.url,
          resolve: m.url,
          reqHeaders: rh,
          ext: "mp4",
        });
        continue;
      }
      if (mainHls && m !== mainHls && m.frameId === mainHls.frameId) continue;
      items.push({ label: `${qualityOf(m.url) || aknanI18n.t("Video")} · MP4 · ${aknanI18n.t("stream")}`, sub: m.url, url: m.url, reqHeaders: rh, ext: "ts" });
      if (!hlsAsked && !m.media) {
        hlsAsked = true;
        items.push({ label: aknanI18n.t("Other qualities of this stream…"), sub: m.url, resolve: m.url, reqHeaders: rh, ext: "mp4" });
      }
      continue;
    }
    const q = qualityOf(m.url);
    items.push({
      label: `${q ? q + " · " : ""}${(m.ext || "").toUpperCase() || m.type}${m.size ? " · " + fmtSize(m.size) : ""}`,
      sub: m.url,
      url: m.url,
      reqHeaders: rh,
      ext: m.ext,
    });
  }
  const seen = new Set();
  return items.filter((it) => {
    const k = it.yt ? `yt|${it.label}|${it.yt.key || ""}|${it.yt.kbps || 0}|${it.yt.duration || 0}` : `${it.label}|${streamPathKey(it.url || it.resolve || "")}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

function qualityOf(url) {
  let path = "";
  try {
    path = new URL(url).pathname;
  } catch {
    return "";
  }
  const m = /(?:^|[^0-9])(4320|2160|1440|1080|720|576|540|480|360|240|144)p?(?=[^0-9]|$)/i.exec(path);
  return m ? `${m[1]}p` : "";
}

async function pushToTab(tabId) {
  const { enabled } = await getOpts();
  if (!enabled) {
    chrome.action.setBadgeBackgroundColor({ color: "#8a94a3" });
    chrome.action.setBadgeText({ tabId, text: "off" }).catch(() => {});
    chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items: [] }).catch(() => {});
    return;
  }
  const items = await itemsForTab(tabId);
  const count = items.filter((i) => !i.disabled).length;
  chrome.action.setBadgeBackgroundColor({ color: "#1f6feb" });
  chrome.action.setBadgeText({ tabId, text: count ? String(count) : "" }).catch(() => {});
  chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items }).catch(() => {});
}

async function clearTab(tabId) {
  if (typeof activeHlsHosts !== "undefined") activeHlsHosts.delete(tabId);
  await chrome.storage.session.remove([`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`, `subs_${tabId}`]);
  chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
  chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items: [] }).catch(() => {});
}
