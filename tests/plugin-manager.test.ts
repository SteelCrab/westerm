import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { act, createElement, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { Window } from "happy-dom";
import { Box } from "lucide-react";
import { I18nProvider, LOCALES, type Locale } from "../src/i18n/index.tsx";
import { MESSAGES } from "../src/i18n/messages.ts";
import { fetchPreview, install, loadCatalog, uninstall, update, type IndexEntry, type InstallBody, type UpdateConfirm } from "../src/plugins/external/api.ts";
import type { ExternalInfo } from "../src/plugins/external/info.ts";
import { InstallFlow, InstallFlowView, type InstallApi, type InstallState } from "../src/plugins/InstallFlow.tsx";
import {
  authorOf,
  badgeOf,
  entries,
  matches,
  permissionRows,
  riskOf,
  type Entry,
} from "../src/plugins/managerModel.ts";
import { PluginDetailView, type DetailState } from "../src/plugins/PluginDetail.tsx";
import { PluginListView, PluginManager } from "../src/plugins/PluginManager.tsx";
import type { PluginText, WestermPlugin } from "../src/plugins/types.ts";

// I18nProvider follows the browser's language, which Node takes from LANG.
// Pin it so the English text below matches on every machine.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

// A page for the tests that click and follow focus: happy-dom as the globals.
// The markup tests render to a string and do not use it.
const page = new Window({ url: "http://westerm.test/" });
const globals = globalThis as Record<string, unknown>;
for (const name of ["window", "document", "HTMLElement", "HTMLIFrameElement", "Node", "MutationObserver", "getComputedStyle", "localStorage"]) {
  Object.defineProperty(globals, name, { value: (page as unknown as Record<string, unknown>)[name], configurable: true, writable: true });
}
globals.IS_REACT_ACT_ENVIRONMENT = true;

const noop = () => {};

const DOCKER_TEXT: PluginText = { name: "Docker", description: "Containers, logs, shells.", permissions: ["Run docker"] };

const DOCKER: WestermPlugin = {
  id: "docker",
  version: "1.0.0",
  icon: Box,
  text: Object.fromEntries(LOCALES.map((locale) => [locale, DOCKER_TEXT])) as Record<Locale, PluginText>,
  start: () => {
    throw new Error("not started in these tests");
  },
};

const K8S_PERMISSIONS = { run: ["kubectl"], panes: true, background: true };

const INDEX_K8S: IndexEntry = {
  id: "k8s-tools.kubernetes",
  name: "Kubernetes",
  description: "See pods by namespace.",
  repo: "https://github.com/k8s-tools/westerm-k8s",
  version: "0.3.1",
  url: "https://github.com/k8s-tools/westerm-k8s/releases/download/v0.3.1/westerm-plugin.tar.gz",
  sha256: "a".repeat(64),
  permissions: K8S_PERMISSIONS,
};

const INDEX_GIT: IndexEntry = {
  id: "git-tools.git",
  name: "Git",
  description: "Branches at a glance.",
  repo: "https://github.com/git-tools/westerm-git",
  version: "1.0.0",
  url: "https://github.com/git-tools/westerm-git/releases/download/v1.0.0/westerm-plugin.tar.gz",
  sha256: "b".repeat(64),
  permissions: { run: ["git"], panes: false, background: false },
};

const INFO: ExternalInfo = {
  id: "k8s-tools.kubernetes",
  version: "0.3.1",
  name: "Kubernetes",
  description: "See pods by namespace.",
  i18n: { ko: { name: "쿠버네티스" } },
  icon: "icon.svg",
  panel: "panel.html",
  source: { kind: "index", url: INDEX_K8S.url },
  granted: K8S_PERMISSIONS,
  installedAt: 1,
};

const TARBALL: ExternalInfo = {
  ...INFO,
  id: "someone.redis",
  name: "Redis",
  source: { kind: "url", url: "https://example.com/redis.tar.gz" },
  granted: { run: ["redis-cli"], panes: true, background: false },
};

const FOLDER: ExternalInfo = {
  ...INFO,
  id: "dev.my-tool",
  name: "My tool",
  source: { kind: "folder", path: "/Users/me/code/my-tool" },
  granted: { run: [], panes: false, background: false },
};

const ENTRY: Entry = {
  id: "k8s-tools.kubernetes",
  name: "Kubernetes",
  description: "See pods by namespace.",
  author: "k8s-tools",
  badge: "verified",
  installed: true,
  update: null,
  updateMode: "check",
  permissions: K8S_PERMISSIONS,
  source: INDEX_K8S.repo,
  version: "0.3.1",
  iconUrl: null,
};

function render(element: ReactElement): string {
  return renderToStaticMarkup(createElement(I18nProvider, null, element));
}

function detail(entry: Entry, state: DetailState = { kind: "idle" }): string {
  return render(createElement(PluginDetailView, { entry, lines: null, state, busy: false, error: null, onAction: noop }));
}

function flow(state: InstallState, error: string | null = null): string {
  return render(createElement(InstallFlowView, { state, busy: false, error, onAction: noop }));
}

// ---- managerModel ------------------------------------------------------

test("search matches name, description, and author", () => {
  const entry = { ...ENTRY, name: "Kubernetes", description: "See pods.", author: "k8s-tools" };
  for (const q of ["kube", "PODS", "k8s-t", "", "  "]) assert.ok(matches(entry, q), q);
  assert.equal(matches(entry, "docker"), false);
  assert.equal(matches({ ...entry, author: null }, "k8s-t"), false);
  assert.equal(authorOf("docker"), null);
  assert.equal(authorOf("k8s-tools.kubernetes"), "k8s-tools");
});

test("Installed lists installed plugins; Browse adds built-ins and the index", () => {
  const catalog = { index: [INDEX_K8S, INDEX_GIT], installed: [] };
  assert.deepEqual(entries("installed", [DOCKER], catalog, [], [], "en").map((e) => e.id), []);
  assert.deepEqual(entries("browse", [DOCKER], catalog, [], [], "en").map((e) => e.id), ["docker", "k8s-tools.kubernetes", "git-tools.git"]);
  assert.deepEqual(
    entries("installed", [DOCKER], catalog, [INFO], ["docker", INFO.id], "en").map((e) => [e.id, e.badge]),
    [
      ["docker", "built-in"],
      [INFO.id, "verified"],
    ],
  );
});

test("Browse shows an installed plugin once, and installed plugins outside the index after it", () => {
  const catalog = { index: [INDEX_K8S, INDEX_GIT], installed: [] };
  const list = entries("browse", [DOCKER], catalog, [INFO, FOLDER], [INFO.id, FOLDER.id], "ko");
  assert.deepEqual(
    list.map((e) => [e.id, e.installed]),
    [
      ["docker", false],
      [INFO.id, true],
      ["git-tools.git", false],
      [FOLDER.id, true],
    ],
  );
  const k8s = list[1];
  assert.equal(k8s.name, "쿠버네티스", "an installed plugin speaks the page's language");
  assert.equal(k8s.source, INDEX_K8S.repo, "an index plugin's source is its repository");
  assert.equal(k8s.iconUrl, "/plugin-assets/k8s-tools.kubernetes/icon.svg");
  assert.equal(list[2].iconUrl, null);
  assert.deepEqual(list[2].permissions, INDEX_GIT.permissions);
  assert.equal(list[3].source, "/Users/me/code/my-tool");
});

test("without the catalog, Browse still has the built-ins and the installed plugins", () => {
  assert.deepEqual(entries("browse", [DOCKER], null, [TARBALL], [TARBALL.id], "en").map((e) => [e.id, e.badge]), [
    ["docker", "built-in"],
    [TARBALL.id, "unverified"],
  ]);
});

test("updates come from the catalog until the installed version catches up", () => {
  const catalog = { index: [INDEX_K8S], installed: [{ ...INFO, update: "0.4.0", updateMode: "check" as const }] };
  const [k8s] = entries("installed", [], catalog, [INFO], [INFO.id], "en");
  assert.equal(k8s.update, "0.4.0");
  assert.equal(k8s.updateMode, "check");
  const updated = { ...INFO, version: "0.4.0", installedAt: 2 };
  assert.equal(entries("installed", [], catalog, [updated], [INFO.id], "en")[0].update, null);
});

test("badges follow the source", () => {
  assert.equal(badgeOf({ kind: "index", url: "https://x/p.tar.gz" }), "verified");
  assert.equal(badgeOf({ kind: "url", url: "https://github.com/o/r" }), "unverified");
  assert.equal(badgeOf({ kind: "folder", path: "/tmp/p" }), "local");
});

test("the run permission reads honestly", () => {
  assert.deepEqual(permissionRows({ run: ["kubectl"], panes: true, background: true }, MESSAGES.en)[0], {
    text: "Can run kubectl with any arguments, as you",
    risk: "high",
  });
  assert.deepEqual(
    permissionRows({ run: ["kubectl"], panes: true, background: true }, MESSAGES.en).map((row) => row.risk),
    ["high", "medium", "low"],
  );
  assert.deepEqual(permissionRows({ run: [], panes: false, background: false }, MESSAGES.en), []);
  assert.equal(riskOf("run"), "high");
  assert.equal(riskOf("panes"), "medium");
  assert.equal(riskOf("background"), "low");
});

test("an update that adds permissions shows what the server says it adds", () => {
  const confirm: UpdateConfirm = {
    token: "t1",
    version: "0.4.0",
    permissions: { run: ["kubectl", "helm"], panes: true, background: true },
    added: { run: ["helm"], panes: false, background: false },
  };
  const html = detail(ENTRY, { kind: "grow", confirm });
  assert.match(html, /This update asks for more:/);
  assert.match(html, /Can run helm with any arguments, as you/);
  assert.doesNotMatch(html, /Can run kubectl/);
  assert.doesNotMatch(html, /Can open terminal and browser panes/);
  assert.match(html, /Update Kubernetes to v0\.4\.0\?/);
  assert.match(html, />Update</);
});

test("a folder Reload that asks for more has its own title", () => {
  const confirm: UpdateConfirm = {
    token: "t1",
    version: "0.3.1",
    permissions: { run: ["kubectl", "helm"], panes: true, background: true },
    added: { run: ["helm"], panes: false, background: false },
  };
  const html = detail({ ...ENTRY, badge: "local", updateMode: "reload", source: "/Users/me/p" }, { kind: "grow", confirm });
  assert.match(html, /Reload Kubernetes from its folder\?/);
  assert.doesNotMatch(html, /Update Kubernetes to/);
  assert.match(html, /This update asks for more:/);
  assert.match(html, /Can run helm with any arguments, as you/);
  assert.match(html, />Reload</);
  assert.doesNotMatch(html, />Update</);
});

// ---- the views ---------------------------------------------------------

function list(failed: boolean, tab: "installed" | "browse" = "browse", items: Entry[] = []): string {
  return render(
    createElement(PluginListView, {
      tab,
      entries: items,
      pickedId: null,
      query: "",
      failed,
      builtins: [DOCKER],
      onTab: noop,
      onQuery: noop,
      onPick: noop,
      onListKey: noop,
      onRetry: noop,
      onInstallFrom: noop,
    }),
  );
}

test("a missing index shows one line and Retry", () => {
  const html = list(true);
  assert.match(html, /Could not load the plugin list/);
  assert.match(html, />Retry</);
  assert.match(html, /Install from URL or folder…/);
  assert.match(html, />Installed</);
  assert.match(html, />Browse</);
  assert.match(html, /placeholder="Search plugins"/);
  const fine = list(false);
  assert.doesNotMatch(fine, /Could not load the plugin list/);
  assert.match(fine, /Install from URL or folder…/);
  assert.doesNotMatch(list(true, "installed"), /Could not load the plugin list/, "only Browse shows the index");
});

test("list rows show the badge, the installed mark, and an update", () => {
  const html = list(false, "installed", [{ ...ENTRY, update: "0.4.0" }, { ...ENTRY, id: "a.b", name: "Other", badge: "local", update: null }]);
  assert.match(html, />verified</);
  assert.match(html, />local</);
  assert.equal((html.match(/plugin-update-tag/g) ?? []).length, 1);
  assert.match(html, />Update</);
  assert.match(html, /aria-selected="false"/);
});

const UNVERIFIED: InstallState = {
  step: "confirm",
  preview: {
    name: "Redis",
    version: "0.1.0",
    size: 31_744,
    badge: "unverified",
    permissions: { run: ["redis-cli"], panes: true, background: false },
    source: "https://github.com/o/r",
    replaces: false,
  },
  trusted: false,
};

test("URL and folder installs need the trust checkbox", () => {
  const locked = flow(UNVERIFIED);
  assert.match(locked, /This plugin is not in the westerm plugin list\. Install it only if you trust where it comes from\./);
  assert.match(locked, /I trust https:\/\/github\.com\/o\/r/);
  assert.match(locked, /type="checkbox"/);
  assert.match(locked, /<button[^>]*disabled=""[^>]*>Install<\/button>/);
  assert.match(locked, /Can run redis-cli with any arguments, as you/);
  assert.match(locked, />high</);
  assert.match(locked, />medium</);
  assert.match(locked, /blocks the plugin&#x27;s direct network requests/);
  assert.match(locked, /v0\.1\.0/);
  assert.match(locked, /31 KB/);
  assert.match(locked, />unverified</);

  const trusted = flow({ ...UNVERIFIED, trusted: true });
  assert.doesNotMatch(trusted, /<button[^>]*disabled=""[^>]*>Install<\/button>/);
  assert.match(trusted, />Install</);

  const local = flow({ ...UNVERIFIED, preview: { ...UNVERIFIED.preview, badge: "local", size: null, source: "/Users/me/p" } });
  assert.match(local, /I trust \/Users\/me\/p/);
  assert.doesNotMatch(local, /KB/, "a folder has no size");

  const verified = flow({ ...UNVERIFIED, preview: { ...UNVERIFIED.preview, badge: "verified" } });
  assert.doesNotMatch(verified, /not in the westerm plugin list/);
  assert.doesNotMatch(verified, /I trust/);
  assert.doesNotMatch(verified, /<button[^>]*disabled=""[^>]*>Install<\/button>/);
});

test("a reinstall says the saved data stays", () => {
  const html = flow({ ...UNVERIFIED, preview: { ...UNVERIFIED.preview, replaces: true }, trusted: true });
  assert.match(html, /Reinstalling replaces the installed version\. Its saved data stays\./);
  assert.match(html, />Reinstall</);
  assert.doesNotMatch(flow(UNVERIFIED), /saved data/);
});

test("the address step has URL and Folder, and shows the server's reason", () => {
  const empty = flow({ step: "address", kind: "url", value: "" });
  assert.match(empty, />URL</);
  assert.match(empty, />Folder</);
  assert.match(empty, /<button[^>]*disabled=""[^>]*>Next<\/button>/);
  const typed = flow({ step: "address", kind: "folder", value: "/tmp/p" }, "plugin.json: not valid JSON");
  assert.doesNotMatch(typed, /<button[^>]*disabled=""[^>]*>Next<\/button>/);
  assert.match(typed, /plugin\.json: not valid JSON/);
  assert.match(typed, /value="\/tmp\/p"/);
});

test("a tarball install says Reinstall to update; a folder has Reload", () => {
  const tarball = detail({ ...ENTRY, badge: "unverified", updateMode: "reinstall", source: "https://example.com/redis.tar.gz" });
  assert.match(tarball, /Reinstall to update/);
  assert.doesNotMatch(tarball, />Reload</);
  const folder = detail({ ...ENTRY, badge: "local", updateMode: "reload", source: "/Users/me/p" });
  assert.match(folder, />Reload</);
  assert.doesNotMatch(folder, /Reinstall to update/);
  assert.match(folder, /\/Users\/me\/p/);
  const update = detail({ ...ENTRY, update: "0.4.0" });
  assert.match(update, /Update to v0\.4\.0/);
  assert.doesNotMatch(detail(ENTRY), /Update to/);
});

test("the details show the version, the author, the source, and the risks", () => {
  const html = detail(ENTRY);
  assert.match(html, /v0\.3\.1/);
  assert.match(html, /by k8s-tools/);
  assert.match(html, /Source/);
  assert.match(html, /https:\/\/github\.com\/k8s-tools\/westerm-k8s/);
  assert.match(html, /Can run kubectl with any arguments, as you/);
  assert.match(html, />high</);
  assert.match(html, />low</);
  assert.match(html, />Uninstall</);
  const index = detail({ ...ENTRY, installed: false });
  assert.match(index, />Install</);
  assert.doesNotMatch(index, />Uninstall</);
});

test("uninstall asks first, and a folder plugin keeps its folder", () => {
  const asked = detail(ENTRY, { kind: "confirmUninstall" });
  assert.match(asked, /Uninstall Kubernetes\? Its files and saved data are deleted\./);
  assert.doesNotMatch(asked, /The folder itself stays\./);
  const folder = detail({ ...ENTRY, badge: "local", updateMode: "reload" }, { kind: "confirmUninstall" });
  assert.match(folder, /Uninstall Kubernetes\? Its files and saved data are deleted\./);
  assert.match(folder, /The folder itself stays\./);
  assert.match(folder, />Cancel</);
  assert.doesNotMatch(detail(ENTRY), /Its files and saved data are deleted/);
});

test("an update asks once before it runs", () => {
  const html = detail({ ...ENTRY, update: "0.4.0" }, { kind: "confirmUpdate" });
  assert.match(html, /Update Kubernetes to v0\.4\.0\?/);
  assert.match(html, />Cancel</);
});

test("parameterized plugin messages include their values in every language", () => {
  for (const locale of LOCALES) {
    const t = MESSAGES[locale].plugins;
    assert.ok(t.by("k8s-tools").includes("k8s-tools"), locale);
    assert.ok(t.trust("https://github.com/o/r").includes("https://github.com/o/r"), locale);
    assert.ok(t.updateTo("0.4.0").includes("0.4.0"), locale);
    assert.ok(t.uninstallConfirm("Kubernetes").includes("Kubernetes"), locale);
    assert.ok(t.size(31).includes("31"), locale);
    assert.ok(t.installTitle("Redis").includes("Redis"), locale);
    assert.ok(t.updateConfirm("Kubernetes", "0.4.0").includes("Kubernetes"), locale);
    assert.ok(t.updateConfirm("Kubernetes", "0.4.0").includes("0.4.0"), locale);
    assert.ok(t.reloadConfirm("Kubernetes").includes("Kubernetes"), locale);
  }
});

// ---- api ---------------------------------------------------------------

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

/** Answers every request with `status` and `body`, and records the requests. */
function answer(status: number, body: unknown): { url: string; init?: RequestInit }[] {
  const seen: { url: string; init?: RequestInit }[] = [];
  globalThis.fetch = (async (url: string, init?: RequestInit) => {
    seen.push({ url, init });
    return new Response(body === undefined ? null : JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
  }) as typeof fetch;
  return seen;
}

test("update tells a permission question from an error", async () => {
  const confirm = { token: "t1", version: "0.4.0", permissions: K8S_PERMISSIONS, added: { run: ["helm"], panes: false, background: false } };
  const seen = answer(409, confirm);
  assert.deepEqual(await update(INFO.id), confirm);
  assert.equal(seen[0].url, "/api/plugins/k8s-tools.kubernetes/update");
  assert.equal(seen[0].init?.method, "POST");
  assert.equal(seen[0].init?.body, "{}");

  answer(409, { error: "reinstall to update" });
  assert.deepEqual(await update(INFO.id), { error: "reinstall to update" });

  const sent = answer(200, { ...INFO, version: "0.4.0" });
  const done = await update(INFO.id, { token: "t1", granted: K8S_PERMISSIONS });
  assert.equal("version" in done && done.version, "0.4.0");
  assert.equal(sent[0].init?.body, JSON.stringify({ token: "t1", granted: K8S_PERMISSIONS }));
});

test("fetchPreview keeps a folder's null size and passes the server's reason on", async () => {
  const preview = {
    token: "t2",
    manifest: { id: "dev.p", name: "P", version: "1.0.0", description: "", icon: null, panel: "p.html", westerm: null, permissions: { run: [], panes: false, background: false }, i18n: {} },
    size: null,
    source: { kind: "folder", path: "/tmp/p" },
  };
  answer(200, preview);
  const got = await fetchPreview({ path: "/tmp/p" });
  assert.ok(!("error" in got));
  assert.equal(got.size, null);
  assert.equal(got.manifest.id, "dev.p");

  answer(400, { error: "plugin.json: not valid JSON" });
  assert.deepEqual(await fetchPreview({ url: "https://example.com/p.tar.gz" }), { error: "plugin.json: not valid JSON" });

  globalThis.fetch = (() => Promise.reject(new Error("offline"))) as typeof fetch;
  assert.deepEqual(await fetchPreview({ path: "/tmp/p" }), { error: "" }, "no reason from the server: the view says it failed");
});

test("install, uninstall, and the catalog", async () => {
  const sent = answer(200, INFO);
  const got = await install({ index: INFO.id, granted: K8S_PERMISSIONS });
  assert.equal("id" in got && got.id, INFO.id);
  assert.equal(sent[0].url, "/api/plugins/install");

  const removed = answer(204, undefined);
  assert.equal(await uninstall(INFO.id), true);
  assert.equal(removed[0].init?.method, "DELETE");
  answer(404, { error: "not installed" });
  assert.equal(await uninstall(INFO.id), false);

  answer(200, { index: null, installed: [{ ...INFO, update: null, updateMode: "check" }, { broken: true }] });
  const catalog = await loadCatalog();
  assert.equal(catalog?.index, null);
  assert.deepEqual(catalog?.installed.map((plugin) => [plugin.id, plugin.updateMode]), [[INFO.id, "check"]]);
  answer(500, { error: "boom" });
  assert.equal(await loadCatalog(), null);
});

// ---- clicks and focus (happy-dom) ---------------------------------------

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

async function mount(element: ReactElement): Promise<{ container: HTMLElement; unmount(): Promise<void> }> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(createElement(I18nProvider, null, element));
    await flush();
  });
  return {
    container,
    async unmount() {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

/** Focuses and clicks `element`, as a keyboard user pressing it does, and lets the answers in. */
async function press(element: HTMLElement | undefined): Promise<void> {
  assert.ok(element, "the control is there");
  await act(async () => {
    element.focus();
    element.click();
    for (let i = 0; i < 4; i += 1) await flush();
  });
}

const buttonIn = (root: ParentNode, text: string) =>
  [...root.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === text);

const checkboxIn = (root: ParentNode) => root.querySelector<HTMLInputElement>('input[type="checkbox"]') ?? undefined;

const REDIS_URL = "https://example.com/redis.tar.gz";

function redisPreview(token: string) {
  return {
    token,
    manifest: {
      id: TARBALL.id,
      name: "Redis",
      version: "0.2.0",
      description: "",
      icon: null,
      panel: "panel.html",
      westerm: null,
      permissions: TARBALL.granted,
      i18n: {},
    },
    size: 2048,
    source: { kind: "url" as const, url: REDIS_URL },
  };
}

test("a reinstall replaces the installed plugin on the server, without uninstalling it first", async () => {
  const calls = { fetched: 0, bodies: [] as InstallBody[] };
  const api: InstallApi = {
    fetchPreview: async () => redisPreview(`t${++calls.fetched}`),
    install: async (body) => {
      calls.bodies.push(body);
      return calls.bodies.length === 1 ? { error: "unknown or expired token" } : { ...TARBALL, version: "0.2.0" };
    },
  };
  const done: string[] = [];
  const view = await mount(
    createElement(InstallFlow, {
      start: { kind: "address", url: REDIS_URL, replaces: TARBALL.id },
      api,
      onInstalled: async (id: string) => void done.push(id),
      onCancel: noop,
    }),
  );
  const { container } = view;

  await press(buttonIn(container, "Next"));
  assert.match(container.textContent ?? "", /Reinstalling replaces the installed version\. Its saved data stays\./);
  assert.doesNotMatch(container.textContent ?? "", /deletes/);
  await press(checkboxIn(container));
  await press(buttonIn(container, "Reinstall"));
  assert.deepEqual(calls.bodies, [{ token: "t1", granted: TARBALL.granted, trusted: true, replace: true }]);
  assert.match(container.textContent ?? "", /unknown or expired token/);
  // The server changed nothing: it is still a reinstall.
  assert.ok(buttonIn(container, "Reinstall"));

  // A fresh fetch (a new token) replaces it again.
  await press(buttonIn(container, "Back"));
  await press(buttonIn(container, "Next"));
  assert.equal(calls.fetched, 2);
  await press(checkboxIn(container));
  await press(buttonIn(container, "Reinstall"));
  assert.deepEqual(calls.bodies[1], { token: "t2", granted: TARBALL.granted, trusted: true, replace: true });
  assert.deepEqual(done, [TARBALL.id]);
  await view.unmount();
});

test("a plain install does not ask to replace anything", async () => {
  const bodies: InstallBody[] = [];
  const api: InstallApi = {
    fetchPreview: async () => redisPreview("t1"),
    install: async (body) => {
      bodies.push(body);
      return { ...TARBALL, version: "0.2.0" };
    },
  };
  const view = await mount(createElement(InstallFlow, { start: { kind: "address", url: REDIS_URL }, api, onInstalled: async () => {}, onCancel: noop }));
  await press(buttonIn(view.container, "Next"));
  await press(checkboxIn(view.container));
  await press(buttonIn(view.container, "Install"));
  assert.deepEqual(bodies, [{ token: "t1", granted: TARBALL.granted, trusted: true }]);
  await view.unmount();
});

/** Answers the manager's requests by path; `routes` returns undefined for a network failure. */
function serve(routes: (path: string, init?: RequestInit) => Promise<Response> | Response | undefined) {
  globalThis.fetch = (async (url: string, init?: RequestInit) => {
    const answer = routes(url, init);
    if (answer === undefined) throw new Error("offline");
    return answer;
  }) as typeof fetch;
}

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

const CATALOG = {
  index: [],
  installed: [
    { ...INFO, update: "0.4.0", updateMode: "check" },
    { ...TARBALL, update: null, updateMode: "reinstall" },
  ],
};

function managerOf(external: ExternalInfo[], selected: string | null) {
  return createElement(PluginManager, {
    builtins: [DOCKER],
    external,
    installed: external.map((info) => info.id),
    selected,
    onSetInstalled: () => Promise.resolve(true),
    onExternalChanged: () => Promise.resolve(),
    onClose: noop,
  });
}

/** Focus is on a usable control inside the dialog, not on <body>. */
function assertFocusInside(container: HTMLElement, where: string): HTMLElement {
  const dialog = container.querySelector<HTMLElement>('[role="dialog"]');
  const active = document.activeElement as HTMLElement | null;
  assert.ok(dialog && active && active !== document.body && dialog.contains(active), `focus is inside the dialog ${where}`);
  assert.notEqual((active as HTMLButtonElement).disabled, true, `focus is not on a disabled control ${where}`);
  return active;
}

test("a confirm row takes the focus, and focus stays in the dialog when it closes or goes busy", async () => {
  serve((path) => (path === "/api/plugins/catalog" ? json(200, CATALOG) : path.endsWith("/update") ? new Promise<Response>(noop) : undefined));
  const view = await mount(managerOf([INFO, TARBALL], INFO.id));
  const { container } = view;

  await press(buttonIn(container, "Uninstall"));
  let active = assertFocusInside(container, "after Uninstall");
  assert.equal(active.textContent, "Cancel");
  assert.ok(active.closest(".plugin-confirm"), "the confirm row has the focus");

  await press(buttonIn(container, "Cancel"));
  assertFocusInside(container, "after Cancel removed the row");

  await press(buttonIn(container, "Update to v0.4.0"));
  active = assertFocusInside(container, "after Update to");
  assert.ok(active.closest(".plugin-confirm"));

  // The row's Update goes busy (the request never answers): its buttons are disabled.
  await press(buttonIn(container.querySelector(".plugin-confirm")!, "Update"));
  assertFocusInside(container, "while the update is busy");
  await view.unmount();
});

test("the install steps take the focus: A its address, B its heading", async () => {
  serve((path) =>
    path === "/api/plugins/catalog" ? json(200, CATALOG) : path === "/api/plugins/fetch" ? json(200, redisPreview("t1")) : undefined,
  );
  const view = await mount(managerOf([INFO, TARBALL], TARBALL.id));
  const { container } = view;

  await press(buttonIn(container, "Reinstall to update"));
  let active = assertFocusInside(container, "after Reinstall to update");
  assert.ok(active.classList.contains("plugin-address"), "step A focuses its address");

  await press(buttonIn(container, "Next"));
  active = assertFocusInside(container, "after Next");
  assert.equal(active.tagName, "H3");
  assert.match(active.textContent ?? "", /Install Redis\?/);

  await press(buttonIn(container, "Back"));
  active = assertFocusInside(container, "after Back");
  assert.ok(active.classList.contains("plugin-address"));
  await view.unmount();
});

test("Retry removes its row, and focus stays in the list", async () => {
  let up = false;
  serve((path) => (path === "/api/plugins/catalog" ? (up ? json(200, { index: [], installed: [] }) : json(500, { error: "down" })) : undefined));
  const view = await mount(managerOf([], null));
  const { container } = view;
  assert.match(container.textContent ?? "", /Could not load the plugin list/);

  up = true;
  await press(buttonIn(container, "Retry"));
  assert.doesNotMatch(container.textContent ?? "", /Could not load the plugin list/);
  const active = assertFocusInside(container, "after Retry");
  assert.ok(active.closest(".plugin-side"), "focus goes back to the list it was in");
  await view.unmount();
});
