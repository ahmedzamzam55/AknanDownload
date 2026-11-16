// Browser download interception hooks (Chromium determiningFilename + Firefox pause/headers).

// Chrome, Edge, Brave, Opera: decided before the browser asks where to
// save it, so its own "Save as" window never shows up for files the app
// takes. The browser's download waits meanwhile and goes on if the app does
// not take it. Firefox has no such moment: its download is paused while the
// app is asked, and resumed if the app does not take it.
const DECIDE_EARLY = !!(chrome.downloads.onDeterminingFilename && chrome.downloads.onDeterminingFilename.addListener);

function onDeterminingFilename(item, suggest) {
  let answered = false;
  const keep = () => {
    if (!answered) {
      answered = true;
      try {
        suggest();
      } catch (_) {
        /* the download is gone */
      }
    }
  };
  // never leave the browser waiting long (the download goes on with it,
  // and is still taken over if the app takes it afterwards)
  const guard = setTimeout(keep, 9000);
  decide(item)
    .then(async (d) => {
      if (!d.take) {
        clearTimeout(guard);
        keep();
        if (d.why) await leftToBrowser(d.why, item);
        return;
      }
      const taken = await takeOver(item, d);
      clearTimeout(guard);
      if (taken) answered = true;
      else keep();
    })
    .catch(() => {
      clearTimeout(guard);
      keep();
    });
  return true; // answered later
}

// Firefox: page loads decided from their answer (below), not asked again
// when they become a download
const DECIDED = new Map();

async function onDownloadCreated(item) {
  if (DECIDE_EARLY) return;
  forgetOld(DECIDED, 120000);
  if (DECIDED.has(item.url) || DECIDED.has(item.finalUrl)) return;
  // paused while the app is asked: nothing is saved twice
  let paused = false;
  try {
    await chrome.downloads.pause(item.id);
    paused = true;
  } catch {
    /* not pausable (no resume on that server) or finished already */
  }
  const d = await decide(item).catch(() => ({ take: false, why: "" }));
  const taken = d.take && (await takeOver(item, d));
  if (!taken) {
    if (d.why) await leftToBrowser(d.why, item);
    if (paused) chrome.downloads.resume(item.id).catch(() => {});
  }
}

// Firefox: a file is caught from the answer to the page load, before Firefox
// asks what to do with it (and downloads it behind that window): the load
// waits while the app is asked, and is stopped if the app takes it.
const SHOWN_BY_BROWSER = /^(text\/|image\/|video\/|audio\/|multipart\/|application\/(pdf|xhtml\+xml|xml|json|javascript|x-javascript|ecmascript|ogg|x-xpinstall|[\w.+-]+\+(xml|json))$)/;

function nameInDisposition(cd) {
  const star = /filename\*\s*=\s*[\w-]+'[^']*'([^;]+)/i.exec(cd);
  if (star) {
    try {
      return decodeURIComponent(star[1].trim().replace(/^"|"$/g, ""));
    } catch {
      /* the plain name below */
    }
  }
  const plain = /(?:^|;)\s*filename\s*=\s*("((?:[^"\\]|\\.)*)"|[^;]+)/i.exec(cd);
  if (!plain) return "";
  return (plain[2] !== undefined ? plain[2].replace(/\\(.)/g, "$1") : plain[1]).trim();
}

async function fileAnswer(d) {
  if (d.method !== "GET" && d.method !== "POST") return {};
  const code = d.statusCode || 0;
  if (code < 200 || code > 299 || code === 204 || code === 205 || code === 206) return {};
  const get = (n) => (((d.responseHeaders || []).find((h) => h.name.toLowerCase() === n) || {}).value || "").trim();
  const mime = get("content-type").split(";")[0].trim().toLowerCase();
  const cd = get("content-disposition");
  if (!/^attachment/i.test(cd) && (!mime || SHOWN_BY_BROWSER.test(mime))) return {};
  let name = nameInDisposition(cd);
  if (!name) {
    try {
      name = decodeURIComponent(d.url.split(/[?#]/)[0].split("/").pop() || "");
    } catch {
      name = d.url.split(/[?#]/)[0].split("/").pop() || "";
    }
  }
  const size = get("content-encoding") ? 0 : Number(get("content-length")) || 0;
  const sent = browserRequest(d.url);
  const ref = sent && sent.find((h) => /^referer$/i.test(h.name));
  const item = {
    url: d.url,
    finalUrl: d.url,
    filename: name.split(/[\\/]/).pop() || "",
    mime,
    totalBytes: size,
    referrer: (ref && ref.value) || d.originUrl || "",
    incognito: !!d.incognito,
    cookieStoreId: d.cookieStoreId,
  };
  forgetOld(DECIDED, 120000);
  DECIDED.set(d.url, { at: Date.now() });
  const dec = await decide(item).catch(() => ({ take: false, why: "" }));
  if (!dec.take) {
    if (dec.why) await leftToBrowser(dec.why, item);
    return {};
  }
  if (!(await takeOver(item, dec))) return {};
  closeEmptyTab([d.url], d.tabId).catch(() => {});
  return { cancel: true };
}
