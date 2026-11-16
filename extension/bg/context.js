// Cookies, site predicates, and localized DRM/Telegram explanatory texts.
/**
 * The site's cookies as the browser sends them. `store`: the cookie store of
 * the window (a private window, a Firefox container) the file came from.
 */
async function cookieHeader(url, store) {
  try {
    const q = store ? { url, storeId: store } : { url };
    const cookies = await chrome.cookies.getAll(q);
    return cookies.map((c) => `${c.name}=${c.value}`).join("; ");
  } catch {
    return "";
  }
}

/** The cookie store of a download / a tab: a private window has its own. */
async function storeOf(item) {
  if (!item) return undefined;
  if (item.cookieStoreId) return item.cookieStoreId;
  if (!item.incognito) return undefined;
  try {
    const stores = await chrome.cookies.getAllCookieStores();
    const tabs = await chrome.tabs.query({});
    const privateTabs = new Set(tabs.filter((t) => t.incognito).map((t) => t.id));
    const s =
      stores.find((st) => (st.tabIds || []).some((id) => privateTabs.has(id))) ||
      // the private store when no private tab is open any more (the
      // download's tab closed): the one that is not the normal one
      stores.find((st) => st.id === "firefox-private") ||
      stores.find((st) => st.id !== "0" && st.id !== "firefox-default");
    return s ? s.id : undefined;
  } catch {
    return undefined;
  }
}

/** The site's cookies for the app, only used if the site asks for a login. */
async function cookieList(url) {
  try {
    const list = await chrome.cookies.getAll({ url });
    return list.map((c) => ({
      domain: c.domain,
      name: c.name,
      value: c.value,
      path: c.path || "/",
      secure: !!c.secure,
      expirationDate: c.expirationDate || null,
    }));
  } catch {
    return [];
  }
}

function isYoutube(url) {
  try {
    return /(^|\.)youtube\.com$|^youtu\.be$/i.test(new URL(url).hostname);
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------- protected videos
// A video protected by its site (DRM): no program can download it. The
// message says so plainly, in the browser's language.
const DRM_TEXTS = {
  ar: "🔒 الفيديو ده محمي من صاحب الموقع (DRM) ومينفعش يتحمّل بأي برنامج تحميل. تقدر تتفرج عليه من الموقع بس.",
  fr: "🔒 Cette vidéo est protégée par son site (DRM) : aucun programme ne peut la télécharger. Elle se regarde seulement sur le site.",
  es: "🔒 Este vídeo está protegido por su sitio (DRM): ningún programa puede descargarlo. Solo se puede ver en el sitio.",
  de: "🔒 Dieses Video ist von seiner Seite geschützt (DRM): Kein Programm kann es herunterladen. Es lässt sich nur auf der Seite ansehen.",
  pt: "🔒 Este vídeo é protegido pelo site (DRM): nenhum programa consegue baixá-lo. Ele só pode ser assistido no site.",
  tr: "🔒 Bu video sitesi tarafından korunuyor (DRM): hiçbir program indiremez. Yalnızca sitede izlenebilir.",
  ru: "🔒 Это видео защищено сайтом (DRM): ни одна программа не может его скачать. Его можно смотреть только на сайте.",
  en: "🔒 This video is protected by its site (DRM): no download program can save it. It can only be watched on the site.",
};
// Telegram Web plays its videos through its own page (a "stream" address
// that exists only inside that page): no program can download it from
// outside, Telegram's own download button can.
const TELEGRAM_TEXTS = {
  ar: "الفيديو ده في تيليجرام ويب مينفعش يتحمّل من برا الصفحة. نزّله من زرار التحميل بتاع تيليجرام نفسه (⋮ ← تنزيل).",
  fr: "Cette vidéo de Telegram Web ne peut pas être téléchargée hors de la page. Utilisez le bouton de téléchargement de Telegram (⋮ → Télécharger).",
  es: "Este vídeo de Telegram Web no se puede descargar fuera de la página. Usa el botón de descarga de Telegram (⋮ → Descargar).",
  de: "Dieses Video in Telegram Web lässt sich nicht außerhalb der Seite laden. Nutze den Download-Knopf von Telegram (⋮ → Herunterladen).",
  pt: "Este vídeo do Telegram Web não pode ser baixado fora da página. Use o botão de download do próprio Telegram (⋮ → Baixar).",
  tr: "Telegram Web'deki bu video sayfanın dışından indirilemez. Telegram'ın kendi indirme düğmesini kullanın (⋮ → İndir).",
  ru: "Это видео в Telegram Web нельзя скачать вне страницы. Используйте кнопку загрузки самого Telegram (⋮ → Скачать).",
  en: "This Telegram Web video can't be downloaded outside the page. Use Telegram's own download button (⋮ → Download).",
};
const isTelegramStream = (url) => /^https:\/\/web\.telegram\.org\/(?:[a-z]\/)?stream\//i.test(url || "");
function telegramText() {
  const lang = aknanI18n.lang();
  return TELEGRAM_TEXTS[lang] || TELEGRAM_TEXTS.en;
}
const isDrm = (text) => /\bDRM\b|drm protected|widevine|playready/i.test(text || "");
function drmText() {
  const lang = aknanI18n.lang();
  return DRM_TEXTS[lang] || DRM_TEXTS.en;
}
