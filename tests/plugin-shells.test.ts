import assert from "node:assert/strict";
import { test } from "node:test";
import { planOpenFolder, planOpenShell } from "../src/pluginPanes.ts";
import type { OpenShellOptions } from "../src/plugins/types.ts";
import { activeTab, createDashboard, dashboardReducer, endedBy, findRunPane, type Dashboard, type DashboardAction } from "../src/workspace.ts";

const run = (state: Dashboard, ...actions: DashboardAction[]) => actions.reduce(dashboardReducer, state);

const runOptions = (target: string, over: Partial<OpenShellOptions> = {}): OpenShellOptions => ({
  action: "run",
  target: target.repeat(32),
  cwd: "/p",
  name: "p plan",
  reuse: false,
  replaceKey: "run:1",
  ...over,
});

/** One workspace, one tab with the shell s1. */
function oneShell(): Dashboard {
  return run(createDashboard("w1", "t1"), { type: "open", id: "s1", kind: "shell", aspect: 1 });
}

/** `oneShell` plus the run pane s2 of `plugin`, opened beside s1. */
function withRunPane(plugin = "terraform"): Dashboard {
  const state = oneShell();
  return run(state, planOpenShell(state, plugin, runOptions("a"), "s2", 1));
}

const ids = (state: Dashboard) => state.sessions.map((session) => session.id);

test("a run pane is replaced in place", () => {
  const before = withRunPane();
  assert.deepEqual(before.sessions.find((session) => session.id === "s2")?.runKey, { plugin: "terraform", key: "run:1" });
  const after = run(before, planOpenShell(before, "terraform", runOptions("b"), "s3", 1));
  assert.deepEqual(ids(after), ["s1", "s3"]);
  const opened = after.sessions.find((session) => session.id === "s3");
  assert.deepEqual(opened?.runKey, { plugin: "terraform", key: "run:1" });
  assert.deepEqual(opened?.pluginLaunch, { plugin: "terraform", action: "run", target: "b".repeat(32) });
  assert.equal(opened?.cwd, "/p");
  assert.equal(opened?.name, "p plan");
  const layout = JSON.stringify(activeTab(before).layout);
  assert.ok(layout.includes('"s2"'));
  assert.equal(JSON.stringify(activeTab(after).layout), layout.replace('"s2"', '"s3"'));
  assert.equal(activeTab(after).focusedId, "s3");
});

test("a replaced run pane ends its session, like a closed pane", () => {
  const before = withRunPane();
  const replace = planOpenShell(before, "terraform", runOptions("b"), "s3", 1);
  assert.deepEqual(endedBy(before, replace), ["s2"], "its shell must not stay on the host without a pane");
  assert.deepEqual(endedBy(before, { type: "close", id: "s1" }), ["s1"]);
  assert.deepEqual(endedBy(before, { type: "closeTab", id: "t1", fallbackId: "t9" }), ["s1", "s2"]);
  assert.deepEqual(endedBy(before, { type: "closeWorkspace", id: "w1", fallbackId: "w9", fallbackTabId: "t9" }), ["s1", "s2"]);
  // Opening beside, focusing, and anything else end nothing.
  assert.deepEqual(endedBy(oneShell(), planOpenShell(oneShell(), "terraform", runOptions("a"), "s2", 1)), []);
  assert.deepEqual(endedBy(before, { type: "focus", id: "s1" }), []);
});

test("another plugin's pane with the same key stays", () => {
  const before = withRunPane("db");
  const after = run(before, planOpenShell(before, "terraform", runOptions("b"), "s3", 1));
  assert.deepEqual(ids(after), ["s1", "s2", "s3"]);
});

test("a run pane in a workspace out of view is not replaced", () => {
  const first = withRunPane();
  const second = run(first, { type: "newWorkspace", id: "w2", tabId: "t2" }, { type: "open", id: "s4", kind: "shell", aspect: 1 });
  assert.equal(findRunPane(second, "terraform", "run:1"), null);
  const after = run(second, planOpenShell(second, "terraform", runOptions("b"), "s3", 1));
  assert.deepEqual(ids(after), ["s1", "s2", "s4", "s3"]);
  assert.deepEqual(activeTab(after).paneIds, ["s4", "s3"]);
});

test("a run pane in another tab is replaced there, and that tab shows", () => {
  const first = withRunPane();
  const second = run(first, { type: "newTab", id: "t2" }, { type: "open", id: "s4", kind: "shell", aspect: 1 });
  assert.equal(activeTab(second).id, "t2");
  const after = run(second, planOpenShell(second, "terraform", runOptions("b"), "s3", 1));
  assert.equal(activeTab(after).id, "t1");
  assert.deepEqual(activeTab(after).paneIds, ["s1", "s3"]);
  assert.equal(activeTab(after).focusedId, "s3");
});

test("without a pane to replace, the shell opens beside the focused pane", () => {
  const state = oneShell();
  const after = run(state, planOpenShell(state, "terraform", runOptions("a"), "s2", 1));
  assert.deepEqual(activeTab(after).paneIds, ["s1", "s2"]);
  assert.equal(activeTab(after).focusedId, "s2");
});

test("replaceKey wins over reuse", () => {
  const before = withRunPane();
  // The same launch again: `reuse` alone would focus s2.
  const action = planOpenShell(before, "terraform", runOptions("a", { reuse: true }), "s3", 1);
  assert.equal(action.type, "open");
  assert.deepEqual(ids(run(before, action)), ["s1", "s3"]);
});

test("reuse still focuses the pane a launch opened, without a replace key", () => {
  const state = oneShell();
  const options: OpenShellOptions = { action: "logs", target: "0123456789ab", cwd: null, name: "logs", reuse: true };
  const opened = run(state, planOpenShell(state, "docker", options, "s2", 1));
  assert.deepEqual(planOpenShell(opened, "docker", options, "s3", 1), { type: "focus", id: "s2" });
  assert.equal(opened.sessions.find((session) => session.id === "s2")?.runKey, null);
});

test("openFolder plans a plain shell in the folder", () => {
  const action = planOpenFolder("/p/modules/eks", "eks", "s2", 1);
  assert.deepEqual(action, { type: "open", id: "s2", kind: "shell", cwd: "/p/modules/eks", name: "eks", aspect: 1 });
  const after = run(oneShell(), action);
  const opened = after.sessions.find((session) => session.id === "s2");
  assert.equal(opened?.pluginLaunch, null);
  assert.equal(opened?.cwd, "/p/modules/eks");
});
