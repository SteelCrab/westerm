import "./dom-page.ts";
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement, type ReactElement } from "react";
import { Box } from "lucide-react";
import { LOCALES, type Locale } from "../src/i18n/messages.ts";
import { PaneHostContext } from "../src/plugins/paneHost.ts";
import { PluginPaneHost } from "../src/plugins/PluginPaneHost.tsx";
import type { PluginInstance, PluginPaneProps, PluginText, WestermPlugin } from "../src/plugins/types.ts";
import { planOpenPane, pluginFolders } from "../src/pluginPanes.ts";
import { createDashboard, dashboardReducer, type Dashboard, type DashboardAction, type Session } from "../src/workspace.ts";
import { click, mount, text, type Mounted } from "./dom-page.ts";

const run = (state: Dashboard, ...actions: DashboardAction[]) => actions.reduce(dashboardReducer, state);

const TEXT: PluginText = { name: "Fake", description: "Fake things.", permissions: [] };

const FAKE: WestermPlugin = {
  id: "fake",
  version: "1.0.0",
  icon: Box,
  text: Object.fromEntries(LOCALES.map((locale) => [locale, TEXT])) as Record<Locale, PluginText>,
  start: () => {
    throw new Error("not started in these tests");
  },
};

/** An instance whose pane shows `data.n` and hands its props to the test. */
function instanceWithPane(seen: PluginPaneProps[]): PluginInstance {
  return {
    subscribe: () => () => {},
    status: () => ({ badge: null, alert: false }),
    setPanelOpen: () => {},
    dispose: () => {},
    Panel: () => null,
    PaneView: (props: PluginPaneProps) => {
      seen.push(props);
      return createElement("p", { className: "fake-pane" }, `n=${(props.data as { n: number }).n} view=${props.view}`);
    },
  };
}

const withPane = () =>
  run(
    createDashboard("w1", "t1"),
    { type: "open", id: "s1", kind: "shell" },
    { type: "open", id: "p1", kind: "plugin", pluginPane: { plugin: "fake", view: "v", reuseKey: "k", data: { n: 7 } }, title: "Seven" },
  );

const paneOf = (state: Dashboard): Session => state.sessions.find((session) => session.id === "p1")!;

let mounted: Mounted | null = null;
afterEach(async () => {
  await mounted?.unmount();
  mounted = null;
});

async function host(session: Session, instances: Map<string, PluginInstance>, dispatched: DashboardAction[]): Promise<Mounted> {
  const element: ReactElement = createElement(
    PaneHostContext.Provider,
    { value: { instances, registry: [FAKE] } },
    createElement(PluginPaneHost, { session, focused: true, dispatch: (action: DashboardAction) => dispatched.push(action) }),
  );
  mounted = await mount(element);
  return mounted;
}

test("a plugin pane renders its plugin's PaneView with its data", async () => {
  const seen: PluginPaneProps[] = [];
  const { container } = await host(paneOf(withPane()), new Map([["fake", instanceWithPane(seen)]]), []);
  assert.equal(text(container.querySelector(".fake-pane")!), "n=7 view=v");
  assert.equal(seen.at(-1)?.focused, true);
});

test("setData, setTitle, and close dispatch for this pane", async () => {
  const seen: PluginPaneProps[] = [];
  const dispatched: DashboardAction[] = [];
  await host(paneOf(withPane()), new Map([["fake", instanceWithPane(seen)]]), dispatched);
  const props = seen.at(-1)!;
  props.setData({ n: 8 });
  props.setTitle("Eight");
  props.close();
  assert.deepEqual(dispatched, [
    { type: "pluginPaneData", id: "p1", data: { n: 8 } },
    { type: "title", id: "p1", title: "Eight" },
    { type: "close", id: "p1" },
  ]);
});

test("a pane whose plugin is not installed shows the missing notice and a close button", async () => {
  const dispatched: DashboardAction[] = [];
  const { container } = await host(paneOf(withPane()), new Map(), dispatched);
  assert.match(text(container), /This pane needs the Fake plugin/);
  await click(container.querySelector("button"));
  assert.deepEqual(dispatched, [{ type: "close", id: "p1" }]);
});

test("an installed plugin without pane views shows the missing notice too", async () => {
  const bare = { ...instanceWithPane([]), PaneView: undefined };
  const { container } = await host(paneOf(withPane()), new Map([["fake", bare]]), []);
  assert.match(text(container), /This pane needs the Fake plugin/);
});

test("an unknown plugin id falls back to the id in the notice", async () => {
  const state = run(createDashboard("w1", "t1"), {
    type: "open",
    id: "p1",
    kind: "plugin",
    pluginPane: { plugin: "gone", view: "v", reuseKey: null, data: null },
  });
  const { container } = await host(paneOf(state), new Map(), []);
  assert.match(text(container), /This pane needs the gone plugin/);
});

test("planOpenPane focuses the pane with the same reuse key instead of opening one", () => {
  const state = withPane();
  assert.deepEqual(planOpenPane(state, "fake", { view: "v", title: "x", data: {}, reuseKey: "k" }, "p9", 1.5), { type: "focus", id: "p1" });
});

test("planOpenPane opens a plugin pane with its title when none matches", () => {
  const state = withPane();
  const options = { view: "v", title: "Other", data: { n: 1 }, reuseKey: "other" };
  assert.deepEqual(planOpenPane(state, "fake", options, "p9", 1.5), {
    type: "open",
    id: "p9",
    kind: "plugin",
    pluginPane: { plugin: "fake", view: "v", reuseKey: "other", data: { n: 1 } },
    title: "Other",
    aspect: 1.5,
  });
  const opened = run(state, planOpenPane(state, "fake", options, "p9", 1.5));
  assert.equal(opened.sessions.find((session) => session.id === "p9")?.title, "Other");
  // No key: always a new pane.
  assert.equal(planOpenPane(state, "fake", { ...options, reuseKey: null }, "p10", 1).type, "open");
});

test("pluginFolders lists each shell folder once, at most 32", () => {
  let state = createDashboard("w1", "t1");
  for (let i = 0; i < 40; i += 1) {
    state = run(state, { type: "open", id: `s${i}`, kind: "shell" }, { type: "place", id: `s${i}`, place: { cwd: `/f/${i % 35}`, branch: null } });
  }
  state = run(state, { type: "open", id: "e1", kind: "editor" }, { type: "place", id: "e1", place: { cwd: "/editor", branch: null } });
  const folders = pluginFolders(state);
  assert.equal(folders.length, 32);
  assert.equal(new Set(folders).size, 32);
  assert.ok(!folders.includes("/editor"), "only shells");
  assert.equal(folders[0], "/f/0");
});
