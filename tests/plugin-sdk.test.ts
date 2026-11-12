import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { Window } from "happy-dom";

const SDK = readFileSync(new URL("../native/src/server/plugins/external/sdk_v1.js", import.meta.url), "utf8");

type Listener = (data: unknown) => void;
type Sdk = {
  run(program: string, args: string[], options?: { cwd?: string }): Promise<{ code: number; stdout: string; stderr: string }>;
  openShell(options: { program: string; args?: string[]; cwd?: string; name?: string; reuse?: boolean }): Promise<unknown>;
  openBrowser(url: string): Promise<unknown>;
  storage: { get(): Promise<unknown>; set(value: unknown): Promise<unknown> };
  setStatus(status: { badge?: number | null; alert?: boolean }): Promise<unknown>;
  on(event: string, listener: Listener): () => void;
  ready: Promise<{ id: string; locale: string }>;
};

/** A copy made in this realm: values the SDK makes have the window's prototypes. */
const plain = (value: unknown) => JSON.parse(JSON.stringify(value)) as unknown;

/** The SDK running in a fresh window whose parent is captured. */
function load() {
  const window = new Window();
  // In happy-dom, the script's own `window` is the context global; replies
  // must come from that object for `event.source === window.parent` to hold.
  const parent = window.eval("window.parent") as { postMessage: (text: unknown, target: string) => void };
  const posted: string[] = [];
  parent.postMessage = (text) => posted.push(text as string);
  window.eval(SDK);
  const reply = (message: unknown, source: unknown = parent, raw = false) =>
    window.dispatchEvent(new window.MessageEvent("message", { data: raw ? message : JSON.stringify(message), source: source as never }));
  const westerm = (window as unknown as { westerm: Sdk }).westerm;
  return { window, posted, reply, westerm };
}

test("calls post one JSON string and resolve with the reply", async () => {
  const sdk = load();
  const promise = sdk.westerm.run("kubectl", ["get", "pods"]);
  assert.equal(typeof sdk.posted[0], "string");
  assert.deepEqual(JSON.parse(sdk.posted[0]), { westerm: 1, id: 1, call: "run", args: { program: "kubectl", args: ["get", "pods"], cwd: null } });
  sdk.reply({ westerm: 1, id: 1, ok: true, result: { code: 0, stdout: "x", stderr: "" } });
  assert.equal((await promise).stdout, "x");
});

test("every call has its own id and shape", async () => {
  const sdk = load();
  void sdk.westerm.run("git", ["status"], { cwd: "/repo" });
  void sdk.westerm.openShell({ program: "kubectl", args: ["logs", "-f", "x"], name: "logs", reuse: true });
  void sdk.westerm.openBrowser("https://example.com/");
  void sdk.westerm.storage.get();
  void sdk.westerm.storage.set({ a: 1 });
  void sdk.westerm.setStatus({ badge: 3, alert: true });
  assert.deepEqual(
    sdk.posted.map((text) => JSON.parse(text)),
    [
      { westerm: 1, id: 1, call: "run", args: { program: "git", args: ["status"], cwd: "/repo" } },
      { westerm: 1, id: 2, call: "openShell", args: { program: "kubectl", args: ["logs", "-f", "x"], cwd: null, name: "logs", reuse: true } },
      { westerm: 1, id: 3, call: "openBrowser", args: { url: "https://example.com/" } },
      { westerm: 1, id: 4, call: "storage.get", args: {} },
      { westerm: 1, id: 5, call: "storage.set", args: { value: { a: 1 } } },
      { westerm: 1, id: 6, call: "setStatus", args: { badge: 3, alert: true } },
    ],
  );
});

test("an error reply rejects with its code", async () => {
  const sdk = load();
  const promise = sdk.westerm.openBrowser("http://127.0.0.1:4317/");
  sdk.reply({ westerm: 1, id: 1, ok: false, error: { code: "permission", message: "needs the panes permission" } });
  await assert.rejects(promise, (error: Error & { code?: string }) => {
    assert.equal(Object.prototype.toString.call(error), "[object Error]", "an Error of the window's realm");
    assert.equal(error.code, "permission");
    assert.equal(error.message, "needs the panes permission");
    return true;
  });
});

test("only string messages from the parent are read", async () => {
  const sdk = load();
  const promise = sdk.westerm.storage.get();
  let settled = false;
  void promise.then(() => (settled = true));
  sdk.reply({ westerm: 1, id: 1, ok: true, result: "from elsewhere" }, sdk.window.eval("({})"));
  sdk.reply({ westerm: 1, id: 1, ok: true, result: "an object" }, undefined, true);
  sdk.reply("not json", undefined, true);
  sdk.reply({ westerm: 2, id: 1, ok: true, result: "another version" });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(settled, false);
  sdk.reply({ westerm: 1, id: 1, ok: true, result: { saved: true } });
  assert.deepEqual(plain(await promise), { saved: true });
});

test("events reach listeners and the theme sets CSS variables", async () => {
  const sdk = load();
  const panel: unknown[] = [];
  const off = sdk.westerm.on("panel", (data) => panel.push(data));
  sdk.reply({ westerm: 1, event: "theme", data: { vars: { "--bg": "#000", color: "red" }, font: "Menlo" } });
  const style = sdk.window.document.documentElement.style;
  assert.equal(style.getPropertyValue("--bg"), "#000");
  assert.equal(style.getPropertyValue("color"), "", "only custom properties are set");
  assert.equal(style.getPropertyValue("--font"), "Menlo");
  sdk.reply({ westerm: 1, event: "panel", data: { open: true } });
  off();
  sdk.reply({ westerm: 1, event: "panel", data: { open: false } });
  assert.deepEqual(plain(panel), [{ open: true }]);
});

test("ready resolves with its data and applies the theme and locale", async () => {
  const sdk = load();
  const data = { id: "k8s-tools.kubernetes", version: "0.3.1", locale: "ko", theme: { vars: { "--text": "#fff" }, font: "x" }, permissions: { run: [], panes: false, background: false }, panelOpen: true };
  sdk.reply({ westerm: 1, event: "ready", data });
  assert.deepEqual(plain(await sdk.westerm.ready), data);
  assert.equal(sdk.window.document.documentElement.lang, "ko");
  assert.equal(sdk.window.document.documentElement.style.getPropertyValue("--text"), "#fff");
  sdk.reply({ westerm: 1, event: "locale", data: { locale: "ja" } });
  assert.equal(sdk.window.document.documentElement.lang, "ja");
});

test("the SDK adds its base styles once", () => {
  const sdk = load();
  sdk.window.eval(SDK);
  const styles = sdk.window.document.querySelectorAll("style[data-westerm]");
  assert.equal(styles.length, 1);
  const css = styles[0].textContent ?? "";
  for (const selector of [".w-row", ".w-button", ".w-dot", ".w-dot.ok", ".w-dot.warn", ".w-dot.danger", ".w-empty"]) {
    assert.ok(css.includes(selector), selector);
  }
});

test("an element with id westerm does not stop the SDK", () => {
  const window = new Window();
  window.document.body.innerHTML = '<div id="westerm"></div>';
  // DOM named access: the element shows up as `window.westerm`.
  assert.equal(window.eval("typeof window.westerm"), "object");
  assert.equal(window.eval("window.westerm instanceof HTMLElement"), true);
  window.eval(SDK);
  assert.equal(window.eval("typeof window.westerm.run"), "function");
  assert.equal(window.document.querySelectorAll("style[data-westerm]").length, 1);

  // Browsers keep named elements on the prototype chain instead.
  const browser = new Window();
  browser.eval('Object.defineProperty(Object.getPrototypeOf(window), "westerm", { value: document.createElement("div"), configurable: true })');
  assert.equal(browser.eval("window.westerm instanceof HTMLElement"), true);
  browser.eval(SDK);
  assert.equal(browser.eval("typeof window.westerm.run"), "function");
});
