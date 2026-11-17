import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { Window } from "happy-dom";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nProvider } from "../src/i18n/index.tsx";
import { gitPlugin } from "../src/plugins/git/index.tsx";
import type { GitFolder, GitFoundRepository, GitStatus } from "../src/plugins/git/model.ts";
import type { PluginContext, PluginPlace } from "../src/plugins/types.ts";

const browser = new Window({ url: "http://127.0.0.1/" });
Object.assign(globalThis, {
  window: browser, document: browser.document, localStorage: browser.localStorage,
  HTMLElement: browser.HTMLElement, Event: browser.Event, IS_REACT_ACT_ENVIRONMENT: true,
});
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });
const originalFetch = globalThis.fetch;
let mounted: Root | null = null;
afterEach(async () => {
  if (mounted) await act(async () => mounted!.unmount());
  mounted = null;
  browser.document.body.innerHTML = "";
  globalThis.fetch = originalFetch;
});

const initial = (): GitStatus => ({
  root: "/repo", branch: "main", head: "abc1234", branches: ["main"], upstream: null, ahead: 0, behind: 0,
  files: [{ path: "file.txt", oldPath: null, index: "M", worktree: "M", conflict: false }],
  commits: [{ hash: "abc1234", message: "Initial commit", author: "Tester", time: "2026-11-10T10:00:00+09:00" }],
});
const body = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status });
/** A folder that is itself a repository, with none inside. */
const repoFolder = (dir: string): GitFolder => ({ root: dir, repository: dir, repos: [], truncated: false });
/** Answers the folder's repository list as `repoFolder`; every other request goes to `handler`. */
const withFolder = (handler: typeof fetch): typeof fetch => async (input, init) => {
  const url = new URL(String(input), "http://127.0.0.1");
  return url.pathname.endsWith("/repos") ? body(repoFolder(url.searchParams.get("dir") ?? "")) : handler(input, init);
};
const node = (selector: string) => {
  const found = browser.document.querySelector(selector);
  assert.ok(found, selector);
  return found as unknown as HTMLElement;
};
// What App hands the plugin: requests go to the plugin's own routes.
const ctx: PluginContext = {
  openShell() {},
  openBrowser() {},
  openPane() {},
  folders: () => [],
  fetch: (path, init) => fetch(`/api/plugins/git${path}`, init),
  refreshInstalled() {},
  openManager() {},
};
const REPO: PluginPlace = { workspace: "w1", focused: "/repo", folders: ["/repo"] };
// The panel as the plugin bar shows it: the running plugin's own Panel.
const { Panel } = gitPlugin.start(ctx);
async function show(place: PluginPlace, compact = false) {
  await act(async () => mounted!.render(createElement(I18nProvider, null, createElement(Panel, { compact, place }))));
}
async function mount(place: PluginPlace = REPO, compact = false) {
  mounted = createRoot(browser.document.body.appendChild(browser.document.createElement("div")) as unknown as HTMLElement);
  await show(place, compact);
}
async function click(selector: string) {
  await act(async () => node(selector).click());
}

// Exercise the rendered controls, not component implementation details.
test("file selection reads the chosen side of a partially staged diff; unstaging preserves the other side", async () => {
  let repo = initial();
  const reads: URL[] = [];
  const operations: unknown[] = [];
  globalThis.fetch = withFolder(async (input, init) => {
    const url = new URL(String(input), "http://127.0.0.1");
    if (url.pathname.endsWith("/diff")) { reads.push(url); return body({ text: "+staged content\n" }); }
    if (init?.method === "POST") {
      operations.push(JSON.parse(String(init.body)));
      repo = { ...repo, files: repo.files.map((file) => ({ ...file, index: " " })) };
      return body({});
    }
    return body(repo);
  });
  await mount();
  assert.equal(browser.document.querySelectorAll(".git-file-select").length, 2);
  await click('[aria-label="Staged changes"] .git-file-select');
  assert.equal(reads[0].searchParams.get("staged"), "true");
  assert.match(node(".git-preview").textContent ?? "", /staged content/);
  await click('[aria-label="Unstage: file.txt"]');
  assert.deepEqual(operations, [{ dir: "/repo", action: "unstage", paths: ["file.txt"] }]);
  assert.equal(browser.document.querySelectorAll(".git-file-select").length, 1);
});

test("a failed Git action stays visible after the repository refreshes", async () => {
  globalThis.fetch = withFolder(async (_input, init) => init?.method === "POST" ? body({ error: "index.lock already exists" }, 409) : body(initial()));
  await mount();
  await click('[aria-label="Unstage: file.txt"]');
  assert.match(node('[role="alert"]').textContent ?? "", /index.lock already exists/);
  await click('[aria-label="Refresh repository"]');
  assert.match(node('[role="alert"]').textContent ?? "", /index.lock already exists/);
});

test("the panel shows an unavailable repository without stale file actions", async () => {
  globalThis.fetch = withFolder(async () => body({ error: "This folder is not in a Git worktree" }, 404));
  await mount();
  assert.match(node('[role="alert"]').textContent ?? "", /not in a Git worktree/);
  assert.equal(browser.document.querySelectorAll(".git-file-select").length, 0);
  assert.equal(browser.document.querySelector(".git-commit-button"), null);
});

test("opening a commit shows its complete message, metadata and per-file historical diffs", async () => {
  const reads: URL[] = [];
  let mutations = 0;
  globalThis.fetch = withFolder(async (input, init) => {
    const url = new URL(String(input), "http://127.0.0.1");
    reads.push(url);
    if (init?.method === "POST") mutations++;
    if (url.pathname.endsWith("/commit")) return body({
      hash: "abc1234".padEnd(40, "0"), message: "Initial commit\n\nFull commit message body", author: "Tester", email: "tester@example.test",
      time: "2026-11-10T10:00:00+09:00", parents: ["1".repeat(40), "2".repeat(40)],
      files: [{ path: "first.txt", oldPath: null, status: "A" }, { path: "renamed.txt", oldPath: "old.txt", status: "R" }],
    });
    if (url.pathname.endsWith("/commit-diff")) return body({ text: `+historical ${url.searchParams.get("path")}\n` });
    return body(initial());
  });
  await mount();
  await click('[aria-label="Open commit: Initial commit"]');
  assert.match(node(".git-commit-message").textContent ?? "", /Full commit message body/);
  assert.match(node(".git-commit-metadata").textContent ?? "", /tester@example.test/);
  assert.match(node(".git-merge-note").textContent ?? "", /first parent/);
  assert.match(node(".git-commit-detail .git-preview").textContent ?? "", /historical first.txt/);
  await click('.git-commit-files [aria-label="renamed.txt · Renamed"]');
  assert.match(node(".git-commit-detail .git-preview").textContent ?? "", /historical renamed.txt/);
  assert.equal(reads.filter((url) => url.pathname.endsWith("/commit-diff")).at(-1)?.searchParams.get("hash"), "abc1234".padEnd(40, "0"));
  assert.equal(mutations, 0, "browsing a commit never modifies the repository");
  await click('[aria-label="Open commit: Initial commit"]');
  assert.equal(browser.document.querySelector(".git-commit-detail"), null);
});

test("a commit load failure appears inline and can be retried by reopening", async () => {
  globalThis.fetch = withFolder(async (input) => String(input).includes("/commit?") ? body({ error: "Commit not found" }, 404) : body(initial()));
  await mount();
  await click('[aria-label="Open commit: Initial commit"]');
  assert.match(node('.git-commit-detail [role="alert"]').textContent ?? "", /Commit not found/);
  await click('[aria-label="Open commit: Initial commit"]');
  globalThis.fetch = withFolder(async (input) => String(input).includes("/commit?") ? body({
    hash: "abc1234", message: "An empty commit", author: "Tester", email: "tester@example.test",
    time: "2026-11-10T10:00:00+09:00", parents: [], files: [],
  }) : body(initial()));
  await click('[aria-label="Open commit: Initial commit"]');
  assert.match(node(".git-commit-message").textContent ?? "", /An empty commit/);
  assert.equal(browser.document.querySelector(".git-commit-detail .git-preview"), null);
});

test("the panel reads the plugin's own routes and follows the focused folder", async () => {
  const reads: URL[] = [];
  globalThis.fetch = withFolder(async (input) => {
    const url = new URL(String(input), "http://127.0.0.1");
    reads.push(url);
    return body({ ...initial(), root: url.searchParams.get("dir") });
  });
  await mount();
  assert.equal(reads[0].pathname, "/api/plugins/git/status");
  assert.equal(reads[0].searchParams.get("dir"), "/repo");
  assert.match(node(".git-repo-name").textContent ?? "", /\/repo/);
  // The user focuses a pane in another folder of the same workspace.
  await show({ workspace: "w1", focused: "/other", folders: ["/repo", "/other"] });
  assert.equal(reads.at(-1)?.searchParams.get("dir"), "/other");
  assert.match(node(".git-repo-name").textContent ?? "", /\/other/);
});

test("a chosen folder stays through a focus change and is dropped in another workspace", async () => {
  const dirs: (string | null)[] = [];
  globalThis.fetch = withFolder(async (input) => {
    const url = new URL(String(input), "http://127.0.0.1");
    dirs.push(url.searchParams.get("dir"));
    return body({ ...initial(), root: url.searchParams.get("dir") });
  });
  await mount({ workspace: "w1", focused: "/repo", folders: ["/repo", "/other"] });
  const select = node(".git-folder-select") as unknown as HTMLSelectElement;
  await act(async () => {
    select.value = "/other";
    select.dispatchEvent(new browser.Event("change", { bubbles: true }) as unknown as Event);
  });
  assert.equal(dirs.at(-1), "/other");
  await show({ workspace: "w1", focused: "/repo", folders: ["/repo", "/other"] });
  assert.equal(dirs.at(-1), "/other", "the choice wins over the focused pane");
  await show({ workspace: "w2", focused: "/elsewhere", folders: ["/elsewhere"] });
  assert.equal(dirs.at(-1), "/elsewhere", "a new workspace starts the panel over");
});

test("a narrow sidebar shows only the number of changed files", async () => {
  globalThis.fetch = withFolder(async () => body(initial()));
  await mount(REPO, true);
  assert.equal(node(".git-compact .git-count").textContent, "1");
  assert.equal(browser.document.querySelector(".git-commit-form"), null);
});

const found = (name: string, extra: Partial<GitFoundRepository> = {}): GitFoundRepository => ({
  path: `/w/${name}`, name, branch: "main", head: "abc1234", changes: 0, upstream: null, ahead: 0, behind: 0, error: null, ...extra,
});
/** `/w` holds repositories and is not one; every other folder is a repository. */
const PROJECTS: GitFolder = {
  root: "/w", repository: null, truncated: true, repos: [
    found("westerm", { changes: 3, upstream: "origin/main", behind: 9 }),
    found("broken", { branch: null, head: null, changes: null, error: "fatal: not a git repository" }),
    found("worktree/web-pane", { branch: "feat/web-pane" }),
  ],
};
const WORKSPACE: PluginPlace = { workspace: "w1", focused: "/w", folders: ["/w"] };
/** Serves PROJECTS for `/w` and records the folders whose status is read. */
function projects(folder: GitFolder = PROJECTS) {
  const statusDirs: string[] = [];
  const folderReads: URL[] = [];
  globalThis.fetch = async (input) => {
    const url = new URL(String(input), "http://127.0.0.1");
    const dir = url.searchParams.get("dir") ?? "";
    if (url.pathname.endsWith("/repos")) { folderReads.push(url); return body(dir === folder.root ? folder : repoFolder(dir)); }
    statusDirs.push(dir);
    return body({ ...initial(), root: dir });
  };
  return { statusDirs, folderReads };
}
const rows = () => browser.document.querySelectorAll(".git-repo-row").length;

test("a folder outside a repository lists its repositories and opens one", async () => {
  const { statusDirs, folderReads } = projects();
  await mount(WORKSPACE);
  assert.equal(folderReads[0].pathname, "/api/plugins/git/repos");
  assert.equal(folderReads[0].searchParams.get("dir"), "/w");
  assert.equal(rows(), 3);
  assert.deepEqual(statusDirs, [], "a folder that is not a repository has no status to read");
  const westerm = node('[aria-label="Open repository: westerm"]').textContent ?? "";
  assert.match(westerm, /main/);
  assert.match(westerm, /3/);
  assert.match(westerm, /9/);
  assert.match(node('[aria-label="Open repository: broken"]').getAttribute("title") ?? "", /not a git repository/);
  assert.match(node(".git-repo-list").textContent ?? "", /worktree/);
  assert.match(node(".git-repo-list").textContent ?? "", /first 3/);
  assert.equal(browser.document.querySelector(".git-commit-form"), null);

  await click('[aria-label="Open repository: westerm"]');
  assert.equal(statusDirs.at(-1), "/w/westerm");
  assert.match(node(".git-repo-name").textContent ?? "", /\/w\/westerm/);
  await click('[aria-label="Repositories (3)"]');
  assert.equal(rows(), 3);
  assert.equal(browser.document.querySelector(".git-commit-form"), null);
});

test("a repository folder with repositories inside links to them", async () => {
  projects({ root: "/repo", repository: "/repo", truncated: false, repos: [found("one"), found("two")] });
  await mount();
  node(".git-commit-form");
  assert.equal(rows(), 0);
  await click('[aria-label="2 repositories inside"]');
  assert.equal(rows(), 2);
  await click('[aria-label="Back to repo"]');
  node(".git-commit-form");
});

test("a folder with no repositories says so without an error", async () => {
  projects({ root: "/w", repository: null, truncated: false, repos: [] });
  await mount(WORKSPACE);
  assert.match(node(".git-repo-list").textContent ?? "", /No Git repositories in this folder/);
  assert.equal(browser.document.querySelector('[role="alert"]'), null);
});

test("a new focused folder drops the opened repository", async () => {
  const { statusDirs } = projects();
  await mount(WORKSPACE);
  await click('[aria-label="Open repository: westerm"]');
  assert.equal(statusDirs.at(-1), "/w/westerm");
  await show({ workspace: "w1", focused: "/repo", folders: ["/w", "/repo"] });
  assert.equal(statusDirs.at(-1), "/repo");
  assert.equal(browser.document.querySelector('[aria-label="Repositories (3)"]'), null);
});

test("a narrow sidebar sums the changes of the listed repositories", async () => {
  projects();
  await mount(WORKSPACE, true);
  assert.equal(node(".git-compact .git-count").textContent, "3");
});

test("an action in an opened repository does not scan the folder again", async () => {
  const { folderReads } = projects();
  await mount(WORKSPACE);
  await click('[aria-label="Open repository: westerm"]');
  const scans = folderReads.length;
  await click('[aria-label="Unstage: file.txt"]');
  assert.equal(folderReads.length, scans);
  await click('[aria-label="Repositories (3)"]');
  assert.equal(folderReads.length, scans + 1, "going back reads the list again");
});
