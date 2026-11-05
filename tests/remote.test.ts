import assert from "node:assert/strict";
import { test } from "node:test";
import { CONFIG_PATH, isPermanentFailure, isSessionKind, SSH_HOSTS_PATH, WS_PATH } from "../shared/protocol.ts";
import { parseSavedDashboard, WORKSPACE_PATH } from "../shared/workspace-file.ts";
import { MESSAGES } from "../src/i18n/messages.ts";
import { remoteWindowCommands, sshCommands } from "../src/paletteCommands.tsx";
import { openRemoteWindow, remoteTargetFrom, serverPath, shouldPostRemoteClose } from "../src/remoteWindow.ts";
import { INITIAL_LINK, linkReducer } from "../src/useRemoteLink.ts";
import { firstLoad, layoutKey, reclaimAction, reconnectAction, writtenKey } from "../src/useWorkspacePersistence.ts";
import { activeTab, createDashboard, dashboardReducer, MAX_RECONNECTS, toSaved } from "../src/workspace.ts";

const layout = (session: Record<string, unknown>) =>
  parseSavedDashboard({
    version: 1,
    activeTabId: "t1",
    tabs: [{ id: "t1", name: null, paneIds: ["sa"], focusedId: "sa", maximizedId: null }],
    sessions: [{ id: "sa", title: "westerm prod", name: null, url: null, ...session }],
  });

test("v0.2.0 remote panes leave the saved layout; the rest stays", () => {
  const saved = parseSavedDashboard({
    version: 1,
    activeTabId: "t1",
    tabs: [
      { id: "t1", name: null, paneIds: ["sa", "sr"], focusedId: "sr", maximizedId: null },
      { id: "t2", name: null, paneIds: ["sq"], focusedId: "sq", maximizedId: null },
    ],
    sessions: [
      { id: "sa", kind: "shell", title: "zsh", name: null, url: null },
      { id: "sr", kind: "remote", target: "prod", title: "westerm prod", name: null, url: null },
      { id: "sq", kind: "remote", target: "prod", title: "westerm prod", name: null, url: null },
    ],
  });
  assert.ok(saved);
  assert.deepEqual(saved.legacyRemoteIds, ["sr", "sq"]);
  const state = dashboardReducer(createDashboard("w1", "t1"), { type: "hydrate", saved });
  assert.deepEqual(state.sessions.map((session) => session.id), ["sa"]);
  assert.equal(state.tabs.length, 1, "a tab left empty goes");
  assert.deepEqual(state.tabs[0].paneIds, ["sa"]);
  assert.equal(activeTab(state).focusedId, "sa");
});

test("a workspace whose only tab held remote panes keeps one empty tab", () => {
  const saved = layout({ kind: "remote", target: "prod" });
  assert.ok(saved);
  const state = dashboardReducer(createDashboard("w1", "t1"), { type: "hydrate", saved });
  assert.equal(state.sessions.length, 0);
  assert.equal(state.tabs.length, 1);
  assert.deepEqual(activeTab(state).paneIds, []);
});

test("a saved remote pane still needs a valid destination", () => {
  assert.equal(layout({ kind: "remote", target: "-oProxyCommand=x" }), null);
  assert.equal(layout({ kind: "remote" }), null);
});

test("remote is no longer a session kind", () => {
  assert.equal(isSessionKind("remote"), false);
});

test("the ssh picker offers no westerm panes", () => {
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, null, MESSAGES.en);
  assert.equal(commands.some((command) => command.id.startsWith("remote-")), false);
  assert.ok(commands.some((command) => command.id === "ssh-prod"));
});

test("the ssh picker opens each host in a new window", () => {
  const opened: string[] = [];
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, (target) => opened.push(target), MESSAGES.en);
  const entry = commands.find((command) => command.id === "window-prod");
  assert.ok(entry);
  assert.equal(entry.title, MESSAGES.en.palette.openRemoteWindow("prod"));
  entry.run();
  assert.deepEqual(opened, ["prod"]);
  assert.ok(commands.some((command) => command.id === "ssh-prod"), "plain ssh stays");
});

test("a typed user@host opens in a new window too", () => {
  const commands = sshCommands([], "pista@msi", () => {}, () => {}, MESSAGES.en);
  assert.ok(commands.some((command) => command.id === "window-pista@msi"));
});

test("a remote window offers no windows of its own", () => {
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, () => {}, null, MESSAGES.en);
  assert.equal(commands.some((command) => command.id.startsWith("window-")), false);
});

test("a remote window opens once per server and is focused after", () => {
  const calls: unknown[][] = [];
  let focused = 0;
  const handle = { closed: false, focus: () => void focused++ } as unknown as Window;
  const open = (...args: [string, string, string]) => (calls.push(args), handle);
  openRemoteWindow("pista@msi", open);
  openRemoteWindow("pista@msi", open);
  assert.deepEqual(calls, [["/?remote=pista%40msi", "westerm-remote-pista%40msi", "popup,width=1280,height=820"]]);
  assert.equal(focused, 1);
});

test("in the app, every open asks the app for the window", () => {
  const posted: unknown[] = [];
  const opened: unknown[] = [];
  const open = (...args: [string, string, string]) => (opened.push(args), null);
  const post = (message: { type: "remoteWindow"; target: string }) => void posted.push(message);
  openRemoteWindow("pista@msi", open, post);
  openRemoteWindow("pista@msi", open, post);
  assert.deepEqual(posted, [
    { type: "remoteWindow", target: "pista@msi" },
    { type: "remoteWindow", target: "pista@msi" },
  ]);
  assert.deepEqual(opened, []);
});

test("only some remote failures are permanent", () => {
  assert.equal(isPermanentFailure("auth"), true);
  assert.equal(isPermanentFailure("newerRemote"), true);
  assert.equal(isPermanentFailure("unreachable"), false);
  assert.equal(isPermanentFailure("transport"), false);
});

test("a remote window names its server in the address", () => {
  assert.equal(remoteTargetFrom("?remote=prod"), "prod");
  assert.equal(remoteTargetFrom("?remote=pista%40msi"), "pista@msi");
  assert.equal(remoteTargetFrom("?remote=-oProxyCommand%3Dx"), null);
  assert.equal(remoteTargetFrom(""), null);
});

test("a remote window sends layout, host list, and sessions to its server", () => {
  assert.equal(serverPath(WORKSPACE_PATH, "prod"), "/remote/prod/api/workspace");
  assert.equal(serverPath(SSH_HOSTS_PATH, "pista@msi"), "/remote/pista%40msi/api/ssh-hosts");
  assert.equal(serverPath(WS_PATH, "prod"), "/remote/prod/ws/session");
  assert.equal(serverPath(CONFIG_PATH, "prod"), CONFIG_PATH, "fonts stay local");
  assert.equal(serverPath(WORKSPACE_PATH, null), WORKSPACE_PATH);
});

/** A small saved layout with one shell. */
const SAMPLE_SAVED = layout({ kind: "shell" });

test("the first load never replaces a server layout it could not read", () => {
  assert.equal(firstLoad(true, { ok: false }), "retry");
  assert.equal(firstLoad(false, { ok: false }), "fresh");
  assert.equal(firstLoad(true, { ok: true, saved: null }), "fresh");
  assert.equal(firstLoad(true, { ok: true, saved: SAMPLE_SAVED }), "hydrate");
  const legacyOnly = layout({ kind: "remote", target: "prod" });
  assert.ok(legacyOnly);
  assert.equal(firstLoad(true, { ok: true, saved: legacyOnly }), "fresh", "nothing left but v0.2.0 remote panes");
  assert.equal(firstLoad(false, { ok: true, saved: legacyOnly }), "fresh");
});

test("a remote window takes its sessions back only with the server's layout in hand", () => {
  assert.equal(reclaimAction(true, { ok: false }), "stay", "a stale layout would replace the server's");
  assert.deepEqual(reclaimAction(true, { ok: true, saved: null }), { saved: null });
  assert.deepEqual(reclaimAction(true, { ok: true, saved: SAMPLE_SAVED }), { saved: SAMPLE_SAVED });
  assert.deepEqual(reclaimAction(false, { ok: false }), { saved: null }, "a local window reclaims as before");
});

test("the link state follows the stream", () => {
  let link = linkReducer(INITIAL_LINK, { type: "hello", version: "westerm 0.3.0 (host rs-a)" });
  link = linkReducer(link, { type: "remote", stage: "checking" });
  assert.equal(link.connected, false);
  link = linkReducer(link, { type: "remote", stage: "connected", via: "quic", killOnLogout: false });
  assert.ok(link.connected && link.connectedOnce);
  link = linkReducer(link, { type: "remote", stage: "failed", reason: "unreachable", detail: "" });
  assert.ok(!link.connected && link.connectedOnce && !link.reload);
  link = linkReducer(link, { type: "hello", version: "westerm 0.3.1 (host rs-b)" });
  assert.equal(link.reload, true, "a new local westerm reloads the window");
});

test("remote window panes keep reconnecting; local shells still give up", () => {
  let remote = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i < 20; i++) remote = dashboardReducer(remote, { type: "disconnected", id: "ss1", keepTrying: true });
  assert.equal(remote.sessions[0].notice, "reconnecting");
  let local = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i <= MAX_RECONNECTS; i++) local = dashboardReducer(local, { type: "disconnected", id: "ss1", keepTrying: false });
  assert.equal(local.sessions[0].status, "error");
});

test("a pane closed while its socket is down reports the close", () => {
  assert.equal(shouldPostRemoteClose("prod", true, 3), true, "closed socket");
  assert.equal(shouldPostRemoteClose("prod", true, 2), true, "closing socket");
  assert.equal(shouldPostRemoteClose("prod", true, 1), false, "an open socket carries it");
  assert.equal(shouldPostRemoteClose("prod", true, 0), false, "a connecting socket sends it once open");
  assert.equal(shouldPostRemoteClose("prod", false, 3), false, "only detaching");
  assert.equal(shouldPostRemoteClose(null, true, 3), false, "a local window");
});

test("a remote window's palette updates its server's host", () => {
  const [command] = remoteWindowCommands("prod", MESSAGES.en);
  assert.equal(command.title, MESSAGES.en.palette.updateThisHost);
  assert.match(command.keywords, /update/);
});

test("a window that comes back reads the server's layout before it saves", () => {
  const synced = layoutKey(SAMPLE_SAVED);
  assert.equal(reconnectAction({ ok: false }, synced, true), "retry", "never saves blind");
  assert.equal(reconnectAction({ ok: true, saved: SAMPLE_SAVED }, synced, false), "none", "nothing changed anywhere");
  assert.equal(reconnectAction({ ok: true, saved: SAMPLE_SAVED }, synced, true), "save", "a save failed while the link was down");
  const newer = layout({ kind: "editor" });
  assert.ok(newer);
  assert.equal(reconnectAction({ ok: true, saved: newer }, synced, true), "adopt", "another window wrote it");
  assert.equal(reconnectAction({ ok: true, saved: newer }, synced, false), "adopt");
  assert.equal(reconnectAction({ ok: true, saved: null }, synced, false), "save", "nothing on the server to lose");
});

test("a layout this window wrote reads back as the one it last synced", () => {
  const state = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "sa", kind: "shell" });
  const body = JSON.stringify(toSaved(state));
  const read = parseSavedDashboard(JSON.parse(body));
  assert.equal(reconnectAction({ ok: true, saved: read }, writtenKey(body), false), "none");
});

test("a window that comes back takes the server's layout, and its panes connect again", () => {
  let state = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "sa", kind: "shell" });
  state = dashboardReducer(state, { type: "disconnected", id: "sa", keepTrying: true });
  const newer = layout({ kind: "shell" });
  assert.ok(newer);
  const adopted = dashboardReducer(state, { type: "adopt", saved: newer });
  assert.deepEqual(adopted.sessions.map((session) => session.id), ["sa"]);
  assert.equal(adopted.sessions[0].status, "connecting");
  assert.equal(adopted.sessions[0].generation, state.sessions[0].generation + 1, "a new terminal connects again");
});

test("the link coming back retries the disconnected panes at once", () => {
  let state = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "sa", kind: "shell" });
  state = dashboardReducer(state, { type: "open", id: "sb", kind: "shell" });
  state = dashboardReducer(state, { type: "ready", id: "sb", pid: 7, mode: "new" });
  for (let i = 0; i < 5; i++) state = dashboardReducer(state, { type: "disconnected", id: "sa", keepTrying: true });
  const retried = dashboardReducer(state, { type: "retryDisconnected" });
  const [a, b] = retried.sessions;
  assert.equal(a.status, "connecting");
  assert.equal(a.generation, state.sessions[0].generation + 1, "a new terminal connects at once");
  assert.equal(a.retries, 0, "the backoff starts over");
  assert.equal(b, state.sessions[1], "a running pane stays as it is");
  const away = dashboardReducer(state, { type: "status", id: "sb", status: "error", notice: "takenOver" });
  assert.equal(dashboardReducer(away, { type: "retryDisconnected" }), away, "a window not in use takes nothing back");
});

test("the saved file leaves out the parse-only legacy ids", () => {
  const state = dashboardReducer(createDashboard("w1", "t1"), { type: "open", id: "sa", kind: "shell" });
  assert.equal("legacyRemoteIds" in toSaved(state), false);
});
