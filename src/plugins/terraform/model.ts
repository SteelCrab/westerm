// The Terraform plugin's logic without the page: what a project may do, how
// a plan's marks land on the tree, and how a plan is grouped.

import type { Mark, PlanAnswer, PlanChange, PlanMark, ProjectInfo, StateAnswer, TreeNode } from "./types.ts";

type ModuleNode = Extract<TreeNode, { kind: "module" }>;
type ResourceNode = Extract<TreeNode, { kind: "resource" }>;

const PROD_WORDS = new Set(["prod", "production", "prd", "live"]);

/** Whether the project's name or its workspace has a piece that names production. */
export function isProd(name: string, workspace: string): boolean {
  return [name, workspace].some((text) => text.split(/[/\-_.]/).some((piece) => PROD_WORDS.has(piece.toLowerCase())));
}

/** What the user types to apply to a production project: the folder's name. */
export function confirmWord(path: string): string {
  return path.split("/").filter((piece) => piece !== "").at(-1) ?? "";
}

/** The number on the plugin's button: every change of the plans that can be applied now. */
export function badgeCount(projects: readonly ProjectInfo[]): number {
  return projects.reduce((sum, { plan }) => {
    if (plan?.state !== "current" || plan.counts === null) return sum;
    const { add, change, destroy, import: imported, forget } = plan.counts;
    return sum + add + change + destroy + imported + forget;
  }, 0);
}

export type Dot = "running" | "failed" | "changes" | "clean" | "none";

/** The project's status dot: its run first, then its plan. */
export function projectDot(project: ProjectInfo): Dot {
  if (project.run?.state === "running") return "running";
  if (project.run?.state === "failed" || project.run?.state === "interrupted") return "failed";
  if (project.plan?.state === "current") return project.plan.applyable ? "changes" : "clean";
  return "none";
}

/** Whether the project can start a run now. */
export function canRun(state: StateAnswer, project: ProjectInfo): boolean {
  return state.shellPanes && project.toolFound && project.run?.state !== "running";
}

/** Whether the panel can apply the project's plan now. */
export function canApply(state: StateAnswer, project: ProjectInfo): boolean {
  return canRun(state, project) && !project.remote && project.plan?.state === "current" && project.plan.applyable;
}

/** The run button that stands out. */
export function primaryAction(project: ProjectInfo): "init" | "plan" {
  return project.needsInit ? "init" : "plan";
}

export interface ResourceRow {
  node: ResourceNode;
  mark: Mark | null;
  /** The instances in the plan, when there are two or more. */
  count: number | null;
  /** The plan removes it and the code no longer has it. */
  removed: boolean;
}

export type Row = { kind: "module"; node: ModuleNode; changed: boolean; children: Row[] } | ({ kind: "resource" } & ResourceRow);

const markKey = (module: string, type: string, name: string) => `${module}\n${type}\n${name}`;

/**
 * The tree with the plan's marks on its resources. A mark without a row (the
 * plan removes what the code no longer has) becomes a removed row under its
 * module, or under the project when the module is gone too. A module is
 * `changed` when a row below it has a mark.
 */
export function mergeMarks(nodes: readonly TreeNode[], marks: readonly PlanMark[]): Row[] {
  const left = new Map(marks.map((mark) => [markKey(mark.module, mark.type, mark.name), mark]));
  const removedRow = (mark: PlanMark): Row => ({
    kind: "resource",
    node: { kind: "resource", address: mark.module === "" ? `${mark.type}.${mark.name}` : `${mark.module}.${mark.type}.${mark.name}`, type: mark.type, name: mark.name },
    mark: mark.mark,
    count: null,
    removed: true,
  });
  const walk = (list: readonly TreeNode[], module: string): Row[] => {
    const rows = list.map((node): Row => {
      if (node.kind === "module") {
        const children = walk(node.children, node.address);
        const changed = children.some((row) => (row.kind === "module" ? row.changed : row.mark !== null));
        return { kind: "module", node, changed, children };
      }
      const key = markKey(module, node.type, node.name);
      const mark = left.get(key);
      left.delete(key);
      return { kind: "resource", node, mark: mark?.mark ?? null, count: mark && mark.count >= 2 ? mark.count : null, removed: false };
    });
    // What the plan removes from this module and the code no longer names.
    for (const [key, mark] of [...left]) {
      if (mark.module !== module) continue;
      left.delete(key);
      rows.push(removedRow(mark));
    }
    return rows;
  };
  const rows = walk(nodes, "");
  return [...rows, ...[...left.values()].map(removedRow)];
}

/** The rows whose address has `query`; a module that matches keeps all its rows, and stays above a match. */
export function filterRows(rows: readonly Row[], query: string): Row[] {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return [...rows];
  const matches = (address: string, name: string) => name.toLowerCase().includes(wanted) || address.toLowerCase().endsWith(wanted);
  return rows.flatMap((row): Row[] => {
    if (row.kind === "resource") return row.node.address.toLowerCase().includes(wanted) ? [row] : [];
    if (matches(row.node.address, row.node.name)) return [row];
    const children = filterRows(row.children, query);
    return children.length > 0 ? [{ ...row, children }] : [];
  });
}

/** The changes by module, the project's own first, then by address. */
export function groupChanges(changes: readonly PlanChange[]): { module: string; changes: PlanChange[] }[] {
  const groups = new Map<string, PlanChange[]>();
  for (const change of [...changes].sort((a, b) => (a.address < b.address ? -1 : a.address > b.address ? 1 : 0))) {
    groups.set(change.module, [...(groups.get(change.module) ?? []), change]);
  }
  return [...groups.entries()].sort(([a], [b]) => (a === "" ? -1 : b === "" ? 1 : a < b ? -1 : 1)).map(([module, list]) => ({ module, changes: list }));
}

/** Whether a change shows its attributes at first: all but what is only added. */
export function opensAtFirst(change: PlanChange): boolean {
  return change.action !== "create" && change.action !== "import";
}

const DESTROYS_SHOWN = 10;

/** The addresses the Apply dialog names, and how many more the plan destroys. */
export function destroyList(plan: PlanAnswer): { shown: string[]; more: number } {
  const shown = plan.destroys.slice(0, DESTROYS_SHOWN);
  return { shown, more: Math.max(0, Math.max(plan.counts?.destroy ?? 0, plan.destroys.length) - shown.length) };
}

/** How long ago `atMs` was, in its largest whole unit. */
export function age(nowMs: number, atMs: number): { n: number; unit: "s" | "m" | "h" | "d" } {
  const seconds = Math.max(0, Math.floor((nowMs - atMs) / 1000));
  if (seconds < 60) return { n: seconds, unit: "s" };
  if (seconds < 3600) return { n: Math.floor(seconds / 60), unit: "m" };
  if (seconds < 86_400) return { n: Math.floor(seconds / 3600), unit: "h" };
  return { n: Math.floor(seconds / 86_400), unit: "d" };
}
