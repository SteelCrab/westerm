import assert from "node:assert/strict";
import { test } from "node:test";
import { MESSAGES } from "../src/i18n/messages.ts";
import { installHint } from "../src/installHints.ts";
import { createDashboard, dashboardReducer, type Dashboard } from "../src/workspace.ts";

const BROWSER_COMMAND = "curl -fsSL https://terminal-browser.sh/install | bash";
const EDITOR_COMMAND = "curl -fsSL https://tode.sh/install | bash";

test("terminal-browser with exit 127 is missing, with its install command", () => {
  assert.deepEqual(installHint("browser", 127, false), {
    type: "missing",
    program: "terminal-browser",
    command: BROWSER_COMMAND,
    remote: false,
  });
});

test("terminal-code with exit 127 is missing, with the tode install command", () => {
  assert.deepEqual(installHint("editor", 127, false), {
    type: "missing",
    program: "terminal-code",
    command: EDITOR_COMMAND,
    remote: false,
  });
});

test("another non-zero exit of a program is a failed start, without a command", () => {
  const hint = installHint("browser", 1, false);
  assert.deepEqual(hint, { type: "failed", program: "terminal-browser", remote: false });
  assert.ok(hint && !("command" in hint));
  assert.deepEqual(installHint("editor", 2, true), { type: "failed", program: "terminal-code", remote: true });
});

test("no hint for a clean exit, a signal, no code, or the other kinds", () => {
  assert.equal(installHint("editor", 0, false), null);
  assert.equal(installHint("browser", null, false), null);
  assert.equal(installHint("shell", 127, false), null);
  assert.equal(installHint("ssh", 127, false), null);
  assert.equal(installHint("shell", 1, true), null);
});

test("the remote flag changes only where the program runs", () => {
  for (const kind of ["browser", "editor"] as const) {
    for (const code of [127, 1]) {
      const local = installHint(kind, code, false);
      const remote = installHint(kind, code, true);
      assert.ok(local && remote);
      assert.equal(local.remote, false);
      assert.equal(remote.remote, true);
      assert.deepEqual({ ...remote, remote: false }, local);
    }
  }
});

test("every language words both hints, local and remote", () => {
  for (const [locale, text] of Object.entries(MESSAGES)) {
    const hint = text.pane.hint;
    for (const remote of [false, true]) {
      assert.ok(hint.missing("terminal-browser", remote).includes("terminal-browser"), `${locale} missing names the program`);
    }
    assert.ok(hint.failed("terminal-browser").includes("terminal-browser"), `${locale} failed names the program`);
    assert.notEqual(hint.missing("x", true), hint.missing("x", false), `${locale} says where`);
    assert.ok(hint.installThenRestart && hint.runOnServer && hint.copy && hint.copied && hint.copyFailed, `${locale} has the other strings`);
  }
});

function exitedWith(code: number | null): Dashboard {
  let state = createDashboard("w1", "t1");
  state = dashboardReducer(state, { type: "open", id: "a", kind: "browser" });
  return dashboardReducer(state, { type: "status", id: "a", status: "exited", detail: "exit 127", exitCode: code });
}

test("an exit sets the exit code and a restart clears it", () => {
  let state = exitedWith(127);
  const session = () => state.sessions.find((item) => item.id === "a");
  assert.equal(session()?.exitCode, 127);
  state = dashboardReducer(state, { type: "restart", id: "a", manual: true });
  assert.equal(session()?.exitCode, null);
});

test("a status without a code leaves no exit code", () => {
  let state = exitedWith(127);
  state = dashboardReducer(state, { type: "status", id: "a", status: "error", detail: "boom" });
  assert.equal(state.sessions.find((item) => item.id === "a")?.exitCode, null);
});
