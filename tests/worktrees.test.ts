import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import type { AgentState } from "../shared/protocol.ts";
import { agentFolders, createReadCache } from "../src/agents.ts";
import {
  agentEntries,
  fetchWorktrees,
  WORKTREES_MAX_AGE,
  worktreesPath,
  type AgentEntry,
  type PullRequest,
  type RepoInfo,
  type Worktree,
} from "../src/worktrees.ts";
import { createDashboard, dashboardReducer, type Dashboard } from "../src/workspace.ts";

const pr = (number: number, extra: Partial<PullRequest> = {}): PullRequest => ({
  number,
  title: `PR ${number}`,
  url: `https://github.com/o/r/pull/${number}`,
  draft: false,
  checks: "passing",
  failed: 0,
  ...extra,
});
const tree = (path: string, branch: string | null, pull: PullRequest | null = null): Worktree => ({
  path,
  branch,
  head: "abc1234",
  pr: pull,
});

test("the worktrees path follows a remote window's server", () => {
  assert.equal(worktreesPath("/a b", null), "/api/agents/worktrees?dir=%2Fa+b");
  assert.equal(worktreesPath("/r", "me@host"), "/remote/me%40host/api/agents/worktrees?dir=%2Fr");
  assert.equal(worktreesPath("C:\\Users\\p\\app", null), "/api/agents/worktrees?dir=C%3A%5CUsers%5Cp%5Capp");
});

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});
const answer = (body: unknown, ok = true) => {
  globalThis.fetch = (async () => ({ ok, json: async () => body })) as unknown as typeof fetch;
};

test("a repository's answer keeps its good worktrees and pull requests", async () => {
  answer({
    root: "/r",
    name: "o/r",
    current: "/wt/x",
    prs: true,
    worktrees: [
      tree("/r", "main"),
      tree("/wt/x", "feat/x", pr(54, { checks: "failing", failed: 2, draft: true })),
      { path: "/wt/loose", branch: null, head: "ccccccc", pr: null },
      { path: 7, branch: "bad", head: "x", pr: null },
      { path: "/wt/no-head", branch: "bad", pr: null },
      null,
    ],
  });
  assert.deepEqual(await fetchWorktrees("/wt/x"), {
    root: "/r",
    name: "o/r",
    current: "/wt/x",
    prs: true,
    worktrees: [
      tree("/r", "main"),
      tree("/wt/x", "feat/x", pr(54, { checks: "failing", failed: 2, draft: true })),
      { path: "/wt/loose", branch: null, head: "ccccccc", pr: null },
    ],
  });
});

test("a pull request that is not whole, or does not open over https, is dropped", async () => {
  const bad = [
    { ...pr(1), url: "javascript:alert(1)" },
    { ...pr(2), url: "http://github.com/o/r/pull/2" },
    { ...pr(3), checks: "green" },
    { ...pr(4), number: "4" },
    { ...pr(5), title: null },
    { ...pr(6), failed: Number.NaN },
    { ...pr(7), draft: "no" },
    "pr",
  ];
  answer({
    root: "/r",
    name: null,
    current: null,
    prs: true,
    worktrees: bad.map((pull, index) => ({ ...tree(`/wt/${index}`, `b${index}`), pr: pull })),
  });
  const info = await fetchWorktrees("/r");
  assert.equal(info?.worktrees.length, bad.length, "the worktree stays");
  assert.deepEqual(
    info?.worktrees.map((worktree) => worktree.pr),
    bad.map(() => null),
  );
  assert.equal(info?.name, null);
  assert.equal(info?.current, null);
});

test("a folder outside Git, a failed call, and an odd answer give null", async () => {
  answer({ error: "This folder is not in a Git worktree" }, false);
  assert.equal(await fetchWorktrees("/tmp"), null);
  for (const body of [{}, null, { root: "/r", name: null, current: null, prs: "yes", worktrees: [] }, { root: 1, prs: true, worktrees: [] }]) {
    answer(body);
    assert.equal(await fetchWorktrees("/r"), null, JSON.stringify(body));
  }
  globalThis.fetch = (async () => {
    throw new Error("down");
  }) as unknown as typeof fetch;
  assert.equal(await fetchWorktrees("/r"), null);
});

const agent = (status: AgentState["status"], cwd: string): AgentState => ({
  kind: "claude",
  status,
  since: null,
  started: null,
  conversation: null,
  resume: null,
  cwd,
  version: null,
  model: null,
});

/** One shell in each folder, in this order; `agents` names the folders whose shell runs an agent, with its status. */
function shells(folders: string[], agents: Record<string, AgentState["status"]> = {}): Dashboard {
  let state = createDashboard("w1", "t1");
  folders.forEach((cwd, index) => {
    const id = `s${index}`;
    state = dashboardReducer(state, { type: "open", id, kind: "shell" });
    state = dashboardReducer(state, { type: "place", id, place: { cwd, branch: null } });
    if (agents[cwd]) state = dashboardReducer(state, { type: "agent", id, agent: agent(agents[cwd], cwd) });
  });
  return state;
}

const westerm = (current: string): RepoInfo => ({
  root: "/Users/p/westerm",
  name: "SteelCrab/westerm",
  current,
  prs: true,
  worktrees: [
    tree("/Users/p/westerm", "main"),
    tree("/Users/p/wt/pr-54", "dependabot/markdown-it", pr(54, { checks: "failing", failed: 1 })),
    tree("/Users/p/wt/terraform", "feat/terraform-plugin"),
    tree("/Users/p/wt/hole-punch", "feat/hole-punch"),
    tree("/Users/p/wt/docker", "feat/docker-plugin", pr(42)),
  ],
});
const entries = (state: Dashboard, infos: Record<string, RepoInfo>): AgentEntry[] =>
  agentEntries(agentFolders(state), (path) => infos[path]);
const shape = (list: AgentEntry[]) =>
  list.map((entry) =>
    entry.kind === "folder"
      ? [entry.folder.key, entry.worktree?.branch ?? null]
      : [entry.repo.key, entry.repo.folders.map(({ folder, worktree }) => [folder.key, worktree?.branch ?? null])],
  );

test("folders without a known repository stay as they are", () => {
  const state = shells(["/Users/p/app", "/tmp"]);
  assert.deepEqual(shape(entries(state, {})), [
    ["/Users/p/app", null],
    ["/tmp", null],
  ]);
});

test("a repository with one worktree stays a folder, with its pull request", () => {
  const info: RepoInfo = { root: "/Users/p/app", name: "o/app", current: "/Users/p/app", prs: true, worktrees: [tree("/Users/p/app", "fix", pr(9))] };
  const [entry] = entries(shells(["/Users/p/app"]), { "/Users/p/app": info });
  assert.equal(entry.kind, "folder");
  assert.deepEqual(entry.kind === "folder" && [entry.worktree?.pr?.number, entry.prs], [9, true]);
});

test("the folders of a repository with more worktrees sit under it, where its first folder was", () => {
  const state = shells(["/tmp", "/Users/p/wt/pr-54", "/Users/p/other", "/Users/p/wt/terraform/native"], {
    "/Users/p/wt/pr-54": "working",
  });
  const list = entries(state, {
    "/Users/p/wt/pr-54": westerm("/Users/p/wt/pr-54"),
    // A shell below the top of its worktree: the server names the worktree.
    "/Users/p/wt/terraform/native": westerm("/Users/p/wt/terraform"),
  });
  assert.deepEqual(shape(list), [
    ["/tmp", null],
    [
      "/Users/p/westerm",
      [
        ["/Users/p/wt/pr-54", "dependabot/markdown-it"],
        ["/Users/p/wt/terraform/native", "feat/terraform-plugin"],
      ],
    ],
    ["/Users/p/other", null],
  ]);
  const repo = list[1].kind === "repo" ? list[1].repo : assert.fail("a repository");
  assert.equal(repo.name, "SteelCrab/westerm");
  assert.equal(repo.count, 5);
  assert.equal(repo.prs, true);
  assert.equal(repo.waiting, false);
  assert.equal(repo.folders[0].worktree?.pr?.number, 54);
});

test("worktrees without a shell are listed apart, those with a pull request first", () => {
  const state = shells(["/Users/p/wt/pr-54"]);
  const [entry] = entries(state, { "/Users/p/wt/pr-54": westerm("/Users/p/wt/pr-54") });
  const repo = entry.kind === "repo" ? entry.repo : assert.fail("a repository");
  assert.deepEqual(
    repo.idle.map((worktree) => worktree.branch),
    ["feat/docker-plugin", "main", "feat/terraform-plugin", "feat/hole-punch"],
  );
});

test("a repository waits when one of its folders does, and takes its folder's name without a remote", () => {
  const info: RepoInfo = { ...westerm("/Users/p/wt/pr-54"), name: null };
  const state = shells(["/Users/p/wt/pr-54"], { "/Users/p/wt/pr-54": "waiting" });
  const [entry] = entries(state, { "/Users/p/wt/pr-54": info });
  const repo = entry.kind === "repo" ? entry.repo : assert.fail("a repository");
  assert.equal(repo.waiting, true);
  assert.equal(repo.name, "westerm");
});

test("Windows folders group the same way", () => {
  const root = "C:\\Users\\p\\westerm";
  const linked = "C:\\Users\\p\\wt\\fix";
  const info: RepoInfo = { root, name: null, current: linked, prs: false, worktrees: [tree(root, "main"), tree(linked, "fix/windows", pr(58))] };
  const [entry] = entries(shells([linked]), { [linked]: info });
  const repo = entry.kind === "repo" ? entry.repo : assert.fail("a repository");
  assert.equal(repo.name, "westerm");
  assert.deepEqual(
    repo.folders.map(({ worktree }) => worktree?.pr?.number),
    [58],
  );
  assert.deepEqual(
    repo.idle.map((worktree) => worktree.path),
    [root],
  );
});

test("a read is kept for its age, shared while it runs, and kept when the next one fails", async () => {
  let now = 0;
  let calls = 0;
  let fail = false;
  const cache = createReadCache<string>(
    async (path) => {
      calls += 1;
      return fail ? null : `${path}#${calls}`;
    },
    WORKTREES_MAX_AGE,
    () => now,
  );
  assert.equal(cache.shown("/r"), undefined);
  const [first, second] = await Promise.all([cache.load("/r", ""), cache.load("/r", "")]);
  assert.deepEqual([first, second, calls], ["/r#1", "/r#1", 1]);
  now = WORKTREES_MAX_AGE - 1;
  assert.equal(await cache.load("/r", ""), "/r#1");
  now = WORKTREES_MAX_AGE;
  assert.equal(await cache.load("/r", ""), "/r#2", "read again after 30 s");
  assert.equal(WORKTREES_MAX_AGE, 30_000);
  fail = true;
  now = 2 * WORKTREES_MAX_AGE;
  assert.equal(await cache.load("/r", ""), null);
  assert.equal(cache.shown("/r"), "/r#2", "a failed read keeps what was shown");
});
