import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

function event() {
  let handler;
  return { addListener(fn) { handler = fn; }, fire(...args) { return handler(...args); } };
}

test("browser reads only with a current host reply and origin permission", async () => {
  const sent = [];
  const port = { onMessage: event(), onDisconnect: event(), postMessage: msg => sent.push(msg) };
  const state = { queries: 0, injections: 0, marked: false, granted: false };
  const chrome = {
    runtime: { connectNative: () => port, onMessage: event() },
    alarms: { create() {}, onAlarm: event() },
    tabs: { onActivated: event(), onUpdated: event(), async query() {
      state.queries++;
      return [{ id: 1, title: state.title ?? "fixture", url: "https://example.invalid/lesson" }];
    } },
    windows: { onFocusChanged: event() },
    permissions: { onRemoved: event(), contains: async () => state.granted },
    storage: { session: { set() {} }, onChanged: event(),
      local: { get: async () => ({ study: state.marked ? ["example.invalid"] : [] }) } },
    declarativeNetRequest: { getDynamicRules: async () => [], updateDynamicRules: async () => {} },
    scripting: { async executeScript({ func, args }) {
      state.injections++;
      if (state.disconnect) port.onDisconnect.fire();
      if (state.navigated) {
        const result = vm.runInNewContext('(' + func + ')(origin)', {
          origin: args[0], location: { origin: "https://elsewhere.invalid" },
          get document() { state.unrelatedRead = true; throw Error("read unrelated document"); },
        });
        assert.equal(result, null);
        return [{ result: null }];
      }
      return [{ result: { origin: args[0], text: "synthetic excerpt" } }];
    } },
  };
  vm.runInNewContext(readFileSync(new URL("../extension/background.js", import.meta.url), "utf8"), { chrome, URL });
  async function poll(capture) {
    chrome.alarms.onAlarm.fire();
    port.onMessage.fire({ ok: true, capture, request: sent.at(-1).request, session: capture !== null, allowed: capture === null ? [] : ["google.com"] });
    await new Promise(resolve => setImmediate(resolve));
  }
  const pages = () => sent.filter(m => m.type === "page");
  await poll(null);
  assert.equal(state.queries, 0);
  await poll(1);
  assert.equal(pages().at(-1).excerpt, null);
  state.marked = true;
  await poll(1);
  assert.equal(state.injections, 0);
  state.granted = true;
  await poll(1);
  assert.equal(pages().at(-1).excerpt, "synthetic excerpt");
  assert.equal(sent.at(-2).type, "site_permission");
  state.navigated = true;
  state.title = "navigation";
  await poll(1);
  assert.equal(pages().at(-1).excerpt, null);
  assert.equal(state.unrelatedRead, undefined);
  const count = pages().length;
  state.navigated = false;
  state.disconnect = true;
  await poll(2);
  assert.equal(pages().length, count);
});
