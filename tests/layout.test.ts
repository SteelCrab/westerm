import assert from "node:assert/strict";
import { test } from "node:test";
import {
  dockPane,
  fromGrid,
  handles,
  insertPane,
  leaves,
  moveShare,
  normalize,
  pane,
  parseLayout,
  rects,
  removePane,
  setSizesAt,
  swapPanes,
  type LayoutNode,
} from "../shared/layout.ts";

const isId = (value: unknown): value is string => typeof value === "string" && /^[a-z]\w*$/.test(value);
const close = (a: number, b: number) => Math.abs(a - b) < 1e-9;
const shape = (node: LayoutNode | null): unknown =>
  !node ? null : node.type === "pane" ? node.id : { [node.dir]: node.children.map(shape) };

test("docking on a side splits the target pane that way", () => {
  let root: LayoutNode | null = pane("a");
  root = dockPane(root, "b", "a", "right");
  assert.deepEqual(shape(root), { row: ["a", "b"] });
  root = dockPane(root, "c", "b", "bottom");
  assert.deepEqual(shape(root), { row: ["a", { column: ["b", "c"] }] });
  root = dockPane(root, "d", "a", "left");
  // Same direction as the parent: merged into it, not nested.
  assert.deepEqual(shape(root), { row: ["d", "a", { column: ["b", "c"] }] });
  const r = rects(root);
  assert.ok(close(r.get("d")!.w, 0.25) && close(r.get("a")!.w, 0.25), "the target's half goes to the new pane");
  assert.ok(close(r.get("c")!.y, 0.5) && close(r.get("c")!.x, 0.5));
});

test("removing a pane collapses splits and gives its space to the siblings", () => {
  let root: LayoutNode | null = dockPane(dockPane(pane("a"), "b", "a", "right"), "c", "b", "bottom");
  root = removePane(root, "b");
  assert.deepEqual(shape(root), { row: ["a", "c"] });
  root = removePane(root, "a");
  assert.deepEqual(shape(root), "c");
  assert.equal(removePane(root, "c"), null);
});

test("new panes split the longer side of the focused pane", () => {
  let root = insertPane(null, "a", null, 16 / 9);
  root = insertPane(root, "b", "a", 16 / 9); // wide layer → side by side
  assert.deepEqual(shape(root), { row: ["a", "b"] });
  root = insertPane(root, "c", "b", 16 / 9); // b is now tall-ish (0.5 × 16/9 < 1) → stacked
  assert.deepEqual(shape(root), { row: ["a", { column: ["b", "c"] }] });
  root = insertPane(root, "d", "missing", 16 / 9); // unknown → beside the last pane
  assert.deepEqual(leaves(root), ["a", "b", "c", "d"]);
});

test("swapping exchanges cells and keeps the shape", () => {
  const root = dockPane(dockPane(pane("a"), "b", "a", "right"), "c", "b", "bottom");
  assert.deepEqual(shape(swapPanes(root, "a", "c")), { row: ["c", { column: ["b", "a"] }] });
});

test("handles sit on boundaries and resizing keeps a minimum share", () => {
  const root = dockPane(dockPane(pane("a"), "b", "a", "right"), "c", "b", "bottom");
  const found = handles(root).map((h) => [h.path.join("."), h.index, h.dir, h.at]);
  assert.deepEqual(found, [
    ["", 0, "row", 0.5],
    ["1", 0, "column", 0.5],
  ]);
  const moved = setSizesAt(root, [], moveShare([0.5, 0.5], 0, 0.3));
  assert.ok(close(rects(moved).get("a")!.w, 0.8));
  assert.ok(close(moveShare([0.5, 0.5], 0, 0.9)[1], 0.1), "a neighbor keeps 10%");
  const inner = setSizesAt(root, [1], [0.25, 0.75]);
  assert.ok(close(rects(inner).get("c")!.h, 0.75));
});

test("old grid layouts migrate to the same arrangement", () => {
  assert.deepEqual(shape(fromGrid(["a"])), "a");
  assert.deepEqual(shape(fromGrid(["a", "b"])), { row: ["a", "b"] });
  assert.deepEqual(shape(fromGrid(["a", "b", "c"])), { column: [{ row: ["a", "b"] }, "c"] });
  assert.equal(fromGrid([]), null);
});

test("saved trees are validated", () => {
  const good = { type: "split", dir: "row", sizes: [2, 2], children: [{ type: "pane", id: "a" }, { type: "pane", id: "b" }] };
  assert.deepEqual(shape(parseLayout(good, isId)), { row: ["a", "b"] });
  assert.ok(close((parseLayout(good, isId) as { sizes: number[] }).sizes[0], 0.5), "sizes are rescaled");
  assert.equal(parseLayout({ ...good, dir: "diagonal" }, isId), null);
  assert.equal(parseLayout({ ...good, sizes: [1] }, isId), null);
  assert.equal(parseLayout({ type: "pane", id: "../x" }, isId), null);
  let deep: unknown = { type: "pane", id: "a" };
  for (let i = 0; i < 20; i++) deep = { type: "split", dir: "row", sizes: [1], children: [deep] };
  assert.equal(parseLayout(deep, isId), null, "depth is bounded");
  assert.deepEqual(shape(normalize({ type: "split", dir: "row", sizes: [1], children: [pane("z")] })), "z");
});
