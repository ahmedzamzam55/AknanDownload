// ---------------------------------------------------------------- per-tab state
// media_<tab>: generic files found on the page
// yt_<tab>:    { videoId, sabr, streams: { "<itag>_<dur>": stream }, resolved... }
// res_<tab>:   any other site: { page, resolving, resolved, resolveError }

async function readTab(tabId) {
  const keys = [`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`];
  const r = await chrome.storage.session.get(keys);
  return { media: r[keys[0]] || [], yt: r[keys[1]] || null, res: r[keys[2]] || null };
}

function optionItems(resolved, source) {
  return resolved.options.map((o) => ({
    label: o.label,
    sub: o.sub,
    yt: { ...o, title: resolved.title, source },
  }));
}

/** Everything the page button and the popup show, in one list. */
async function itemsForTab(tabId) {
  const { media, yt, res } = await readTab(tabId);
  const items = [];
  if (res && !yt) {
    if (res.resolved && res.resolved.options.length) {
      items.push(...optionItems(res.resolved, res.page));
    } else if (res.resolving) {
      items.push({ label: aknanI18n.t("Getting video qualities…"), sub: aknanI18n.t("This takes a few seconds"), disabled: true, busy: true });
    } else if (res.resolveError && !media.length) {
      // the page itself is no video page the app knows: the streams the
      // player opens show up here as soon as it plays
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
  if (yt) {
    if (yt.resolved && yt.resolved.options.length) {
      // qualities found by the app (yt-dlp): works with every stream format
      items.push(...optionItems(yt.resolved, `https://www.youtube.com/watch?v=${yt.videoId}`));
    } else {
      // fallback: streams captured from the player itself
      for (const o of buildOptions(yt.streams, appConfig && appConfig.mp3Kbps)) items.push({ label: o.label, sub: o.sub, yt: o });
      if (!items.length) {
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
  const resolvedFound = items.some((i) => i.yt);
  let hlsAsked = false;
  for (const m of media) {
    // when the app already found the real qualities, raw streams are noise
    if (resolvedFound && /HLS|DASH|video/i.test(m.type)) continue;
    if (m.dash) {
      // its qualities are read by the app when it is clicked
      items.push({ label: aknanI18n.t("Video stream (DASH) · choose a quality"), sub: m.url, resolve: m.url, ext: "mp4" });
      continue;
    }
    if (m.type === "HLS stream") {
      // a quality list of a stream whose qualities are listed already
      if (m.child) continue;
      if (m.master && !m.master.separateAudio) {
        // every quality of the stream, read from its playlist
        for (const v of m.master.variants) {
          items.push({ label: `${hlsLabel(v)} · MP4 · ${aknanI18n.t("stream")}`, sub: v.url, url: v.url, ext: "ts" });
        }
        continue;
      }
      if (m.master) {
        // picture and sound come in separate lists: the app puts them together
        items.push({
          label: `${m.master.variants.map(hlsLabel).slice(0, 4).join(" / ")} · ${aknanI18n.t("choose a quality")}`,
          sub: m.url,
          resolve: m.url,
          ext: "mp4",
        });
        continue;
      }
      // saved as an MP4 file; the quality first when the address tells it
      items.push({ label: `${qualityOf(m.url) || aknanI18n.t("Video")} · MP4 · ${aknanI18n.t("stream")}`, sub: m.url, url: m.url, ext: "ts" });
      if (!hlsAsked && !m.media) {
        hlsAsked = true;
        items.push({ label: aknanI18n.t("Other qualities of this stream…"), sub: m.url, resolve: m.url, ext: "mp4" });
      }
      continue;
    }
    const q = qualityOf(m.url);
    items.push({
      label: `${q ? q + " · " : ""}${(m.ext || "").toUpperCase() || m.type}${m.size ? " · " + fmtSize(m.size) : ""}`,
      sub: m.url,
      url: m.url,
      ext: m.ext,
    });
  }
  return items;
}

/** "720p" when a stream's address says which quality it is. */
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
    // switched off: no buttons on the page, "off" on the add-in's icon
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
  await chrome.storage.session.remove([`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`]);
  chrome.action.setBadgeText({ tabId, text: "" }).catch(() => {});
  chrome.tabs.sendMessage(tabId, { type: "swiftget-media", items: [] }).catch(() => {});
}
