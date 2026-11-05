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

/** `remote`: the program runs on a server (remote window), not on this computer. */
export type InstallHint =
  | { type: "missing"; program: string; command: string; remote: boolean }
  | { type: "failed"; program: string; remote: boolean };

/** The hint for a pane that ended with `code`, or null when there is nothing to add. */
export function installHint(kind: SessionKind, code: number | null, remote: boolean): InstallHint | null {
  const entry = PROGRAMS[kind];
  if (!entry || code === null || code === 0) return null;
  if (code === COMMAND_NOT_FOUND) return { type: "missing", ...entry, remote };
  return { type: "failed", program: entry.program, remote };
}
