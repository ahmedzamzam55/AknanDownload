// Download takeover helpers: empty tab cleanup, browser download drop, and link clicks.

// Tabs opened in the last seconds: one opened only for a download (a link
// with target="_blank") is closed when the app takes the download.
const NEW_TABS = new Map();

function trackNewTab(tab) {
  forgetOld(NEW_TABS, 15000);
  NEW_TABS.set(tab.id, { at: Date.now() });
}

async function closeEmptyTab(urls, tabId) {
  forgetOld(NEW_TABS, 15000);
  const ids = tabId >= 0 ? [tabId] : [...NEW_TABS.keys()];
  for (const id of ids) {
    if (!NEW_TABS.has(id)) continue;
    try {
      const tab = await chrome.tabs.get(id);
      const at = tab.pendingUrl || tab.url || "";
      // only a tab opened for the file's own address (or still empty), never one the user opened
      if (urls.includes(at) || ((at === "" || at === "about:blank") && tabId >= 0)) {
        NEW_TABS.delete(id);
        await chrome.tabs.remove(id);
      }
    } catch {
      NEW_TABS.delete(id);
    }
  }
}

/** The browser's own copy of a download the app has: gone without a trace. */
async function dropBrowserCopy(item) {
  if (item.id === undefined) return;
  try {
    await chrome.downloads.cancel(item.id);
  } catch {
    /* finished already */
  }
  try {
    const [d] = await chrome.downloads.search({ id: item.id });
    if (d && d.state === "complete" && d.exists !== false) await chrome.downloads.removeFile(item.id);
  } catch {
    /* gone already */
  }
  try {
    await chrome.downloads.erase({ id: item.id });
  } catch {
    /* gone already */
  }
  closeEmptyTab([item.url, item.finalUrl].filter(Boolean)).catch(() => {});
}

/**
 * Give the download to the app; only once the app has it does the browser's
 * own copy go (a link that works once is never lost). True: the app took it.
 */
async function takeOver(item, decided) {
  const url = item.finalUrl || item.url;
  const d = decided || { take: true };
  if (d.already || (await sentRecently(url))) {
    await dropBrowserCopy(item);
    return true;
  }
  try {
    // a form's download goes from the form's own address (its answer may send it on)
    await sendToApp({
      url: d.form ? item.url : url,
      alsoUrl: item.url,
      referrer: item.referrer,
      fileName: baseName(item.filename),
      store: await storeOf(item),
      form: d.form,
    });
  } catch (e) {
    // the app may have got it anyway (it was only slow to answer): never
    // shown twice
    if (e && e.name === "AbortError") {
      await dropBrowserCopy(item);
      return true;
    }
    // the app could not be reached: the browser keeps it
    await leftToBrowser("not-running", item);
    return false;
  }
  await dropBrowserCopy(item);
  return true;
}

/** A file link the page caught a click on: to the app, or back to the page. */
async function takeLink(url, referrer, store) {
  if (!/^https?:/i.test(url || "")) throw new Error("not a web address");
  const opts = await getOpts();
  if (!opts.enabled || !opts.intercept) throw new Error("sending is off");
  if (!(await appAlive())) throw new Error("Aknan Download is not running");
  if (await appSaysBrowser([url])) throw new Error("the browser keeps it");
  // the same link clicked twice in a row: once is enough
  if (!(await sentRecently(url))) await sendToApp({ url, referrer, store });
  // the browser may still start it (a second handler of the page): not again
  await addBypassTaken(url);
  return true;
}

// links just handed over from a click: if the browser starts them anyway, they are dropped
const TAKEN_KEY = "takenLinks";
async function addBypassTaken(url) {
  const list = (await chrome.storage.session.get(TAKEN_KEY))[TAKEN_KEY] || [];
  list.push({ url, at: Date.now() });
  await chrome.storage.session.set({ [TAKEN_KEY]: list.slice(-50) });
}
async function justTaken(url) {
  const list = (await chrome.storage.session.get(TAKEN_KEY))[TAKEN_KEY] || [];
  return list.some((l) => l.url === url && Date.now() - l.at < 15000);
}
