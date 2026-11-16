// Aknan Download extension i18n (all 8 interface languages of the desktop app).
// Defaults to "en" when unset so automated Node tests without navigator.language keep working.
var aknanI18n = (() => {
  const LANGS = ["en", "ar", "fr", "es", "de", "pt", "tr", "ru"];
  const dicts = typeof globalThis !== "undefined" ? (globalThis.__aknanLangs = globalThis.__aknanLangs || {}) : {};
  if (typeof require === "function" && typeof module !== "undefined") {
    for (const code of LANGS) {
      if (code !== "en" && !dicts[code]) {
        try {
          dicts[code] = require(`./i18n/${code}.js`);
        } catch {}
      }
    }
  }

  const STRINGS = {};
  for (const code of LANGS) {
    if (code === "en" || !dicts[code]) continue;
    for (const [k, v] of Object.entries(dicts[code])) {
      (STRINGS[k] = STRINGS[k] || {})[code] = v;
    }
  }

  const normalize = (code) => {
    const base = String(code || "").trim().slice(0, 2).toLowerCase();
    return LANGS.includes(base) ? base : "";
  };

  let storedLang = "";

  const lang = () => {
    if (storedLang) return storedLang;
    if (typeof navigator !== "undefined" && navigator.language) {
      return normalize(navigator.language) || "en";
    }
    return "en";
  };

  const setLang = (code) => {
    storedLang = normalize(code);
  };

  let ready = Promise.resolve("en");
  try {
    if (typeof chrome !== "undefined" && chrome.storage && chrome.storage.local) {
      ready = new Promise((resolve) => {
        try {
          const res = chrome.storage.local.get({ appLang: "" }, (o) => {
            storedLang = normalize(o && o.appLang);
            resolve(lang());
          });
          if (res && typeof res.then === "function") {
            res.then(
              (o) => {
                storedLang = normalize(o && o.appLang);
                resolve(lang());
              },
              () => resolve(lang())
            );
          }
        } catch {
          resolve(lang());
        }
      });
      if (chrome.storage.onChanged && chrome.storage.onChanged.addListener) {
        chrome.storage.onChanged.addListener((ch, area) => {
          if (area && area !== "local") return;
          if (ch && ch.appLang) storedLang = normalize(ch.appLang.newValue);
        });
      }
    }
  } catch {
    /* headless / test environment */
  }

  const t = (key, ...args) => {
    const l = lang();
    const row = STRINGS[key];
    let text = (row && row[l]) || key;
    if (!args.length) return text;
    let i = 0;
    return text.replace(/\{\}/g, () => (i < args.length ? String(args[i++]) : "{}"));
  };

  return {
    LANGS,
    STRINGS,
    ready,
    lang,
    setLang,
    isRtl: () => lang() === "ar",
    t,
  };
})();

if (typeof globalThis !== "undefined") globalThis.aknanI18n = aknanI18n;
if (typeof module !== "undefined" && module.exports) module.exports = aknanI18n;
