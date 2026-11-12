import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Box } from "lucide-react";
import { ActivityBar } from "../src/components/ActivityBar.tsx";
import { I18nProvider, LOCALES, type Locale } from "../src/i18n/index.tsx";
import { closesManager, externalOpensPanel, parseInstalled, statusStore, syncInstances, withPlugin } from "../src/plugins/installed.ts";
import { PluginManager } from "../src/plugins/PluginManager.tsx";
import type { PluginContext, PluginInstance, PluginStatus, PluginText, WestermPlugin } from "../src/plugins/types.ts";

// I18nProvider follows the browser's language, which Node takes from LANG.
// Pin it so the English text below matches on every machine.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

const noop = () => {};
const QUIET: PluginStatus = { badge: null, alert: false };

const TEXT: PluginText = {
  name: "Fake",
  description: "Shows fake things.",
  permissions: ["Run fake commands", "Open fake panes"],
};

interface Fake {
  plugin: WestermPlugin;
  started: number;
  disposed: number;
  contexts: PluginContext[];
  /** The last started instance's status setter; it tells the subscribers. */
  setStatus(status: PluginStatus): void;
}

function fakePlugin(id: string): Fake {
  const fake: Fake = {
    started: 0,
    disposed: 0,
    contexts: [],
    setStatus: noop,
    plugin: {
      id,
      version: "1.2.3",
      icon: Box,
      text: Object.fromEntries(LOCALES.map((locale) => [locale, TEXT])) as Record<Locale, PluginText>,
      start(ctx) {
        fake.started += 1;
        fake.contexts.push(ctx);
        let status = QUIET;
        const listeners = new Set<() => void>();
        fake.setStatus = (next) => {
          status = next;
          for (const listener of listeners) listener();
        };
        const instance: PluginInstance = {
          subscribe(listener) {
            listeners.add(listener);
            return () => listeners.delete(listener);
          },
          status: () => status,
          setPanelOpen: noop,
          dispose: () => {
            fake.disposed += 1;
          },
          Panel: () => null,
        };
        return instance;
      },
    },
  };
  return fake;
}

const ctxFor = (id: string): PluginContext => ({
  openShell: noop,
  openBrowser: noop,
  openPane: noop,
  folders: () => [],
  fetch: () => Promise.reject(new Error(`no fetch for ${id}`)),
  refreshInstalled: noop,
  openManager: noop,
});

test("installed lists keep known ids in registry order", () => {
  assert.deepEqual(parseInstalled({ installed: ["b", "x", "a"] }, ["a", "b"]), ["a", "b"]);
  assert.deepEqual(parseInstalled("junk", ["a"]), []);
  assert.deepEqual(parseInstalled({ installed: [1, "a"] }, ["a"]), ["a"]);
  assert.deepEqual(parseInstalled({ installed: "a" }, ["a"]), []);
  assert.deepEqual(withPlugin(["b"], "a", true, ["a", "b"]), ["a", "b"]);
  assert.deepEqual(withPlugin(["a", "b"], "a", false, ["a", "b"]), ["b"]);
  assert.deepEqual(withPlugin(["a"], "a", true, ["a", "b"]), ["a"]);
});

test("a successful install closes the manager; an uninstall or a failure keeps it open", () => {
  assert.equal(closesManager(true, true), true);
  assert.equal(closesManager(true, false), false);
  assert.equal(closesManager(false, true), false);
  assert.equal(closesManager(false, false), false);
});

test("a new external plugin opens its panel; an update or a removal does not", () => {
  assert.equal(externalOpensPanel(true, false), true);
  assert.equal(externalOpensPanel(true, true), false, "an update or a reinstall keeps the manager open");
  assert.equal(externalOpensPanel(false, true), false);
});

test("nothing starts while nothing is installed", () => {
  const fake = fakePlugin("a");
  assert.equal(syncInstances(new Map(), [], [fake.plugin], ctxFor).size, 0);
  assert.equal(fake.started, 0);
});

test("installing starts the plugin once, with its own context", () => {
  const a = fakePlugin("a");
  const b = fakePlugin("b");
  const first = syncInstances(new Map(), ["a"], [a.plugin, b.plugin], ctxFor);
  assert.deepEqual([...first.keys()], ["a"]);
  assert.equal(a.started, 1);
  assert.equal(a.contexts.length, 1);
  // Installing another keeps the running instance: no second start.
  const second = syncInstances(first, ["a", "b"], [a.plugin, b.plugin], ctxFor);
  assert.deepEqual([...second.keys()], ["a", "b"]);
  assert.equal(second.get("a"), first.get("a"));
  assert.equal(a.started, 1);
  assert.equal(b.started, 1);
});

test("uninstalling disposes the instance", () => {
  const fake = fakePlugin("a");
  const map = syncInstances(new Map(), ["a"], [fake.plugin], ctxFor);
  assert.equal(syncInstances(map, [], [fake.plugin], ctxFor).size, 0);
  assert.equal(fake.disposed, 1);
});

test("the status store keeps one snapshot until a plugin's status changes", () => {
  const a = fakePlugin("a");
  const b = fakePlugin("b");
  const instances = syncInstances(new Map(), ["a", "b"], [a.plugin, b.plugin], ctxFor);
  const store = statusStore(instances);
  const first = store.snapshot();
  assert.deepEqual(first, { a: QUIET, b: QUIET });
  assert.equal(store.snapshot(), first);

  let calls = 0;
  const unsubscribe = store.subscribe(() => {
    calls += 1;
  });
  b.setStatus({ badge: 3, alert: true });
  assert.equal(calls, 1);
  const second = store.snapshot();
  assert.notEqual(second, first);
  assert.deepEqual(second.b, { badge: 3, alert: true });
  assert.equal(store.snapshot(), second);

  unsubscribe();
  b.setStatus(QUIET);
  assert.equal(calls, 1);
  assert.deepEqual(statusStore(new Map()).snapshot(), {});
});

function bar(change: Partial<Parameters<typeof ActivityBar>[0]>) {
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(ActivityBar, {
        sidebarOpen: false,
        counts: { shell: 0, browser: 0, editor: 0, ssh: 0, docs: 0, plugin: 0 },
        onLaunch: noop,
        onToggleSidebar: null,
        onOpenSettings: noop,
        onOpenPalette: noop,
        onOpenRemoteWindow: null,
        paletteShortcut: "⌘K",
        agentsOpen: false,
        agentsWaiting: false,
        onToggleAgents: noop,
        tunnelOpen: false,
        tunnelOn: false,
        onToggleTunnel: null,
        plugins: [],
        onTogglePlugin: noop,
        onOpenPlugins: noop,
        ...change,
      }),
    ),
  );
}

test("the plugin bar shows installed plugins, their badge, and the puzzle button", () => {
  const html = bar({
    plugins: [{ id: "a", label: "Fake", icon: Box, status: { badge: 5, alert: true }, pressed: true, external: false }],
  });
  assert.match(html, /class="plugin-bar"/);
  assert.match(html, /aria-label="Plugins"/);
  assert.match(html, /aria-label="Fake · 5"[^>]*aria-pressed="true"/);
  assert.match(html, />5</);
  assert.match(html, /plugin-alert/);

  const quiet = bar({ plugins: [{ id: "a", label: "Fake", icon: Box, status: QUIET, pressed: false, external: false }] });
  assert.match(quiet, /aria-label="Fake"[^>]*aria-pressed="false"/);
  assert.doesNotMatch(quiet, /plugin-alert/);
  assert.doesNotMatch(quiet, /activity-badge/);
});

test("the plugin bar shows an external icon as an img with a mark", () => {
  const html = bar({
    plugins: [
      {
        id: "k8s-tools.kubernetes",
        label: "Kubernetes",
        icon: Box,
        iconUrl: "/plugin-assets/k8s-tools.kubernetes/icon.svg",
        external: true,
        note: "Plugin stopped: it tried to leave its page",
        status: { badge: null, alert: false, note: "Plugin stopped: it tried to leave its page" },
        pressed: false,
      },
    ],
  });
  assert.match(html, /<img src="\/plugin-assets\/k8s-tools\.kubernetes\/icon\.svg" alt="" width="19" height="19"/);
  assert.match(html, /class="plugin-external"/);
  assert.match(html, /aria-label="Kubernetes · Plugin stopped: it tried to leave its page"/);
  assert.match(html, /role="presentation">Kubernetes · Plugin stopped: it tried to leave its page</);

  const builtin = bar({ plugins: [{ id: "a", label: "Fake", icon: Box, status: QUIET, pressed: false, external: false }] });
  assert.doesNotMatch(builtin, /<img/);
  assert.doesNotMatch(builtin, /plugin-external/);
});

test("with nothing installed the plugin bar has only the puzzle button", () => {
  const html = bar({});
  assert.match(html, /class="plugin-bar"/);
  assert.match(html, /aria-label="Plugins"/);
  assert.doesNotMatch(html, /plugin-button/);
});

test("a remote window's plugin bar has the plugins that run there and no plugin manager", () => {
  const html = bar({
    onOpenPlugins: null,
    plugins: [{ id: "a", label: "Fake", icon: Box, status: QUIET, pressed: false, external: false }],
  });
  assert.match(html, /class="plugin-bar"/);
  assert.match(html, /aria-label="Fake"/);
  assert.doesNotMatch(html, /aria-label="Plugins"/);
  // With no such plugin there is no bar at all.
  assert.doesNotMatch(bar({ onOpenPlugins: null }), /plugin-bar/);
});

function manager(installed: string[], selected: string | null = null) {
  const fake = fakePlugin("a");
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(PluginManager, {
        builtins: [fake.plugin],
        external: [],
        installed,
        selected,
        onSetInstalled: () => Promise.resolve(true),
        onExternalChanged: () => Promise.resolve(),
        onClose: noop,
      }),
    ),
  );
}

test("the manager lists permissions and offers Install", () => {
  const html = manager([]);
  assert.match(html, /role="dialog"/);
  assert.match(html, /aria-modal="true"/);
  assert.match(html, />Plugins</);
  assert.match(html, />Fake <span class="plugin-badge built-in">built-in<\/span>/);
  assert.match(html, /v1\.2\.3/);
  assert.match(html, /Shows fake things\./);
  assert.match(html, /This plugin can/);
  assert.match(html, /<li>Run fake commands<\/li><li>Open fake panes<\/li>/);
  assert.match(html, />Install</);
  assert.doesNotMatch(html, />Uninstall</);
});

test("the manager opens on Browse when nothing is installed, else on Installed", () => {
  assert.match(manager([]), /<button[^>]*aria-pressed="true"[^>]*>Browse<\/button>/);
  assert.match(manager(["a"]), /<button[^>]*aria-pressed="true"[^>]*>Installed<\/button>/);
});

test("the manager offers Uninstall for an installed plugin", () => {
  const html = manager(["a"], "a");
  assert.match(html, />Uninstall</);
  assert.doesNotMatch(html, />Install</);
  assert.doesNotMatch(html, /Could not change the plugins/);
});
