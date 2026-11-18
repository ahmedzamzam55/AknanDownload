// Verifies manifest scripts, HTML script tags, Firefox background boot, and content script boot.
// node extension/test/bundle.test.js
const fs = require("fs");
const path = require("path");
const vm = require("vm");
const assert = require("assert");

const dir = path.join(__dirname, "..");

const mChrome = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
const mFirefox = JSON.parse(fs.readFileSync(path.join(dir, "manifest.firefox.json"), "utf8"));

// 1. Every file referenced in manifests and HTML pages exists
for (const m of [mChrome, mFirefox]) {
  for (const cs of m.content_scripts) {
    for (const f of cs.js) assert.ok(fs.existsSync(path.join(dir, f)), "missing content script: " + f);
  }
}
for (const f of mFirefox.background.scripts) assert.ok(fs.existsSync(path.join(dir, f)), "missing firefox bg script: " + f);

for (const html of ["popup.html", "options.html"]) {
  const src = fs.readFileSync(path.join(dir, html), "utf8");
  for (const m of src.matchAll(/<script\s+src="([^"]+)"/g)) {
    assert.ok(fs.existsSync(path.join(dir, m[1])), `missing script in ${html}: ${m[1]}`);
  }
}

// Check selfupdate OWN_FILES covers all shipped js/html files + manifest.json
{
  const su = fs.readFileSync(path.join(dir, "bg/selfupdate.js"), "utf8");
  const m = /const OWN_FILES = (\[[\s\S]*?\]);/.exec(su);
  assert.ok(m, "OWN_FILES found");
  const own = new Set(vm.runInNewContext(m[1]));
  const walk = (sub) => {
    const out = [];
    for (const ent of fs.readdirSync(path.join(dir, sub), { withFileTypes: true })) {
      const rel = sub ? `${sub}/${ent.name}` : ent.name;
      if (ent.isDirectory()) {
        if (rel !== "test" && rel !== "icons") out.push(...walk(rel));
      } else if (/\.(js|html)$/.test(ent.name) || ent.name === "manifest.json") {
        out.push(rel);
      }
    }
    return out;
  };
  for (const rel of walk("")) assert.ok(own.has(rel), "missing from OWN_FILES: " + rel);
}

// 2. Firefox background scripts load sequentially without importScripts
{
  const on = () => ({ addListener: () => {} });
  const chrome = {
    storage: { onChanged: on(), local: { get: async () => ({}), set: async () => {}, remove: async () => {} }, session: { get: async () => ({}), set: async () => {}, remove: async () => {}, clear: async () => {} } },
    downloads: { onCreated: on(), onDeterminingFilename: on() },
    webRequest: { onHeadersReceived: on(), onBeforeRequest: on(), onBeforeSendHeaders: on(), onSendHeaders: on() },
    tabs: { onUpdated: on(), onRemoved: on(), onCreated: on(), query: async () => [] },
    action: { setBadgeBackgroundColor: () => {}, setBadgeText: async () => {} },
    runtime: { id: "fx", onInstalled: on(), onStartup: on(), onMessage: on(), getManifest: () => ({ version: "1.0.12" }), getURL: (p) => p },
    permissions: { contains: async () => false },
    contextMenus: { removeAll: () => {}, create: () => {}, onClicked: on() },
    cookies: { getAll: async () => [] },
  };
  const ctx = {
    chrome,
    fetch: async () => ({ ok: false }),
    AbortController,
    TextEncoder,
    TextDecoder,
    URLSearchParams,
    btoa,
    crypto: globalThis.crypto,
    setTimeout,
    clearTimeout,
    console,
    URL,
    navigator: { userAgent: "Mozilla/5.0 Firefox/150.0" },
  };
  vm.createContext(ctx);
  for (const f of mFirefox.background.scripts) {
    vm.runInContext(fs.readFileSync(path.join(dir, f), "utf8"), ctx);
  }
  assert.strictEqual(typeof ctx.aknanI18n.t, "function");
  assert.strictEqual(typeof ctx.fetchWithTimeout, "function");
  assert.strictEqual(ctx.__aknanBgModulesLoaded, true);
}

// 3. Content scripts load sequentially in isolated world and guard re-injection
{
  const noop = () => {};
  const docEl = { appendChild: noop };
  const doc = {
    documentElement: docEl,
    body: { innerText: "" },
    title: "Test Page",
    querySelectorAll: () => [],
    addEventListener: noop,
    createElement: () => ({ setAttribute: noop, append: noop, style: {}, addEventListener: noop }),
  };
  const ctx = {
    location: { hostname: "example.com", href: "https://example.com/", search: "" },
    document: doc,
    innerWidth: 1280,
    innerHeight: 720,
    addEventListener: noop,
    setInterval: () => 1,
    setTimeout,
    clearTimeout,
    getSelection: () => null,
    URL,
    chrome: {
      storage: { local: { get: noop, set: noop }, onChanged: { addListener: noop } },
      runtime: { sendMessage: noop, onMessage: { addListener: noop }, lastError: null },
    },
  };
  ctx.window = ctx;
  ctx.window.top = ctx.window;
  vm.createContext(ctx);
  for (const f of mChrome.content_scripts[1].js) {
    vm.runInContext(fs.readFileSync(path.join(dir, f), "utf8"), ctx);
  }
  assert.strictEqual(ctx.__aknanDownload, true);
  assert.strictEqual(typeof ctx.__aknanCS.place, "function");
  assert.strictEqual(typeof ctx.__aknanCS.renderBubble, "function");
  assert.strictEqual(typeof ctx.__aknanCS.clipPanel, "function");
  assert.strictEqual(typeof ctx.__aknanCS.collectLinks, "function");
  // Re-injection is a no-op
  for (const f of mChrome.content_scripts[1].js) {
    vm.runInContext(fs.readFileSync(path.join(dir, f), "utf8"), ctx);
  }
}

console.log("bundle.test: all checks passed");
