// Aknan Download browser integration (Manifest V3 service worker)
//  1. takes over browser downloads and hands them to the Aknan Download app
//     together with cookies + referrer, so links behind a login still work
//  2. watches network responses for video/audio and HLS streams and offers
//     a "Download this video" button on the page
//  3. Videos on web pages: the app finds the qualities with yt-dlp,
//     downloads video + audio with many connections and joins them
//  4. adds "Download with Aknan Download", "Download all links with Aknan
//     Download" and "Download video with Aknan Download" to the right-click menu
//  5. follows the app's Options: file types to take, sites to leave alone,
//     the keys that skip / force Aknan Download, the video panel

// Chrome / Edge / Opera load all background dependencies here; Firefox lists them in its manifest
if (typeof importScripts === "function" && typeof __aknanBgModulesLoaded === "undefined") {
  importScripts(
    "yt-itags.js",
    "yt-core.js",
    "i18n/ar.js",
    "i18n/fr.js",
    "i18n/es.js",
    "i18n/de.js",
    "i18n/pt.js",
    "i18n/tr.js",
    "i18n/ru.js",
    "i18n.js",
    "bg/helper.js",
    "bg/auth.js",
    "bg/selfupdate.js",
    "bg/context.js",
    "bg/requests.js",
    "bg/rules.js",
    "bg/downloads.js",
    "bg/takeover.js",
    "bg/intercept.js",
    "bg/tab-media.js",
    "bg/sniffer.js",
    "bg/resolve.js",
    "bg/menus.js",
    "bg/messages.js"
  );
}

// Lifecycle & self-update
chrome.runtime.onInstalled.addListener(async (d) => {
  await cleanOnInstalled(d);
  aknanI18n.ready.then(makeMenus);
});
chrome.runtime.onStartup.addListener(() => {
  rememberLoadedFiles();
  aknanI18n.ready.then(makeMenus);
});
followAppVersion();

// Request headers & POST forms
try {
  // Chrome / Edge / Brave need "extraHeaders" to show Cookie and Referer
  const extra = chrome.webRequest.OnSendHeadersOptions && chrome.webRequest.OnSendHeadersOptions.EXTRA_HEADERS;
  chrome.webRequest.onSendHeaders.addListener(
    rememberRequest,
    { urls: ["<all_urls>"], types: ["main_frame", "sub_frame", "media", "object", "other", "xmlhttprequest"] },
    extra ? ["requestHeaders", extra] : ["requestHeaders"]
  );
} catch (_) {
  /* no request headers here: the cookies and the page still go along */
}

try {
  const loads = { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] };
  chrome.webRequest.onBeforeRequest.addListener(onFormBeforeRequest, loads, ["requestBody"]);
  chrome.webRequest.onBeforeSendHeaders.addListener(onFormBeforeSendHeaders, loads, ["requestHeaders"]);
} catch (_) {
  /* no form data here: such downloads stay in the browser */
}

// Browser download interception
if (DECIDE_EARLY) {
  chrome.downloads.onDeterminingFilename.addListener(onDeterminingFilename);
}
chrome.downloads.onCreated.addListener(onDownloadCreated);
if (!DECIDE_EARLY && IS_FIREFOX) {
  try {
    chrome.webRequest.onHeadersReceived.addListener(
      (d) => fileAnswer(d).catch(() => ({})),
      { urls: ["<all_urls>"], types: ["main_frame", "sub_frame"] },
      ["blocking", "responseHeaders"]
    );
  } catch (_) {
    /* no blocking here: downloads are decided when they start */
  }
}

// Media & YouTube stream capture (registered after Firefox blocking listener & form listener)
chrome.webRequest.onHeadersReceived.addListener(onMediaHeadersReceived, { urls: ["<all_urls>"] }, ["responseHeaders"]);
chrome.webRequest.onBeforeRequest.addListener(onYoutubeBeforeRequest, { urls: ["*://*.googlevideo.com/videoplayback*"] });

// Tab lifecycle
chrome.tabs.onCreated.addListener(trackNewTab);
chrome.tabs.onUpdated.addListener(async (tabId, info, tab) => {
  if (!info.url) return;
  const ytKey = `yt_${tabId}`;
  const { [ytKey]: yt } = await chrome.storage.session.get(ytKey);
  const newId = videoIdOf(info.url);
  if (yt && newId && yt.videoId === newId) return; // same video (e.g. timestamp change)
  seenStreams.delete(tabId);
  const resKey = `res_${tabId}`;
  const { [resKey]: res } = await chrome.storage.session.get(resKey);
  if (info.status === "loading" || yt || res) await clearTab(tabId);
  if (newId && isYoutube(info.url)) resolveYoutube(tabId, info.url);
});
chrome.tabs.onRemoved.addListener((tabId) => {
  seenStreams.delete(tabId);
  chrome.storage.session.remove([`media_${tabId}`, `yt_${tabId}`, `res_${tabId}`]);
});

// Options / switch / language changes
chrome.storage.onChanged.addListener((ch, area) => {
  if ((!area || area === "local") && ch && ch.appLang) makeMenus();
  if (area !== "local" || !ch.enabled) return;
  chrome.action.setBadgeText({ text: ch.enabled.newValue === false ? "off" : "" }).catch(() => {});
  chrome.tabs.query({}).then((tabs) => tabs.forEach((t) => t.id != null && pushToTab(t.id))).catch(() => {});
});

// Context menus & runtime messages
chrome.contextMenus.onClicked.addListener(onMenuClicked);
chrome.runtime.onMessage.addListener(onRuntimeMessage);
