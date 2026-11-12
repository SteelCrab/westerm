// The agents panel with repositories: worktrees under their repository, the
// pull request of each branch, and the worktrees without a shell.

import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { AgentLaunch, AgentState } from "../shared/protocol.ts";
import { AgentPanel } from "../src/components/AgentPanel.tsx";
import { I18nProvider } from "../src/i18n/index.tsx";
import { WORKTREES_REFRESH, type PullRequest, type RepoInfo, type Worktree } from "../src/worktrees.ts";
import { createDashboard, dashboardReducer, type Dashboard } from "../src/workspace.ts";

const browser = new Window({ url: "http://127.0.0.1/" });
Object.assign(globalThis, {
  window: browser, document: browser.document, localStorage: browser.localStorage,
  HTMLElement: browser.HTMLElement, Event: browser.Event, IS_REACT_ACT_ENVIRONMENT: true,
});
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

let mounted: Root | null = null;
afterEach(async () => {
  if (mounted) await act(async () => mounted!.unmount());
  mounted = null;
  browser.document.body.innerHTML = "";
});

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
const PR_54 = "/Users/p/wt/pr-54";
const TERRAFORM = "/Users/p/wt/terraform";
const westerm = (current: string, prs = true): RepoInfo => ({
  root: "/Users/p/westerm",
  name: "SteelCrab/westerm",
  current,
  prs,
  worktrees: [
    tree("/Users/p/westerm", "main"),
    tree(PR_54, "dependabot/markdown-it", pr(54, { checks: "failing", failed: 1 })),
    tree(TERRAFORM, "feat/terraform-plugin"),
    tree("/Users/p/wt/hole-punch", "feat/hole-punch"),
    tree("/Users/p/wt/docker", "feat/docker-plugin", pr(42, { checks: "running", draft: true })),
  ],
});
const BOTH = { [PR_54]: westerm(PR_54), [TERRAFORM]: westerm(TERRAFORM) };

const agent = (cwd: string): AgentState => ({
  kind: "claude",
  status: "working",
  since: null,
  started: null,
  conversation: null,
  resume: null,
  cwd,
  version: null,
  model: null,
});

/** One shell in each folder; the first runs Claude Code. */
function shells(...folders: string[]): Dashboard {
  let state = createDashboard("w1", "t1");
  folders.forEach((cwd, index) => {
    const id = `s${index}`;
    state = dashboardReducer(state, { type: "open", id, kind: "shell" });
    state = dashboardReducer(state, { type: "place", id, place: { cwd, branch: null } });
    if (index === 0) state = dashboardReducer(state, { type: "agent", id, agent: agent(cwd) });
  });
  return state;
}

const noop = () => {};
interface Mounted {
  /** The folders the panel asked the server about, in order. */
  asked: string[];
  opened: string[];
  started: [string, AgentLaunch][];
}

async function mount(state: Dashboard, infos: Record<string, RepoInfo>, compact = false): Promise<Mounted> {
  const seen: Mounted = { asked: [], opened: [], started: [] };
  const worktrees = async (dir: string) => {
    seen.asked.push(dir);
    return infos[dir] ?? null;
  };
  mounted = createRoot(browser.document.body.appendChild(browser.document.createElement("div")) as unknown as HTMLElement);
  await act(async () =>
    mounted!.render(
      createElement(
        I18nProvider,
        null,
        createElement(AgentPanel, {
          state,
          renaming: null,
          onRename: noop,
          onSelectWorkspace: noop,
          onNewWorkspace: noop,
          onCloseWorkspace: noop,
          onRenameWorkspace: noop,
          onSetWorkspaceIcon: noop,
          onSetWorkspaceColor: noop,
          onFocus: noop,
          onStart: (folder: string, launch: AgentLaunch) => seen.started.push([folder, launch]),
          onOpenUrl: (url: string) => seen.opened.push(url),
          compact,
          history: async () => [],
          worktrees,
        }),
      ),
    ),
  );
  return seen;
}

const all = (selector: string) => [...browser.document.querySelectorAll(selector)] as unknown as HTMLElement[];
const node = (selector: string) => {
  const [found] = all(selector);
  assert.ok(found, selector);
  return found;
};
const text = (selector: string) => all(selector).map((item) => item.textContent ?? "");
async function click(selector: string) {
  await act(async () => node(selector).click());
}

test("the folders of a repository sit under it, each with its branch and pull request", async () => {
  await mount(shells(PR_54, TERRAFORM), BOTH);
  assert.equal(all(".agents-repo").length, 1, "one repository for both folders");
  assert.match(node(".agents-repo").textContent ?? "", /SteelCrab\/westerm/);
  assert.match(node(".agents-repo").textContent ?? "", /5 worktrees/);
  assert.equal(node(".agents-repo-toggle").getAttribute("aria-expanded"), "true", "a repository starts expanded");

  const rows = text(".agents-repo-node .agents-worktree");
  assert.equal(rows.length, 2);
  assert.match(rows[0], /dependabot\/markdown-it/, "the branch is the row's name");
  assert.match(rows[0], /wt\/pr-54/, "the folder is below it");
  assert.match(rows[0], /1 shell\b/);
  // The second line is beside no button: it has the row's width.
  assert.match(node(".agents-worktree > .agents-folder-meta").textContent ?? "", /wt\/pr-54·1 shell/);
  assert.equal(all(".agents-worktree .tree-count").length, 0, "the agents show below; the row keeps its width for the branch");
  assert.match(rows[1], /feat\/terraform-plugin/);
  assert.match(rows[1], /No PR/, "GitHub answered, and the branch has none");
  assert.doesNotMatch(rows[0], /No PR/);

  const chip = node(".agents-worktree .agents-pr");
  assert.match(chip.className, /agents-pr-failing/);
  assert.equal(chip.textContent, "#541", "the number, then the failed checks");
  assert.equal(chip.getAttribute("aria-label"), "Open pull request #54: PR 54 · 1 check failed");
  assert.equal(all(".agents-repo-node .agents-folder-node .agents-agent").length, 1, "agents stay under their folder");
});

test("a pull request opens in a browser pane", async () => {
  const seen = await mount(shells(PR_54), BOTH);
  await click(".agents-worktree .agents-pr");
  assert.deepEqual(seen.opened, ["https://github.com/o/r/pull/54"]);
});

test("worktrees without a shell are folded; unfolded, an agent starts in one", async () => {
  const seen = await mount(shells(PR_54), BOTH);
  const toggle = node(".agents-idle-toggle");
  assert.equal(toggle.textContent, "Worktrees without a shell 4");
  assert.equal(toggle.getAttribute("aria-expanded"), "false");
  assert.equal(all(".agents-idle-row").length, 0);

  await click(".agents-idle-toggle");
  const rows = all(".agents-idle-row");
  assert.deepEqual(
    rows.map((row) => row.querySelector(".agents-idle-name")?.textContent),
    ["feat/docker-plugin", "main", "feat/terraform-plugin", "feat/hole-punch"],
    "those with a pull request first",
  );
  assert.equal(rows[1].getAttribute("title"), "/Users/p/westerm");
  const chip = rows[0].querySelector(".agents-pr");
  assert.match(chip?.className ?? "", /agents-pr-running/);
  assert.equal(chip?.getAttribute("aria-label"), "Open pull request #42: PR 42 · Draft · Checks running");
  assert.doesNotMatch(rows[1].textContent ?? "", /No PR/, "a long list stays quiet");

  await act(async () => (rows[1].querySelector('[aria-label="Start Codex here"]') as unknown as HTMLElement).click());
  assert.deepEqual(seen.started, [["/Users/p/westerm", { kind: "codex", conversation: null }]]);
});

test("a repository folds with its folders", async () => {
  await mount(shells(PR_54), BOTH);
  await click(".agents-repo-toggle");
  assert.equal(node(".agents-repo-toggle").getAttribute("aria-expanded"), "false");
  assert.equal(all(".agents-worktree").length, 0);
  assert.equal(all(".agents-idle-toggle").length, 0);
});

test("a repository with one worktree keeps its folder row and shows the pull request", async () => {
  const info: RepoInfo = { root: "/Users/p/app", name: "o/app", current: "/Users/p/app", prs: true, worktrees: [tree("/Users/p/app", "fix", pr(9))] };
  await mount(shells("/Users/p/app", "/Users/p/plain"), { "/Users/p/app": info });
  assert.equal(all(".agents-repo").length, 0);
  assert.equal(all(".agents-worktree").length, 0);
  const rows = all(".agents-folder");
  assert.equal(rows.length, 2);
  assert.match(rows[0].textContent ?? "", /~\/app/);
  assert.equal(rows[0].querySelector(".tree-count")?.textContent, "1", "a folder on its own keeps its agent count");
  assert.equal(rows[0].querySelector(".agents-pr")?.textContent, "#9");
  assert.equal(rows[1].querySelector(".agents-pr"), null, "a folder outside Git");
  assert.doesNotMatch(node(".tree").textContent ?? "", /No PR/);
});

test("without an answer from GitHub no branch says it has no pull request", async () => {
  await mount(shells(TERRAFORM), { [TERRAFORM]: westerm(TERRAFORM, false) });
  assert.equal(all(".agents-worktree").length, 1);
  assert.doesNotMatch(node(".tree").textContent ?? "", /No PR/);
});

test("a detached worktree is named by its commit", async () => {
  const loose = "/Users/p/wt/loose";
  const info: RepoInfo = { ...westerm(loose), worktrees: [tree("/Users/p/westerm", "main"), tree(loose, null)] };
  await mount(shells(loose), { [loose]: info });
  assert.match(node(".agents-worktree .agents-folder-name").textContent ?? "", /^abc1234$/);
  assert.doesNotMatch(node(".agents-worktree").textContent ?? "", /No PR/, "no branch, no pull request to miss");
});

test("icons only: folders as before, and the server is not asked", async () => {
  const seen = await mount(shells(PR_54, TERRAFORM), BOTH, true);
  assert.deepEqual(seen.asked, []);
  assert.equal(all(".agents-repo").length, 0);
  assert.equal(all(".agents-pr").length, 0);
  assert.equal(all(".agents-folder").length, 2);
});

test("the panel asks about each folder once, and again every minute", async (t) => {
  t.mock.timers.enable({ apis: ["setInterval", "Date"] });
  const seen = await mount(shells(PR_54, TERRAFORM), BOTH);
  assert.deepEqual(seen.asked, [PR_54, TERRAFORM]);
  await act(async () => t.mock.timers.tick(WORKTREES_REFRESH - 1));
  assert.equal(seen.asked.length, 2);
  await act(async () => t.mock.timers.tick(1));
  assert.deepEqual(seen.asked.slice(2), [PR_54, TERRAFORM]);
  assert.equal(WORKTREES_REFRESH, 60_000);
});
