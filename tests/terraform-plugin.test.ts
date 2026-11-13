import assert from "node:assert/strict";
import { test } from "node:test";
import {
  deletePlans,
  fetchPlan,
  fetchState,
  fetchTree,
  listWorkspaces,
  selectWorkspace,
  setCheckVersions,
  setTool,
  startRun,
} from "../src/plugins/terraform/api.ts";
import {
  age,
  badgeCount,
  canApply,
  canRun,
  confirmWord,
  destroyList,
  filterRows,
  groupChanges,
  isProd,
  mergeMarks,
  opensAtFirst,
  primaryAction,
  projectDot,
  type Row,
} from "../src/plugins/terraform/model.ts";
import type { Counts, PlanAnswer, PlanChange, PlanMark, ProjectInfo, StateAnswer, TreeNode } from "../src/plugins/terraform/types.ts";
import type { PluginContext } from "../src/plugins/types.ts";

// ---- Fixtures ----

const counts = (over: Partial<Counts> = {}): Counts => ({ add: 0, change: 0, destroy: 0, import: 0, forget: 0, ...over });

function project(over: Partial<ProjectInfo> = {}): ProjectInfo {
  return {
    id: "0123456789abcdef",
    path: "/w/acme-infra/envs/dev",
    name: "envs/dev",
    sub: "~/acme-infra",
    tool: "terraform",
    toolFound: true,
    workspace: "default",
    workspaceLocked: false,
    remote: false,
    needsInit: false,
    unreadable: [],
    run: null,
    plan: null,
    ...over,
  };
}

const current = (over: Partial<Counts> = { add: 2, change: 1 }, applyable = true): ProjectInfo["plan"] => ({
  state: "current",
  counts: counts(over),
  applyable,
  at: 1000,
  target: null,
});

function state(projects: ProjectInfo[], over: Partial<StateAnswer> = {}): StateAnswer {
  return { tools: { terraform: true, tofu: false }, shellPanes: true, checkVersions: true, projects, truncated: false, ...over };
}

const moduleNode = (address: string, children: TreeNode[] = []): TreeNode => ({
  kind: "module",
  name: address.split(".").at(-1)!,
  address,
  source: "local",
  label: "local",
  newer: null,
  dir: `/w/${address}`,
  children,
});

const resourceNode = (address: string): TreeNode => {
  const parts = address.split(".");
  return { kind: "resource", address, type: parts.at(-2)!, name: parts.at(-1)! };
};

const change = (address: string, action: PlanChange["action"], module = ""): PlanChange => {
  const parts = address.split(".");
  return { address, module, type: parts.at(-2)!, name: parts.at(-1)!, action, attrs: [], moreAttrs: 0 };
};

function plan(over: Partial<PlanAnswer> = {}): PlanAnswer {
  return {
    state: "current",
    counts: counts(),
    applyable: true,
    moved: 0,
    outputs: 0,
    target: null,
    workspace: "default",
    at: 1000,
    seconds: 12,
    changes: [],
    moreChanges: 0,
    destroys: [],
    ...over,
  };
}

// ---- Model ----

test("isProd looks at whole pieces, in any case", () => {
  for (const [name, workspace] of [
    ["envs/prod", "default"],
    ["infra", "Production"],
    ["acme-prd", "x"],
    ["live/eu", "x"],
    ["envs/dev", "prod-eu"],
    ["envs.PROD_eu", "x"],
  ]) {
    assert.equal(isProd(name, workspace), true, `${name} ${workspace}`);
  }
  for (const [name, workspace] of [
    ["envs/dev", "default"],
    ["product", "default"],
    ["reproduce", "x"],
    ["delivery", "x"],
    ["nonprod2", "olive"],
  ]) {
    assert.equal(isProd(name, workspace), false, `${name} ${workspace}`);
  }
});

test("confirmWord is the folder name", () => {
  assert.equal(confirmWord("/Users/a/acme-infra/envs/prod"), "prod");
  assert.equal(confirmWord("/Users/a/acme-infra/envs/prod/"), "prod");
  assert.equal(confirmWord("prod"), "prod");
});

test("badgeCount adds the current plans only", () => {
  const projects = [
    project({ plan: current({ add: 2, change: 1 }) }),
    project({ plan: { ...current({ add: 5 })!, state: "stale" } }),
    project({ plan: { ...current({ add: 7 })!, state: "applied" } }),
    project({ plan: current({ destroy: 1, import: 1, forget: 1 }) }),
    project({ plan: null }),
  ];
  assert.equal(badgeCount(projects), 6);
  assert.equal(badgeCount([]), 0);
});

test("projectDot follows the run, then the plan", () => {
  const run = (state: NonNullable<ProjectInfo["run"]>["state"]): ProjectInfo["run"] => ({ action: "plan", state, at: 1 });
  assert.equal(projectDot(project({ run: run("running"), plan: current() })), "running");
  assert.equal(projectDot(project({ run: run("failed"), plan: current() })), "failed");
  assert.equal(projectDot(project({ run: run("interrupted") })), "failed");
  assert.equal(projectDot(project({ run: run("ok"), plan: current() })), "changes");
  assert.equal(projectDot(project({ plan: current({}, false) })), "clean");
  assert.equal(projectDot(project({ plan: { ...current()!, state: "stale" } })), "none");
  assert.equal(projectDot(project()), "none");
});

test("canRun needs a shell, the tool, and no running run", () => {
  assert.equal(canRun(state([]), project()), true);
  assert.equal(canRun(state([], { shellPanes: false }), project()), false);
  assert.equal(canRun(state([]), project({ toolFound: false })), false);
  assert.equal(canRun(state([]), project({ run: { action: "init", state: "running", at: 1 } })), false);
  assert.equal(canRun(state([]), project({ run: { action: "init", state: "failed", at: 1 } })), true);
});

test("canApply needs a current plan with something to apply", () => {
  const ok = project({ plan: current() });
  assert.equal(canApply(state([]), ok), true);
  assert.equal(canApply(state([]), project({ plan: { ...current()!, state: "stale" } })), false);
  assert.equal(canApply(state([]), project({ plan: current(), remote: true })), false);
  assert.equal(canApply(state([]), project({ plan: current(), run: { action: "plan", state: "running", at: 1 } })), false);
  assert.equal(canApply(state([]), project({ plan: current(), toolFound: false })), false);
  assert.equal(canApply(state([], { shellPanes: false }), ok), false);
  assert.equal(canApply(state([]), project({ plan: current({}, false) })), false);
  assert.equal(canApply(state([]), project()), false);
});

test("primaryAction is init for a project that needs it", () => {
  assert.equal(primaryAction(project({ needsInit: true })), "init");
  assert.equal(primaryAction(project()), "plan");
});

test("mergeMarks puts marks, counts, and removed rows in place", () => {
  const tree: TreeNode[] = [
    moduleNode("module.vpc", [resourceNode("module.vpc.aws_subnet.private"), resourceNode("module.vpc.aws_vpc.this")]),
    moduleNode("module.quiet", [resourceNode("module.quiet.x.y")]),
    resourceNode("aws_route53_record.app"),
  ];
  const marks: PlanMark[] = [
    { module: "module.vpc", type: "aws_subnet", name: "private", mark: "delete", count: 3 },
    { module: "", type: "aws_route53_record", name: "app", mark: "create", count: 1 },
    { module: "module.vpc", type: "aws_eip", name: "old", mark: "delete", count: 1 },
    { module: "module.gone", type: "aws_s3_bucket", name: "logs", mark: "delete", count: 1 },
  ];
  const rows = mergeMarks(tree, marks);
  const vpc = rows[0] as Extract<Row, { kind: "module" }>;
  assert.equal(vpc.changed, true);
  const inside = vpc.children.map((row) => (row.kind === "resource" ? [row.node.address, row.mark, row.count, row.removed] : null));
  assert.deepEqual(inside, [
    ["module.vpc.aws_subnet.private", "delete", 3, false],
    ["module.vpc.aws_vpc.this", null, null, false],
    ["module.vpc.aws_eip.old", "delete", null, true],
  ]);
  assert.equal((rows[1] as Extract<Row, { kind: "module" }>).changed, false);
  const top = rows.filter((row) => row.kind === "resource").map((row) => (row.kind === "resource" ? [row.node.address, row.mark, row.count, row.removed] : null));
  assert.deepEqual(top, [
    ["aws_route53_record.app", "create", null, false],
    // Its module is not in the code any more: the row goes under the project.
    ["module.gone.aws_s3_bucket.logs", "delete", null, true],
  ]);
  // Without marks every row is plain.
  const plain = mergeMarks(tree, []);
  assert.equal((plain[0] as Extract<Row, { kind: "module" }>).changed, false);
  assert.equal(plain.length, 3);
});

test("a change deep inside marks every module above it", () => {
  const tree = [moduleNode("module.a", [moduleNode("module.a.module.b", [resourceNode("module.a.module.b.x.y")])])];
  const rows = mergeMarks(tree, [{ module: "module.a.module.b", type: "x", name: "y", mark: "update", count: 1 }]);
  const outer = rows[0] as Extract<Row, { kind: "module" }>;
  const inner = outer.children[0] as Extract<Row, { kind: "module" }>;
  assert.deepEqual([outer.changed, inner.changed], [true, true]);
});

test("filterRows keeps the modules above a match", () => {
  const rows = mergeMarks(
    [moduleNode("module.vpc", [resourceNode("module.vpc.aws_subnet.private"), resourceNode("module.vpc.aws_vpc.this")]), moduleNode("module.eks", [resourceNode("module.eks.aws_eks_cluster.this")]), resourceNode("aws_route53_record.app")],
    [],
  );
  const names = (list: readonly Row[]): string[] => list.flatMap((row) => (row.kind === "module" ? [row.node.address, ...names(row.children)] : [row.node.address]));
  assert.deepEqual(names(filterRows(rows, "subnet")), ["module.vpc", "module.vpc.aws_subnet.private"]);
  assert.deepEqual(names(filterRows(rows, "EKS")), ["module.eks", "module.eks.aws_eks_cluster.this"], "a module that matches keeps its rows");
  assert.deepEqual(names(filterRows(rows, "  ")), names(rows));
  assert.deepEqual(names(filterRows(rows, "nothing")), []);
});

test("groupChanges puts the root first", () => {
  const groups = groupChanges([change("module.vpc.aws_vpc.this", "create", "module.vpc"), change("aws_route53_record.app", "create"), change("module.eks.x.y", "update", "module.eks"), change("aws_a.b", "delete")]);
  assert.deepEqual(
    groups.map((group) => [group.module, group.changes.map((item) => item.address)]),
    [
      ["", ["aws_a.b", "aws_route53_record.app"]],
      ["module.eks", ["module.eks.x.y"]],
      ["module.vpc", ["module.vpc.aws_vpc.this"]],
    ],
  );
  assert.deepEqual(groupChanges([]), []);
});

test("opensAtFirst is for the changes a person must read", () => {
  const opens = (action: PlanChange["action"]) => opensAtFirst(change("a.b", action));
  assert.deepEqual([opens("update"), opens("replace"), opens("delete"), opens("forget")], [true, true, true, true]);
  assert.deepEqual([opens("create"), opens("import")], [false, false]);
});

test("destroyList shows ten and counts the rest", () => {
  const destroys = Array.from({ length: 13 }, (_, i) => `a.r${i}`);
  const list = destroyList(plan({ destroys, counts: counts({ destroy: 13 }) }));
  assert.equal(list.shown.length, 10);
  assert.equal(list.shown[0], "a.r0");
  assert.equal(list.more, 3);
  assert.deepEqual(destroyList(plan()), { shown: [], more: 0 });
  // The plan lists at most 2000 addresses; the count is whole.
  assert.equal(destroyList(plan({ destroys: destroys.slice(0, 3), counts: counts({ destroy: 2500 }) })).more, 2497);
});

test("age picks the largest whole unit", () => {
  const at = 1_000_000;
  assert.deepEqual(age(at + 59_000, at), { n: 59, unit: "s" });
  assert.deepEqual(age(at + 2 * 60_000 + 5000, at), { n: 2, unit: "m" });
  assert.deepEqual(age(at + 3 * 3_600_000, at), { n: 3, unit: "h" });
  assert.deepEqual(age(at + 2 * 86_400_000, at), { n: 2, unit: "d" });
  assert.deepEqual(age(at - 5000, at), { n: 0, unit: "s" }, "a clock that is behind");
});

// ---- Requests ----

interface FakeCtx {
  ctx: PluginContext;
  calls: { path: string; init?: RequestInit }[];
}
function fakeCtx(respond: (path: string, init?: RequestInit) => Response): FakeCtx {
  const calls: FakeCtx["calls"] = [];
  const ctx: PluginContext = {
    openShell() {},
    openFolder() {},
    openBrowser() {},
    openPane() {},
    folders: () => [],
    fetch: async (path, init) => {
      calls.push({ path, init });
      return respond(path, init);
    },
    refreshInstalled() {},
    openManager() {},
  };
  return { ctx, calls };
}
const json = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });

test("fetchState sends each folder, and scan only when asked", async () => {
  const answer = state([project()]);
  const { ctx, calls } = fakeCtx(() => json(answer));
  const scanned = await fetchState(ctx, ["/a b", "/c"], true);
  assert.deepEqual(scanned, { ok: true, value: answer });
  await fetchState(ctx, ["/a b", "/c"], false);
  await fetchState(ctx, [], false);
  assert.deepEqual(
    calls.map((call) => call.path),
    ["/state?scan=1&folder=%2Fa%20b&folder=%2Fc", "/state?folder=%2Fa%20b&folder=%2Fc", "/state"],
  );
});

test("startRun posts the action and reads a refusal code", async () => {
  const { ctx, calls } = fakeCtx((_path, init) => (JSON.parse(String(init?.body)).action === "apply" ? json({ error: "stale" }, 409) : json({ token: "a".repeat(32), name: "envs/dev plan" })));
  assert.deepEqual(await startRun(ctx, "0123456789abcdef", "plan", "module.eks"), { ok: true, value: { token: "a".repeat(32), name: "envs/dev plan" } });
  assert.deepEqual(await startRun(ctx, "0123456789abcdef", "apply", null, 1234), { ok: false, failure: { type: "refused", error: "stale" } });
  assert.equal(calls[0].path, "/projects/0123456789abcdef/runs");
  assert.equal(calls[0].init?.method, "POST");
  assert.deepEqual(JSON.parse(String(calls[0].init?.body)), { action: "plan", target: "module.eks" });
  // An apply names the plan the user confirmed, by its time.
  assert.deepEqual(JSON.parse(String(calls[1].init?.body)), { action: "apply", planAt: 1234 });
  assert.equal(new Headers(calls[0].init?.headers).get("content-type"), "application/json");
});

test("an unknown project reads as removed, an off plugin as gone", async () => {
  const removed = fakeCtx(() => json({ removed: true }, 404));
  assert.deepEqual(await fetchTree(removed.ctx, "0123456789abcdef"), { ok: false, failure: { type: "removed" } });
  const gone = fakeCtx(() => new Response(null, { status: 404 }));
  assert.deepEqual(await fetchTree(gone.ctx, "0123456789abcdef"), { ok: false, failure: { type: "gone" } });
  const broken = fakeCtx(() => {
    throw new Error("offline");
  });
  assert.deepEqual(await fetchTree(broken.ctx, "0123456789abcdef"), { ok: false, failure: { type: "error", error: "Request failed" } });
  const odd = fakeCtx(() => new Response("oops", { status: 500 }));
  assert.deepEqual(await fetchTree(odd.ctx, "0123456789abcdef"), { ok: false, failure: { type: "error", error: "Request failed" } });
});

test("fetchPlan reads 204 as no plan", async () => {
  const none = fakeCtx(() => new Response(null, { status: 204 }));
  assert.deepEqual(await fetchPlan(none.ctx, "0123456789abcdef"), { ok: true, value: null });
  const some = fakeCtx(() => json(plan()));
  assert.deepEqual(await fetchPlan(some.ctx, "0123456789abcdef"), { ok: true, value: plan() });
  assert.equal(some.calls[0].path, "/projects/0123456789abcdef/plan");
});

test("the other requests go to their routes", async () => {
  const { ctx, calls } = fakeCtx((path) => (path.endsWith("/workspaces") ? json({ workspaces: ["default", "dev"], current: "dev" }) : new Response(null, { status: 204 })));
  assert.deepEqual(await listWorkspaces(ctx, "0123456789abcdef"), { ok: true, value: { workspaces: ["default", "dev"], current: "dev" } });
  assert.deepEqual(await selectWorkspace(ctx, "0123456789abcdef", "dev"), { ok: true, value: null });
  assert.deepEqual(await setTool(ctx, "0123456789abcdef", "tofu"), { ok: true, value: null });
  assert.deepEqual(await setCheckVersions(ctx, false), { ok: true, value: null });
  assert.deepEqual(await deletePlans(ctx), { ok: true, value: null });
  assert.deepEqual(
    calls.map((call) => [call.init?.method ?? "GET", call.path, call.init?.body ? JSON.parse(String(call.init.body)) : null]),
    [
      ["GET", "/projects/0123456789abcdef/workspaces", null],
      ["POST", "/projects/0123456789abcdef/workspace", { name: "dev" }],
      ["PUT", "/projects/0123456789abcdef/tool", { tool: "tofu" }],
      ["PUT", "/settings", { checkVersions: false }],
      ["DELETE", "/plans", null],
    ],
  );
});

test("a failed workspace list carries the tool's error line", async () => {
  const { ctx } = fakeCtx(() => json({ error: "Error: No valid credential sources found" }, 502));
  assert.deepEqual(await listWorkspaces(ctx, "0123456789abcdef"), { ok: false, failure: { type: "refused", error: "Error: No valid credential sources found" } });
});
