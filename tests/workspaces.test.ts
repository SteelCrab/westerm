import assert from "node:assert/strict";
import { test } from "node:test";
import { parseSavedDashboard } from "../shared/workspace-file.ts";
import {
  activeTab,
  activeWorkspace,
  createDashboard,
  dashboardReducer,
  stepTab,
  tabByNumber,
  tabSessions,
  toSaved,
  visibleTabs,
  workspaceLabel,
  workspaceTabs,
  type Dashboard,
  type DashboardAction,
} from "../src/workspace.ts";

const start = () => createDashboard("w1", "t1");
const run = (state: Dashboard, ...actions: DashboardAction[]) => actions.reduce(dashboardReducer, state);
const open = (state: Dashboard, id: string) => dashboardReducer(state, { type: "open", id, kind: "shell" });
const tabIds = (state: Dashboard) => visibleTabs(state).map((tab) => tab.id);
const tabsOf = (state: Dashboard, workspaceId: string) => workspaceTabs(state, workspaceId).map((tab) => tab.id);
const workspaceIds = (state: Dashboard) => state.workspaces.map((workspace) => workspace.id);
const sessionIds = (state: Dashboard) => state.sessions.map((session) => session.id);
/** The UI's numbered name for a workspace without one; the reducer stays language-free. */
const untitled = (number: number) => `Workspace ${number}`;

/** w1: t1 (a), t2 (b) · w2: t3 (c), shown. */
const twoWorkspaces = () => {
  let state = open(start(), "a");
  state = open(dashboardReducer(state, { type: "newTab", id: "t2" }), "b");
  return open(dashboardReducer(state, { type: "newWorkspace", id: "w2", tabId: "t3" }), "c");
};

test("a dashboard starts with one workspace holding one empty tab", () => {
  const state = start();
  assert.deepEqual(workspaceIds(state), ["w1"]);
  assert.equal(activeWorkspace(state).id, "w1");
  assert.deepEqual(tabIds(state), ["t1"]);
  assert.equal(activeTab(state).id, "t1");
});

test("a new workspace shows only its own tab; the other workspace keeps its tabs and sessions", () => {
  const state = twoWorkspaces();
  assert.equal(activeWorkspace(state).id, "w2");
  assert.deepEqual(tabIds(state), ["t3"]);
  assert.deepEqual(tabSessions(state, "t3").map((s) => s.id), ["c"], "new sessions go to the workspace in view");
  assert.deepEqual(tabsOf(state, "w1"), ["t1", "t2"]);
  assert.deepEqual(sessionIds(state), ["a", "b", "c"], "sessions of hidden workspaces keep running");
  assert.equal(dashboardReducer(state, { type: "newWorkspace", id: "w2", tabId: "t9" }), state, "ids are unique");
});

test("new tabs join the workspace in view", () => {
  const state = dashboardReducer(twoWorkspaces(), { type: "newTab", id: "t4" });
  assert.deepEqual(tabIds(state), ["t3", "t4"]);
  assert.deepEqual(tabsOf(state, "w1"), ["t1", "t2"]);
});

test("switching back shows the workspace's last tab and its focus", () => {
  let state = open(start(), "a");
  state = open(state, "a2");
  state = run(state, { type: "focus", id: "a" }, { type: "newTab", id: "t2" }, { type: "selectTab", id: "t1" });
  state = dashboardReducer(state, { type: "newWorkspace", id: "w2", tabId: "t3" });
  state = dashboardReducer(state, { type: "selectWorkspace", id: "w1" });
  assert.equal(activeWorkspace(state).id, "w1");
  assert.equal(activeTab(state).id, "t1");
  assert.equal(activeTab(state).focusedId, "a");
  assert.equal(dashboardReducer(state, { type: "selectWorkspace", id: "missing" }), state, "unknown ids are ignored");
});

test("focusing a session in another workspace switches to that workspace", () => {
  const state = dashboardReducer(twoWorkspaces(), { type: "focus", id: "b" });
  assert.equal(activeWorkspace(state).id, "w1");
  assert.equal(activeTab(state).id, "t2");
});

test("tab numbers and tab steps count only the workspace in view", () => {
  const state = dashboardReducer(twoWorkspaces(), { type: "newTab", id: "t4" });
  assert.equal(tabByNumber(state, 1)?.id, "t3");
  assert.equal(tabByNumber(state, 9)?.id, "t4", "9 is the workspace's last tab");
  assert.equal(tabByNumber(state, 3), null);
  assert.equal(stepTab(state, 1).id, "t3", "wraps within the workspace");
});

test("closing a tab activates a neighbor in the same workspace", () => {
  // Overall order t1, t2, t3, t4: w1 holds t1 and t4, w2 holds t2 and t3.
  let state = run(start(), { type: "newWorkspace", id: "w2", tabId: "t2" }, { type: "newTab", id: "t3" });
  state = run(state, { type: "selectWorkspace", id: "w1" }, { type: "newTab", id: "t4" }, { type: "selectTab", id: "t1" });
  state = dashboardReducer(state, { type: "closeTab", id: "t1", fallbackId: "unused" });
  assert.deepEqual(tabIds(state), ["t4"]);
  assert.equal(activeTab(state).id, "t4", "the next tab in this workspace, not t2 from w2");
});

test("closing a workspace's last tab leaves an empty tab in that workspace", () => {
  const state = dashboardReducer(twoWorkspaces(), { type: "closeTab", id: "t3", fallbackId: "t9" });
  assert.equal(activeWorkspace(state).id, "w2");
  assert.deepEqual(tabIds(state), ["t9"]);
  assert.equal(activeTab(state).id, "t9");
  assert.deepEqual(tabsOf(state, "w1"), ["t1", "t2"]);
  assert.deepEqual(sessionIds(state), ["a", "b"]);
});

test("tabs reorder within their workspace", () => {
  let state = run(twoWorkspaces(), { type: "newTab", id: "t4" }, { type: "newTab", id: "t5" });
  state = dashboardReducer(state, { type: "moveTab", id: "t5", toIndex: 0 });
  assert.deepEqual(tabIds(state), ["t5", "t3", "t4"]);
  state = dashboardReducer(state, { type: "moveTab", id: "t5", toIndex: 99 });
  assert.deepEqual(tabIds(state), ["t3", "t4", "t5"], "indexes are clamped to the workspace");
  assert.deepEqual(tabsOf(state, "w1"), ["t1", "t2"]);
  assert.equal(dashboardReducer(state, { type: "moveTab", id: "t3", toIndex: 0 }), state, "no-op keeps state");
});

test("a workspace is numbered until it is named", () => {
  let state = twoWorkspaces();
  const label = (id: string) => workspaceLabel(state, state.workspaces.find((w) => w.id === id)!, untitled);
  assert.equal(label("w1"), "Workspace 1");
  assert.equal(label("w2"), "Workspace 2");
  state = dashboardReducer(state, { type: "renameWorkspace", id: "w1", name: "  westerm\n 앱 " });
  assert.equal(label("w1"), "westerm 앱", "whitespace is collapsed");
  state = dashboardReducer(state, { type: "renameWorkspace", id: "w1", name: " " });
  assert.equal(label("w1"), "Workspace 1");
  assert.equal(dashboardReducer(state, { type: "renameWorkspace", id: "nope", name: "x" }), state);
});

test("closing a workspace ends its sessions and shows a neighbor", () => {
  // w1: a · w2: c · w3: d (in view)
  let state = run(
    twoWorkspaces(),
    { type: "closeTab", id: "t2", fallbackId: "unused" },
    { type: "newWorkspace", id: "w3", tabId: "t4" },
  );
  state = open(state, "d");
  state = dashboardReducer(state, { type: "selectWorkspace", id: "w2" });
  state = dashboardReducer(state, { type: "closeWorkspace", id: "w2", fallbackId: "unused", fallbackTabId: "unused" });
  assert.deepEqual(workspaceIds(state), ["w1", "w3"]);
  assert.equal(activeWorkspace(state).id, "w3", "the workspace below takes over");
  assert.deepEqual(sessionIds(state), ["a", "d"]);
  assert.deepEqual(state.tabs.map((tab) => tab.id), ["t1", "t4"]);
  state = dashboardReducer(state, { type: "closeWorkspace", id: "w1", fallbackId: "unused", fallbackTabId: "unused" });
  assert.equal(activeWorkspace(state).id, "w3", "closing a hidden workspace keeps the one in view");
  state = dashboardReducer(state, { type: "closeWorkspace", id: "w3", fallbackId: "w9", fallbackTabId: "t9" });
  assert.deepEqual(workspaceIds(state), ["w9"], "the last workspace is replaced by an empty one");
  assert.deepEqual(tabIds(state), ["t9"]);
  assert.deepEqual(sessionIds(state), []);
});

test("workspaces can be reordered", () => {
  let state = dashboardReducer(twoWorkspaces(), { type: "newWorkspace", id: "w3", tabId: "t4" });
  state = dashboardReducer(state, { type: "moveWorkspace", id: "w3", toIndex: 0 });
  assert.deepEqual(workspaceIds(state), ["w3", "w1", "w2"]);
  state = dashboardReducer(state, { type: "moveWorkspace", id: "w3", toIndex: 99 });
  assert.deepEqual(workspaceIds(state), ["w1", "w2", "w3"]);
  assert.equal(dashboardReducer(state, { type: "moveWorkspace", id: "w1", toIndex: 0 }), state, "no-op keeps state");
  assert.equal(activeWorkspace(state).id, "w3", "moving does not change the workspace in view");
});

test("a tab can move to another workspace", () => {
  let state = twoWorkspaces();
  state = dashboardReducer(state, { type: "moveTabToWorkspace", id: "t3", workspaceId: "w1", fallbackId: "t9" });
  assert.deepEqual(tabsOf(state, "w1"), ["t1", "t2", "t3"], "it joins the end");
  assert.deepEqual(tabsOf(state, "w2"), ["t9"], "a workspace left without tabs gets an empty one");
  assert.equal(activeWorkspace(state).id, "w1", "the tab stays in view");
  assert.equal(activeTab(state).id, "t3");
  state = dashboardReducer(state, { type: "moveTabToWorkspace", id: "t1", workspaceId: "w2", fallbackId: "unused" });
  assert.deepEqual(tabsOf(state, "w1"), ["t2", "t3"]);
  assert.deepEqual(tabsOf(state, "w2"), ["t9", "t1"]);
  assert.equal(activeTab(state).id, "t1");
  state = dashboardReducer(state, { type: "selectWorkspace", id: "w1" });
  assert.equal(activeTab(state).id, "t3", "the source workspace keeps a tab of its own in view");
  const same = { type: "moveTabToWorkspace", id: "t2", workspaceId: "w1", fallbackId: "unused" } as const;
  assert.equal(dashboardReducer(state, same), state, "the same workspace is a no-op");
});

test("a pane moved into another workspace's tab brings that workspace into view", () => {
  const state = dashboardReducer(twoWorkspaces(), { type: "movePane", id: "c", toTabId: "t1" });
  assert.equal(activeWorkspace(state).id, "w1");
  assert.deepEqual(tabSessions(state, "t1").map((s) => s.id), ["a", "c"]);
});

test("a saved dashboard round-trips its workspaces", () => {
  // Saved ids need two characters or more.
  let state = open(open(start(), "sa"), "sb");
  state = run(state, { type: "newTab", id: "t2" }, { type: "newWorkspace", id: "w2", tabId: "t3" });
  state = open(state, "sc");
  state = run(state, { type: "renameWorkspace", id: "w2", name: "서버" }, { type: "selectTab", id: "t2" });
  const saved = toSaved(state);
  assert.equal(saved.activeTabId, "t2", "files keep the tab in view for older versions");
  const parsed = parseSavedDashboard(JSON.parse(JSON.stringify(saved)));
  assert.deepEqual(parsed, saved);
  const restored = dashboardReducer(createDashboard("wz", "tz"), { type: "hydrate", saved: parsed! });
  assert.deepEqual(toSaved(restored), saved);
  assert.equal(activeWorkspace(restored).id, "w1");
  assert.equal(activeTab(restored).id, "t2");
  assert.equal(workspaceLabel(restored, restored.workspaces[1], untitled), "서버");
});

test("taking the sessions back brings the other window's workspaces", () => {
  // The other window named w1 and showed it, then saved its layout.
  const saved = toSaved(run(twoWorkspaces(), { type: "renameWorkspace", id: "w1", name: "앱" }, { type: "selectTab", id: "t1" }));
  let state = dashboardReducer(twoWorkspaces(), { type: "status", id: "a", status: "error", notice: "takenOver" });
  state = dashboardReducer(state, { type: "reclaim", saved });
  assert.equal(activeWorkspace(state).id, "w1");
  assert.equal(activeTab(state).id, "t1");
  assert.equal(workspaceLabel(state, activeWorkspace(state), untitled), "앱");
  assert.deepEqual(tabsOf(state, "w2"), ["t3"]);
});

const legacyFile = {
  version: 1,
  activeTabId: "t2",
  tabs: [
    { id: "t1", name: null, paneIds: ["sa"], focusedId: "sa", maximizedId: null },
    { id: "t2", name: "logs", paneIds: ["sb"], focusedId: "sb", maximizedId: null },
  ],
  sessions: [
    { id: "sa", kind: "shell", title: "Shell 1", name: null, url: null },
    { id: "sb", kind: "shell", title: "Shell 2", name: null, url: null },
  ],
};

test("a file from before workspaces opens as one workspace with all its tabs", () => {
  const saved = parseSavedDashboard(legacyFile);
  assert.ok(saved);
  const state = dashboardReducer(start(), { type: "hydrate", saved });
  assert.equal(state.workspaces.length, 1);
  assert.deepEqual(tabIds(state), ["t1", "t2"]);
  assert.equal(activeTab(state).id, "t2");
});

test("broken workspace data falls back to one workspace instead of losing the tabs", () => {
  const broken = [
    { ...legacyFile, workspaces: "nope", activeWorkspaceId: "wa" },
    { ...legacyFile, workspaces: [{ id: "wa", name: null, activeTabId: "t1" }], activeWorkspaceId: "wa" },
    {
      ...legacyFile,
      workspaces: [{ id: "wa", name: null, activeTabId: "t2" }],
      activeWorkspaceId: "wa",
      tabs: legacyFile.tabs.map((tab) => ({ ...tab, workspaceId: "wb" })),
    },
    {
      ...legacyFile,
      workspaces: [
        { id: "wa", name: null, activeTabId: "t1" },
        { id: "wb", name: null, activeTabId: "t1" },
      ],
      activeWorkspaceId: "wa",
      tabs: legacyFile.tabs.map((tab) => ({ ...tab, workspaceId: "wa" })),
    },
  ];
  for (const file of broken) {
    const saved = parseSavedDashboard(file);
    assert.ok(saved, JSON.stringify(file.workspaces));
    assert.equal(saved.workspaces.length, 1);
    assert.deepEqual(saved.tabs.map((tab) => tab.workspaceId), [saved.workspaces[0].id, saved.workspaces[0].id]);
    assert.equal(saved.workspaces[0].activeTabId, "t2");
  }
});
