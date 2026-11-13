// The shapes the Terraform plugin's server routes answer with. Times are Unix milliseconds.

export type Tool = "terraform" | "tofu";
export type Action = "init" | "validate" | "plan" | "apply";
export type RunState = "running" | "ok" | "failed" | "interrupted";
export type PlanState = "current" | "stale" | "unreadable" | "applied" | "applyFailed";
/** What a plan does to a resource of the tree. */
export type Mark = "create" | "update" | "delete" | "replace";

/** A plan's numbers, counted as the tool prints them. */
export interface Counts {
  add: number;
  change: number;
  destroy: number;
  import: number;
  forget: number;
}

/** A folder where a tool runs. */
export interface ProjectInfo {
  id: string;
  path: string;
  name: string;
  /** The dimmer text beside the name: the repository root. */
  sub: string;
  tool: Tool;
  toolFound: boolean;
  workspace: string;
  /** `TF_WORKSPACE` sets the workspace. */
  workspaceLocked: boolean;
  /** Runs happen elsewhere: its plan cannot be saved or applied from the panel. */
  remote: boolean;
  needsInit: boolean;
  /** Files of the project that could not be read. */
  unreadable: string[];
  /** The running run, else the last one. */
  run: { action: Action; state: RunState; at: number } | null;
  plan: { state: PlanState; counts: Counts | null; applyable: boolean; at: number; target: string | null } | null;
}

export interface StateAnswer {
  tools: { terraform: boolean; tofu: boolean };
  /** Whether westerm can type into the user's shell: without it nothing can run. */
  shellPanes: boolean;
  checkVersions: boolean;
  projects: ProjectInfo[];
  /** A limit cut the scan short. */
  truncated: boolean;
}

export type TreeNode =
  | {
      kind: "module";
      name: string;
      address: string;
      source: "local" | "registry" | "git" | "other";
      /** The version, `git · <ref>`, or `local`. */
      label: string | null;
      /** A newer version in the public registry. */
      newer: string | null;
      /** The module's folder, when it is known. */
      dir: string | null;
      children: TreeNode[];
    }
  | { kind: "resource"; address: string; type: string; name: string };

/** What the current plan does to a resource; `module` is "" for the project itself. */
export interface PlanMark {
  module: string;
  type: string;
  name: string;
  mark: Mark;
  /** The resource's instances in the plan. */
  count: number;
}

export interface TreeAnswer {
  nodes: TreeNode[];
  truncated: boolean;
  marks: PlanMark[];
  /** The tool's version in this project. */
  version: string | null;
}

export type AttrValue = { kind: "value"; text: string; cut: boolean } | { kind: "unknown" } | { kind: "sensitive" };

export interface PlanAttr {
  path: string;
  before: AttrValue | null;
  after: AttrValue | null;
  /** The change of this attribute is why the resource is replaced. */
  forces: boolean;
}

export interface PlanChange {
  address: string;
  module: string;
  type: string;
  name: string;
  action: Mark | "import" | "forget";
  attrs: PlanAttr[];
  /** Attribute rows left out. */
  moreAttrs: number;
}

export interface PlanAnswer {
  state: PlanState;
  counts: Counts | null;
  applyable: boolean;
  /** Resources that only get a new address. */
  moved: number;
  /** Outputs that change. */
  outputs: number;
  target: string | null;
  workspace: string;
  at: number;
  /** How long the plan took. */
  seconds: number;
  changes: PlanChange[];
  /** Changes left out. */
  moreChanges: number;
  /** The addresses the plan destroys, at most 2000. */
  destroys: string[];
}

/** The state of a plan pane, saved with the layout. */
export interface PlanPaneData {
  projectId: string;
  path: string;
  /** The address of the change to show first; cleared once shown. */
  focus?: string;
}
