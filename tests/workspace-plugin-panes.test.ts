import assert from "node:assert/strict";
import { test } from "node:test";
import { fromGrid, leaves, type LayoutNode } from "../shared/layout.ts";
import { parseSavedDashboard, PLUGIN_PANE_DATA_MAX, type SavedPluginPane } from "../shared/workspace-file.ts";
import {
  createDashboard,
  dashboardReducer,
  findPluginPane,
  toSaved,
  type Dashboard,
  type DashboardAction,
  type PluginPane,
} from "../src/workspace.ts";

const run = (state: Dashboard, ...actions: DashboardAction[]) => actions.reduce(dashboardReducer, state);

const pluginPane: PluginPane = { plugin: "db", view: "table", reuseKey: "c1/app/public/orders", data: { sql: "SELECT 1" } };
const saved: SavedPluginPane = { id: "p1", title: "orders", name: null, ...pluginPane };

const split = (a: string, b: string): LayoutNode => ({
  type: "split",
  dir: "row",
  sizes: [0.5, 0.5],
  children: [
    { type: "pane", id: a },
    { type: "pane", id: b },
  ],
});

/** One workspace, one tab with shell s1 and plugin pane p1 side by side. */
const file = (pluginPanes: unknown, tab: Record<string, unknown> = {}) => ({
  version: 1,
  activeTabId: "t1",
  activeWorkspaceId: "w1",
  workspaces: [{ id: "w1", name: null, activeTabId: "t1", icon: "layers", color: "blue" }],
  sessions: [{ id: "s1", kind: "shell", title: "Shell 1", name: null, url: null }],
  tabs: [{ id: "t1", workspaceId: "w1", name: null, paneIds: ["s1"], pluginIds: ["p1"], layout: split("s1", "p1"), focusedId: "p1", maximizedId: null, ...tab }],
  pluginPanes,
});

/** A shell s1 and a plugin pane p1 beside it. */
const withPluginPane = () =>
  run(
    createDashboard("w1", "t1"),
    { type: "open", id: "s1", kind: "shell" },
    { type: "open", id: "p1", kind: "plugin", pluginPane, name: "orders" },
  );

test("a new plugin pane runs at once and keeps its plugin state", () => {
  const pane = withPluginPane().sessions.find((session) => session.id === "p1");
  assert.equal(pane?.kind, "plugin");
  assert.equal(pane?.status, "running");
  assert.deepEqual(pane?.pluginPane, pluginPane);
  assert.equal(withPluginPane().sessions.find((session) => session.id === "s1")?.pluginPane, null);
});

test("plugin panes survive a save and a load", () => {
  const state = withPluginPane();
  const written = toSaved(state);
  assert.deepEqual(written.sessions.map((session) => session.id), ["s1"], "an older version checks sessions");
  assert.deepEqual(written.tabs[0].paneIds, ["s1"]);
  assert.deepEqual(written.tabs[0].pluginIds, ["p1"]);
  assert.deepEqual(leaves(written.tabs[0].layout ?? null), ["s1", "p1"]);
  assert.deepEqual(written.pluginPanes, [{ ...saved, title: written.pluginPanes?.[0]?.title ?? "", name: "orders" }]);

  const parsed = parseSavedDashboard(JSON.parse(JSON.stringify(written)));
  assert.ok(parsed);
  const loaded = run(createDashboard("w9", "t9"), { type: "hydrate", saved: parsed });
  const pane = loaded.sessions.find((session) => session.id === "p1");
  assert.equal(pane?.kind, "plugin");
  assert.equal(pane?.status, "running");
  assert.equal(pane?.name, "orders");
  assert.deepEqual(pane?.pluginPane, pluginPane);
  assert.deepEqual(leaves(loaded.tabs[0].layout), ["s1", "p1"]);
});

test("a plugin pane keeps its place in the split", () => {
  const parsed = parseSavedDashboard(file([saved]));
  assert.ok(parsed);
  assert.deepEqual(parsed.pluginPanes, [saved]);
  assert.deepEqual(parsed.tabs[0].pluginIds, ["p1"]);
  assert.deepEqual(parsed.tabs[0].layout, split("s1", "p1"));
  assert.equal(parsed.tabs[0].focusedId, "p1");
});

test("a file from before plugin panes loads with none", () => {
  const parsed = parseSavedDashboard({ ...file(undefined), tabs: [{ id: "t1", workspaceId: "w1", name: null, paneIds: ["s1"], focusedId: null, maximizedId: null }] });
  assert.ok(parsed);
  assert.deepEqual(parsed.pluginPanes, []);
  assert.deepEqual(parsed.tabs[0].pluginIds, []);
});

test("a broken plugin pane entry costs the plugin panes, not the file", () => {
  const broken = [
    [{ ...saved, plugin: "Bad Id" }],
    [{ ...saved, view: "" }],
    [{ ...saved, id: "s1" }],
    [{ ...saved, reuseKey: "x".repeat(513) }],
    [saved, saved],
    "nope",
  ];
  for (const pluginPanes of broken) {
    const parsed = parseSavedDashboard(file(pluginPanes));
    assert.ok(parsed, JSON.stringify(pluginPanes).slice(0, 80));
    assert.deepEqual(parsed.pluginPanes, []);
    assert.deepEqual(parsed.tabs[0].pluginIds, []);
    assert.deepEqual(parsed.tabs[0].layout, fromGrid(["s1"]));
  }
});

test("an external plugin's id is accepted", () => {
  const parsed = parseSavedDashboard(file([{ ...saved, plugin: "k8s-tools.kubernetes" }]));
  assert.equal(parsed?.pluginPanes?.[0]?.plugin, "k8s-tools.kubernetes");
});

test("plugin pane data over the limit is dropped", () => {
  const parsed = parseSavedDashboard(file([{ ...saved, data: { sql: "x".repeat(PLUGIN_PANE_DATA_MAX) } }]));
  assert.ok(parsed);
  assert.deepEqual(parsed.pluginPanes, []);
});

test("findPluginPane finds the pane by key in the workspace in view only", () => {
  const state = withPluginPane();
  assert.equal(findPluginPane(state, "db", "c1/app/public/orders"), "p1");
  assert.equal(findPluginPane(state, "docker", "c1/app/public/orders"), null);
  assert.equal(findPluginPane(state, "db", "other"), null);
  const away = run(state, { type: "newWorkspace", id: "w2", tabId: "t2" }, { type: "open", id: "s2", kind: "shell" });
  assert.equal(findPluginPane(away, "db", "c1/app/public/orders"), null);
});

test("pluginPaneData over the saved-file limit keeps the old data", () => {
  const state = withPluginPane();
  const big = run(state, { type: "pluginPaneData", id: "p1", data: { sql: "x".repeat(PLUGIN_PANE_DATA_MAX) } });
  assert.equal(big, state, "the layout must stay loadable");
  const parsed = parseSavedDashboard(JSON.parse(JSON.stringify(toSaved(big))));
  assert.equal(parsed?.pluginPanes?.length, 1);
});

test("pluginPaneData replaces the data of that pane only", () => {
  const state = run(withPluginPane(), { type: "pluginPaneData", id: "p1", data: { sql: "SELECT 2" } });
  assert.deepEqual(state.sessions.find((session) => session.id === "p1")?.pluginPane, { ...pluginPane, data: { sql: "SELECT 2" } });
  assert.equal(run(state, { type: "pluginPaneData", id: "s1", data: {} }), state, "not a plugin pane");
});
