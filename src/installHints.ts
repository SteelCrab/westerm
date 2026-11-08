// What a pane says when its program ended without running. terminal-browser
// and terminal-code start through the login shell, which exits with 127 when
// it does not find the program. westerm never installs anything: it shows the
// official command, and the user runs it and restarts the pane.

import type { SessionKind } from "../shared/protocol.ts";

/** The shell's exit code for "command not found". */
const COMMAND_NOT_FOUND = 127;

/** The programs that panes start, with the official install command of each. */
const PROGRAMS: Partial<Record<SessionKind, { program: string; command: string }>> = {
  browser: { program: "terminal-browser", command: "curl -fsSL https://terminal-browser.sh/install | bash" },
  editor: { program: "terminal-code", command: "curl -fsSL https://tode.sh/install | bash" },
};

/**
 * `remote`: the program runs on a server (remote window), not on this computer.
 * `wsl`: this computer is Windows, where the programs run in WSL: they have
 * Linux builds only.
 */
export type InstallHint =
  | { type: "missing"; program: string; command: string; remote: boolean; wsl: boolean }
  | { type: "failed"; program: string; remote: boolean; wsl: boolean };

/**
 * The hint for a pane that ended with `code`, or null when there is nothing
 * to add. `windows`: this computer runs Windows.
 */
export function installHint(kind: SessionKind, code: number | null, remote: boolean, windows = false): InstallHint | null {
  const entry = PROGRAMS[kind];
  if (!entry || code === null || code === 0) return null;
  // A remote server is never Windows.
  const wsl = windows && !remote;
  if (code === COMMAND_NOT_FOUND) {
    // Run from PowerShell. --exec hands bash the script as one argument.
    const command = wsl ? `wsl --exec bash -c "${entry.command}"` : entry.command;
    return { type: "missing", program: entry.program, command, remote, wsl };
  }
  return { type: "failed", program: entry.program, remote, wsl };
}
