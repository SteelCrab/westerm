import assert from "node:assert/strict";
import { test } from "node:test";
import { parseAgentState, type AgentState } from "../shared/protocol.ts";
import { parseSavedDashboard } from "../shared/workspace-file.ts";
import { homeShort, minutesSince } from "../src/components/AgentCard.tsx";
import { DEFAULT_SETTINGS, parseSettings } from "../src/settings.ts";
import { sessionQuery, type ConnectionOptions } from "../src/terminal/connection.ts";
import {
  agentDisplay,
  createDashboard,
  dashboardReducer,
  tabAgentDisplay,
  toSaved,
  type Dashboard,
} from "../src/workspace.ts";

const NO_DETAILS = { since: null, started: null, conversation: null, resume: null, cwd: null, version: null, model: null };
const claude = (status: AgentState["status"]): AgentState => ({ kind: "claude", status, ...NO_DETAILS });
const reduce = (state: Dashboard, ...actions: Parameters<typeof dashboardReducer>[1][]) =>
  actions.reduce(dashboardReducer, state);
const session = (state: Dashboard, id: string) => state.sessions.find((item) => item.id === id)!;

/** Tab t1 with shell "a", tab t2 with shell "b"; t2 is in view. */
function twoTabs(): Dashboard {
  return reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "a", kind: "shell" },
    { type: "newTab", id: "t2" },
    { type: "open", id: "b", kind: "shell" },
  );
}

test("a pane shows the agent the host reports, and loses it when the agent or the shell ends", () => {
  let state = reduce(createDashboard("w1", "t1"), { type: "open", id: "a", kind: "shell" });
  assert.equal(agentDisplay(session(state, "a")), null);
  state = reduce(state, { type: "agent", id: "a", agent: claude("working") });
  assert.equal(agentDisplay(session(state, "a")), "working");
  state = reduce(state, { type: "agent", id: "a", agent: null });
  assert.equal(agentDisplay(session(state, "a")), null);
  state = reduce(state, { type: "agent", id: "a", agent: claude("waiting") }, { type: "status", id: "a", status: "exited" });
  assert.equal(session(state, "a").agent, null, "a shell that exited has no agent");
});

test("an agent that finishes out of view shows done until its pane is seen", () => {
  let state = reduce(twoTabs(), { type: "agent", id: "a", agent: claude("working") });
  assert.equal(tabAgentDisplay(state, "t1"), "working");
  state = reduce(state, { type: "agent", id: "a", agent: claude("idle") });
  assert.equal(agentDisplay(session(state, "a")), "done");
  assert.equal(tabAgentDisplay(state, "t1"), "done");
  state = reduce(state, { type: "agent", id: "a", agent: claude("idle") });
  assert.equal(agentDisplay(session(state, "a")), "done", "still unseen");
  state = reduce(state, { type: "selectTab", id: "t1" });
  assert.equal(agentDisplay(session(state, "a")), "idle", "seen once its tab shows it");
  assert.equal(tabAgentDisplay(state, "t1"), null, "an idle agent leaves the tab plain");
});

test("an agent that finishes in view is just idle", () => {
  let state = reduce(twoTabs(), { type: "agent", id: "b", agent: claude("working") }, { type: "agent", id: "b", agent: claude("idle") });
  assert.equal(agentDisplay(session(state, "b")), "idle");
  state = reduce(state, { type: "agent", id: "b", agent: claude("working") });
  assert.equal(agentDisplay(session(state, "b")), "working", "working again clears done");
});

test("a tab shows its most urgent agent", () => {
  let state = reduce(
    twoTabs(),
    { type: "open", id: "c", kind: "shell" },
    { type: "agent", id: "b", agent: claude("working") },
    { type: "agent", id: "c", agent: { kind: "codex", status: "waiting", ...NO_DETAILS } },
  );
  assert.equal(tabAgentDisplay(state, "t2"), "waiting");
  state = reduce(state, { type: "agent", id: "c", agent: null });
  assert.equal(tabAgentDisplay(state, "t2"), "working");
});

test("agents are live state, never saved with the layout", () => {
  const state = reduce(twoTabs(), { type: "agent", id: "a", agent: claude("working") });
  assert.ok(!JSON.stringify(toSaved(state)).includes("agent"));
});

test("the host's agent field is checked", () => {
  assert.deepEqual(parseAgentState({ kind: "codex", status: "idle" }), { kind: "codex", status: "idle", ...NO_DETAILS }, "an older host");
  assert.deepEqual(parseAgentState({ kind: "gemini", status: "working" }), { kind: "gemini", status: "working", ...NO_DETAILS });
  const host = {
    kind: "claude",
    status: "waiting",
    since: 1793941404068,
    started: 1793941354672,
    conversation: "84d627c8-2ed2-47e0-9e53-597e59bd574f",
    resume: "claude --resume 84d627c8-2ed2-47e0-9e53-597e59bd574f",
    cwd: "/Users/pista",
    version: "2.1.319",
    model: null,
  };
  assert.deepEqual(parseAgentState(host), host);
  const odd = parseAgentState({ ...host, since: "soon", started: -1, cwd: 7, version: "" });
  assert.deepEqual(
    odd && [odd.status, odd.since, odd.started, odd.cwd, odd.version, odd.conversation],
    ["waiting", null, null, null, null, host.conversation],
    "a wrong detail is left out, the agent stays",
  );
  assert.equal(parseAgentState(null), null);
  assert.equal(parseAgentState({ kind: "aider", status: "idle" }), null, "an agent this page does not know");
  assert.equal(parseAgentState({ kind: "claude", status: "sleeping" }), null);
  assert.equal(parseAgentState("claude"), null);
});

test("agents resume unless the settings turn it off", () => {
  assert.equal(DEFAULT_SETTINGS.resumeAgents, true);
  assert.equal(parseSettings('{"theme":"dark"}').resumeAgents, true, "settings saved before the choice");
  assert.equal(parseSettings('{"resumeAgents":false}').resumeAgents, false);
  assert.equal(parseSettings('{"resumeAgents":"no"}').resumeAgents, true);
});

test("a pane can keep its agent from resuming, and the layout remembers it", () => {
  // Saved ids have two characters or more.
  let state = reduce(
    createDashboard("w1", "t1"),
    { type: "open", id: "sa", kind: "shell" },
    { type: "open", id: "sb", kind: "shell" },
    { type: "autoResume", id: "sa", on: false },
  );
  assert.equal(session(state, "sa").autoResume, false);
  assert.equal(session(state, "sb").autoResume, true, "the default");
  const saved = toSaved(state);
  assert.deepEqual(
    saved.sessions.map((item) => item.autoResume),
    [false, undefined],
    "written only when off",
  );
  const parsed = parseSavedDashboard(JSON.parse(JSON.stringify(saved)));
  assert.ok(parsed);
  const loaded = reduce(createDashboard("w9", "t9"), { type: "hydrate", saved: parsed });
  assert.deepEqual(loaded.sessions.map((item) => [item.id, item.autoResume]), [["sa", false], ["sb", true]]);
  state = reduce(state, { type: "autoResume", id: "sa", on: true });
  assert.ok(!JSON.stringify(toSaved(state)).includes("autoResume"));
});

test("the info card counts whole minutes and shortens the home folder", () => {
  assert.equal(minutesSince(1_000, 1_000 + 59_999), 0);
  assert.equal(minutesSince(1_000, 1_000 + 7 * 60_000), 7);
  assert.equal(minutesSince(5_000, 1_000), 0, "a host clock a moment ahead");
  assert.equal(homeShort("/Users/pista/westerm"), "~/westerm");
  assert.equal(homeShort("/home/me"), "~");
  assert.equal(homeShort("/Users/pistachio-x"), "~");
  assert.equal(homeShort("/srv/app"), "/srv/app");
  assert.equal(homeShort("/Users"), "/Users");
  assert.equal(homeShort("C:\\Users\\pyh55"), "~");
  assert.equal(homeShort("c:\\Users\\pyh55\\westerm"), "~\\westerm");
  assert.equal(homeShort("C:\\Users"), "C:\\Users");
  assert.equal(homeShort("D:\\work\\app"), "D:\\work\\app");
});

test("a shell keeps its place, but the layout does not save it", () => {
  let state = reduce(createDashboard("w1", "t1"), { type: "open", id: "a", kind: "shell" });
  assert.equal(session(state, "a").place, null);
  state = reduce(state, { type: "place", id: "a", place: { cwd: "/w", branch: "main" } });
  assert.deepEqual(session(state, "a").place, { cwd: "/w", branch: "main" });
  state = reduce(state, { type: "place", id: "a", place: null });
  assert.equal(session(state, "a").place, null);
  state = reduce(state, { type: "place", id: "a", place: { cwd: "/w", branch: null } });
  const saved = JSON.stringify(toSaved(state));
  for (const key of ["place", "cwd", "launch"]) assert.ok(!saved.includes(`"${key}"`), key);
});

test("ready clears the launch but keeps the folder", () => {
  const launch = { kind: "claude" as const, conversation: null };
  let state = reduce(createDashboard("w1", "t1"), { type: "open", id: "a", kind: "shell", cwd: "/w", launch });
  assert.equal(session(state, "a").cwd, "/w");
  assert.deepEqual(session(state, "a").launch, launch);
  state = reduce(state, { type: "ready", id: "a", pid: 7, mode: "new" });
  assert.equal(session(state, "a").launch, null);
  assert.equal(session(state, "a").cwd, "/w");
  const plain = reduce(createDashboard("w1", "t1"), { type: "open", id: "p", kind: "shell" });
  assert.equal(session(plain, "p").cwd, null);
  assert.equal(session(plain, "p").launch, null);
});

test("a new shell asks for its folder and agent", () => {
  const options: Pick<ConnectionOptions, "session" | "kind" | "url" | "cwdFrom" | "cwd" | "launch" | "target" | "lang" | "resumeAgents" | "size"> = {
    session: "s1",
    kind: "shell",
    url: null,
    cwdFrom: null,
    cwd: "/w",
    launch: { kind: "gemini", conversation: "c-1" },
    target: null,
    lang: "en",
    resumeAgents: true,
    size: { cols: 80, rows: 24, width: 800, height: 600 },
  };
  const query = sessionQuery(options, "w1");
  assert.equal(query.get("cwd"), "/w");
  assert.equal(query.get("agent"), "gemini");
  assert.equal(query.get("conversation"), "c-1");
  const bare = sessionQuery({ ...options, cwd: null, launch: { kind: "codex", conversation: null } }, "w1");
  assert.equal(bare.get("cwd"), null);
  assert.equal(bare.get("agent"), "codex");
  assert.equal(bare.get("conversation"), null);
  const plain = sessionQuery({ ...options, launch: null }, "w1");
  assert.equal(plain.get("cwd"), "/w");
  assert.equal(plain.get("agent"), null);
  assert.equal(plain.get("conversation"), null);
});
