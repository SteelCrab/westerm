import assert from "node:assert/strict";
import { test } from "node:test";
import { isPermanentFailure, isSessionKind } from "../shared/protocol.ts";
import { parseSavedWorkspace } from "../shared/workspace-file.ts";
import { MESSAGES } from "../src/i18n/messages.ts";
import { sshCommands } from "../src/paletteCommands.tsx";
import { shouldPostRemoteClose } from "../src/remoteApi.ts";
import { createWorkspace, MAX_RECONNECTS, workspaceReducer } from "../src/workspace.ts";

const layout = (session: Record<string, unknown>) =>
  parseSavedWorkspace({
    version: 1,
    activeTabId: "t1",
    tabs: [{ id: "t1", name: null, paneIds: ["sa"], focusedId: "sa", maximizedId: null }],
    sessions: [{ id: "sa", title: "westerm prod", name: null, url: null, ...session }],
  });

test("saved remote sessions need a valid destination", () => {
  assert.equal(layout({ kind: "remote", target: "prod" })?.sessions[0].target, "prod");
  assert.equal(layout({ kind: "remote" }), null);
  assert.equal(layout({ kind: "remote", target: "-oProxyCommand=x" }), null);
});

const openRemote = () => workspaceReducer(createWorkspace("t1"), { type: "open", id: "sr1", kind: "remote", target: "prod" });

test("remote panes get a westerm title", () => {
  const state = openRemote();
  assert.equal(state.sessions[0].title, "westerm prod");
  assert.equal(state.sessions[0].remote, null);
});

test("remote panes keep reconnecting; shells still give up", () => {
  let remote = openRemote();
  for (let i = 0; i < 20; i++) remote = workspaceReducer(remote, { type: "disconnected", id: "sr1" });
  assert.equal(remote.sessions[0].status, "disconnected");
  assert.equal(remote.sessions[0].notice, "reconnecting");
  let shell = workspaceReducer(createWorkspace("t1"), { type: "open", id: "ss1", kind: "shell" });
  for (let i = 0; i <= MAX_RECONNECTS; i++) shell = workspaceReducer(shell, { type: "disconnected", id: "ss1" });
  assert.equal(shell.sessions[0].status, "error");
});

test("remote stages are kept and cleared on restart", () => {
  let state = workspaceReducer(openRemote(), {
    type: "remoteStage",
    id: "sr1",
    stage: { stage: "connected", via: "quic", killOnLogout: false },
  });
  assert.deepEqual(state.sessions[0].remote, { stage: "connected", via: "quic", killOnLogout: false });
  state = workspaceReducer(state, { type: "ready", id: "sr1", pid: 7, mode: "new" });
  assert.equal(state.sessions[0].remote?.stage, "connected", "ready keeps how the pane is connected");
  state = workspaceReducer(state, { type: "restart", id: "sr1", manual: true });
  assert.equal(state.sessions[0].remote, null);
});

test("the ssh picker offers westerm for each host", () => {
  const calls: unknown[][] = [];
  const commands = sshCommands([{ alias: "prod", hostname: null, user: null }], null, (...args) => calls.push(args), MESSAGES.en);
  const westerm = commands.find((command) => command.id === "remote-prod");
  assert.ok(westerm, "a westerm entry");
  assert.equal(westerm.title, "westerm prod");
  westerm.run();
  assert.deepEqual(calls, [["remote", null, "prod"]]);
  assert.ok(commands.some((command) => command.id === "ssh-prod"), "plain ssh stays");
});

test("remote panes report a close the socket cannot carry", () => {
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 3), true, "closed socket");
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 2), true, "closing socket");
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 1), false, "open socket carries it");
  assert.equal(shouldPostRemoteClose("remote", "prod", true, 0), false, "a connecting socket sends it once open");
  assert.equal(shouldPostRemoteClose("remote", "prod", false, 3), false, "only detaching");
  assert.equal(shouldPostRemoteClose("shell", null, true, 3), false);
});

test("remote is a session kind and only some failures are permanent", () => {
  assert.equal(isSessionKind("remote"), true);
  assert.equal(isPermanentFailure("auth"), true);
  assert.equal(isPermanentFailure("newerRemote"), true);
  assert.equal(isPermanentFailure("unreachable"), false);
  assert.equal(isPermanentFailure("transport"), false);
});
