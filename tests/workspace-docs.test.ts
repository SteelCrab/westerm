import assert from "node:assert/strict";
import { test } from "node:test";
import { fromGrid, leaves, type LayoutNode } from "../shared/layout.ts";
import { parseSavedDashboard, type SavedDocs } from "../shared/workspace-file.ts";

const split = (a: string, b: string): LayoutNode => ({
  type: "split",
  dir: "row",
  sizes: [0.5, 0.5],
  children: [
    { type: "pane", id: a },
    { type: "pane", id: b },
  ],
});

const docsPane: SavedDocs = {
  id: "d1",
  title: "westerm",
  name: null,
  root: "/Users/me/westerm",
  followFrom: "s1",
  file: "docs/a.md",
  mode: "split",
};

/** One workspace, one tab with shell s1 and docs pane d1 side by side. */
const file = (docs: unknown, tab: Record<string, unknown> = {}) => ({
  version: 1,
  activeTabId: "t1",
  activeWorkspaceId: "w1",
  workspaces: [{ id: "w1", name: null, activeTabId: "t1", icon: "layers", color: "blue" }],
  sessions: [{ id: "s1", kind: "shell", title: "Shell 1", name: null, url: null }],
  tabs: [{ id: "t1", workspaceId: "w1", name: null, paneIds: ["s1"], docIds: ["d1"], layout: split("s1", "d1"), focusedId: "d1", maximizedId: null, ...tab }],
  docs,
});

test("a file from before docs panes loads with none", () => {
  const saved = parseSavedDashboard({ ...file(undefined), tabs: [{ id: "t1", workspaceId: "w1", name: null, paneIds: ["s1"], focusedId: null, maximizedId: null }] });
  assert.ok(saved);
  assert.deepEqual(saved.docs, []);
  assert.deepEqual(saved.tabs[0].docIds, []);
  assert.deepEqual(saved.tabs[0].layout, fromGrid(["s1"]));
});

test("a docs pane keeps its state and its place in the split", () => {
  const saved = parseSavedDashboard(file([docsPane]));
  assert.ok(saved);
  assert.deepEqual(saved.docs, [docsPane]);
  assert.deepEqual(saved.tabs[0].paneIds, ["s1"]);
  assert.deepEqual(saved.tabs[0].docIds, ["d1"]);
  assert.deepEqual(saved.tabs[0].layout, split("s1", "d1"));
  assert.equal(saved.tabs[0].focusedId, "d1");
});

test("broken docs data drops the docs panes, not the file", () => {
  for (const docs of [[{ ...docsPane, mode: "bogus" }], [{ ...docsPane, id: "s1" }], "nope", [docsPane, docsPane]]) {
    const saved = parseSavedDashboard(file(docs));
    assert.ok(saved, JSON.stringify(docs));
    assert.deepEqual(saved.docs, []);
    assert.deepEqual(saved.tabs[0].docIds, []);
    assert.deepEqual(leaves(saved.tabs[0].layout ?? null), ["s1"]);
    assert.equal(saved.tabs[0].focusedId, null);
  }
});

test("docs fields that point nowhere are cleared", () => {
  const saved = parseSavedDashboard(file([{ ...docsPane, followFrom: "gone", file: "../x.md" }]));
  assert.equal(saved?.docs?.[0].followFrom, null);
  assert.equal(saved?.docs?.[0].file, null);
  for (const file_ of ["/etc/x.md", "a\\..\\..\\x.md", ""]) {
    assert.equal(parseSavedDashboard(file([{ ...docsPane, file: file_ }]))?.docs?.[0].file, null, file_);
  }
});

test("a docs root must be an absolute path or none", () => {
  assert.equal(parseSavedDashboard(file([{ ...docsPane, root: null }]))?.docs?.[0].root, null);
  assert.equal(parseSavedDashboard(file([{ ...docsPane, root: "C:\\Users\\me" }]))?.docs?.[0].root, "C:\\Users\\me");
  assert.deepEqual(parseSavedDashboard(file([{ ...docsPane, root: "relative" }]))?.docs, []);
});

test("a docs pane in no tab is dropped", () => {
  const saved = parseSavedDashboard(file([docsPane], { docIds: [], layout: null }));
  assert.deepEqual(saved?.docs, []);
});

test("an unknown docs id in a tab drops the tab's docs, not the file", () => {
  const saved = parseSavedDashboard(file([docsPane], { docIds: ["d1", "d9"] }));
  assert.ok(saved);
  assert.deepEqual(saved.tabs[0].docIds, []);
  assert.deepEqual(saved.docs, []);
});

// ---- Dashboard state (src/workspace.ts) ----

import { firstLoad } from "../src/useWorkspacePersistence.ts";
import { createDashboard, dashboardReducer, toSaved, type Dashboard, type DashboardAction } from "../src/workspace.ts";

const run = (state: Dashboard, ...actions: DashboardAction[]) => actions.reduce(dashboardReducer, state);
const docsStart = { root: null, followFrom: "s1", file: null, mode: "view" } as const;

/** A shell s1 and a docs pane d1 beside it. */
const withDocs = () =>
  run(
    createDashboard("w1", "t1"),
    { type: "open", id: "s1", kind: "shell" },
    { type: "open", id: "d1", kind: "docs", docs: docsStart },
  );

test("a new docs pane runs at once and is named Docs 1", () => {
  const docs = withDocs().sessions.find((session) => session.id === "d1");
  assert.equal(docs?.kind, "docs");
  assert.equal(docs?.status, "running");
  assert.equal(docs?.title, "Docs 1");
  assert.deepEqual(docs?.docs, docsStart);
});

test("docsUpdate changes only that docs pane", () => {
  const state = run(withDocs(), { type: "docsUpdate", id: "d1", change: { root: "/r", file: "a.md" } });
  assert.deepEqual(state.sessions.find((session) => session.id === "d1")?.docs, { ...docsStart, root: "/r", file: "a.md" });
  assert.equal(state.sessions.find((session) => session.id === "s1")?.docs, null);
  assert.equal(run(state, { type: "docsUpdate", id: "s1", change: { root: "/x" } }), state, "not a docs pane");
});

test("the saved file keeps docs panes out of sessions and paneIds", () => {
  const saved = toSaved(run(withDocs(), { type: "docsUpdate", id: "d1", change: { root: "/r" } }));
  assert.deepEqual(saved.sessions.map((session) => session.id), ["s1"]);
  assert.deepEqual(saved.tabs[0].paneIds, ["s1"], "an older version checks paneIds against sessions");
  assert.deepEqual(saved.tabs[0].docIds, ["d1"]);
  assert.deepEqual(leaves(saved.tabs[0].layout ?? null), ["s1", "d1"]);
  assert.deepEqual(saved.docs, [{ id: "d1", title: "Docs 1", name: null, root: "/r", followFrom: "s1", file: null, mode: "view" }]);
});

test("a docs pane survives a save and a load", () => {
  const state = run(withDocs(), { type: "docsUpdate", id: "d1", change: { root: "/r", file: "docs/a.md", mode: "split" } });
  const saved = parseSavedDashboard(JSON.parse(JSON.stringify(toSaved(state))));
  assert.ok(saved);
  const loaded = run(createDashboard("w9", "t9"), { type: "hydrate", saved });
  const docs = loaded.sessions.find((session) => session.id === "d1");
  assert.equal(docs?.status, "running");
  assert.deepEqual(docs?.docs, { root: "/r", followFrom: "s1", file: "docs/a.md", mode: "split" });
  assert.deepEqual(loaded.tabs[0].paneIds, ["s1", "d1"]);
});

test("reconnecting panes leaves docs panes running", () => {
  const state = withDocs();
  const taken = run(state, { type: "status", id: "s1", status: "error", notice: "takenOver" });
  const back = run(taken, { type: "reclaim", saved: null });
  assert.equal(back.sessions.find((session) => session.id === "d1")?.status, "running");
  assert.equal(back.sessions.find((session) => session.id === "s1")?.status, "connecting");
  assert.equal(run(state, { type: "restart", id: "d1" }).sessions.find((session) => session.id === "d1")?.status, "running");
});

test("a layout with only docs panes is loaded, not replaced", () => {
  const only = run(createDashboard("w1", "t1"), { type: "open", id: "d1", kind: "docs", docs: { ...docsStart, followFrom: null } });
  assert.equal(firstLoad(false, { ok: true, saved: parseSavedDashboard(toSaved(only)) }), "hydrate");
});

// ---- A new docs pane's first state ----

import { newDocsPane } from "../src/workspace.ts";

test("a new docs pane follows the focused shell", () => {
  const state = run(createDashboard("w1", "t1"), { type: "open", id: "s1", kind: "shell" }, { type: "ready", id: "s1", pid: 7, mode: "new" });
  assert.deepEqual(newDocsPane(state), { root: null, followFrom: "s1", file: null, mode: "view" });
});

test("a new docs pane beside a docs pane opens that pane's folder", () => {
  const state = run(withDocs(), { type: "docsUpdate", id: "d1", change: { root: "/r", file: "a.md", mode: "split" } });
  assert.deepEqual(newDocsPane(state), { root: "/r", followFrom: "s1", file: null, mode: "view" });
});

test("a new docs pane with nothing to go by asks for the home folder", () => {
  assert.deepEqual(newDocsPane(createDashboard("w1", "t1")), { root: null, followFrom: null, file: null, mode: "view" });
});
