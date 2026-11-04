// Split-tree pane layout, as in tiling window managers and VS Code editor
// groups. A tab's layout is a tree: leaves are panes, splits arrange their
// children side by side ("row") or stacked ("column") with size weights.
// Pure functions shared by the reducer, the renderer, and saved files.

import { gridLayout } from "./grid.ts";

export type SplitDir = "row" | "column";
export type Side = "left" | "right" | "top" | "bottom";

export type LayoutNode =
  | { type: "pane"; id: string }
  | { type: "split"; dir: SplitDir; children: LayoutNode[]; sizes: number[] };

/** Fractions of the layer: 0..1 on both axes. */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** A draggable boundary between two children of a split. */
export interface Handle {
  /** Child indexes from the root to the split. */
  path: number[];
  /** Boundary after this child. */
  index: number;
  dir: SplitDir;
  /** Where the boundary is, along the split's axis, in layer fractions. */
  at: number;
  /** The split's rectangle; the handle spans its cross axis. */
  rect: Rect;
}

/** No child gets less than this share of its split. */
export const MIN_SHARE = 0.1;
const MAX_DEPTH = 12;
const MAX_CHILDREN = 16;

export const pane = (id: string): LayoutNode => ({ type: "pane", id });

function split(dir: SplitDir, children: LayoutNode[], sizes: number[]): LayoutNode {
  return { type: "split", dir, children, sizes };
}

/** Pane ids in reading order (depth first). */
export function leaves(node: LayoutNode | null): string[] {
  if (!node) return [];
  return node.type === "pane" ? [node.id] : node.children.flatMap(leaves);
}

function scale(sizes: number[]): number[] {
  const total = sizes.reduce((sum, size) => sum + size, 0);
  return total > 0 ? sizes.map((size) => size / total) : sizes.map(() => 1 / sizes.length);
}

/**
 * Removes empty splits, lifts single children, and merges a split into its
 * parent when both run the same way, so the tree stays flat.
 */
export function normalize(node: LayoutNode | null): LayoutNode | null {
  if (!node || node.type === "pane") return node;
  const children: LayoutNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((child, index) => {
    const next = normalize(child);
    if (!next) return;
    const share = node.sizes[index] ?? 0;
    if (next.type === "split" && next.dir === node.dir) {
      next.children.forEach((grandchild, j) => {
        children.push(grandchild);
        sizes.push(share * next.sizes[j]);
      });
    } else {
      children.push(next);
      sizes.push(share);
    }
  });
  if (children.length === 0) return null;
  if (children.length === 1) return children[0];
  return split(node.dir, children, scale(sizes));
}

export function removePane(node: LayoutNode | null, id: string): LayoutNode | null {
  const strip = (current: LayoutNode): LayoutNode | null => {
    if (current.type === "pane") return current.id === id ? null : current;
    const kept = current.children.map(strip);
    return split(
      current.dir,
      kept.filter((child): child is LayoutNode => child !== null),
      current.sizes.filter((_, index) => kept[index] !== null),
    );
  };
  return node ? normalize(strip(node)) : null;
}

/**
 * Puts pane `id` on one side of pane `targetId`, splitting the target in
 * half. `id` must not be in the tree already.
 */
export function dockPane(root: LayoutNode | null, id: string, targetId: string, side: Side): LayoutNode {
  if (!root) return pane(id);
  const dir: SplitDir = side === "left" || side === "right" ? "row" : "column";
  const first = side === "left" || side === "top";
  let found = false;
  const place = (node: LayoutNode): LayoutNode => {
    if (node.type === "pane") {
      if (node.id !== targetId) return node;
      found = true;
      return split(dir, first ? [pane(id), node] : [node, pane(id)], [0.5, 0.5]);
    }
    return split(node.dir, node.children.map(place), node.sizes);
  };
  const placed = place(root);
  // Unknown target: add beside the whole layout.
  const result = found ? placed : split(dir, first ? [pane(id), root] : [root, pane(id)], [0.5, 0.5]);
  return normalize(result)!;
}

/**
 * Where a new pane goes: beside `nearId`, splitting its longer side.
 * `aspect` is the layer's width / height in pixels.
 */
export function insertPane(root: LayoutNode | null, id: string, nearId: string | null, aspect: number): LayoutNode {
  if (!root) return pane(id);
  const ids = leaves(root);
  const target = nearId && ids.includes(nearId) ? nearId : ids[ids.length - 1];
  const rect = rects(root).get(target)!;
  const wide = rect.w * (Number.isFinite(aspect) && aspect > 0 ? aspect : 1.6) >= rect.h;
  return dockPane(root, id, target, wide ? "right" : "bottom");
}

export function swapPanes(node: LayoutNode | null, a: string, b: string): LayoutNode | null {
  if (!node) return null;
  if (node.type === "pane") return node.id === a ? pane(b) : node.id === b ? pane(a) : node;
  return split(node.dir, node.children.map((child) => swapPanes(child, a, b)!), node.sizes);
}

export function rects(root: LayoutNode | null): Map<string, Rect> {
  const out = new Map<string, Rect>();
  const walk = (node: LayoutNode, rect: Rect) => {
    if (node.type === "pane") {
      out.set(node.id, rect);
      return;
    }
    let offset = 0;
    node.children.forEach((child, index) => {
      const share = node.sizes[index];
      walk(
        child,
        node.dir === "row"
          ? { x: rect.x + offset * rect.w, y: rect.y, w: share * rect.w, h: rect.h }
          : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: share * rect.h },
      );
      offset += share;
    });
  };
  if (root) walk(root, { x: 0, y: 0, w: 1, h: 1 });
  return out;
}

export function handles(root: LayoutNode | null): Handle[] {
  const out: Handle[] = [];
  const walk = (node: LayoutNode, rect: Rect, path: number[]) => {
    if (node.type === "pane") return;
    let offset = 0;
    node.children.forEach((child, index) => {
      const share = node.sizes[index];
      const childRect =
        node.dir === "row"
          ? { x: rect.x + offset * rect.w, y: rect.y, w: share * rect.w, h: rect.h }
          : { x: rect.x, y: rect.y + offset * rect.h, w: rect.w, h: share * rect.h };
      offset += share;
      if (index < node.children.length - 1) {
        out.push({
          path,
          index,
          dir: node.dir,
          at: node.dir === "row" ? rect.x + offset * rect.w : rect.y + offset * rect.h,
          rect,
        });
      }
      walk(child, childRect, [...path, index]);
    });
  };
  if (root) walk(root, { x: 0, y: 0, w: 1, h: 1 }, []);
  return out;
}

/** Moves the boundary after child `index` by `delta` (a share of the split). */
export function moveShare(sizes: number[], index: number, delta: number): number[] {
  if (index < 0 || index >= sizes.length - 1 || !Number.isFinite(delta)) return sizes;
  const pair = sizes[index] + sizes[index + 1];
  const min = Math.min(MIN_SHARE, pair / 2);
  const left = Math.min(pair - min, Math.max(min, sizes[index] + delta));
  const next = [...sizes];
  next[index] = left;
  next[index + 1] = pair - left;
  return next;
}

/** Replaces the sizes of the split at `path`. */
export function setSizesAt(root: LayoutNode | null, path: number[], sizes: number[]): LayoutNode | null {
  if (!root) return null;
  if (path.length === 0) {
    if (root.type !== "split" || sizes.length !== root.children.length) return root;
    return split(root.dir, root.children, scale(sizes));
  }
  if (root.type !== "split") return root;
  const [head, ...rest] = path;
  return split(
    root.dir,
    root.children.map((child, index) => (index === head ? setSizesAt(child, rest, sizes)! : child)),
    root.sizes,
  );
}

export function nodeAt(root: LayoutNode | null, path: number[]): LayoutNode | null {
  let node = root;
  for (const index of path) {
    if (!node || node.type !== "split") return null;
    node = node.children[index] ?? null;
  }
  return node;
}

/** The old automatic grid as a tree: a column of rows. Used to migrate saved layouts. */
export function fromGrid(ids: string[]): LayoutNode | null {
  if (ids.length === 0) return null;
  const { columns } = gridLayout(ids.length);
  const rows: LayoutNode[] = [];
  for (let at = 0; at < ids.length; at += columns) {
    const row = ids.slice(at, at + columns).map(pane);
    rows.push(row.length === 1 ? row[0] : split("row", row, row.map(() => 1 / row.length)));
  }
  return normalize(rows.length === 1 ? rows[0] : split("column", rows, rows.map(() => 1 / rows.length)));
}

/** Validates an untrusted tree; every pane id must pass `isPaneId`. */
export function parseLayout(value: unknown, isPaneId: (id: unknown) => id is string, depth = 0): LayoutNode | null {
  if (typeof value !== "object" || value === null || depth > MAX_DEPTH) return null;
  const node = value as Record<string, unknown>;
  if (node.type === "pane") return isPaneId(node.id) ? pane(node.id) : null;
  if (node.type !== "split" || (node.dir !== "row" && node.dir !== "column")) return null;
  if (!Array.isArray(node.children) || !Array.isArray(node.sizes)) return null;
  if (node.children.length < 1 || node.children.length > MAX_CHILDREN) return null;
  if (node.sizes.length !== node.children.length) return null;
  if (!node.sizes.every((size) => typeof size === "number" && Number.isFinite(size) && size > 0)) return null;
  const children: LayoutNode[] = [];
  for (const child of node.children) {
    const parsed = parseLayout(child, isPaneId, depth + 1);
    if (!parsed) return null;
    children.push(parsed);
  }
  return normalize(split(node.dir, children, scale(node.sizes as number[])));
}

export type Direction = "left" | "right" | "up" | "down";

/**
 * The pane beside `fromId` in `direction`: of the panes on that side that
 * share some of its edge, the nearest; on a tie, the one best centred on it.
 */
export function neighbor(root: LayoutNode | null, fromId: string, direction: Direction): string | null {
  const all = rects(root);
  const from = all.get(fromId);
  if (!from) return null;
  const EPSILON = 1e-6;
  const across = direction === "left" || direction === "right";
  let best: { id: string; distance: number; offset: number } | null = null;
  for (const [id, rect] of all) {
    if (id === fromId) continue;
    const distance = {
      left: from.x - (rect.x + rect.w),
      right: rect.x - (from.x + from.w),
      up: from.y - (rect.y + rect.h),
      down: rect.y - (from.y + from.h),
    }[direction];
    const overlap = across
      ? Math.min(from.y + from.h, rect.y + rect.h) - Math.max(from.y, rect.y)
      : Math.min(from.x + from.w, rect.x + rect.w) - Math.max(from.x, rect.x);
    if (distance < -EPSILON || overlap <= EPSILON) continue;
    const offset = across
      ? Math.abs(rect.y + rect.h / 2 - (from.y + from.h / 2))
      : Math.abs(rect.x + rect.w / 2 - (from.x + from.w / 2));
    const closer = !best || distance < best.distance - EPSILON;
    const tie = best && Math.abs(distance - best.distance) <= EPSILON && offset < best.offset;
    if (closer || tie) best = { id, distance, offset };
  }
  return best?.id ?? null;
}
