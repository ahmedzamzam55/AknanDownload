// ---------------------------------------------------------------- 4. context menu

function makeMenus() {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: "swiftget-link", title: aknanI18n.t("Download with Aknan Download"), contexts: ["link"] });
    chrome.contextMenus.create({
      id: "swiftget-media",
      title: aknanI18n.t("Download this media with Aknan Download"),
      contexts: ["video", "audio", "image"],
    });
    chrome.contextMenus.create({
      id: "swiftget-video",
      title: aknanI18n.t("Download video with Aknan Download"),
      contexts: ["page", "video", "frame"],
    });
    chrome.contextMenus.create({
      id: "swiftget-all",
      title: aknanI18n.t("Download all links with Aknan Download"),
      contexts: ["page", "frame", "link", "image"],
    });
    chrome.contextMenus.create({
      id: "swiftget-selected",
      title: aknanI18n.t("Download selected links with Aknan Download"),
      contexts: ["selection"],
    });
  });
}

/** Every link of the page (or of the selection) to the app's "Download all links". */
async function sendAllLinks(tab, selection) {
  const res = await chrome.tabs.sendMessage(tab.id, { type: "swiftget-collect-links", selection }).catch(() => null);
  if (!res || !res.links || !res.links.length) return;
  const { port } = await getOpts();
  await fetchWithTimeout(
    apiUrl(port, "/links"),
    {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-SwiftGet": "1" },
      body: JSON.stringify({
        page: res.page || tab.url,
        links: res.links,
        cookies: await cookieHeader(res.page || tab.url),
        userAgent: navigator.userAgent,
      }),
    },
    5000
  );
}

function onMenuClicked(info, tab) {
  if (info.menuItemId === "swiftget-all" || info.menuItemId === "swiftget-selected") {
    if (tab) sendAllLinks(tab, info.menuItemId === "swiftget-selected").catch((e) => console.warn("Aknan Download:", e));
    return;
  }
  if (info.menuItemId === "swiftget-video") {
    if (!tab) return;
    // find the qualities and open the panel on the page
    if (isYoutube(tab.url)) resolveYoutube(tab.id, tab.url);
    else resolvePage(tab.id, info.frameUrl && info.frameUrl !== tab.url ? tab.url : tab.url);
    chrome.tabs.sendMessage(tab.id, { type: "swiftget-open-panel" }).catch(() => {});
    pushToTab(tab.id);
    return;
  }
  const url = info.linkUrl || info.srcUrl;
  if (!url || !/^https?:/i.test(url)) return;
  storeOf(tab)
    .then((store) => sendToApp({ url, referrer: info.pageUrl, store }))
    .catch((e) => console.warn("Aknan Download:", e));
}
