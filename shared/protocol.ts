// Messages between the browser and the dashboard server (native/, Rust).
// Keep in step with native/src/protocol.rs, which validates the same fields.

/** shell: login shell · browser: terminal-browser · editor: terminal-code (tode) · ssh: remote shell */
export type SessionKind = "shell" | "browser" | "editor" | "ssh";

const SESSION_KINDS: readonly SessionKind[] = ["shell", "browser", "editor", "ssh"];

export function isSessionKind(value: unknown): value is SessionKind {
  return SESSION_KINDS.includes(value as SessionKind);
}

/** Client-made ids: a letter prefix plus base36. They also name files on the server. */
export function isId(value: unknown): value is string {
  return typeof value === "string" && /^[a-z][a-z0-9]{1,40}$/.test(value);
}

/** Grid and pixel size of a terminal. Kitty clients read the pixel size from the PTY. */
export interface PtySize {
  cols: number;
  rows: number;
  /** Terminal content size in CSS pixels. */
  width: number;
  height: number;
}

export type ClientMessage =
  | { type: "input"; data: string }
  | ({ type: "resize" } & PtySize)
  | { type: "ack"; bytes: number }
  /** The user closed the pane: end the process and forget the session. */
  | { type: "close" };

/**
 * new: a fresh process. reattached: the same live process, screen replayed.
 * revived: a new process after the old one ended (program restart or reboot),
 * with the saved screen and directory restored.
 */
export type AttachMode = "new" | "reattached" | "revived";

export type ServerMessage =
  | { type: "ready"; pid: number; mode: AttachMode }
  | { type: "exit"; code: number; signal: number }
  | { type: "error"; message: string };

export const WS_PATH = "/ws/session";
/** Another window attached to the same session. Do not reconnect automatically. */
export const CLOSE_TAKEN_OVER = 4001;
export const CONFIG_PATH = "/api/terminal-config";
/** Concrete Host aliases from ~/.ssh/config. */
export const SSH_HOSTS_PATH = "/api/ssh-hosts";

/** Terminal typography read from the user's Ghostty config. */
export interface TerminalConfig {
  /** Ghostty `font-family` values in fallback order; empty when unset. */
  fontFamilies: string[];
  /** Ghostty `font-size` in points; null when unset. */
  fontSize: number | null;
}

/** The browser acknowledges parsed output in batches of at least this many bytes. */
export const ACK_THRESHOLD = 256 * 1024;

/** Only http(s) URLs or bare hosts reach terminal-browser. */
export function normalizeBrowserUrl(input: string | null | undefined): string | null {
  const text = (input ?? "").trim();
  if (!text || text.length > 2048) return null;
  if (/[\u0000-\u001f\u007f]/.test(text)) return null;
  if (/^https?:\/\//i.test(text)) return text;
  // "host:port" is fine; any other scheme (file:, javascript:, ...) is not.
  if (/^[a-z][a-z0-9+.-]*:(?!\d)/i.test(text)) return null;
  if (text.startsWith("-")) return null;
  return text;
}

/**
 * An ssh destination: a ~/.ssh/config alias or [user@]host. No leading dash
 * (it would read as an option), no spaces, no shell characters.
 */
export function normalizeSshTarget(input: string | null | undefined): string | null {
  const text = (input ?? "").trim();
  if (!text || text.length > 255) return null;
  return /^(?!-)[A-Za-z0-9._%+-]+(@(?!-)[A-Za-z0-9._:\[\]-]+)?$/.test(text) ? text : null;
}
