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
    wsl: false,
  });
});

test("terminal-code with exit 127 is missing, with the tode install command", () => {
  assert.deepEqual(installHint("editor", 127, false), {
    type: "missing",
    program: "terminal-code",
    command: EDITOR_COMMAND,
    remote: false,
    wsl: false,
  });
});

test("another non-zero exit of a program is a failed start, without a command", () => {
  const hint = installHint("browser", 1, false);
  assert.deepEqual(hint, { type: "failed", program: "terminal-browser", remote: false, wsl: false });
  assert.ok(hint && !("command" in hint));
  assert.deepEqual(installHint("editor", 2, true), { type: "failed", program: "terminal-code", remote: true, wsl: false });
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

test("on Windows the programs run in WSL, so the command installs them there", () => {
  assert.deepEqual(installHint("browser", 127, false, true), {
    type: "missing",
    program: "terminal-browser",
    command: `wsl --exec bash -c "${BROWSER_COMMAND}"`,
    remote: false,
    wsl: true,
  });
  const editor = installHint("editor", 127, false, true);
  assert.equal(editor?.type === "missing" ? editor.command : null, `wsl --exec bash -c "${EDITOR_COMMAND}"`);
  assert.deepEqual(installHint("browser", 1, false, true), { type: "failed", program: "terminal-browser", remote: false, wsl: true });
});

test("a remote window runs the programs on the server, also from Windows", () => {
  assert.deepEqual(installHint("browser", 127, true, true), installHint("browser", 127, true, false));
  assert.equal(installHint("editor", 1, true, true)?.wsl, false);
});

test("every language words both hints, local and remote", () => {
  for (const [locale, text] of Object.entries(MESSAGES)) {
    const hint = text.pane.hint;
    for (const remote of [false, true]) {
      assert.ok(hint.missing("terminal-browser", remote).includes("terminal-browser"), `${locale} missing names the program`);
    }
    assert.ok(hint.failed("terminal-browser").includes("terminal-browser"), `${locale} failed names the program`);
    assert.ok(hint.missingWsl("terminal-browser").includes("WSL") && hint.missingWsl("terminal-browser").includes("terminal-browser"), `${locale} missing in WSL`);
    assert.ok(hint.wslSetup.includes("wsl --install"), `${locale} says how to set up WSL`);
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
