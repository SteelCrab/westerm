import assert from "node:assert/strict";
import { test } from "node:test";
import { leaves, rects } from "../shared/layout.ts";
import {
  activeTab,
  createDashboard,
  focusedShellId,
  MAX_RECONNECTS,
  paneToward,
  stepPane,
  stepTab,
  tabByNumber,
  toSaved,
  sessionLabel,
  tabLabel,
  tabLead,
  tabSessions,
  takenOver,
  dashboardReducer,
  type Dashboard,
} from "../src/workspace.ts";

const start = () => createDashboard("w1", "t1");
/** The UI's word for an empty tab; the reducer stays language-free. */
const UNTITLED = "New tab";
const open = (state: Dashboard, id: string, kind: "shell" | "browser" | "editor" = "shell") =>
  dashboardReducer(state, { type: "open", id, kind });
const focused = (state: Dashboard) => activeTab(state).focusedId;
const maximized = (state: Dashboard) => activeTab(state).maximizedId;

test("opening a session focuses it and names it by kind", () => {
  let state = open(start(), "a");
  state = open(state, "b", "browser");
  state = open(state, "c");
  assert.equal(focused(state), "c");
  assert.deepEqual(state.sessions.map((s) => s.title), ["Shell 1", "Browser 1", "Shell 2"]);
  assert.equal(state.sessions[1].status, "connecting");
});

test("closing the focused session focuses its neighbor", () => {
  let state = open(open(open(start(), "a"), "b"), "c");
  state = dashboardReducer(state, { type: "focus", id: "b" });
  state = dashboardReducer(state, { type: "close", id: "b" });
  assert.equal(focused(state), "c");
  state = dashboardReducer(state, { type: "close", id: "c" });
  assert.equal(focused(state), "a");
  state = dashboardReducer(state, { type: "close", id: "a" });
  assert.equal(focused(state), null);
});

test("maximize toggles and follows focus; opening a session leaves maximize", () => {
  let state = open(open(start(), "a"), "b");
  state = dashboardReducer(state, { type: "toggleMaximize", id: "a" });
  assert.equal(maximized(state), "a");
  state = dashboardReducer(state, { type: "focus", id: "b" });
  assert.equal(maximized(state), "b");
  state = dashboardReducer(state, { type: "toggleMaximize", id: "b" });
  assert.equal(maximized(state), null);
  state = dashboardReducer(state, { type: "toggleMaximize", id: "a" });
  state = open(state, "c");
  assert.equal(maximized(state), null);
});

test("restart bumps the generation and resets status", () => {
  let state = open(start(), "a");
  state = dashboardReducer(state, { type: "status", id: "a", status: "exited", detail: "exit 0" });
  state = dashboardReducer(state, { type: "restart", id: "a" });
  assert.equal(state.sessions[0].generation, 1);
  assert.equal(state.sessions[0].status, "connecting");
  assert.equal(state.sessions[0].detail, null);
});

test("blank titles are ignored", () => {
  let state = open(start(), "a");
  state = dashboardReducer(state, { type: "title", id: "a", title: "  " });
  assert.equal(state.sessions[0].title, "Shell 1");
  state = dashboardReducer(state, { type: "title", id: "a", title: "vim main.rs" });
  assert.equal(state.sessions[0].title, "vim main.rs");
});

test("an editor remembers the focused running shell as its folder source", () => {
  let state = open(start(), "a");
  assert.equal(focusedShellId(state), null, "not before the shell is running");
  state = dashboardReducer(state, { type: "ready", id: "a", pid: 4242, mode: "new" });
  assert.equal(state.sessions[0].status, "running");
  assert.equal(focusedShellId(state), "a");
  state = dashboardReducer(state, { type: "open", id: "e", kind: "editor", cwdFrom: focusedShellId(state) });
  assert.equal(state.sessions[1].cwdFrom, "a");
  assert.equal(state.sessions[1].title, "Code 1");
  // A focused editor or browser is not a folder source.
  assert.equal(focusedShellId(state), null);
  state = dashboardReducer(state, { type: "focus", id: "a" });
  state = dashboardReducer(state, { type: "status", id: "a", status: "exited", detail: "exit 0" });
  assert.equal(focusedShellId(state), null, "an exited shell is not a folder source");
  state = dashboardReducer(state, { type: "restart", id: "a" });
  assert.equal(state.sessions[0].pid, null);
});

test("new sessions go to the active tab; each tab keeps its own focus", () => {
  let state = open(start(), "a");
  state = dashboardReducer(state, { type: "newTab", id: "t2" });
  assert.equal(activeTab(state).id, "t2");
  assert.equal(focused(state), null, "a new tab starts empty");
  state = open(state, "b");
  assert.deepEqual(tabSessions(state, "t1").map((s) => s.id), ["a"]);
  assert.deepEqual(tabSessions(state, "t2").map((s) => s.id), ["b"]);
  state = dashboardReducer(state, { type: "selectTab", id: "t1" });
  assert.equal(focused(state), "a");
  state = dashboardReducer(state, { type: "selectTab", id: "missing" });
  assert.equal(activeTab(state).id, "t1", "unknown tabs are ignored");
});

test("focusing a session in another tab switches to that tab", () => {
  let state = open(start(), "a");
  state = open(dashboardReducer(state, { type: "newTab", id: "t2" }), "b");
  state = dashboardReducer(state, { type: "focus", id: "a" });
  assert.equal(activeTab(state).id, "t1");
  assert.equal(focused(state), "a");
  // Maximize is per tab: maximizing in t1 does not touch t2.
  state = dashboardReducer(state, { type: "toggleMaximize", id: "a" });
  assert.equal(state.tabs.find((tab) => tab.id === "t2")?.maximizedId, null);
});

test("closing a tab closes its sessions and activates a neighbor", () => {
  let state = open(start(), "a");
  state = open(dashboardReducer(state, { type: "newTab", id: "t2" }), "b");
  state = open(dashboardReducer(state, { type: "newTab", id: "t3" }), "c");
  state = dashboardReducer(state, { type: "selectTab", id: "t2" });
  state = dashboardReducer(state, { type: "closeTab", id: "t2", fallbackId: "unused" });
  assert.deepEqual(state.tabs.map((tab) => tab.id), ["t1", "t3"]);
  assert.equal(activeTab(state).id, "t3", "the tab to the right takes over");
  assert.deepEqual(state.sessions.map((s) => s.id), ["a", "c"]);
  state = dashboardReducer(state, { type: "closeTab", id: "t3", fallbackId: "unused" });
  assert.equal(activeTab(state).id, "t1", "the last tab falls back to the left");
  // Closing a background tab keeps the active one.
  state = open(dashboardReducer(state, { type: "newTab", id: "t4" }), "d");
  state = dashboardReducer(state, { type: "closeTab", id: "t1", fallbackId: "unused" });
  assert.equal(activeTab(state).id, "t4");
});

test("closing the last tab leaves one empty tab", () => {
  let state = open(start(), "a");
  state = dashboardReducer(state, { type: "closeTab", id: "t1", fallbackId: "t9" });
  assert.deepEqual(state.tabs, [{ id: "t9", workspaceId: "w1", name: null, layout: null, paneIds: [], focusedId: null, maximizedId: null }]);
  assert.equal(activeTab(state).id, "t9");
  assert.equal(state.sessions.length, 0);
});

test("a tab is named after its focused pane", () => {
  let state = open(open(start(), "a"), "b", "browser");
  state = dashboardReducer(state, { type: "title", id: "b", title: "Hacker News" });
  assert.equal(tabLabel(state, activeTab(state), UNTITLED), "Hacker News");
  state = dashboardReducer(state, { type: "focus", id: "a" });
  assert.equal(tabLead(state, activeTab(state))?.title, "Shell 1");
  state = dashboardReducer(state, { type: "newTab", id: "t2" });
  assert.equal(tabLead(state, activeTab(state)), null);
});

test("renaming a tab overrides its automatic name; an empty name restores it", () => {
  let state = open(start(), "a");
  const label = () => tabLabel(state, activeTab(state), UNTITLED);
  assert.equal(label(), "Shell 1");
  state = dashboardReducer(state, { type: "renameTab", id: "t1", name: "  백엔드\n  서버  " });
  assert.equal(label(), "백엔드 서버", "whitespace is collapsed");
  state = dashboardReducer(state, { type: "title", id: "a", title: "vim" });
  assert.equal(label(), "백엔드 서버", "pane titles no longer rename the tab");
  state = dashboardReducer(state, { type: "renameTab", id: "t1", name: "x".repeat(100) });
  assert.equal(label().length, 60);
  state = dashboardReducer(state, { type: "renameTab", id: "t1", name: "   " });
  assert.equal(label(), "vim");
  state = dashboardReducer(state, { type: "newTab", id: "t2" });
  assert.equal(label(), UNTITLED);
  assert.equal(dashboardReducer(state, { type: "renameTab", id: "nope", name: "x" }), state);
});

test("renaming a session overrides the program title until cleared", () => {
  let state = open(start(), "a");
  const session = () => state.sessions[0];
  state = dashboardReducer(state, { type: "renameSession", id: "a", name: " 빌드 로그 " });
  assert.equal(sessionLabel(session()), "빌드 로그");
  state = dashboardReducer(state, { type: "title", id: "a", title: "npm run build" });
  assert.equal(sessionLabel(session()), "빌드 로그", "OSC titles do not replace a custom name");
  assert.equal(tabLabel(state, activeTab(state), UNTITLED), "빌드 로그", "the tab follows the session's label");
  state = dashboardReducer(state, { type: "renameSession", id: "a", name: "" });
  assert.equal(sessionLabel(session()), "npm run build");
});

const ids = (state: Dashboard, tabId: string) => tabSessions(state, tabId).map((s) => s.id);
const threeTabs = () => {
  let state = open(open(start(), "a"), "b");
  state = open(dashboardReducer(state, { type: "newTab", id: "t2" }), "c");
  state = dashboardReducer(state, { type: "newTab", id: "t3" });
  return state;
};

test("tabs can be reordered", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "moveTab", id: "t3", toIndex: 0 });
  assert.deepEqual(state.tabs.map((tab) => tab.id), ["t3", "t1", "t2"]);
  state = dashboardReducer(state, { type: "moveTab", id: "t3", toIndex: 99 });
  assert.deepEqual(state.tabs.map((tab) => tab.id), ["t1", "t2", "t3"], "indexes are clamped");
  assert.equal(dashboardReducer(state, { type: "moveTab", id: "t1", toIndex: 0 }), state, "no-op keeps state");
  assert.equal(activeTab(state).id, "t3", "moving does not change the active tab");
});

test("a pane can move to another tab", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "movePane", id: "b", toTabId: "t2" });
  assert.deepEqual(ids(state, "t1"), ["a"]);
  assert.deepEqual(ids(state, "t2"), ["c", "b"], "placed beside the tab's focused pane");
  assert.equal(activeTab(state).id, "t2", "the destination tab becomes active");
  assert.equal(activeTab(state).focusedId, "b");
  assert.equal(state.tabs[0].focusedId, "a", "the source tab focuses a neighbor");
  assert.deepEqual(state.sessions.map((s) => s.id), ["a", "b", "c"], "sessions keep creation order");
  // Into an empty tab.
  state = dashboardReducer(state, { type: "movePane", id: "a", toTabId: "t3" });
  assert.deepEqual(ids(state, "t1"), []);
  assert.equal(state.tabs[0].layout, null);
  assert.equal(state.tabs[0].focusedId, null);
  assert.deepEqual(ids(state, "t3"), ["a"]);
  assert.equal(dashboardReducer(state, { type: "movePane", id: "a", toTabId: "t3" }), state, "same tab is a no-op");
});

test("docking a pane on a side of another splits it, also across tabs", () => {
  let state = threeTabs(); // t1: a | b, t2: c
  state = dashboardReducer(state, { type: "dockPane", id: "c", targetId: "a", side: "bottom" });
  assert.deepEqual(ids(state, "t1"), ["a", "c", "b"]);
  assert.deepEqual(ids(state, "t2"), []);
  assert.equal(activeTab(state).id, "t1");
  assert.equal(activeTab(state).focusedId, "c");
  const r = rects(state.tabs[0].layout);
  assert.ok(Math.abs(r.get("c")!.y - 0.5) < 1e-9 && Math.abs(r.get("c")!.w - 0.5) < 1e-9, "c takes a's lower half");
  // Within a tab: move b to the left of everything's first pane.
  state = dashboardReducer(state, { type: "dockPane", id: "b", targetId: "a", side: "left" });
  assert.deepEqual(leaves(state.tabs[0].layout), ["b", "a", "c"]);
  assert.equal(dashboardReducer(state, { type: "dockPane", id: "b", targetId: "b", side: "left" }), state);
});

test("divider drags resize one split", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "setSplitSizes", tabId: "t1", path: [], sizes: [0.7, 0.3] });
  const r = rects(state.tabs[0].layout);
  assert.ok(Math.abs(r.get("a")!.w - 0.7) < 1e-9);
});

test("moving a maximized pane clears maximize in the source tab", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "toggleMaximize", id: "b" });
  state = dashboardReducer(state, { type: "movePane", id: "b", toTabId: "t3" });
  assert.equal(state.tabs[0].maximizedId, null);
  assert.equal(state.tabs[2].maximizedId, null, "not maximized in a tab that had none");
});

test("panes swap cells within a tab and across tabs", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "swapPanes", id: "a", withId: "b" });
  assert.deepEqual(ids(state, "t1"), ["b", "a"]);
  assert.equal(state.tabs[0].focusedId, "a", "the dragged pane keeps focus");
  state = dashboardReducer(state, { type: "focus", id: "c" });
  state = dashboardReducer(state, { type: "swapPanes", id: "a", withId: "c" });
  assert.deepEqual(ids(state, "t1"), ["b", "c"]);
  assert.deepEqual(ids(state, "t2"), ["a"]);
  assert.equal(state.tabs[1].focusedId, "a", "focus follows the cell");
  assert.equal(dashboardReducer(state, { type: "swapPanes", id: "a", withId: "a" }), state);
});

test("disconnects retry with a budget; a successful attach resets it", () => {
  let state = open(start(), "a");
  for (let i = 1; i <= MAX_RECONNECTS; i++) {
    state = dashboardReducer(state, { type: "disconnected", id: "a", keepTrying: false });
    assert.equal(state.sessions[0].status, "disconnected");
    state = dashboardReducer(state, { type: "restart", id: "a" });
  }
  assert.equal(state.sessions[0].notice, null, "restart clears the notice");
  state = dashboardReducer(state, { type: "disconnected", id: "a", keepTrying: false });
  assert.equal(state.sessions[0].status, "error", "gives up after the budget");
  assert.equal(state.sessions[0].notice, "unreachable");
  state = dashboardReducer(state, { type: "restart", id: "a", manual: true });
  assert.equal(state.sessions[0].retries, 0, "a manual restart starts over");
  state = dashboardReducer(state, { type: "disconnected", id: "a", keepTrying: false });
  state = dashboardReducer(state, { type: "ready", id: "a", pid: 7, mode: "reattached" });
  assert.equal(state.sessions[0].retries, 0);
  assert.equal(state.sessions[0].detail, "pid 7");
  assert.equal(state.sessions[0].notice, "reattached");
});

test("a saved workspace round-trips names, order, and focus", () => {
  let state = threeTabs();
  state = dashboardReducer(state, { type: "renameTab", id: "t2", name: "서버" });
  state = dashboardReducer(state, { type: "renameSession", id: "b", name: "로그" });
  state = dashboardReducer(state, { type: "swapPanes", id: "a", withId: "b" });
  state = dashboardReducer(state, { type: "ready", id: "a", pid: 1, mode: "new" });
  state = dashboardReducer(state, { type: "setSplitSizes", tabId: "t1", path: [], sizes: [0.65, 0.35] });
  const saved = toSaved(state);
  const restored = dashboardReducer(createDashboard("wz", "tz"), { type: "hydrate", saved });
  assert.deepEqual(toSaved(restored), saved);
  assert.equal(restored.sessions[0].status, "connecting", "runtime state is not saved");
  assert.equal(restored.sessions[0].pid, null);
  assert.equal(tabLabel(restored, restored.tabs[1], UNTITLED), "서버");
  assert.deepEqual(restored.tabs[0].layout, state.tabs[0].layout, "split sizes survive a reload");
});

test("keyboard switching counts tabs like browsers and wraps around", () => {
  let state = open(start(), "a");
  for (const id of ["t2", "t3"]) state = dashboardReducer(state, { type: "newTab", id });
  assert.equal(tabByNumber(state, 1)?.id, "t1");
  assert.equal(tabByNumber(state, 9)?.id, "t3", "9 is the last tab");
  assert.equal(tabByNumber(state, 5), null);
  assert.equal(stepTab(state, 1).id, "t1", "after the last comes the first");
  assert.equal(stepTab(state, -1).id, "t2");
});

test("pane switching follows reading order and the layout", () => {
  let state = open(open(start(), "a"), "b", "browser");
  assert.equal(activeTab(state).focusedId, "b");
  assert.equal(stepPane(state, 1), "a", "wraps around");
  assert.equal(stepPane(state, -1), "a");
  const [left, right] = activeTab(state).paneIds;
  state = dashboardReducer(state, { type: "focus", id: left });
  assert.equal(paneToward(state, "right"), right);
  assert.equal(paneToward(state, "left"), null);
  state = dashboardReducer(state, { type: "newTab", id: "t2" });
  assert.equal(stepPane(state, 1), null, "an empty tab has no panes");
});

test("taking the sessions back loads the other window's layout and connects every pane", () => {
  let state = open(open(start(), "a"), "b");
  state = dashboardReducer(state, { type: "ready", id: "a", pid: 1, mode: "new" });
  assert.equal(takenOver(state), false);
  state = dashboardReducer(state, { type: "status", id: "a", status: "error", notice: "takenOver" });
  assert.equal(takenOver(state), true, "one pane told is the whole window");
  const generation = state.sessions[0].generation;

  // Meanwhile the other window closed "b" and saved its layout.
  const saved = toSaved(dashboardReducer(state, { type: "close", id: "b" }));
  state = dashboardReducer(state, { type: "reclaim", saved });
  assert.equal(takenOver(state), false);
  assert.deepEqual(state.sessions.map((session) => session.id), ["a"]);
  assert.equal(state.sessions[0].status, "connecting");
  assert.equal(state.sessions[0].generation, generation + 1, "a new terminal connects again");
  assert.equal(dashboardReducer(state, { type: "reclaim", saved }), state, "only a window that was taken over");
});

test("taking the sessions back without a saved layout keeps this one", () => {
  let state = open(open(start(), "a"), "b");
  state = dashboardReducer(state, { type: "status", id: "b", status: "error", notice: "takenOver" });
  state = dashboardReducer(state, { type: "reclaim", saved: null });
  assert.deepEqual(state.sessions.map((session) => [session.id, session.status, session.generation]), [
    ["a", "connecting", 1],
    ["b", "connecting", 1],
  ]);
});
