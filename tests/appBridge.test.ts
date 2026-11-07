import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { appBridge, appCommandKeys, type AppMessage } from "../src/appBridge.ts";
import { fetchUpdateStatus } from "../src/useUpdate.ts";

const global = globalThis as { window?: unknown };
afterEach(() => {
  delete global.window;
});

test("a browser has no app bridge", () => {
  assert.equal(appBridge(), null, "no page");
  global.window = {};
  assert.equal(appBridge(), null, "a page without westermApp");
  global.window = { westermApp: { request: "not a function" } };
  assert.equal(appBridge(), null, "a westermApp without request");
});

test("the desktop app's bridge passes requests on", async () => {
  const asked: AppMessage[] = [];
  global.window = { westermApp: { request: async (message: AppMessage) => (asked.push(message), "relaunching") } };
  const bridge = appBridge();
  assert.ok(bridge);
  assert.equal(await bridge.request({ type: "update" }), "relaunching");
  assert.deepEqual(asked, [{ type: "update" }]);
});

test("⌘ terminal keys belong to the macOS app only", () => {
  const bridge = { request: async () => undefined };
  assert.equal(appCommandKeys(bridge, true), true, "macOS app");
  assert.equal(appCommandKeys(bridge, false), false, "Windows and Linux app: ⌘ is the Windows or Super key");
  assert.equal(appCommandKeys(null, true), false, "Safari or Chrome on a Mac keep ⌘ keys");
  assert.equal(appCommandKeys(null, false), false);
});

test("in the app, the update panel asks the app about itself", async () => {
  const asked: AppMessage[] = [];
  const answer = { current: "0.2.1", channel: "app", latest: "0.3.0", newer: true };
  const bridge = { request: async (message: AppMessage) => (asked.push(message), answer) };
  assert.deepEqual(await fetchUpdateStatus(true, bridge), answer);
  assert.deepEqual(await fetchUpdateStatus(false, bridge), answer);
  assert.deepEqual(asked, [
    { type: "updateStatus", refresh: true },
    { type: "updateStatus", refresh: false },
  ]);
});
