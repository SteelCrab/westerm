// The automatic near-square grid that layouts used before split trees.
// Kept to migrate saved layouts (see fromGrid in layout.ts).

export interface GridLayout {
  columns: number;
  rows: number;
  /** Column span of the last pane, so it fills an incomplete final row. */
  lastSpan: number;
}

/** 1 → 1×1, 2 → 2×1, 3–4 → 2×2, 5–6 → 3×2, … */
export function gridLayout(count: number): GridLayout {
  if (count <= 1) return { columns: 1, rows: 1, lastSpan: 1 };
  const columns = Math.ceil(Math.sqrt(count));
  const rows = Math.ceil(count / columns);
  return { columns, rows, lastSpan: columns * rows - count + 1 };
}
