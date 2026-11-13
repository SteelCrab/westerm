import "./dom-page.ts";
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { TERRAFORM_COLOR, TERRAFORM_ICON_URL } from "../src/plugins/terraform/icon.tsx";
import { terraformPlugin } from "../src/plugins/terraform/index.tsx";
import { TF_TEXT } from "../src/plugins/terraform/i18n.ts";
import { createStore, type TerraformStore } from "../src/plugins/terraform/store.ts";
import { TerraformPanel } from "../src/plugins/terraform/TerraformPanel.tsx";
import type { Counts, PlanMark, ProjectInfo, StateAnswer, TreeAnswer, TreeNode } from "../src/plugins/terraform/types.ts";
import { availableBuiltins } from "../src/plugins/installed.ts";
import { BUILTIN_PLUGINS } from "../src/plugins/registry.ts";
import type { OpenPaneOptions, OpenShellOptions, PluginContext } from "../src/plugins/types.ts";
import { LOCALES } from "../src/i18n/messages.ts";
import { entries } from "../src/plugins/managerModel.ts";
import { click, mount, settle, text, type Mounted } from "./dom-page.ts";

let mounted: Mounted | null = null;
let stores: TerraformStore[] = [];
afterEach(async () => {
  await mounted?.unmount();
  mounted = null;
  for (const store of stores) store.dispose();
  stores = [];
});

// ---- Fixtures ----

const DEV = "0123456789abcdef";
const PROD = "fedcba9876543210";
const TOKEN = "a".repeat(32);

const counts = (over: Partial<Counts> = {}): Counts => ({ add: 0, change: 0, destroy: 0, import: 0, forget: 0, ...over });

function project(over: Partial<ProjectInfo> = {}): ProjectInfo {
  return {
    id: DEV,
    path: "/w/envs/dev",
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

const plan = (state: NonNullable<ProjectInfo["plan"]>["state"], over: Partial<Counts> = { add: 2, change: 1 }): ProjectInfo["plan"] => ({
  state,
  counts: counts(over),
  applyable: true,
  at: 1000,
  target: null,
});

const moduleNode = (address: string, over: Partial<Extract<TreeNode, { kind: "module" }>> = {}, children: TreeNode[] = []): TreeNode => ({
  kind: "module",
  name: address.split(".").at(-1)!,
  address,
  source: "local",
  label: "local",
  newer: null,
  dir: `/w/envs/dev/${address.split(".").at(-1)}`,
  children,
  ...over,
});

const resourceNode = (address: string): TreeNode => {
  const parts = address.split(".");
  return { kind: "resource", address, type: parts.at(-2)!, name: parts.at(-1)! };
};

const TREE: TreeNode[] = [
  moduleNode("module.eks", {}, [resourceNode("module.eks.aws_eks_cluster.this")]),
  moduleNode("module.vpc", { source: "registry", label: "5.13.0", newer: "5.16.0", dir: null }, [resourceNode("module.vpc.aws_subnet.private")]),
  resourceNode("aws_route53_record.app"),
];

/** What the fake server holds; a test changes it between polls. */
interface Server {
  state: StateAnswer;
  marks: PlanMark[];
  /** The answer to a new run: a token, or a refusal code. */
  run: { token: string } | { error: string };
  workspaces: { workspaces: string[]; current: string } | { error: string };
  gone: boolean;
  calls: { method: string; path: string; body: unknown }[];
  shells: OpenShellOptions[];
  panes: OpenPaneOptions[];
  folders: [string, string | null][];
  refreshed: number;
  ctx: PluginContext;
}

const json = (status: number, body?: unknown) => new Response(body === undefined ? null : JSON.stringify(body), { status });

function server(projects: ProjectInfo[], over: Partial<StateAnswer> = {}): Server {
  const result: Server = {
    state: { tools: { terraform: true, tofu: false }, shellPanes: true, checkVersions: true, projects, truncated: false, ...over },
    marks: [],
    run: { token: TOKEN },
    workspaces: { workspaces: ["default", "dev", "prod-eu"], current: "default" },
    gone: false,
    calls: [],
    shells: [],
    panes: [],
    folders: [],
    refreshed: 0,
    ctx: null as unknown as PluginContext,
  };
  result.ctx = {
    openShell: (options) => result.shells.push(options),
    openFolder: (cwd, name) => result.folders.push([cwd, name]),
    openBrowser() {},
    openPane: (options) => result.panes.push(options),
    folders: () => ["/w"],
    fetch: async (path, init) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : null;
      result.calls.push({ method, path, body });
      if (result.gone) return json(404);
      if (path.startsWith("/state")) return json(200, result.state);
      if (path.endsWith("/tree")) {
        const answer: TreeAnswer = { nodes: TREE, truncated: false, marks: result.marks, version: "1.9.8" };
        return json(200, answer);
      }
      if (path.endsWith("/runs")) return "token" in result.run ? json(200, { token: result.run.token, name: "envs/dev plan" }) : json(409, result.run);
      if (path.endsWith("/workspaces")) return "error" in result.workspaces ? json(502, result.workspaces) : json(200, result.workspaces);
      return json(204);
    },
    refreshInstalled: () => {
      result.refreshed += 1;
    },
    openManager() {},
  };
  return result;
}

/** A store that has read the state once. */
async function ready(s: Server): Promise<TerraformStore> {
  const store = createStore(s.ctx);
  stores.push(store);
  await settle();
  return store;
}

/** The panel, open, over a store that has read the state. */
async function panel(s: Server, openApply: (id: string) => void = () => {}): Promise<{ container: HTMLElement; store: TerraformStore }> {
  const store = await ready(s);
  store.setPanelOpen(true);
  mounted = await mount(createElement(TerraformPanel, { ctx: s.ctx, store, openApply }));
  await settle();
  return { container: mounted.container, store };
}

/** One poll, then the page settles. */
async function poll(store: TerraformStore): Promise<void> {
  store.refresh();
  await settle();
}

const button = (root: Element, label: string) =>
  [...root.querySelectorAll("button")].find((each) => each.getAttribute("aria-label") === label || text(each) === label);

const row = (root: Element, name: string) => [...root.querySelectorAll(".tf-row")].find((each) => each.querySelector(".tf-name")?.textContent === name);

const calls = (s: Server, part: string) => s.calls.filter((call) => call.path.includes(part));

// ---- The plugin ----

test("the plugin is registered after the databases plugin, with the brand icon", () => {
  assert.deepEqual(
    BUILTIN_PLUGINS.map((plugin) => plugin.id),
    ["git", "docker", "db", "terraform"],
  );
  assert.equal(terraformPlugin.version, "0.1.0");
  assert.equal(TERRAFORM_COLOR, "#844FBA");
  assert.equal(terraformPlugin.iconUrl, TERRAFORM_ICON_URL);
  assert.ok(TERRAFORM_ICON_URL.startsWith("data:image/svg+xml,"));
  const svg = decodeURIComponent(TERRAFORM_ICON_URL.slice("data:image/svg+xml,".length));
  assert.ok(svg.includes('fill="#844FBA"'), svg);
  assert.ok(svg.includes('viewBox="0 0 24 24"'));
  for (const locale of LOCALES) {
    assert.equal(terraformPlugin.text[locale].name, "Terraform");
    assert.equal(terraformPlugin.text[locale].permissions.length, 4);
  }
  assert.deepEqual(terraformPlugin.text.en.permissions, [
    "Read `.tf` files in your session folders and the folders below them",
    "Run `terraform` or `tofu` in terminal panes: init, validate, plan, and apply",
    "Keep plan files in westerm's private state folder",
    "Ask the public module registry for new versions (you can turn this off)",
  ]);
  assert.equal(TF_TEXT.en.empty, "No Terraform projects in your open folders. Open a shell in a folder that has `.tf` files.");
});

test("a build without the terraform plugin hides it from the manager", () => {
  assert.deepEqual(
    availableBuiltins(BUILTIN_PLUGINS, ["git", "docker", "db"]).map((plugin) => plugin.id),
    ["git", "docker", "db"],
  );
  assert.ok(availableBuiltins(BUILTIN_PLUGINS, ["git", "docker", "db", "terraform"]).some((plugin) => plugin.id === "terraform"));
});

test("the plugin manager lists the plugin with its logo", () => {
  const listed = entries("browse", BUILTIN_PLUGINS, null, [], [], "en");
  assert.equal(listed.find((entry) => entry.id === "terraform")?.iconUrl, TERRAFORM_ICON_URL);
  assert.equal(listed.find((entry) => entry.id === "docker")?.iconUrl, null, "a plugin without an image keeps its drawn icon");
});

// ---- The store ----

test("the store reads the state at start with a scan, then without", async () => {
  const s = server([project()]);
  const store = await ready(s);
  assert.equal(store.state()?.projects.length, 1);
  assert.deepEqual(
    calls(s, "/state").map((call) => call.path),
    ["/state?scan=1&folder=%2Fw"],
  );
  await poll(store);
  await poll(store);
  assert.equal(calls(s, "scan=1").length, 1, "a poll does not walk the folders");
  store.setPanelOpen(true);
  await settle();
  assert.equal(calls(s, "scan=1").length, 2, "opening the panel does");
  store.refresh(true);
  await settle();
  assert.equal(calls(s, "scan=1").length, 3, "and so does Refresh");
});

test("the badge adds the changes of current plans, and hides at zero", async () => {
  const s = server([project({ plan: plan("current", { add: 2, change: 1 }) }), project({ id: PROD, name: "envs/prod", plan: plan("stale", { add: 5 }) })]);
  const store = await ready(s);
  assert.deepEqual(store.status(), { badge: 3, alert: false });
  s.state = { ...s.state, projects: [project()] };
  await poll(store);
  assert.deepEqual(store.status(), { badge: null, alert: false });
});

test("a run opens the project's run pane in place of its last one", async () => {
  const s = server([project()]);
  const store = await ready(s);
  assert.equal(await store.run(DEV, "plan", null), null);
  assert.deepEqual(s.shells, [{ action: "run", target: TOKEN, cwd: "/w/envs/dev", name: "envs/dev plan", reuse: false, replaceKey: `run:${DEV}` }]);
  assert.deepEqual(calls(s, "/runs")[0], { method: "POST", path: `/projects/${DEV}/runs`, body: { action: "plan" } });
});

test("a refused run opens no pane and gives its code", async () => {
  const s = server([project()]);
  s.run = { error: "stale" };
  const store = await ready(s);
  assert.equal(await store.run(DEV, "apply", null, 1000), "stale");
  assert.deepEqual(s.shells, []);
  assert.deepEqual(calls(s, "/runs")[0]?.body, { action: "apply", planAt: 1000 }, "the apply names its plan");
});

test("a finished plan opens the plan pane once", async () => {
  const s = server([project()]);
  const store = await ready(s);
  await store.run(DEV, "plan", null);
  s.state = { ...s.state, projects: [project({ run: { action: "plan", state: "running", at: 1 } })] };
  await poll(store);
  assert.deepEqual(s.panes, []);
  s.state = { ...s.state, projects: [project({ run: { action: "plan", state: "ok", at: 2 }, plan: plan("current") })] };
  await poll(store);
  assert.deepEqual(s.panes, [{ view: "plan", title: "Plan · envs/dev", data: { projectId: DEV, path: "/w/envs/dev" }, reuseKey: `plan:${DEV}` }]);
  await poll(store);
  assert.equal(s.panes.length, 1, "a later poll does not open it again");
});

test("a plan this window did not start, a failed one, and a remote one open no pane", async () => {
  const s = server([project({ run: { action: "plan", state: "running", at: 1 } })]);
  const store = await ready(s);
  s.state = { ...s.state, projects: [project({ run: { action: "plan", state: "ok", at: 2 }, plan: plan("current") })] };
  await poll(store);
  assert.deepEqual(s.panes, [], "started in another window");

  await store.run(DEV, "plan", null);
  s.state = { ...s.state, projects: [project({ run: { action: "plan", state: "failed", at: 3 } })] };
  await poll(store);
  s.state = { ...s.state, projects: [project({ run: { action: "plan", state: "ok", at: 4 }, plan: plan("current") })] };
  await poll(store);
  assert.deepEqual(s.panes, [], "the failed plan ended what this window started");

  s.state = { ...s.state, projects: [project({ remote: true })] };
  await poll(store);
  await store.run(DEV, "plan", null);
  s.state = { ...s.state, projects: [project({ remote: true, run: { action: "plan", state: "ok", at: 5 } })] };
  await poll(store);
  assert.deepEqual(s.panes, []);
});

test("a failed run shows the red dot only while the panel is closed", async () => {
  const running = project({ run: { action: "apply", state: "running", at: 1 } });
  const failed = project({ run: { action: "apply", state: "failed", at: 2 } });
  const s = server([running]);
  const store = await ready(s);
  s.state = { ...s.state, projects: [failed] };
  await poll(store);
  assert.equal(store.status().alert, true);
  store.setPanelOpen(true);
  await settle();
  assert.equal(store.status().alert, false, "opening the panel clears it");
  // The same failure while the panel is open raises nothing.
  s.state = { ...s.state, projects: [running] };
  await poll(store);
  s.state = { ...s.state, projects: [{ ...failed, run: { action: "apply", state: "interrupted", at: 3 } }] };
  await poll(store);
  assert.equal(store.status().alert, false);
});

test("the end of a run reads the folders again", async () => {
  const s = server([project({ run: { action: "init", state: "running", at: 1 } })]);
  const store = await ready(s);
  const before = calls(s, "scan=1").length;
  s.state = { ...s.state, projects: [project({ run: { action: "init", state: "ok", at: 2 } })] };
  await poll(store);
  await settle();
  assert.equal(calls(s, "scan=1").length, before + 1);
});

test("a gone plugin stops asking and refreshes the installed list", async () => {
  const s = server([project()]);
  const store = await ready(s);
  s.gone = true;
  await poll(store);
  assert.equal(s.refreshed, 1);
  const asked = calls(s, "/state").length;
  await poll(store);
  assert.equal(calls(s, "/state").length, asked, "it asks no more");
});

// ---- The panel ----

test("one project is open at first, several are closed", async () => {
  const one = await panel(server([project()]));
  assert.ok(row(one.container, "module.eks"), "its tree shows");
  assert.match(text(one.container.querySelector(".tf-header")!), /1 project/);
  await mounted?.unmount();
  mounted = null;

  const s = server([project(), project({ id: PROD, name: "envs/prod", path: "/w/envs/prod" })]);
  const two = await panel(s);
  assert.ok(!(row(two.container, "module.eks")), "the element is not there");
  assert.equal(calls(s, "/tree").length, 0, "no tree is read before a project opens");
  assert.match(text(two.container.querySelector(".tf-header")!), /2 projects/);
  await click(row(two.container, "envs/prod")?.querySelector(".tf-toggle"));
  assert.equal(calls(s, `/projects/${PROD}/tree`).length, 1);
  assert.match(text(two.container.querySelector(".tf-footer")!), /envs\/prod · default/);
  assert.match(text(two.container.querySelector(".tf-footer")!), /terraform 1\.9\.8/);
});

test("a project that shows up later opens when it is the only one", async () => {
  // The first scan finds nothing (the shell is in another folder); then the user moves into a project.
  const s = server([]);
  const { container, store } = await panel(s);
  assert.ok(container.querySelector(".tf-empty"));
  s.state = { ...s.state, projects: [project()] };
  await poll(store);
  assert.ok(row(container, "module.eks"), "the only project is open");
  // Once the user closes it, it stays closed.
  await click(row(container, "envs/dev")?.querySelector(".tf-toggle"));
  await poll(store);
  assert.ok(!(row(container, "module.eks")), "the element is not there");
});

test("Plan starts a run for the selected project", async () => {
  const s = server([project()]);
  const { container } = await panel(s);
  await click(button(container.querySelector(".tf-runbar")!, "Plan"));
  assert.deepEqual(calls(s, "/runs")[0]?.body, { action: "plan" });
  assert.equal(s.shells[0]?.replaceKey, `run:${DEV}`);
});

test("a module's buttons plan it alone and open its folder", async () => {
  const s = server([project()]);
  const { container } = await panel(s);
  const eks = row(container, "module.eks")!;
  await click(button(eks, "Plan this module only"));
  assert.deepEqual(calls(s, "/runs")[0]?.body, { action: "plan", target: "module.eks" });
  await click(button(eks, "Open a terminal in the module folder"));
  assert.deepEqual(s.folders, [["/w/envs/dev/eks", "eks"]]);
  // A module whose folder is not known has the button off.
  const vpc = row(container, "module.vpc")!;
  assert.equal((button(vpc, "Open a terminal in the module folder") as HTMLButtonElement).disabled, true);
  assert.match(text(vpc), /5\.13\.0/);
  assert.match(text(vpc), /↑ 5\.16\.0/);
});

test("Init is the primary button when the project needs init", async () => {
  const { container } = await panel(server([project({ needsInit: true })]));
  const bar = container.querySelector(".tf-runbar")!;
  assert.ok(button(bar, "Init")?.classList.contains("primary"));
  assert.ok(!button(bar, "Plan")?.classList.contains("primary"));
  assert.match(text(row(container, "envs/dev")!), /init needed/);
  await mounted?.unmount();
  mounted = null;
  const plain = await panel(server([project()]));
  assert.ok(button(plain.container.querySelector(".tf-runbar")!, "Plan")?.classList.contains("primary"));
});

test("the run buttons are off while a run is running, and the bar says so", async () => {
  const { container } = await panel(server([project({ run: { action: "plan", state: "running", at: 1 } })]));
  const bar = container.querySelector(".tf-runbar")!;
  assert.match(text(bar), /Plan running…/);
  assert.ok(!(button(bar, "Plan")), "the element is not there");
  assert.ok(row(container, "envs/dev")?.querySelector(".tf-dot-running"));
  assert.equal((button(row(container, "module.eks")!, "Plan this module only") as HTMLButtonElement).disabled, true);
});

test("Apply is off for a stale plan and the row says Code changed", async () => {
  const applied: string[] = [];
  const s = server([project({ plan: plan("stale") })]);
  const { container, store } = await panel(s, (id) => applied.push(id));
  const bar = container.querySelector(".tf-runbar")!;
  assert.equal((button(bar, "Apply…") as HTMLButtonElement).disabled, true);
  assert.match(text(row(container, "envs/dev")!), /Code changed/);
  assert.ok(container.querySelector(".tf-sum.stale"), "the numbers are struck through");
  // A current plan turns it on, and it opens the confirmation.
  s.state = { ...s.state, projects: [project({ plan: plan("current") })] };
  await poll(store);
  assert.match(text(container.querySelector(".tf-sum")!), /\+2 ~1 -0/);
  await click(button(bar, "Apply…"));
  assert.deepEqual(applied, [DEV]);
  assert.equal(calls(s, "/runs").length, 0, "the confirmation starts the run, not the button");
});

test("the numbers of a plan that was applied do not stay on the row", async () => {
  const { container } = await panel(server([project({ plan: plan("applied") })]));
  assert.ok(!(container.querySelector(".tf-sum")), "the element is not there");
  await mounted?.unmount();
  mounted = null;
  const failed = await panel(server([project({ plan: plan("applyFailed") })]));
  assert.ok(!(failed.container.querySelector(".tf-sum")), "the element is not there");
});

test("a remote project has no Apply and says why", async () => {
  const { container } = await panel(server([project({ remote: true, plan: null })]));
  const apply = button(container.querySelector(".tf-runbar")!, "Apply…") as HTMLButtonElement;
  assert.equal(apply.disabled, true);
  assert.equal(apply.title, "This project runs remotely. Apply it from a terminal or from HCP Terraform.");
  assert.match(text(row(container, "envs/dev")!), /cloud/);
});

test("project rows show production and the tool", async () => {
  const { container } = await panel(server([project({ id: PROD, name: "envs/prod", path: "/w/envs/prod" }), project({ tool: "tofu" })], { tools: { terraform: true, tofu: true } }));
  assert.match(text(row(container, "envs/prod")!), /prod/);
  assert.ok(row(container, "envs/prod")?.querySelector(".tf-logo"));
  assert.ok(row(container, "envs/dev")?.querySelector(".tf-tofu"));
  assert.ok(!(row(container, "envs/dev")?.querySelector(".tf-tag-prod")), "the element is not there");
});

test("resource marks and removed rows show in the tree", async () => {
  const s = server([project({ plan: plan("current") })]);
  s.marks = [
    { module: "module.vpc", type: "aws_subnet", name: "private", mark: "update", count: 3 },
    { module: "", type: "aws_route53_record", name: "app", mark: "create", count: 1 },
    { module: "module.eks", type: "aws_iam_role", name: "old", mark: "delete", count: 1 },
  ];
  const { container } = await panel(s);
  const subnet = row(container, "aws_subnet.private")!;
  assert.equal(subnet.querySelector(".tf-mark")?.textContent, "~");
  assert.match(text(subnet), /×3/);
  assert.equal(row(container, "aws_route53_record.app")?.querySelector(".tf-mark")?.textContent, "+");
  const old = row(container, "aws_iam_role.old")!;
  assert.ok(old.classList.contains("removed"));
  assert.equal(old.querySelector(".tf-mark")?.textContent, "-");
  assert.ok(!(row(container, "aws_eks_cluster.this")?.querySelector(".tf-mark")), "the element is not there");
  // A marked resource opens the plan pane at its change.
  await click(subnet.querySelector("button"));
  assert.deepEqual(s.panes, [{ view: "plan", title: "Plan · envs/dev", data: { projectId: DEV, path: "/w/envs/dev", focus: "module.vpc.aws_subnet.private" }, reuseKey: `plan:${DEV}` }]);
});

test("the tree is read again when the project's plan changes", async () => {
  const s = server([project()]);
  const { store } = await panel(s);
  assert.equal(calls(s, "/tree").length, 1);
  await poll(store);
  assert.equal(calls(s, "/tree").length, 1, "nothing changed");
  s.state = { ...s.state, projects: [project({ plan: plan("current") })] };
  await poll(store);
  assert.equal(calls(s, "/tree").length, 2);
});

test("the tree is read once more a little later, for a newer module version", async () => {
  // The registry answers behind the first tree answer.
  const s = server([project()]);
  const store = await ready(s);
  store.setPanelOpen(true);
  mounted = await mount(createElement(TerraformPanel, { ctx: s.ctx, store, openApply: () => {}, secondLookMs: 30 }));
  await settle();
  assert.equal(calls(s, "/tree").length, 1);
  // Polls in between give the page a new state object each time.
  await poll(store);
  await poll(store);
  await new Promise((resolve) => setTimeout(resolve, 60));
  await settle();
  assert.equal(calls(s, "/tree").length, 2);
  await new Promise((resolve) => setTimeout(resolve, 60));
  await settle();
  assert.equal(calls(s, "/tree").length, 2, "once");
});

test("the filter keeps the rows that match", async () => {
  const { container } = await panel(server([project()]));
  const { type } = await import("./dom-page.ts");
  await type(container.querySelector('input[type="search"]'), "subnet");
  assert.ok(row(container, "aws_subnet.private"));
  assert.ok(row(container, "module.vpc"), "the module above stays");
  assert.ok(!(row(container, "module.eks")), "the element is not there");
});

test("the workspace list is asked only when the picker opens", async () => {
  const s = server([project()]);
  const { container } = await panel(s);
  assert.equal(calls(s, "/workspaces").length, 0);
  await click(container.querySelector(".tf-ws-pick"));
  assert.equal(calls(s, "/workspaces").length, 1);
  await click(button(container, "prod-eu"));
  assert.deepEqual(calls(s, "/workspace").at(-1), { method: "POST", path: `/projects/${DEV}/workspace`, body: { name: "prod-eu" } });
});

test("a failed workspace list shows the tool's line and what to do", async () => {
  const s = server([project()]);
  s.workspaces = { error: "Error: No valid credential sources found" };
  const { container } = await panel(s);
  await click(container.querySelector(".tf-ws-pick"));
  const notice = text(container.querySelector(".tf-notice")!);
  assert.match(notice, /Error: No valid credential sources found/);
  assert.match(notice, /Run `terraform workspace list` in a terminal to see why\./);
});

test("a locked workspace says Set by TF_WORKSPACE", async () => {
  const s = server([project({ workspace: "prod", workspaceLocked: true })]);
  const { container } = await panel(s);
  const pick = container.querySelector(".tf-ws-pick") as HTMLButtonElement;
  assert.equal(pick.disabled, true);
  assert.equal(pick.title, "Set by TF_WORKSPACE");
  assert.match(text(pick), /prod/);
});

test("a refusal shows as one line", async () => {
  const s = server([project()]);
  s.run = { error: "running" };
  const { container } = await panel(s);
  await click(button(container.querySelector(".tf-runbar")!, "Validate"));
  assert.equal(text(container.querySelector(".tf-notice")!), "This project is already running something.");
});

test("no tool shows the install hint and keeps the tree", async () => {
  const { container } = await panel(server([project({ toolFound: false })], { tools: { terraform: false, tofu: false } }));
  const banner = text(container.querySelector(".tf-banner")!);
  assert.match(banner, /terraform not found/);
  assert.match(banner, /brew install hashicorp\/tap\/terraform|developer\.hashicorp\.com\/terraform\/install/);
  assert.ok(row(container, "module.eks"), "the tree is still drawn");
  for (const label of ["Init", "Validate", "Plan", "Apply…"]) {
    assert.equal((button(container.querySelector(".tf-runbar")!, label) as HTMLButtonElement).disabled, true, label);
  }
});

test("no projects shows the empty state", async () => {
  const { container } = await panel(server([]));
  assert.equal(text(container.querySelector(".tf-empty")!), "No Terraform projects in your open folders. Open a shell in a folder that has `.tf` files.");
  assert.ok(!(container.querySelector(".tf-runbar")), "the element is not there");
  assert.ok(!(container.querySelector('input[type="search"]')), "the element is not there");
});

test("Refresh walks the folders again", async () => {
  const s = server([project()]);
  const { container } = await panel(s);
  const before = calls(s, "scan=1").length;
  await click(button(container, "Refresh"));
  assert.equal(calls(s, "scan=1").length, before + 1);
});

test("the more menu turns the version check off and deletes plans after a confirmation", async () => {
  const s = server([project()]);
  const { container } = await panel(s);
  await click(button(container, "More actions"));
  await click([...document.querySelectorAll('[role="menuitem"], [role="menuitemcheckbox"]')].find((item) => text(item).includes("Check for new module versions")));
  assert.deepEqual(calls(s, "/settings").at(-1), { method: "PUT", path: "/settings", body: { checkVersions: false } });
  await click(button(container, "More actions"));
  await click([...document.querySelectorAll('[role="menuitem"]')].find((item) => text(item).includes("Delete saved plans…")));
  assert.equal(calls(s, "/plans").length, 0, "not before the confirmation");
  assert.match(text(container.querySelector(".tf-confirm")!), /Delete all saved plans\?/);
  await click(button(container.querySelector(".tf-confirm")!, "Delete"));
  assert.deepEqual(calls(s, "/plans").at(-1), { method: "DELETE", path: "/plans", body: null });
});
