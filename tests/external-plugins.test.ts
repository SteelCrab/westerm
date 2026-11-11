import assert from "node:assert/strict";
import { test } from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { I18nProvider, type Locale } from "../src/i18n/index.tsx";
import { createBridge, MAX_MESSAGE_BYTES } from "../src/plugins/external/bridge.ts";
import { localText, parsePluginList, type ExternalInfo, type ExternalPermissions } from "../src/plugins/external/info.ts";
import { externalPlugin, type FrameEnvironment } from "../src/plugins/external/plugin.tsx";
import { syncInstances } from "../src/plugins/installed.ts";
import type { OpenShellOptions, PluginContext, PluginStatus } from "../src/plugins/types.ts";
import { usePlugins, type Plugins } from "../src/plugins/usePlugins.ts";

// A page for the frames: happy-dom as the globals, without loading iframe
// pages (no network). A test gives an iframe a fake contentWindow and fires
// its load events itself.
const page = new Window({ url: "http://westerm.test/", settings: { disableIframePageLoading: true } });
const globals = globalThis as Record<string, unknown>;
for (const name of ["window", "document", "HTMLElement", "HTMLIFrameElement", "Node", "MutationObserver", "getComputedStyle", "localStorage"]) {
  Object.defineProperty(globals, name, { value: (page as unknown as Record<string, unknown>)[name], configurable: true, writable: true });
}
globals.ResizeObserver = page.ResizeObserver;
globals.IS_REACT_ACT_ENVIRONMENT = true;
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

const INFO: ExternalInfo = {
  id: "k8s-tools.kubernetes",
  version: "0.3.1",
  name: "Kubernetes",
  description: "See pods.",
  i18n: { ko: { name: "쿠버네티스" } },
  icon: "icon.svg",
  panel: "panel.html",
  source: { kind: "index" },
  granted: { run: ["kubectl"], panes: true, background: false },
  installedAt: 1,
};
const BACKGROUND: ExternalInfo = { ...INFO, granted: { ...INFO.granted, background: true } };
const STOPPED = "Plugin stopped: it tried to leave its page";

const noop = () => {};
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** An environment whose host is a fixed div, with a theme and locale the test changes. */
function fakeEnv() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  let locale: Locale = "en";
  let theme = { vars: { "--bg": "#000000" }, font: "Mono" };
  const listeners = new Set<() => void>();
  const env: FrameEnvironment & { set(next: { locale?: Locale; theme?: typeof theme }): void } = {
    host: () => host,
    locale: () => locale,
    theme: () => theme,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    set(next) {
      locale = next.locale ?? locale;
      theme = next.theme ?? theme;
      for (const listener of [...listeners]) listener();
    },
  };
  return env;
}

type Answer = Response | Promise<Response>;

/** A context that records what the plugin asked for; `answer` decides each fetch. */
function fakeCtx(answer: (path: string, init?: RequestInit) => Answer = () => new Response(null, { status: 500 })) {
  const record = {
    fetched: [] as { path: string; init?: RequestInit }[],
    opened: [] as OpenShellOptions[],
    browsed: [] as string[],
    refreshes: 0,
    managers: 0,
  };
  const ctx: PluginContext = {
    openShell: (options) => record.opened.push(options),
    openBrowser: (url) => record.browsed.push(url),
    openPane: () => {},
    folders: () => [],
    fetch: (path, init) => {
      record.fetched.push({ path, init });
      return Promise.resolve(answer(path, init));
    },
    refreshInstalled: () => {
      record.refreshes += 1;
    },
    openManager: () => {
      record.managers += 1;
    },
  };
  // One live object: its counts change as the plugin asks.
  return Object.assign(record, { ctx, record });
}

const json = (value: unknown, status = 200) =>
  new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json" } });

// ---- the bridge on its own ---------------------------------------------

const SOURCE = { name: "the plugin's window" };
const OTHER = { name: "another window" };

function bridgeFor(
  granted: ExternalPermissions = INFO.granted,
  answer?: (path: string, init?: RequestInit) => Answer,
  shellPanes = true,
) {
  const fake = fakeCtx(answer);
  const posted: string[] = [];
  const statuses: PluginStatus[] = [];
  let gone = 0;
  const bridge = createBridge({
    id: INFO.id,
    granted,
    ctx: fake.ctx,
    isSource: (source) => source === SOURCE,
    post: (text) => posted.push(text),
    setStatus: (status) => statuses.push(status),
    gone: () => {
      gone += 1;
    },
    shellPanes: () => shellPanes,
  });
  const call = (id: number, name: string, args: unknown, source: unknown = SOURCE) =>
    bridge.receive(source, JSON.stringify({ westerm: 1, id, call: name, args }));
  const replies = () => posted.map((text) => JSON.parse(text) as Record<string, unknown>);
  return { bridge, posted, statuses, fake, call, replies, gone: () => gone };
}

/** A storage.set message of exactly `bytes` bytes in UTF-8. */
function storageSetOfBytes(bytes: number): string {
  const shell = (value: string) => JSON.stringify({ westerm: 1, id: 1, call: "storage.set", args: { value } });
  const text = shell("x".repeat(bytes - shell("").length));
  assert.equal(new TextEncoder().encode(text).length, bytes);
  return text;
}

test("calls without permission answer an error and reach nothing", () => {
  const { call, posted, fake } = bridgeFor();
  call(7, "run", { program: "helm", args: [], cwd: null });
  assert.deepEqual(JSON.parse(posted.at(-1)!), {
    westerm: 1,
    id: 7,
    ok: false,
    error: { code: "permission", message: "helm is not permitted" },
  });
  assert.equal(fake.fetched.length, 0);
});

test("run asks the server and answers its output", async () => {
  const { call, replies, fake } = bridgeFor(INFO.granted, () => json({ code: 0, stdout: "pod/a\n", stderr: "" }));
  call(1, "run", { program: "kubectl", args: ["get", "pods"], cwd: "/work" });
  await flush();
  assert.equal(fake.fetched[0].path, "/run");
  assert.equal(fake.fetched[0].init?.method, "POST");
  assert.deepEqual(JSON.parse(fake.fetched[0].init?.body as string), { program: "kubectl", args: ["get", "pods"], cwd: "/work" });
  assert.deepEqual(replies(), [{ westerm: 1, id: 1, ok: true, result: { code: 0, stdout: "pod/a\n", stderr: "" } }]);
});

test("messages over 256 KiB are dropped", () => {
  assert.equal(MAX_MESSAGE_BYTES, 262_144);
  const { bridge, posted, fake } = bridgeFor(INFO.granted, () => new Response(null, { status: 204 }));
  bridge.receive(SOURCE, storageSetOfBytes(262_145));
  assert.equal(fake.fetched.length, 0);
  assert.equal(posted.length, 0);
  bridge.receive(SOURCE, storageSetOfBytes(262_144));
  assert.equal(fake.fetched.length, 1);
});

test("messages from another window are ignored", async () => {
  const { call, posted, fake } = bridgeFor();
  call(1, "run", { program: "kubectl", args: [], cwd: null }, OTHER);
  call(2, "run", { program: "helm", args: [], cwd: null }, OTHER);
  await flush();
  assert.equal(posted.length, 0);
  assert.equal(fake.fetched.length, 0);
});

test("messages that are not a call string are dropped without an answer", () => {
  const { bridge, posted, call, replies } = bridgeFor();
  bridge.receive(SOURCE, { westerm: 1, id: 1, call: "storage.get", args: {} });
  bridge.receive(SOURCE, "not json");
  bridge.receive(SOURCE, JSON.stringify({ westerm: 2, id: 1, call: "storage.get", args: {} }));
  bridge.receive(SOURCE, JSON.stringify({ westerm: 1, id: "1", call: "storage.get", args: {} }));
  assert.equal(posted.length, 0);
  call(3, "format the disk", {});
  assert.equal((replies()[0].error as { code: string }).code, "invalid");
});

test("openShell sends the argv as the launch target", () => {
  const { call, fake, replies } = bridgeFor();
  call(1, "openShell", { program: "kubectl", args: ["logs", "pod/a"], cwd: null, name: "a logs", reuse: true });
  assert.deepEqual(fake.opened[0], { action: "run", target: '["kubectl","logs","pod/a"]', cwd: null, name: "a logs", reuse: true });
  assert.deepEqual(replies()[0], { westerm: 1, id: 1, ok: true, result: null });

  // Without a name the pane is named after the program.
  call(2, "openShell", { program: "kubectl", args: [], cwd: "/work", name: null, reuse: false });
  assert.deepEqual(fake.opened[1], { action: "run", target: '["kubectl"]', cwd: "/work", name: "kubectl", reuse: false });

  const code = (index: number) => (replies()[index].error as { code: string }).code;
  const noPanes = bridgeFor({ ...INFO.granted, panes: false });
  noPanes.call(3, "openShell", { program: "kubectl", args: [], cwd: null, name: "x", reuse: false });
  assert.equal((noPanes.replies()[0].error as { code: string }).code, "permission");
  call(4, "openShell", { program: "helm", args: [], cwd: null, name: "x", reuse: false });
  assert.equal(code(2), "permission");
  call(5, "openShell", { program: "kubectl", args: ["logs", "\u001b[2J"], cwd: null, name: "x", reuse: false });
  assert.equal(code(3), "invalid");
  call(6, "openShell", { program: "kubectl", args: [], cwd: null, name: "x".repeat(81), reuse: false });
  assert.equal(code(4), "invalid");
  assert.equal(fake.opened.length, 2);
  assert.equal(noPanes.fake.opened.length, 0);
});

test("openShell fails without a pane when the server's shell gets no plugin panes", () => {
  const { call, fake, replies } = bridgeFor(INFO.granted, undefined, false);
  call(1, "openShell", { program: "kubectl", args: ["get", "pods"], cwd: null, name: "pods", reuse: false });
  assert.deepEqual(replies()[0], {
    westerm: 1,
    id: 1,
    ok: false,
    error: { code: "failed", message: "Your shell is not supported for plugin panes" },
  });
  assert.equal(fake.opened.length, 0);
  assert.equal(fake.fetched.length, 0);
});

test("openBrowser asks the server before opening", async () => {
  const allowed = bridgeFor(INFO.granted, () => new Response(null, { status: 204 }));
  allowed.call(1, "openBrowser", { url: "http://localhost:3000/" });
  await flush();
  assert.equal(allowed.fake.fetched[0].path, "/browser");
  assert.deepEqual(JSON.parse(allowed.fake.fetched[0].init?.body as string), { url: "http://localhost:3000/" });
  assert.deepEqual(allowed.fake.browsed, ["http://localhost:3000/"]);
  assert.deepEqual(allowed.replies()[0], { westerm: 1, id: 1, ok: true, result: null });

  const refused = bridgeFor(INFO.granted, () => json({ error: "the address is westerm itself" }, 403));
  refused.call(1, "openBrowser", { url: "http://127.0.0.1:4317/" });
  await flush();
  assert.deepEqual(refused.fake.browsed, []);
  assert.deepEqual(refused.replies()[0].error, { code: "failed", message: "the address is westerm itself" });

  // Only the address the server checked opens: any other success answer,
  // even one that names an address, opens nothing.
  const other200 = bridgeFor(INFO.granted, () => json({ url: "http://127.0.0.1:4317/" }));
  other200.call(1, "openBrowser", { url: "http://localhost:3000/" });
  await flush();
  assert.deepEqual(other200.fake.browsed, []);
  assert.deepEqual(other200.replies()[0].error, { code: "failed", message: "The address was not approved" });

  const other = bridgeFor(INFO.granted, () => new Response(null, { status: 204 }));
  other.call(1, "openBrowser", { url: "file:///etc/passwd" });
  other.call(2, "openBrowser", { url: "javascript:alert(1)" });
  assert.equal(other.fake.fetched.length, 0);
  assert.deepEqual(
    other.replies().map((reply) => (reply.error as { code: string }).code),
    ["invalid", "invalid"],
  );
  const noPanes = bridgeFor({ ...INFO.granted, panes: false });
  noPanes.call(1, "openBrowser", { url: "https://example.com/" });
  assert.equal((noPanes.replies()[0].error as { code: string }).code, "permission");
  assert.equal(noPanes.fake.fetched.length, 0);
});

test("storage reads and writes the plugin's value", async () => {
  const { call, replies, fake } = bridgeFor(INFO.granted, (_path, init) =>
    init?.method === "PUT" ? new Response(null, { status: 204 }) : json({ context: "dev" }),
  );
  call(1, "storage.get", {});
  call(2, "storage.set", { value: { context: "prod" } });
  await flush();
  assert.equal(fake.fetched[0].path, "/storage");
  assert.equal(fake.fetched[1].init?.method, "PUT");
  assert.equal(fake.fetched[1].init?.body, '{"context":"prod"}');
  assert.deepEqual(replies().sort((a, b) => (a.id as number) - (b.id as number)), [
    { westerm: 1, id: 1, ok: true, result: { context: "dev" } },
    { westerm: 1, id: 2, ok: true, result: null },
  ]);
});

test("setStatus takes a badge of 0 to 999 or none", () => {
  const { call, statuses, replies } = bridgeFor({ run: [], panes: false, background: false });
  call(1, "setStatus", { badge: 3, alert: true });
  call(2, "setStatus", { badge: null, alert: false });
  call(3, "setStatus", { badge: 1000, alert: false });
  call(4, "setStatus", { badge: 1.5, alert: false });
  assert.deepEqual(statuses, [
    { badge: 3, alert: true },
    { badge: null, alert: false },
  ]);
  assert.deepEqual(
    replies().map((reply) => reply.ok),
    [true, true, false, false],
  );
});

test("every call is answered, also one pending when the bridge stops", async () => {
  let resolve: (response: Response) => void = noop;
  const { bridge, call, replies } = bridgeFor(INFO.granted, () => new Promise<Response>((done) => (resolve = done)));
  call(1, "run", { program: "kubectl", args: [], cwd: null });
  bridge.stop();
  assert.deepEqual(replies(), [{ westerm: 1, id: 1, ok: false, error: { code: "gone", message: "The plugin stopped" } }]);
  resolve(json({ code: 0, stdout: "", stderr: "" }));
  await flush();
  assert.equal(replies().length, 1, "one answer per call");
  bridge.send("theme", {});
  call(2, "setStatus", { badge: null, alert: false });
  assert.equal(replies().length, 1, "nothing after stop");
});

test("at most 8 server calls are in flight per frame; more fail at once", async () => {
  const waiting: ((response: Response) => void)[] = [];
  const { call, replies, fake } = bridgeFor(INFO.granted, () => new Promise<Response>((done) => waiting.push(done)));
  for (let id = 1; id <= 8; id += 1) call(id, "storage.get", {});
  assert.equal(fake.fetched.length, 8);
  assert.equal(replies().length, 0);

  call(9, "run", { program: "kubectl", args: [], cwd: null });
  assert.equal(fake.fetched.length, 8, "the 9th never reaches the server");
  assert.deepEqual(replies(), [{ westerm: 1, id: 9, ok: false, error: { code: "failed", message: "Too many calls at once" } }]);
  // Calls that need no server still answer.
  call(10, "setStatus", { badge: 1, alert: false });
  assert.equal(replies().at(-1)!.ok, true);

  waiting[0](json("saved"));
  await flush();
  assert.deepEqual(replies().at(-1), { westerm: 1, id: 1, ok: true, result: "saved" });
  call(11, "storage.get", {});
  assert.equal(fake.fetched.length, 9, "a slot is free again");

  // A failed request frees its slot too.
  waiting[1](json({ error: "boom" }, 500));
  await flush();
  call(12, "storage.get", {});
  assert.equal(fake.fetched.length, 10);
});

test("a 404 answers gone and tells the plugin", async () => {
  const { call, replies, gone } = bridgeFor(INFO.granted, () => json({ error: "not installed" }, 404));
  call(1, "storage.get", {});
  await flush();
  assert.equal((replies()[0].error as { code: string }).code, "gone");
  assert.equal(gone(), 1);
});

// ---- the plugin and its frame ------------------------------------------

test("the plugin list carries external plugins", () => {
  const parsed = parsePluginList({
    installed: ["docker", INFO.id],
    external: [INFO, { id: "bad" }, { ...INFO, id: "x.y", panel: "../../api/x" }],
  });
  assert.deepEqual(parsed.installed, ["docker", INFO.id]);
  assert.deepEqual(parsed.external, [INFO]);
  assert.deepEqual(parsePluginList({ installed: ["docker"] }), { installed: ["docker"], external: [], shellPanes: false, builtin: null });
  assert.deepEqual(parsePluginList(null), { installed: [], external: [], shellPanes: false, builtin: null });
  // The built-in plugins of the server's build.
  assert.deepEqual(parsePluginList({ installed: [], builtin: ["docker", 7] }).builtin, ["docker"]);
  // Plugin panes only when the server says its shell takes them.
  assert.equal(parsePluginList({ installed: [], external: [], shellPanes: true }).shellPanes, true);
  assert.equal(parsePluginList({ installed: [], external: [], shellPanes: "yes" }).shellPanes, false);
  // The server writes a missing translation as null.
  const fromServer = parsePluginList({ installed: [], external: [{ ...INFO, i18n: { ko: { name: "쿠버네티스", description: null } } }] });
  assert.deepEqual(localText(fromServer.external[0], "ko"), { name: "쿠버네티스", description: "See pods." });
});

test("an external plugin becomes a WestermPlugin", () => {
  const plugin = externalPlugin(INFO, fakeEnv());
  assert.equal(plugin.id, INFO.id);
  assert.equal(plugin.version, "0.3.1");
  assert.equal(plugin.text.ko.name, "쿠버네티스");
  assert.equal(plugin.text.ja.name, "Kubernetes");
  assert.ok(plugin.text.en.permissions.includes("Can run kubectl with any arguments, as you"));
  assert.ok(plugin.text.en.permissions.includes("Can open terminal and browser panes"));
  assert.equal(plugin.iconUrl, "/plugin-assets/k8s-tools.kubernetes/icon.svg");
  assert.equal(externalPlugin({ ...INFO, icon: null }, fakeEnv()).iconUrl, undefined);
  assert.deepEqual(plugin.external, { source: INFO.source, granted: INFO.granted, installedAt: 1 });
  const none = externalPlugin({ ...INFO, granted: { run: [], panes: false, background: false } }, fakeEnv());
  assert.deepEqual(none.text.en.permissions, ["Needs no permissions"]);
});

/** A started plugin, with helpers to wire and drive its iframe. */
function running(
  info: ExternalInfo = INFO,
  answer?: (path: string, init?: RequestInit) => Answer,
  shellPanes: () => boolean = () => true,
) {
  const env = fakeEnv();
  const fake = fakeCtx(answer);
  const instance = externalPlugin(info, env, shellPanes).start(fake.ctx);
  const posted: string[] = [];
  const frame = () => env.host().querySelector("iframe");
  /** Gives the current iframe a window that collects what westerm posts to it. */
  const wire = () => {
    const iframe = frame()!;
    const win = { postMessage: (text: string) => posted.push(text) };
    Object.defineProperty(iframe, "contentWindow", { value: win, configurable: true });
    return win;
  };
  const load = () => frame()!.dispatchEvent(new page.Event("load") as unknown as Event);
  const message = (source: unknown, value: unknown) =>
    page.dispatchEvent(new page.MessageEvent("message", { data: JSON.stringify(value), source: source as never }));
  const events = (name: string) =>
    posted.map((text) => JSON.parse(text) as { event?: string; data?: unknown }).filter((each) => each.event === name);
  return { env, fake, instance, posted, frame, wire, load, message, events };
}

test("the frame is sandboxed with allow-scripts only", () => {
  const { instance, frame, env } = running();
  assert.equal(frame(), null, "no frame before the panel opens");
  instance.setPanelOpen(true);
  const iframe = frame()!;
  assert.equal(iframe.getAttribute("sandbox"), "allow-scripts");
  assert.equal(iframe.getAttribute("src"), "/plugin-assets/k8s-tools.kubernetes/panel.html");
  assert.equal(iframe.getAttribute("referrerpolicy"), "no-referrer");
  assert.equal(iframe.getAttribute("title"), "Kubernetes");
  assert.equal(iframe.parentElement, env.host());
  instance.dispose();
  assert.equal(iframe.isConnected, false);
});

test("the first load sends ready, and theme and locale follow the page", () => {
  const { instance, wire, load, events, env } = running();
  instance.setPanelOpen(true);
  wire();
  load();
  assert.deepEqual(events("ready")[0].data, {
    id: INFO.id,
    version: "0.3.1",
    locale: "en",
    theme: { vars: { "--bg": "#000000" }, font: "Mono" },
    permissions: INFO.granted,
    panelOpen: true,
  });
  env.set({ locale: "ko" });
  env.set({ theme: { vars: { "--bg": "#ffffff" }, font: "Mono" } });
  assert.deepEqual(events("locale").map((each) => each.data), [{ locale: "ko" }]);
  assert.deepEqual(events("theme").map((each) => each.data), [{ vars: { "--bg": "#ffffff" }, font: "Mono" }]);
  instance.dispose();
});

test("openShell reads the server's shellPanes answer at each call", () => {
  let panes = false;
  const { instance, wire, load, message, posted, fake } = running(INFO, undefined, () => panes);
  instance.setPanelOpen(true);
  const win = wire();
  load();
  const shell = (id: number) =>
    message(win, { westerm: 1, id, call: "openShell", args: { program: "kubectl", args: [], cwd: null, name: "k", reuse: false } });
  shell(1);
  assert.equal(fake.opened.length, 0);
  panes = true;
  shell(2);
  assert.equal(fake.opened.length, 1);
  const answers = posted.map((text) => JSON.parse(text) as { id?: number; ok?: boolean }).filter((each) => each.id !== undefined);
  assert.deepEqual(answers.map((each) => [each.id, each.ok]), [[1, false], [2, true]]);
  instance.dispose();
});

test("the plugin's own window reaches the bridge through the page", async () => {
  const { instance, wire, load, message, posted, fake } = running();
  instance.setPanelOpen(true);
  const win = wire();
  load();
  message(win, { westerm: 1, id: 4, call: "setStatus", args: { badge: 2, alert: false } });
  message({}, { westerm: 1, id: 5, call: "setStatus", args: { badge: 9, alert: false } });
  assert.deepEqual(instance.status(), { badge: 2, alert: false });
  assert.deepEqual(JSON.parse(posted.at(-1)!), { westerm: 1, id: 4, ok: true, result: null });
  assert.equal(fake.fetched.length, 0);
  instance.dispose();
  message(win, { westerm: 1, id: 6, call: "setStatus", args: { badge: 7, alert: false } });
  assert.deepEqual(instance.status(), { badge: 2, alert: false }, "no listener after dispose");
});

test("a burst of setStatus messages tells the page once", async () => {
  const { instance, wire, load, message } = running();
  instance.setPanelOpen(true);
  const win = wire();
  load();
  let emits = 0;
  instance.subscribe(() => {
    emits += 1;
  });
  for (let badge = 1; badge <= 50; badge += 1) message(win, { westerm: 1, id: badge, call: "setStatus", args: { badge, alert: false } });
  assert.deepEqual(instance.status(), { badge: 50, alert: false }, "the latest value at once");
  assert.equal(emits, 0, "not one render per message");
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.equal(emits, 1);

  // A burst that ends where it started tells nothing, and keeps the same object.
  const before = instance.status();
  message(win, { westerm: 1, id: 51, call: "setStatus", args: { badge: 7, alert: true } });
  message(win, { westerm: 1, id: 52, call: "setStatus", args: { badge: 50, alert: false } });
  await new Promise((resolve) => setTimeout(resolve, 150));
  assert.equal(emits, 1);
  assert.equal(instance.status(), before);
  instance.dispose();
});

test("a second load removes the frame and stops messages", () => {
  const { instance, frame, wire, load, message, posted, env } = running();
  instance.setPanelOpen(true);
  const first = frame()!;
  const win = wire();
  load();
  load();
  assert.equal(first.isConnected, false);
  assert.deepEqual(instance.status(), { badge: null, alert: false, note: STOPPED });
  const before = posted.length;
  message(win, { westerm: 1, id: 1, call: "setStatus", args: { badge: 1, alert: false } });
  assert.equal(posted.length, before);

  instance.setPanelOpen(false);
  instance.setPanelOpen(true);
  assert.equal(instance.status().note, undefined);
  const second = frame()!;
  assert.notEqual(second, first);
  assert.equal(second.parentElement, env.host());
  instance.dispose();
});

test("without background closing the panel removes the frame; with background it stays hidden", () => {
  const plain = running();
  plain.instance.setPanelOpen(true);
  const iframe = plain.frame()!;
  plain.instance.setPanelOpen(false);
  assert.equal(iframe.isConnected, false);
  plain.instance.dispose();

  const kept = running(BACKGROUND);
  const hidden = kept.frame()!;
  assert.ok(hidden, "a background plugin starts its frame at once");
  assert.equal(hidden.hidden, true);
  kept.instance.setPanelOpen(true);
  kept.instance.setPanelOpen(false);
  assert.equal(hidden.isConnected, true);
  assert.equal(hidden.hidden, true);
  kept.instance.dispose();
  assert.equal(hidden.isConnected, false);
});

test("a layout change keeps the frame where it is", async () => {
  const { instance, frame, wire, load, events, env } = running(BACKGROUND);
  const iframe = frame()!;
  wire();
  load();
  // Count every insertion of the iframe anywhere in the page from here on.
  const watcher = new page.MutationObserver(noop);
  watcher.observe(page.document.body, { childList: true, subtree: true });
  let insertions = 1;
  const count = () => {
    for (const record of watcher.takeRecords()) {
      insertions += [...record.addedNodes].filter((node) => node === (iframe as unknown)).length;
    }
  };

  const { Panel } = instance;
  const show = async (container: HTMLElement) => {
    const root = createRoot(container);
    await act(async () => root.render(createElement(I18nProvider, null, createElement(Panel))));
    return root;
  };
  // The sidebar layout, then a closed panel, then the top layout in another container.
  const sidebar = document.createElement("div");
  document.body.appendChild(sidebar);
  instance.setPanelOpen(true);
  const first = await show(sidebar);
  assert.ok(sidebar.querySelector(".plugin-frame-slot"), "the panel draws a slot");
  assert.equal(iframe.hidden, false);
  await act(async () => first.unmount());
  instance.setPanelOpen(false);
  assert.equal(iframe.hidden, true);

  const top = document.createElement("section");
  document.body.appendChild(top);
  instance.setPanelOpen(true);
  const second = await show(top);
  assert.equal(iframe.hidden, false);
  count();
  assert.equal(iframe.parentElement, env.host());
  assert.equal(insertions, 1, "inserted once, never moved");
  assert.equal(events("ready").length, 1, "loaded once");
  assert.deepEqual(events("panel").map((each) => each.data), [{ open: true }, { open: false }, { open: true }]);
  await act(async () => second.unmount());
  watcher.disconnect();
  instance.dispose();
});

test("a stopped panel shows the note in its slot", async () => {
  const { instance, wire, load } = running();
  instance.setPanelOpen(true);
  wire();
  load();
  load();
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => root.render(createElement(I18nProvider, null, createElement(instance.Panel))));
  assert.match(container.textContent ?? "", /Plugin stopped: it tried to leave its page/);
  assert.match(container.innerHTML, /external/);
  assert.match(container.innerHTML, /v0\.3\.1/);
  assert.match(container.innerHTML, /<img[^>]*src="\/plugin-assets\/k8s-tools\.kubernetes\/icon\.svg"/);
  await act(async () => root.unmount());
  instance.dispose();
});

test("a 404 stops the frame and refreshes the installed list", async () => {
  const { instance, frame, wire, load, message, posted, fake } = running(INFO, () => json({ error: "not installed" }, 404));
  instance.setPanelOpen(true);
  const iframe = frame()!;
  const win = wire();
  load();
  message(win, { westerm: 1, id: 1, call: "run", args: { program: "kubectl", args: [], cwd: null } });
  await flush();
  assert.equal((JSON.parse(posted.at(-1)!) as { error: { code: string } }).error.code, "gone");
  assert.equal(fake.refreshes, 1);
  assert.equal(iframe.isConnected, false);
  assert.equal(instance.status().note, undefined);
  instance.dispose();
});

test("a new version restarts the instance", () => {
  const env = fakeEnv();
  const ctxFor = () => fakeCtx().ctx;
  const first = syncInstances(new Map(), [INFO.id], [externalPlugin(INFO, env)], ctxFor);
  const same = syncInstances(first, [INFO.id], [externalPlugin(INFO, env)], ctxFor);
  assert.equal(same.get(INFO.id), first.get(INFO.id), "the same files keep the instance");
  const next = syncInstances(same, [INFO.id], [externalPlugin({ ...INFO, version: "0.4.0" }, env)], ctxFor);
  assert.notEqual(next.get(INFO.id), first.get(INFO.id));
  const reloaded = syncInstances(next, [INFO.id], [externalPlugin({ ...INFO, version: "0.4.0", installedAt: 2 }, env)], ctxFor);
  assert.notEqual(reloaded.get(INFO.id), next.get(INFO.id), "a folder Reload too");
  for (const instance of reloaded.values()) instance.dispose();
});

// ---- the installed list -------------------------------------------------

test("toggling a built-in plugin keeps the external plugins installed", async () => {
  const puts: unknown[] = [];
  // A server whose build has Docker only.
  let list: unknown = { installed: [INFO.id], external: [INFO], builtin: ["docker"] };
  const realFetch = globalThis.fetch;
  globalThis.fetch = (async (_url: string, init?: RequestInit) => {
    if (init?.method === "PUT") {
      puts.push(JSON.parse(init.body as string));
      return new Response(null, { status: 204 });
    }
    return json(list);
  }) as typeof fetch;
  // Docker's poller and the external instance get a context that never answers.
  const ctxFor = (): PluginContext => ({ ...fakeCtx().ctx, fetch: () => new Promise<Response>(noop) });
  let plugins: Plugins | null = null;
  function Probe() {
    plugins = usePlugins(ctxFor, true);
    return null;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    await act(async () => root.render(createElement(Probe)));
    await act(async () => plugins!.refresh());
    assert.deepEqual(plugins!.installed, [INFO.id]);
    assert.deepEqual(plugins!.external, [INFO]);
    assert.deepEqual(
      plugins!.registry.map((plugin) => plugin.id),
      ["docker", INFO.id],
    );
    const external = plugins!.registry[1];

    await act(async () => {
      assert.equal(await plugins!.setInstalled("docker", true), true);
    });
    assert.deepEqual(plugins!.installed, ["docker", INFO.id]);
    assert.deepEqual([...plugins!.instances.keys()], ["docker", INFO.id]);

    await act(async () => {
      assert.equal(await plugins!.setInstalled("docker", false), true);
    });
    assert.deepEqual(plugins!.installed, [INFO.id]);
    assert.deepEqual(puts, [{ installed: ["docker", INFO.id] }, { installed: [INFO.id] }]);

    // The same files keep the same plugin object; a new version makes a new one.
    await act(async () => plugins!.refresh());
    assert.equal(plugins!.registry[1], external);
    list = { installed: [INFO.id], external: [{ ...INFO, version: "0.4.0" }], builtin: ["docker"] };
    await act(async () => plugins!.refresh());
    assert.notEqual(plugins!.registry[1], external);
    assert.equal(plugins!.registry[1].version, "0.4.0");
  } finally {
    await act(async () => root.unmount());
    globalThis.fetch = realFetch;
  }
});
