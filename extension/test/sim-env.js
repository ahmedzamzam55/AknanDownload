// Shared VM simulation environment for background.js and helper tests.
const fs = require("fs");
const path = require("path");
const vm = require("vm");
const assert = require("assert");

const dir = path.join(__dirname, "..");

function createSimEnv(opts = {}) {
  const listeners = {};
  const all = {};
  const on = (n) => ({
    addListener: (f) => {
      (all[n] = all[n] || []).push(f);
      listeners[n] = f;
    },
  });
  const session = {};
  const localStore = {};
  const fetched = [];
  const posted = [];
  const tabMsgs = [];
  const cancelled = [];
  const browserDl = [];
  const state = {
    reloads: 0,
    tabUrl: "https://www.youtube.com/watch?v=AAAAAAAAAAA",
  };

  const chrome = {
    storage: {
      onChanged: on("store"),
      local: {
        get: async (d) =>
          typeof d === "string"
            ? d in localStore
              ? { [d]: localStore[d] }
              : {}
            : { ...(typeof d === "object" && !Array.isArray(d) ? d : {}), ...localStore },
        set: async (o) => Object.assign(localStore, o),
        remove: async () => {},
      },
      session: {
        get: async (k) => {
          const ks = Array.isArray(k) ? k : [k];
          const o = {};
          for (const x of ks) if (x in session) o[x] = JSON.parse(JSON.stringify(session[x]));
          return o;
        },
        set: async (o) => Object.assign(session, JSON.parse(JSON.stringify(o))),
        remove: async (k) => {
          for (const x of [].concat(k)) delete session[x];
        },
        clear: async () => {
          for (const k of Object.keys(session)) delete session[k];
        },
      },
    },
    downloads: {
      onCreated: on("dl"),
      onDeterminingFilename: on("det"),
      cancel: async (id) => {
        cancelled.push(id);
      },
      erase: async () => {},
      download: (o) => browserDl.push(o.url),
      search: async () => [],
      removeFile: async () => {},
    },
    webRequest: {
      onHeadersReceived: on("headers"),
      onBeforeRequest: on("before"),
      onBeforeSendHeaders: on("beforeSend"),
      onSendHeaders: on("sendHeaders"),
    },
    tabs: {
      onUpdated: on("upd"),
      onRemoved: on("rm"),
      onCreated: on("tabNew"),
      get: async (id) => ({ id, url: state.tabUrl }),
      query: async () => [{ id: 7 }],
      remove: async () => {},
      sendMessage: async (_id, m) => tabMsgs.push(m),
    },
    action: { setBadgeBackgroundColor: () => {}, setBadgeText: async () => {} },
    runtime: {
      id: "abcdefghijklmnopabcdefghijklmnop",
      onInstalled: on("inst"),
      onStartup: on("start"),
      onMessage: on("msg"),
      getManifest: () => ({ version: opts.version || "1.0.12" }),
      reload: () => {
        state.reloads++;
      },
      getURL: (p) => "chrome-extension://sim/" + p,
    },
    permissions: { contains: async () => !!opts.granted },
    contextMenus: { removeAll: () => {}, create: () => {}, onClicked: on("ctx") },
    cookies: {
      getAll: async (q) => (q && q.storeId === "1" ? [{ name: "private", value: "yes" }] : []),
      getAllCookieStores: async () => [{ id: "0", tabIds: [7] }, { id: "1", tabIds: [9] }],
    },
  };

  if (opts.helper) {
    chrome.runtime.connectNative = (name) => {
      assert.strictEqual(name, "com.aknan.download");
      const msgL = [];
      const discL = [];
      return {
        onMessage: { addListener: (f) => msgL.push(f) },
        onDisconnect: { addListener: (f) => discL.push(f) },
        disconnect: () => {},
        postMessage: (m) => {
          posted.push(m);
          setTimeout(() => {
            const a = opts.helper(m);
            if (a === "gone") discL.forEach((f) => f());
            else msgL.forEach((f) => f({ id: m.id, ...a }));
          }, 5);
        },
      };
    };
  }

  const fetch = async (url, fetchOpts = {}) => {
    fetched.push(url);
    if (url === "chrome-extension://sim/token.json") return { ok: true, status: 200, json: async () => ({ token: "k".repeat(64), port: 47000 }) };
    if (url.startsWith("chrome-extension://sim/")) return { ok: true, status: 200, text: async () => "file" };
    if (opts.onFetch) return opts.onFetch(url, fetchOpts);
    throw new TypeError("Failed to fetch");
  };

  const ctx = {
    chrome,
    fetch,
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
    navigator: { userAgent: opts.userAgent || (opts.firefox ? "Mozilla/5.0 Firefox/150.0" : "Chrome/150") },
    importScripts: (...f) => f.forEach((x) => vm.runInContext(fs.readFileSync(path.join(dir, x), "utf8"), ctx)),
  };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(dir, "background.js"), "utf8"), ctx);

  return { ctx, listeners, all, session, localStore, fetched, posted, tabMsgs, cancelled, browserDl, state };
}

module.exports = { createSimEnv };
