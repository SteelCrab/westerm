import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { AgentLaunch, AgentState } from "../shared/protocol.ts";
import {
  agentFolders,
  anyWaiting,
  createHistoryCache,
  elsewhereWaiting,
  fetchHistory,
  folderLabel,
  HISTORY_MAX_AGE,
  historyCache,
  historyPath,
  pastConversations,
  pressAgents,
  sidebarView,
  type SidebarFlags,
} from "../src/agents.ts";
import { AgentIcon } from "../src/agentIcons.tsx";
import { ActivityBar } from "../src/components/ActivityBar.tsx";
import { AgentMark } from "../src/components/AgentMark.tsx";
import { AgentPanel, PastList } from "../src/components/AgentPanel.tsx";
import { I18nProvider } from "../src/i18n/index.tsx";
import { createDashboard, dashboardReducer, type Dashboard } from "../src/workspace.ts";

// I18nProvider follows the browser's language, which Node takes from LANG.
// Pin it so the English text below matches on every machine.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

const NO_DETAILS = { since: null, started: null, conversation: null, resume: null, cwd: null, version: null, model: null };
const agent = (status: AgentState["status"], extra: Partial<AgentState> = {}): AgentState => ({
  kind: "claude",
  status,
  ...NO_DETAILS,
  ...extra,
});
const reduce = (state: Dashboard, ...actions: Parameters<typeof dashboardReducer>[1][]) =>
  actions.reduce(dashboardReducer, state);
const at = (id: string, cwd: string, branch: string | null = null) =>
  ({ type: "place", id, place: { cwd, branch } }) as const;

/** Tab t1: shells a, b, c; tab t2: shell d, an ssh pane, and a docs pane. */
function layout(): Dashboard {
  return reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "a", kind: "shell" },
    { type: "open", id: "b", kind: "shell" },
    { type: "open", id: "c", kind: "shell" },
    { type: "newTab", id: "t2" },
    { type: "open", id: "d", kind: "shell" },
    { type: "open", id: "s", kind: "ssh", target: "host" },
    { type: "open", id: "x", kind: "docs", docs: { root: null, followFrom: null, file: null, mode: "view" } },
    at("a", "/Users/p/app", "main"),
    at("b", "/Users/p/app", "dev"),
    at("c", "/Users/p/old"),
    at("s", "/srv/remote"),
    { type: "agent", id: "b", agent: agent("idle") },
    { type: "agent", id: "c", agent: agent("working", { cwd: "/Users/p/lib" }) },
  );
}

test("folders group shells by their place", () => {
  const folders = agentFolders(layout());
  assert.deepEqual(
    folders.map((folder) => [folder.key, folder.shells, folder.agents.length]),
    [
      ["/Users/p/app", 2, 1],
      ["/Users/p/lib", 1, 1],
      ["", 1, 0],
    ],
  );
  assert.equal(folders[0].branch, "main", "the first shell's branch");
  assert.equal(folders[0].path, "/Users/p/app");
  assert.deepEqual([folders[0].name, folders[0].parent], ["app", "~/"]);
  assert.equal(folders[2].path, null);
  assert.deepEqual([folders[2].name, folders[2].parent], ["", ""]);
});

test("a folder's branch comes only from a shell in that folder", () => {
  // c runs its agent in ~/lib, but the shell itself sits in ~/old on another branch.
  let state = reduce(layout(), at("c", "/Users/p/old", "feature"));
  const lib = (s: Dashboard) => agentFolders(s).find((folder) => folder.key === "/Users/p/lib")!;
  assert.equal(lib(state).branch, null);
  state = reduce(state, at("d", "/Users/p/lib", "trunk"));
  assert.equal(lib(state).branch, "trunk", "d's own folder is ~/lib");
});

test("folders keep pane order when states change", () => {
  let state = reduce(
    layout(),
    at("d", "/Users/p/lib"),
    { type: "agent", id: "a", agent: agent("working") },
    { type: "agent", id: "d", agent: agent("working", { cwd: "/Users/p/lib" }) },
  );
  const order = (s: Dashboard) => agentFolders(s).map((folder) => folder.key);
  const before = order(state);
  state = reduce(state, { type: "agent", id: "c", agent: agent("idle", { cwd: "/Users/p/lib" }) });
  state = reduce(state, { type: "agent", id: "a", agent: agent("waiting") });
  assert.deepEqual(order(state), before);
  state = reduce(state, { type: "agent", id: "d", agent: agent("waiting", { cwd: "/Users/p/lib" }) });
  const lib = agentFolders(state).find((folder) => folder.key === "/Users/p/lib")!;
  assert.deepEqual(lib.agents.map((item) => item.id), ["d", "c"], "waiting before idle");
  assert.equal(lib.waiting, true);
  assert.equal(agentFolders(state).find((folder) => folder.key === "/Users/p/app")?.waiting, true, "a waits too");
});

test("same status keeps pane order inside a folder", () => {
  const state = reduce(
    layout(),
    at("c", "/Users/p/app"),
    { type: "agent", id: "a", agent: agent("working") },
    { type: "agent", id: "b", agent: agent("working") },
    { type: "agent", id: "c", agent: agent("working") },
  );
  assert.deepEqual(agentFolders(state)[0].agents.map((item) => item.id), ["a", "b", "c"]);
});

test("only the workspace in view", () => {
  let state = reduce(
    layout(),
    { type: "agent", id: "a", agent: agent("waiting") },
    { type: "newWorkspace", id: "w2", tabId: "t3" },
    { type: "open", id: "e", kind: "shell" },
    at("e", "/Users/p/other"),
    { type: "agent", id: "e", agent: agent("waiting") },
  );
  assert.deepEqual(agentFolders(state).map((folder) => folder.key), ["/Users/p/other"]);
  assert.equal(elsewhereWaiting(state), 1, "only a is waiting out of view");
  assert.equal(anyWaiting(state), true);
  state = reduce(state, { type: "agent", id: "a", agent: agent("idle") });
  assert.equal(elsewhereWaiting(state), 0);
  assert.equal(anyWaiting(state), true, "e waits in view");
  state = reduce(state, { type: "agent", id: "e", agent: agent("working") });
  assert.equal(anyWaiting(state), false);
});

test("folder labels", () => {
  assert.deepEqual(folderLabel("/Users/p"), { name: "~", parent: "" });
  assert.deepEqual(folderLabel("/Users/p/westerm"), { name: "westerm", parent: "~/" });
  assert.deepEqual(folderLabel("/Users/p/westerm/.claude/worktrees/x"), { name: "x", parent: "worktrees/" });
  assert.deepEqual(folderLabel("/srv/app"), { name: "app", parent: "srv/" });
  assert.deepEqual(folderLabel("/"), { name: "/", parent: "" });
});

const flags = (change: Partial<SidebarFlags>): SidebarFlags => ({
  layout: "top",
  narrow: false,
  sidebarOpen: false,
  drawerOpen: false,
  agentsView: false,
  ...change,
});
const press = (o: SidebarFlags): SidebarFlags => ({ ...o, ...pressAgents(o) });

test("the agents button", () => {
  let o = flags({});
  assert.equal(sidebarView(o), null);
  o = press(o);
  assert.equal(sidebarView(o), "agents");
  o = press(o);
  assert.equal(sidebarView(o), null);

  o = flags({ layout: "sidebar", sidebarOpen: true });
  assert.equal(sidebarView(o), "sessions");
  o = press(o);
  assert.equal(sidebarView(o), "agents");
  assert.equal(o.sidebarOpen, true);
  o = press(o);
  assert.equal(sidebarView(o), "sessions");
  assert.equal(o.sidebarOpen, true, "stays open");

  o = press(flags({ layout: "sidebar" }));
  assert.equal(sidebarView(o), "agents");
  assert.equal(o.sidebarOpen, true);

  for (const layout of ["top", "sidebar"] as const) {
    o = flags({ layout, narrow: true });
    assert.equal(sidebarView(o), null);
    o = press(o);
    assert.equal(o.drawerOpen, true);
    assert.equal(sidebarView(o), "agents");
    o = press(o);
    assert.equal(o.drawerOpen, false);
    assert.equal(sidebarView(o), null);
  }
});

test("running conversations are not past", () => {
  const list = [
    { kind: "claude" as const, id: "c1", title: "one", updated: 2 },
    { kind: "claude" as const, id: "c2", title: null, updated: 1 },
  ];
  const state = reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "a", kind: "shell" },
    { type: "agent", id: "a", agent: agent("working", { conversation: "c1" }) },
  );
  assert.deepEqual(pastConversations(list, state, "/w"), [list[1]]);
  assert.deepEqual(pastConversations(list, createDashboard("w1", "t1"), "/w"), list);
});

test("an agent that has not told its conversation hides the ones of its kind written since it started", () => {
  const list = [
    { kind: "codex" as const, id: "live", title: null, updated: 1_500 },
    { kind: "codex" as const, id: "old", title: null, updated: 500 },
    { kind: "claude" as const, id: "other-kind", title: null, updated: 2_000 },
    { kind: "gemini" as const, id: "other-folder", title: null, updated: 3_000 },
    { kind: "codex" as const, id: "at-start", title: null, updated: 1_000 },
  ];
  const state = reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "a", kind: "shell" },
    { type: "open", id: "b", kind: "shell" },
    { type: "open", id: "c", kind: "shell" },
    at("a", "/w"),
    at("c", "/w"),
    { type: "agent", id: "a", agent: agent("working", { kind: "codex", started: 1_000 }) },
    // A Gemini CLI in another folder hides nothing here.
    { type: "agent", id: "b", agent: agent("working", { kind: "gemini", started: 0, cwd: "/elsewhere" }) },
    // A Claude Code that told its conversation hides only that one.
    { type: "agent", id: "c", agent: agent("working", { conversation: "c9", started: 0 }) },
  );
  assert.deepEqual(
    pastConversations(list, state, "/w").map((item) => item.id),
    ["old", "other-kind", "other-folder"],
  );
});

test("past conversations are read again after 30 s or when the running ones change", async () => {
  let clock = 0;
  const reads: string[] = [];
  const answers: (string[] | null)[] = [];
  const cache = createHistoryCache(async (dir) => {
    reads.push(dir);
    const ids = answers.length > 0 ? answers.shift()! : ["c1"];
    return ids && ids.map((id) => ({ kind: "claude" as const, id, title: null, updated: 1 }));
  }, () => clock);
  const ids = (list: { id: string }[] | null) => list?.map((item) => item.id) ?? null;

  assert.deepEqual(cache.shown("/w"), []);
  assert.deepEqual(ids(await cache.load("/w", "")), ["c1"]);
  // The folder folds and unfolds, or the panel closes and opens: a new mount within 30 s reads nothing.
  clock = HISTORY_MAX_AGE - 1;
  assert.deepEqual(ids(cache.shown("/w")), ["c1"]);
  assert.deepEqual(ids(await cache.load("/w", "")), ["c1"]);
  assert.deepEqual(reads, ["/w"]);
  // 30 s later it reads again.
  clock = HISTORY_MAX_AGE;
  answers.push(["c2"]);
  assert.deepEqual(ids(await cache.load("/w", "")), ["c2"]);
  assert.equal(reads.length, 2);
  // A conversation that ends changes the running set: read at once.
  clock += 1;
  answers.push(["c3"]);
  assert.deepEqual(ids(await cache.load("/w", "?")), ["c3"]);
  assert.equal(reads.length, 3);
  // Each folder has its own list.
  await cache.load("/v", "");
  assert.deepEqual(reads, ["/w", "/w", "/w", "/v"]);
  // A failed read keeps what was shown, and the next mount tries again.
  answers.push(null);
  assert.equal(await cache.load("/w", "x"), null);
  assert.deepEqual(ids(cache.shown("/w")), ["c3"]);
  answers.push(["c4"]);
  assert.deepEqual(ids(await cache.load("/w", "x")), ["c4"]);
  assert.equal(reads.length, 6);
});

test("reads that overlap share one request", async () => {
  let calls = 0;
  let finish: (list: never[]) => void = () => {};
  const cache = createHistoryCache(() => {
    calls += 1;
    return new Promise((resolve) => (finish = resolve));
  }, () => 0);
  const first = cache.load("/w", "");
  const second = cache.load("/w", "");
  finish([]);
  assert.deepEqual(await Promise.all([first, second]), [[], []]);
  assert.equal(calls, 1);
});

test("the kept list outlives the panel", async () => {
  const reader = async () => [{ kind: "codex" as const, id: "x1", title: "Fix fonts", updated: Date.now() }];
  assert.equal(historyCache(reader), historyCache(reader), "one cache per reader");
  assert.notEqual(historyCache(reader), historyCache(async () => []));
  await historyCache(reader).load("/Users/p/empty", "");
  // A panel that mounts again shows the kept list before any read ends.
  assert.match(panel(twoFolders(), { history: reader }), /Past conversations 1[^]*Fix fonts/);
  assert.doesNotMatch(panel(twoFolders()), /Fix fonts/, "another reader has its own cache");
});

test("history path follows a remote window's server", () => {
  assert.equal(historyPath("/a b", null), "/api/agents/history?dir=%2Fa+b");
  assert.equal(historyPath("/r", "me@host"), "/remote/me%40host/api/agents/history?dir=%2Fr");
});

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});
const answer = (body: unknown, ok = true) => {
  globalThis.fetch = (async () => ({ ok, json: async () => body })) as unknown as typeof fetch;
};

test("history drops bad items and reports a failure as null", async () => {
  answer({
    conversations: [
      { kind: "claude", id: "a", title: "t", updated: 5 },
      { kind: "codex", id: "b", title: null, updated: 4 },
      { kind: "vim", id: "c", title: "x", updated: 3 },
      { kind: "claude", id: 7, title: "x", updated: 3 },
      { kind: "claude", id: "d", title: 5, updated: 3 },
      { kind: "claude", id: "e", title: "x", updated: Number.NaN },
      { kind: "claude", id: "f", title: "x", updated: "3" },
      null,
    ],
  });
  assert.deepEqual(await fetchHistory("/d"), [
    { kind: "claude", id: "a", title: "t", updated: 5 },
    { kind: "codex", id: "b", title: null, updated: 4 },
  ]);
  answer({}, false);
  assert.equal(await fetchHistory("/d"), null);
  answer({ nope: 1 });
  assert.equal(await fetchHistory("/d"), null);
  globalThis.fetch = (async () => {
    throw new Error("down");
  }) as unknown as typeof fetch;
  assert.equal(await fetchHistory("/d"), null);
});

test("AgentIcon draws each agent's own logo at the given size", () => {
  const claude = renderToStaticMarkup(createElement(AgentIcon, { kind: "claude", size: 16 }));
  assert.match(claude, /#D97757/);
  assert.match(claude, /width="16"/);
  assert.match(claude, /height="16"/);
  assert.match(claude, /viewBox="0 0 24 24"/);
  assert.match(claude, /aria-hidden="true"/);
  const codex = renderToStaticMarkup(createElement(AgentIcon, { kind: "codex", size: 20 }));
  assert.match(codex, /currentColor/);
  assert.match(codex, /width="20"/);
  const gemini = renderToStaticMarkup(createElement(AgentIcon, { kind: "gemini", size: 16 }));
  assert.match(gemini, /#3186FF/);
  assert.equal(gemini.match(/<linearGradient/g)?.length, 3);
});

test("two Gemini logos on one page do not share gradient ids", () => {
  const both = renderToStaticMarkup(
    createElement("div", null, createElement(AgentIcon, { kind: "gemini", size: 16 }), createElement(AgentIcon, { kind: "gemini", size: 16 })),
  );
  const ids = [...both.matchAll(/<linearGradient[^>]* id="([^"]+)"/g)].map((m) => m[1]);
  assert.equal(ids.length, 6);
  assert.equal(new Set(ids).size, 6);
  // Every fill points at a gradient that exists in the markup.
  for (const [, ref] of both.matchAll(/fill="url\(#([^)]+)\)"/g)) assert.ok(ids.includes(ref), ref);
});

test("AgentMark shows the agent's logo, not the generic sparkle", () => {
  const agentOf = (kind: AgentState["kind"]) => agent("working", { kind });
  for (const kind of ["claude", "codex", "gemini"] as const) {
    const html = renderToStaticMarkup(
      createElement(I18nProvider, null, createElement(AgentMark, { agent: agentOf(kind), display: "working", size: 14 })),
    );
    assert.doesNotMatch(html, /lucide-sparkle/);
    assert.match(html, new RegExp(`agent-mark agent-${kind} agent-working`));
    assert.match(html, /<svg[^>]*viewBox="0 0 24 24"/);
  }
});

const noHistory = async () => [];
const noop = () => {};
function panel(state: Dashboard, change: Partial<Parameters<typeof AgentPanel>[0]> = {}) {
  return renderToStaticMarkup(
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
        onStart: noop,
        compact: false,
        history: noHistory,
        ...change,
      }),
    ),
  );
}

/** Two folders: ~/app runs a Claude Code that asks; ~/empty has only a shell. */
function twoFolders(): Dashboard {
  return reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "a", kind: "shell" },
    { type: "open", id: "b", kind: "shell" },
    at("a", "/Users/p/app", "main"),
    at("b", "/Users/p/empty", "dev"),
    { type: "renameSession", id: "a", name: "fix-login" },
    { type: "agent", id: "a", agent: agent("waiting", { cwd: "/Users/p/app" }) },
  );
}

test("the panel lists folders, branches, agents, and start buttons", () => {
  const html = panel(twoFolders());
  assert.match(html, /~\/<\/span><span[^>]*>app<\/span>/, "parent then folder name");
  assert.match(html, /~\/<\/span><span[^>]*>empty<\/span>/);
  assert.match(html, />main</);
  assert.match(html, />dev</);
  assert.match(html, /fix-login/);
  assert.match(html, /Needs your input/);
  assert.match(html, /No agents/);
  for (const name of ["Start Claude Code here", "Start Codex here", "Start Gemini CLI here"]) {
    assert.match(html, new RegExp(`aria-label="${name}"`), name);
  }
  assert.match(html, /Folders/);
  assert.match(html, /1 agent\b/);
  assert.match(html, /aria-expanded="true"/, "folders start expanded");
  assert.doesNotMatch(html, /Agents show on macOS and Linux/, "shells report their place");
  // "Start here" can hide when the panel is narrow: the buttons' group keeps it as its name.
  assert.match(html, /<span class="agents-none-text">No agents<\/span>/);
  assert.match(html, /<span class="agents-start-label" aria-hidden="true">Start here<\/span>/);
  assert.match(html, /role="group" aria-label="Start here"/);
});

test("the panel names the unknown folder and the platform without places", () => {
  const html = panel(reduce(createDashboard("w1", "t1"), { type: "open", id: "a", kind: "shell" }));
  assert.match(html, /Unknown folder/);
  assert.match(html, /Agents show on macOS and Linux/);
  assert.doesNotMatch(html, /Start Claude Code here/, "no folder to start in");
});

test("an empty workspace says it has no shells", () => {
  assert.match(panel(createDashboard("w1", "t1")), /No shells in this workspace/);
});

test("agents waiting in another workspace show next to the menu", () => {
  const state = reduce(twoFolders(), { type: "newWorkspace", id: "w2", tabId: "t2" });
  const html = panel(state);
  assert.match(html, /Elsewhere 1/);
  assert.match(html, /1 agent needs your input in other workspaces/);
  assert.doesNotMatch(panel(twoFolders()), /Elsewhere/);
});

test("icons only: folder icons and agent marks, names in tooltips", () => {
  const html = panel(twoFolders(), { compact: true });
  assert.match(html, /agent-mark agent-claude agent-waiting/);
  assert.match(html, /title="fix-login"/);
  assert.match(html, /title="\/Users\/p\/empty"/);
  assert.doesNotMatch(html, /No agents/);
  assert.doesNotMatch(html, /Start Claude Code here/);
});

test("past conversations: logo, title or a stand-in, and a resume button", () => {
  const resumed: [string, AgentLaunch][] = [];
  const html = renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(PastList, {
        path: "/Users/p/app",
        items: [
          { kind: "codex", id: "x1", title: "Fix fonts", updated: Date.now() - 5 * 60_000 },
          { kind: "gemini", id: "x2", title: null, updated: Date.now() - 3 * 86_400_000 },
        ],
        onStart: (folder: string, launch: AgentLaunch) => resumed.push([folder, launch]),
      }),
    ),
  );
  assert.match(html, /Past conversations 2/);
  assert.match(html, /aria-expanded="true"/, "starts expanded");
  assert.match(html, /Fix fonts/);
  assert.match(html, /5 min ago/);
  assert.match(html, /Untitled conversation/);
  assert.match(html, /aria-label="Resume Fix fonts"/);
  assert.match(html, /aria-label="Resume Untitled conversation"/);
  assert.match(html, />Resume</);
  // The title has a line of its own; when and Resume sit on the line below.
  assert.match(
    html,
    /class="agents-past-line">[^]*?Fix fonts<\/span><\/span><span class="agents-past-meta"><span class="agents-time">5 min ago<\/span><button[^>]*>Resume<\/button><\/span>/,
  );
  assert.match(html, /title="Resume Fix fonts"/, "Resume has a tooltip too");
});

function bar(change: Partial<Parameters<typeof ActivityBar>[0]>) {
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(ActivityBar, {
        sidebarOpen: false,
        counts: { shell: 0, browser: 0, editor: 0, ssh: 0, docs: 0 },
        onLaunch: noop,
        onToggleSidebar: null,
        onOpenSettings: noop,
        onOpenPalette: noop,
        onOpenRemoteWindow: null,
        paletteShortcut: "⌘K",
        agentsOpen: false,
        agentsWaiting: false,
        onToggleAgents: noop,
        ...change,
      }),
    ),
  );
}

test("the activity bar's agents button shows its state and a waiting dot", () => {
  const closed = bar({});
  assert.match(closed, /class="activity-button activity-agents" aria-label="Agents" title="Agents — AI agents in this workspace, by folder" aria-pressed="false"/);
  assert.doesNotMatch(closed, /activity-dot/);
  const open = bar({ agentsOpen: true, agentsWaiting: true });
  assert.match(open, /aria-pressed="true"/);
  assert.match(open, /activity-dot/);
});
