import assert from "node:assert/strict";
import { test } from "node:test";
import { gridLayout } from "../shared/grid.ts";
import { leaves } from "../shared/layout.ts";
import { parseSavedDashboard } from "../shared/workspace-file.ts";

test("the legacy grid stays near square", () => {
  assert.deepEqual(gridLayout(1), { columns: 1, rows: 1, lastSpan: 1 });
  assert.deepEqual(gridLayout(3), { columns: 2, rows: 2, lastSpan: 2 });
  assert.deepEqual(gridLayout(5), { columns: 3, rows: 2, lastSpan: 2 });
});

const base = {
  version: 1,
  activeTabId: "t1",
  sessions: [
    { id: "sa", kind: "shell", title: "Shell 1", name: null, url: null },
    { id: "sb", kind: "shell", title: "Shell 2", name: null, url: null },
  ],
};

test("saved layouts from before split trees migrate to a grid tree", () => {
  const saved = parseSavedDashboard({
    ...base,
    tabs: [{ id: "t1", name: null, paneIds: ["sa", "sb"], focusedId: "sa", maximizedId: null, sizes: { columns: [0.5, 0.5], rows: [1] } }],
  });
  assert.equal(saved?.tabs[0].layout?.type, "split");
  assert.deepEqual(leaves(saved!.tabs[0].layout!), ["sa", "sb"]);
});

test("a saved tree must hold exactly the tab's panes", () => {
  const tree = { type: "split", dir: "column", sizes: [0.3, 0.7], children: [{ type: "pane", id: "sb" }, { type: "pane", id: "sa" }] };
  const kept = parseSavedDashboard({ ...base, tabs: [{ id: "t1", name: null, paneIds: ["sa", "sb"], layout: tree, focusedId: null, maximizedId: null }] });
  assert.deepEqual(kept?.tabs[0].paneIds, ["sb", "sa"], "tree order wins");
  const missing = { type: "pane", id: "sa" };
  const fixed = parseSavedDashboard({ ...base, tabs: [{ id: "t1", name: null, paneIds: ["sa", "sb"], layout: missing, focusedId: null, maximizedId: null }] });
  assert.deepEqual(fixed?.tabs[0].paneIds, ["sa", "sb"], "a tree missing panes falls back to grid order");
});
