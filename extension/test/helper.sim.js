// The add-in talks to the program through the program's helper (native
// messaging); without it (or when it can't reach the program) the port.
// node extension/test/helper.sim.js
const assert = require("assert");
const vm = require("vm");
const { createSimEnv } = require("./sim-env.js");

function load({ helper, firefox = false, granted = false }) {
  return createSimEnv({
    helper,
    firefox,
    granted,
    onFetch: async (url) => {
      if (url.includes(":9614/ping") || url.includes(":47000/")) {
        return { ok: true, status: 200, json: async () => ({ ok: true, via: "port" }) };
      }
      throw new TypeError("Failed to fetch");
    },
  });
}

const run = (s, code) => vm.runInContext(code, s.ctx);

(async () => {
  // 1. the helper answers: nothing goes over the port
  let s = load({ helper: (m) => ({ status: 200, body: JSON.stringify({ ok: true, via: "helper", path: m.path }) }) });
  await new Promise((r) => setTimeout(r, 30));
  s.fetched.length = 0;
  let r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), { headers: { "X-SwiftGet": "1" } }, 1000)`);
  assert.strictEqual((await r.json()).via, "helper");
  assert.ok(!s.fetched.some((u) => u.includes("127.0.0.1")), "no port at all: " + s.fetched);
  const m = s.posted.find((x) => x.path === "/config");
  assert.strictEqual(m.headers["X-Aknan-Id"], "abcdefghijklmnopabcdefghijklmnop");
  assert.strictEqual(m.start, false);
  assert.strictEqual(await run(s, `appAlive()`), true);
  // a download may open a closed program
  await run(s, `fetchWithTimeout(apiUrl(9614, "/add"), { method: "POST", body: "{}" }, 1000)`);
  assert.strictEqual(s.posted.find((x) => x.path === "/add").start, true);

  // 2. the helper can't reach the program: the port is tried as before
  s = load({ helper: () => ({ error: "down" }) });
  r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), { headers: { "X-SwiftGet": "1" } }, 1000)`);
  assert.strictEqual((await r.json()).via, "port");

  // 3. no helper on this computer (an older program): the port, and the
  //    helper is not asked again for a while
  s = load({ helper: () => "gone" });
  r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), { headers: { "X-SwiftGet": "1" } }, 1000)`);
  assert.strictEqual((await r.json()).via, "port");
  const asked = s.posted.length;
  await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), {}, 1000)`);
  assert.strictEqual(s.posted.length, asked, "not asked again at once");

  // 4. the program takes too long through the helper: told so (not "not running")
  s = load({ helper: () => ({ error: "timeout" }) });
  await assert.rejects(run(s, `fetchWithTimeout(apiUrl(9614, "/resolve"), { method: "POST", body: "{}" }, 1000)`), (e) => e.name === "AbortError");

  // 5. Firefox: only after the user allowed it
  s = load({ helper: () => ({ status: 200, body: '{"via":"helper"}' }), firefox: true, granted: false });
  r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), {}, 1000)`);
  assert.strictEqual((await r.json()).via, "port");
  assert.strictEqual(s.posted.length, 0);
  s = load({ helper: () => ({ status: 200, body: '{"via":"helper"}' }), firefox: true, granted: true });
  r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), {}, 1000)`);
  assert.strictEqual((await r.json()).via, "helper");

  // 6. the browser has no native messaging at all: the port
  s = load({ helper: null });
  r = await run(s, `fetchWithTimeout(apiUrl(9614, "/config"), {}, 1000)`);
  assert.strictEqual((await r.json()).via, "port");
  console.log("helper.sim: all good");
  process.exit(0);
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
