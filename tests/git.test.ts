import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { focusedGitFolder, gitEntries, gitFolders, gitPath, runGitOperation, type GitFile } from "../src/git.ts";
import { pressPanel, sidebarView, type SidebarFlags } from "../src/agents.ts";
import { createDashboard, dashboardReducer } from "../src/workspace.ts";

const file = (path: string, index: string, worktree: string, conflict = false): GitFile => ({ path, index, worktree, conflict, oldPath: null });

test("partially staged files appear in both groups and conflicts cannot be committed", () => {
  const { staged, changes } = gitEntries([
    file("partial.txt", "M", "M"), file("new.txt", "?", "?"), file("deleted.txt", "D", " "), file("conflict.txt", "U", "U", true),
  ]);
  assert.deepEqual(staged.map((entry) => [entry.path, entry.status, entry.staged]), [["partial.txt", "M", true], ["deleted.txt", "D", true]]);
  assert.deepEqual(changes.map((entry) => [entry.path, entry.status, entry.staged]), [["partial.txt", "M", false], ["new.txt", "?", false], ["conflict.txt", "U", false]]);
});

test("repository paths preserve filenames and route remote requests to the chosen server", () => {
  const path = gitPath("diff", { dir: "/repo with spaces", path: "한글 & file.txt", staged: "true" }, "user@server");
  const url = new URL(path, "http://localhost");
  assert.equal(url.pathname, "/remote/user%40server/api/git/diff");
  assert.equal(url.searchParams.get("path"), "한글 & file.txt");
  assert.equal(url.searchParams.get("dir"), "/repo with spaces");
  assert.equal(gitPath("action", undefined, null), "/api/git/action");
});

test("the Git sidebar follows the focused shell and only offers the active workspace's folders", () => {
  let state = createDashboard("w1", "t1");
  state = dashboardReducer(state, { type: "open", id: "a", kind: "shell", cwd: "/repo-a" });
  state = dashboardReducer(state, { type: "place", id: "a", place: { cwd: "/repo-a/subfolder", branch: "main" } });
  state = dashboardReducer(state, { type: "open", id: "b", kind: "shell", cwd: "/repo-b" });
  assert.equal(focusedGitFolder(state), "/repo-b");
  assert.deepEqual(gitFolders(state), ["/repo-a/subfolder", "/repo-b"]);
  state = dashboardReducer(state, { type: "newWorkspace", id: "w2", tabId: "t2" });
  assert.deepEqual(gitFolders(state), []);
  assert.equal(focusedGitFolder(state), null);
});

test("Git and Agents share the sidebar slot in both layouts and in a narrow drawer", () => {
  for (const layout of ["top", "sidebar"] as const) {
    for (const narrow of [false, true]) {
      const initial: SidebarFlags = { layout, narrow, sidebarOpen: layout === "sidebar", drawerOpen: false, panel: null };
      let state = pressPanel(initial, "git");
      assert.equal(sidebarView(state), "git");
      state = pressPanel(state, "agents");
      assert.equal(sidebarView(state), "agents");
      state = pressPanel(state, "git");
      assert.equal(sidebarView(state), "git");
      state = pressPanel(state, "git");
      assert.equal(sidebarView(state), !narrow && layout === "sidebar" ? "sessions" : null);
    }
  }
});

const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

test("Git mutations send structured JSON and preserve the server's error", async () => {
  let body = "";
  globalThis.fetch = async (_url, init) => {
    assert.equal(init?.method, "POST");
    body = String(init.body);
    return new Response(JSON.stringify({ error: "Author identity unknown" }), { status: 409 });
  };
  const result = await runGitOperation("/repo", { action: "commit", message: "title\n\nbody" });
  assert.deepEqual(JSON.parse(body), { dir: "/repo", action: "commit", message: "title\n\nbody" });
  assert.deepEqual(result, { ok: false, error: "Author identity unknown" });
});

test("an older server returning HTML gives a restart instruction for commit details", async () => {
  const { fetchGitCommit } = await import("../src/git.ts");
  globalThis.fetch = async () => new Response("<!doctype html><html>old UI</html>", { headers: { "Content-Type": "text/html" } });
  const result = await fetchGitCommit("/repo", "abc1234");
  assert.equal(result.ok, false);
  if (!result.ok) assert.match(result.error, /Restart westerm/);
});
