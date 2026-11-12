import assert from "node:assert/strict";
import { test } from "node:test";
import { pressPanel, sidebarView, type SidebarFlags } from "../src/agents.ts";
import { pluginPlace } from "../src/pluginPlace.ts";
import { fetchGitCommit, gitPath, runGitOperation } from "../src/plugins/git/api.ts";
import { gitPlugin } from "../src/plugins/git/index.tsx";
import { gitEntries, type GitFile } from "../src/plugins/git/model.ts";
import { pagePlugins } from "../src/plugins/installed.ts";
import { BUILTIN_PLUGINS } from "../src/plugins/registry.ts";
import type { PluginContext } from "../src/plugins/types.ts";
import { pluginPath } from "../src/remoteWindow.ts";
import { readSidebarPanel } from "../src/useSidebarPanel.ts";
import { createDashboard, dashboardReducer } from "../src/workspace.ts";

const file = (path: string, index: string, worktree: string, conflict = false): GitFile => ({ path, index, worktree, conflict, oldPath: null });

/** A plugin context whose only working part is `fetch`. */
const context = (fetch: PluginContext["fetch"]): PluginContext => ({
  openShell() {},
  openBrowser() {},
  openPane() {},
  folders: () => [],
  fetch,
  refreshInstalled() {},
  openManager() {},
});

test("partially staged files appear in both groups and conflicts cannot be committed", () => {
  const { staged, changes } = gitEntries([
    file("partial.txt", "M", "M"), file("new.txt", "?", "?"), file("deleted.txt", "D", " "), file("conflict.txt", "U", "U", true),
  ]);
  assert.deepEqual(staged.map((entry) => [entry.path, entry.status, entry.staged]), [["partial.txt", "M", true], ["deleted.txt", "D", true]]);
  assert.deepEqual(changes.map((entry) => [entry.path, entry.status, entry.staged]), [["partial.txt", "M", false], ["new.txt", "?", false], ["conflict.txt", "U", false]]);
});

test("repository paths preserve filenames, and a remote window sends them to its server", () => {
  const route = gitPath("diff", { dir: "/repo with spaces", path: "한글 & file.txt", staged: "true" });
  const url = new URL(pluginPath("git", route, "user@server"), "http://localhost");
  assert.equal(url.pathname, "/remote/user%40server/api/plugins/git/diff");
  assert.equal(url.searchParams.get("path"), "한글 & file.txt");
  assert.equal(url.searchParams.get("dir"), "/repo with spaces");
  assert.equal(pluginPath("git", gitPath("action"), null), "/api/plugins/git/action");
});

test("the Git panel follows the focused shell and only offers the active workspace's folders", () => {
  let state = createDashboard("w1", "t1");
  state = dashboardReducer(state, { type: "open", id: "a", kind: "shell", cwd: "/repo-a" });
  state = dashboardReducer(state, { type: "place", id: "a", place: { cwd: "/repo-a/subfolder", branch: "main" } });
  state = dashboardReducer(state, { type: "open", id: "b", kind: "shell", cwd: "/repo-b" });
  assert.deepEqual(pluginPlace(state), { workspace: "w1", focused: "/repo-b", folders: ["/repo-a/subfolder", "/repo-b"] });
  // A browser pane has no folder: the first folder of the workspace counts.
  state = dashboardReducer(state, { type: "open", id: "c", kind: "browser", url: "https://example.test" });
  assert.equal(pluginPlace(state).focused, "/repo-a/subfolder");
  state = dashboardReducer(state, { type: "newWorkspace", id: "w2", tabId: "t2" });
  assert.deepEqual(pluginPlace(state), { workspace: "w2", focused: null, folders: [] });
});

test("Git and Agents share the sidebar slot in both layouts and in a narrow drawer", () => {
  for (const layout of ["top", "sidebar"] as const) {
    for (const narrow of [false, true]) {
      const initial: SidebarFlags = { layout, narrow, sidebarOpen: layout === "sidebar", drawerOpen: false, panel: null };
      let state = pressPanel(initial, "plugin:git");
      assert.equal(sidebarView(state), "plugin:git");
      state = pressPanel(state, "agents");
      assert.equal(sidebarView(state), "agents");
      state = pressPanel(state, "plugin:git");
      assert.equal(sidebarView(state), "plugin:git");
      state = pressPanel(state, "plugin:git");
      assert.equal(sidebarView(state), !narrow && layout === "sidebar" ? "sessions" : null);
    }
  }
});

test("a Git panel remembered before Git was a plugin comes back as the plugin's panel", () => {
  const storage = { getItem: (key: string) => (key === "westerm.sidebarPanel" ? "git" : null) };
  assert.equal(readSidebarPanel(storage), "plugin:git");
});

test("Git is the first built-in plugin, and the only one in a remote window or on a tunnel page", () => {
  assert.equal(BUILTIN_PLUGINS[0], gitPlugin);
  assert.deepEqual(pagePlugins(BUILTIN_PLUGINS, true), [...BUILTIN_PLUGINS]);
  assert.deepEqual(pagePlugins(BUILTIN_PLUGINS, false), [gitPlugin]);
});

test("Git mutations send structured JSON through the plugin's route and preserve the server's error", async () => {
  let sent: { path: string; init?: RequestInit } | null = null;
  const ctx = context(async (path, init) => {
    sent = { path, init };
    return new Response(JSON.stringify({ error: "Author identity unknown" }), { status: 409 });
  });
  const result = await runGitOperation(ctx, "/repo", { action: "commit", message: "title\n\nbody" });
  assert.equal(sent!.path, "/action");
  assert.equal(sent!.init?.method, "POST");
  assert.deepEqual(JSON.parse(String(sent!.init?.body)), { dir: "/repo", action: "commit", message: "title\n\nbody" });
  assert.deepEqual(result, { ok: false, error: "Author identity unknown" });
});

test("an older server returning HTML gives a restart instruction for commit details", async () => {
  const ctx = context(async () => new Response("<!doctype html><html>old UI</html>", { headers: { "Content-Type": "text/html" } }));
  const result = await fetchGitCommit(ctx, "/repo", "abc1234");
  assert.equal(result.ok, false);
  if (!result.ok) assert.match(result.error, /Restart westerm/);
});

test("a server that cannot be reached is an error, not a throw", async () => {
  const ctx = context(async () => {
    throw new TypeError("fetch failed");
  });
  assert.deepEqual(await fetchGitCommit(ctx, "/repo", "abc1234"), { ok: false, error: "Cannot reach the Git server" });
});
