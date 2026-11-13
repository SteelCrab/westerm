import "./dom-page.ts";
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement, Fragment } from "react";
import { ApplyDialog, useApply } from "../src/plugins/terraform/ApplyDialog.tsx";
import { PlanPane } from "../src/plugins/terraform/PlanPane.tsx";
import type { TerraformStore } from "../src/plugins/terraform/store.ts";
import type { Action, Counts, PlanAnswer, PlanAttr, PlanChange, ProjectInfo, StateAnswer } from "../src/plugins/terraform/types.ts";
import type { PluginContext } from "../src/plugins/types.ts";
import { click, mount, settle, text, type, type Mounted } from "./dom-page.ts";

let mounted: Mounted | null = null;
afterEach(async () => {
  await mounted?.unmount();
  mounted = null;
});

// ---- Fixtures ----

const DEV = "0123456789abcdef";
/** Two minutes ago: the same value in every fixture, so a new fixture is not a new plan. */
const AT = Date.now() - 120_000;

const counts = (over: Partial<Counts> = {}): Counts => ({ add: 0, change: 0, destroy: 0, import: 0, forget: 0, ...over });

function project(over: Partial<ProjectInfo> = {}): ProjectInfo {
  return {
    id: DEV,
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
    run: { action: "plan", state: "ok", at: AT },
    plan: { state: "current", counts: counts({ add: 2, change: 1 }), applyable: true, at: AT, target: null },
    ...over,
  };
}

const attr = (path: string, before: PlanAttr["before"], after: PlanAttr["after"], forces = false): PlanAttr => ({ path, before, after, forces });
const value = (textOf: string, cut = false) => ({ kind: "value" as const, text: textOf, cut });

const change = (address: string, action: PlanChange["action"], module = "", attrs: PlanAttr[] = [], moreAttrs = 0): PlanChange => {
  const rest = module === "" ? address : address.slice(module.length + 1);
  const [type, name] = rest.replace(/\[.*$/, "").split(".");
  return { address, module, type, name, action, attrs, moreAttrs };
};

function plan(over: Partial<PlanAnswer> = {}): PlanAnswer {
  return {
    state: "current",
    counts: counts({ add: 2, change: 1 }),
    applyable: true,
    moved: 0,
    outputs: 0,
    target: null,
    workspace: "default",
    at: AT,
    seconds: 12,
    changes: [
      change("module.vpc.aws_nat_gateway.this", "update", "module.vpc", [
        attr("tags.Env", value('"dev"'), value('"development"')),
        attr("tags.Owner", null, value('"platform"')),
      ]),
      change("module.eks.aws_iam_role.cluster", "create", "module.eks", [attr("name", null, value('"cluster"'))]),
      change("aws_route53_record.app[0]", "create", "", [attr("zone_id", null, { kind: "unknown" }), attr("records", null, { kind: "sensitive" })]),
    ],
    moreChanges: 0,
    destroys: [],
    ...over,
  };
}

interface World {
  ctx: PluginContext;
  store: TerraformStore;
  /** What the plan route answers: a plan, or null for 204. */
  plan: PlanAnswer | null;
  state: StateAnswer | null;
  /** What a run answers: null starts it, a word refuses it. */
  refuse: string | null;
  runs: [string, Action, string | null, number | undefined][];
  planCalls: number;
  data: unknown[];
  titles: string[];
  closed: number;
  applied: string[];
  listeners: Set<() => void>;
}

function world(projects: ProjectInfo[] | null, answer: PlanAnswer | null = plan()): World {
  const w: World = {
    plan: answer,
    state: projects === null ? null : { tools: { terraform: true, tofu: false }, shellPanes: true, checkVersions: true, projects, truncated: false },
    refuse: null,
    runs: [],
    planCalls: 0,
    data: [],
    titles: [],
    closed: 0,
    applied: [],
    listeners: new Set(),
    ctx: null as unknown as PluginContext,
    store: null as unknown as TerraformStore,
  };
  w.ctx = {
    openShell() {},
    openFolder() {},
    openBrowser() {},
    openPane() {},
    folders: () => [],
    fetch: async (path) => {
      if (path.endsWith("/plan")) {
        w.planCalls += 1;
        return w.plan === null ? new Response(null, { status: 204 }) : new Response(JSON.stringify(w.plan), { status: 200 });
      }
      return new Response(null, { status: 404 });
    },
    refreshInstalled() {},
    openManager() {},
  };
  w.store = {
    state: () => w.state,
    subscribe: (listener) => {
      w.listeners.add(listener);
      return () => w.listeners.delete(listener);
    },
    status: () => ({ badge: null, alert: false }),
    run: async (id, action, target, planAt) => {
      w.runs.push([id, action, target, planAt]);
      return w.refuse;
    },
    refresh() {},
    setPanelOpen() {},
    dispose() {},
  };
  return w;
}

/** Sets the projects and tells the page. */
async function setProjects(w: World, projects: ProjectInfo[]) {
  w.state = { ...w.state!, projects };
  for (const listener of [...w.listeners]) listener();
  await settle();
}

async function pane(w: World, data: unknown = { projectId: DEV, path: "/w/acme-infra/envs/dev" }, focused = true): Promise<HTMLElement> {
  mounted = await mount(
    createElement(PlanPane, {
      view: "plan",
      data,
      focused,
      setData: (next: unknown) => w.data.push(next),
      setTitle: (title: string) => w.titles.push(title),
      close: () => {
        w.closed += 1;
      },
      ctx: w.ctx,
      store: w.store,
      openApply: (id: string) => w.applied.push(id),
    }),
  );
  await settle();
  return mounted.container;
}

const button = (root: Element, label: string) =>
  [...root.querySelectorAll("button")].find((each) => each.getAttribute("aria-label") === label || text(each) === label);

const changeRow = (root: Element, address: string) => [...root.querySelectorAll<HTMLElement>(".tf-change")].find((each) => each.dataset.address === address) ?? null;

// ---- The plan pane ----

test("the pane groups changes by module with the root first", async () => {
  const container = await pane(world([project()]));
  assert.deepEqual(
    [...container.querySelectorAll(".tf-plan-group h3")].map((heading) => text(heading)),
    ["Root", "module.eks", "module.vpc"],
  );
  const header = text(container.querySelector(".tf-pane-header")!);
  assert.match(header, /Plan/);
  assert.match(header, /envs\/dev · workspace default/);
  assert.match(header, /2m ago/);
  assert.equal(text(changeRow(container, "aws_route53_record.app[0]")!.querySelector(".tf-name")!), "aws_route53_record.app[0]");
  assert.equal(text(changeRow(container, "module.vpc.aws_nat_gateway.this")!.querySelector(".tf-name")!), "aws_nat_gateway.this");
  assert.match(text(container.querySelector(".tf-plan-footer")!), /3 changes · 12 s/);
  assert.match(text(container.querySelector(".tf-plan-footer")!), /Plan file saved/);
});

test("the pane names itself after its project", async () => {
  const w = world([project()]);
  await pane(w);
  assert.deepEqual([...new Set(w.titles)], ["Plan · envs/dev"]);
});

test("updates open at first, creates stay closed", async () => {
  const container = await pane(world([project()]));
  const update = changeRow(container, "module.vpc.aws_nat_gateway.this")!;
  assert.equal(update.querySelector(".tf-change-head")?.getAttribute("aria-expanded"), "true");
  assert.match(text(update), /tags\.Env "dev" → "development"/);
  assert.match(text(update), /Update in place/);
  const create = changeRow(container, "module.eks.aws_iam_role.cluster")!;
  assert.equal(create.querySelector(".tf-change-head")?.getAttribute("aria-expanded"), "false");
  assert.ok(!(create.querySelector(".tf-attrs")), "the element is not there");
  await click(create.querySelector(".tf-change-head"));
  assert.match(text(create), /name \(none\) → "cluster"/);
  await click(update.querySelector(".tf-change-head"));
  assert.ok(!(update.querySelector(".tf-attrs")), "the element is not there");
});

test("sensitive and unknown values show as words", async () => {
  const w = world([project()]);
  w.plan = plan({
    changes: [
      change("aws_instance.web", "replace", "", [attr("ami", value('"ami-1"'), value('"ami-2"'), true), attr("token", { kind: "sensitive" }, { kind: "sensitive" }), attr("arn", value('"a"'), { kind: "unknown" }), attr("user_data", null, value('"abc', true))], 4),
    ],
  });
  const container = await pane(w);
  const row = text(changeRow(container, "aws_instance.web")!);
  assert.match(row, /Replace/);
  assert.match(row, /ami "ami-1" → "ami-2" forces replacement/);
  assert.match(row, /token \(sensitive\) → \(sensitive\)/);
  assert.match(row, /arn "a" → \(known after apply\)/);
  assert.match(row, /user_data \(none\) → "abc…/);
  assert.match(row, /… 4 more/);
});

test("the chips follow the plan's numbers", async () => {
  const w = world([project()]);
  const container = await pane(w);
  const chips = () => [...container.querySelectorAll(".tf-chip")].map((chip) => text(chip));
  assert.deepEqual(chips(), ["+2 to add", "~1 to change", "-0 to destroy"]);
  w.plan = plan({ counts: counts({ add: 1, import: 2, forget: 1 }), target: "module.eks" });
  await setProjects(w, [project({ plan: { ...project().plan!, at: Date.now() } })]);
  assert.deepEqual(chips(), ["+1 to add", "~0 to change", "-0 to destroy", "2 to import", "1 to forget"]);
  assert.match(text(container.querySelector(".tf-plan-bar")!), /Target: module\.eks/);
});

test("a stale plan dims the list and turns Apply off", async () => {
  const w = world([project({ plan: { ...project().plan!, state: "stale" } })], plan({ state: "stale" }));
  const container = await pane(w);
  assert.equal(text(container.querySelector(".tf-plan-state")!), "The code changed after this plan. Plan again.");
  assert.ok(container.querySelector(".tf-plan-list.stale"));
  assert.equal((button(container, "Apply…") as HTMLButtonElement).disabled, true);
});

test("each plan state has its line", async () => {
  const line = async (w: World) => {
    const container = await pane(w);
    const found = text(container.querySelector(".tf-plan-state")!);
    await mounted?.unmount();
    mounted = null;
    return found;
  };
  assert.equal(await line(world([project({ run: null, plan: null })], null)), "No plan yet. Press Plan.");
  assert.equal(await line(world([project({ run: { action: "plan", state: "running", at: 1 }, plan: null })], null)), "Plan is running in the terminal…");
  assert.equal(await line(world([project({ run: { action: "plan", state: "failed", at: 1 }, plan: null })], null)), "The last plan failed. See the terminal.");
  assert.equal(await line(world([project()], plan({ changes: [], counts: counts(), applyable: false }))), "No changes. Your infrastructure matches the code.");
  assert.equal(await line(world([project()], plan({ state: "unreadable", counts: null, changes: [] }))), "Could not read the plan. See the terminal.");
  assert.equal(await line(world([project()], plan({ state: "applied", counts: counts({ add: 2, change: 1 }), changes: [] }))), "Applied 3 changes.");
  assert.equal(await line(world([project()], plan({ state: "applyFailed", changes: [] }))), "Apply did not finish. Run Plan again to see what is left.");
});

test("a plan of moves only says it changes state only", async () => {
  const container = await pane(world([project()], plan({ changes: [], counts: counts(), moved: 2, outputs: 1 })));
  assert.equal(text(container.querySelector(".tf-plan-state")!), "This plan changes state only: 2 moved, 1 outputs.");
});

test("a long plan says how many changes it shows", async () => {
  const container = await pane(world([project()], plan({ moreChanges: 100 })));
  assert.match(text(container.querySelector(".tf-plan-list")!), /Showing the first 3 changes/);
  assert.match(text(container.querySelector(".tf-plan-footer")!), /103 changes/);
});

test("a missing project shows the not found notice and a close button", async () => {
  const w = world([]);
  const container = await pane(w);
  assert.match(text(container), /This project was not found/);
  await click(button(container, "Close"));
  assert.equal(w.closed, 1);
  assert.equal(w.planCalls, 0);
});

test("broken pane data shows the not found notice", async () => {
  for (const data of [5, null, { projectId: 1 }, { projectId: DEV }]) {
    const container = await pane(world([project()]), data);
    assert.match(text(container), /This project was not found/, JSON.stringify(data));
    await mounted?.unmount();
    mounted = null;
  }
});

test("the pane waits for the first state instead of saying not found", async () => {
  const w = world(null);
  const container = await pane(w);
  assert.doesNotMatch(text(container), /This project was not found/);
  w.state = { tools: { terraform: true, tofu: false }, shellPanes: true, checkVersions: true, projects: [project()], truncated: false };
  for (const listener of [...w.listeners]) listener();
  await settle();
  assert.ok(changeRow(container, "module.vpc.aws_nat_gateway.this"));
});

test("the plan is read again when the project's run or plan moves on, and on focus", async () => {
  const w = world([project()]);
  await pane(w, undefined, false);
  assert.equal(w.planCalls, 1);
  await setProjects(w, [project()]);
  assert.equal(w.planCalls, 1, "nothing moved");
  await setProjects(w, [project({ plan: { ...project().plan!, state: "stale" } })]);
  assert.equal(w.planCalls, 2);
  await mounted!.rerender(
    createElement(PlanPane, { view: "plan", data: { projectId: DEV, path: "/w/acme-infra/envs/dev" }, focused: true, setData() {}, setTitle() {}, close() {}, ctx: w.ctx, store: w.store, openApply() {} }),
  );
  await settle();
  assert.equal(w.planCalls, 3, "the pane got the focus");
});

test("the focus address opens its change and is cleared", async () => {
  const w = world([project()]);
  const container = await pane(w, { projectId: DEV, path: "/w/acme-infra/envs/dev", focus: "module.eks.aws_iam_role.cluster" });
  assert.equal(changeRow(container, "module.eks.aws_iam_role.cluster")?.querySelector(".tf-change-head")?.getAttribute("aria-expanded"), "true");
  assert.deepEqual(w.data, [{ projectId: DEV, path: "/w/acme-infra/envs/dev" }]);
});

test("Plan again keeps the target, and Apply opens the confirmation", async () => {
  const w = world([project()], plan({ target: "module.eks" }));
  const container = await pane(w);
  await click(button(container, "Plan again"));
  assert.deepEqual(w.runs, [[DEV, "plan", "module.eks", undefined]]);
  await click(button(container, "Apply…"));
  assert.deepEqual(w.applied, [DEV]);
  assert.equal(w.runs.length, 1, "the confirmation starts the apply");
});

test("a refused Plan again shows why", async () => {
  const w = world([project()]);
  w.refuse = "running";
  const container = await pane(w);
  await click(button(container, "Plan again"));
  assert.equal(text(container.querySelector(".tf-notice")!), "This project is already running something.");
});

test("Copy JSON copies the answer", async () => {
  const copied: string[] = [];
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (textOf: string) => void copied.push(textOf) } });
  const w = world([project()]);
  const container = await pane(w);
  await click(button(container, "Copy JSON"));
  assert.deepEqual(JSON.parse(copied[0]), JSON.parse(JSON.stringify(w.plan)));
});

test("a production project's pane is marked", async () => {
  const container = await pane(world([project({ name: "envs/prod", path: "/w/acme-infra/envs/prod" })]));
  assert.ok(container.querySelector(".tf-plan-pane.tf-plan-prod"));
  await mounted?.unmount();
  mounted = null;
  const plain = await pane(world([project()]));
  assert.ok(!(plain.querySelector(".tf-plan-prod")), "the element is not there");
});

// ---- The Apply dialog ----

interface DialogWorld {
  applied: number;
  closed: number;
}

async function dialog(over: { project?: ProjectInfo; plan?: PlanAnswer; refusal?: string | null } = {}): Promise<{ container: HTMLElement; w: DialogWorld }> {
  const w: DialogWorld = { applied: 0, closed: 0 };
  mounted = await mount(
    createElement(ApplyDialog, {
      project: over.project ?? project(),
      plan: over.plan ?? plan(),
      refusal: over.refusal ?? null,
      onApply: () => {
        w.applied += 1;
      },
      onClose: () => {
        w.closed += 1;
      },
    }),
  );
  return { container: mounted.container, w };
}

const prod = () => project({ name: "envs/prod", path: "/w/acme-infra/envs/prod" });

test("a prod project needs its folder name typed", async () => {
  const { container, w } = await dialog({ project: prod() });
  const apply = () => button(container, "Apply") as HTMLButtonElement;
  assert.match(text(container), /Type prod to continue\./);
  assert.equal(apply().disabled, true);
  await type(container.querySelector("input"), "pro");
  assert.equal(apply().disabled, true);
  await type(container.querySelector("input"), "prod");
  assert.equal(apply().disabled, false);
  await click(apply());
  assert.equal(w.applied, 1);
  const body = text(container);
  assert.match(body, /\/w\/acme-infra\/envs\/prod/);
  assert.match(body, /default/);
  assert.match(body, /Applies the saved plan file/);
});

test("a workspace that names production asks for the word too", async () => {
  const { container } = await dialog({ plan: plan({ workspace: "prod-eu" }) });
  assert.match(text(container), /Type dev to continue\./);
  assert.equal((button(container, "Apply") as HTMLButtonElement).disabled, true);
});

test("a project that is not prod applies with one click", async () => {
  const { container, w } = await dialog();
  assert.ok(!(container.querySelector("input")), "the element is not there");
  await click(button(container, "Apply"));
  assert.equal(w.applied, 1);
  await click(button(container, "Cancel"));
  assert.equal(w.closed, 1);
});

test("destroys show in the red box, ten at most", async () => {
  const destroys = Array.from({ length: 13 }, (_, i) => `aws_s3_bucket.b${String(i).padStart(2, "0")}`);
  const { container } = await dialog({ plan: plan({ destroys, counts: counts({ destroy: 13 }) }) });
  const box = container.querySelector(".tf-destroys")!;
  assert.match(text(box), /13 resources will be destroyed\./);
  assert.equal(box.querySelectorAll("li").length, 10);
  assert.match(text(box), /and 3 more/);
  assert.ok(button(container, "Apply")?.classList.contains("tf-danger"));
  await mounted?.unmount();
  mounted = null;
  const none = await dialog();
  assert.ok(!(none.container.querySelector(".tf-destroys")), "the element is not there");
  assert.ok(!button(none.container, "Apply")?.classList.contains("tf-danger"));
});

test("a targeted plan names its target", async () => {
  const { container } = await dialog({ plan: plan({ target: "module.eks" }) });
  assert.match(text(container), /Target: module\.eks/);
});

test("a stale refusal says to plan again and turns Apply off", async () => {
  const { container } = await dialog({ refusal: "stale" });
  assert.equal(text(container.querySelector(".tf-notice")!), "The code changed after this plan. Plan again.");
  assert.equal((button(container, "Apply") as HTMLButtonElement).disabled, true);
});

// ---- Opening the dialog ----

function Host({ w }: { w: World }) {
  const { openApply, dialog: shown } = useApply(w.ctx, w.store);
  return createElement(Fragment, null, createElement("button", { type: "button", onClick: () => openApply(DEV) }, "open"), shown);
}

test("the confirmation reads the plan again, applies, and closes", async () => {
  const w = world([project()]);
  mounted = await mount(createElement(Host, { w }));
  assert.ok(!(mounted.container.querySelector(".tf-apply")), "the element is not there");
  await click(button(mounted.container, "open"));
  assert.equal(w.planCalls, 1);
  assert.ok(mounted.container.querySelector(".tf-apply"));
  await click(button(mounted.container.querySelector(".tf-apply")!, "Apply"));
  assert.deepEqual(w.runs, [[DEV, "apply", null, AT]], "the apply names the plan the dialog showed, by its time");
  assert.ok(!(mounted.container.querySelector(".tf-apply")), "the element is not there");
});

test("a stale refusal keeps the dialog open and says to plan again", async () => {
  const w = world([project()]);
  w.refuse = "stale";
  mounted = await mount(createElement(Host, { w }));
  await click(button(mounted.container, "open"));
  await click(button(mounted.container.querySelector(".tf-apply")!, "Apply"));
  const shown = mounted.container.querySelector(".tf-apply")!;
  assert.ok(shown);
  assert.equal(text(shown.querySelector(".tf-notice")!), "The code changed after this plan. Plan again.");
  assert.equal((button(shown, "Apply") as HTMLButtonElement).disabled, true);
});

test("without a plan to apply the confirmation does not open", async () => {
  const w = world([project()], null);
  mounted = await mount(createElement(Host, { w }));
  await click(button(mounted.container, "open"));
  assert.ok(!(mounted.container.querySelector(".tf-apply")), "the element is not there");
});
