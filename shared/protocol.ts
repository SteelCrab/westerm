// Messages between the browser and the dashboard server (native/, Rust).
// Keep in step with native/src/protocol.rs, which validates the same fields.

/**
 * shell: login shell · browser: terminal-browser · editor: terminal-code (tode) · ssh: remote shell
 */
export type SessionKind = "shell" | "browser" | "editor" | "ssh";

const SESSION_KINDS: readonly SessionKind[] = ["shell", "browser", "editor", "ssh"];

/** What a pane shows: a session in the PTY host, or the docs pane, which has no process. */
export type PaneKind = SessionKind | "docs";

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
  /** One pixel update is drawn; the host may send the next. */
  | { type: "painted" }
  /** The user closed the pane: end the process and forget the session. */
  | { type: "close" };

/**
 * How a pixel app's pane takes its pictures (WebSocket ?pixels=): raw
 * rectangles locally, deflated ones in a remote window. Without it the host
 * passes the app's Kitty images through, base64 and all.
 */
export type PixelCodec = "raw" | "deflate";

/**
 * After a "ready" with `pixels: true`, every binary message starts with a
 * kind byte: terminal output, or a pixel update (native/src/pixels/mod.rs).
 */
export const KIND_TEXT = 0;
export const KIND_PIXELS = 1;
/** Pixel update flag: rectangle data is raw deflate. */
export const PIXEL_FLAG_DEFLATE = 1;

/**
 * new: a fresh process. reattached: the same live process, screen replayed.
 * revived: a new process after the old one ended (program restart or reboot),
 * with the saved screen and directory restored.
 */
export type AttachMode = "new" | "reattached" | "revived";

/** How the dashboard reaches a remote westerm. */
export type RemoteVia = "quic" | "tunnel";

/** Why a remote westerm could not connect; the UI shows each in its language. */
export type RemoteFailure =
  | "auth"
  | "hostKey"
  | "unreachable"
  | "unsupported"
  | "download"
  | "devBinary"
  | "newerRemote"
  | "incompatible"
  | "transport"
  | "setup";

/** Where a remote connection is while the dashboard prepares the server. */
export type RemoteStage =
  | { stage: "checking" }
  | { stage: "installing"; version: string }
  | { stage: "starting" }
  | { stage: "connecting" }
  /** The server's host is an older build with running shells: replace it? */
  | { stage: "confirm"; sessions: number }
  | { stage: "connected"; via: RemoteVia; killOnLogout: boolean }
  | { stage: "failed"; reason: RemoteFailure; detail: string };

/** False for failures that a later try can get past on its own. Same rule as native/src/protocol.rs. */
export function isPermanentFailure(reason: RemoteFailure): boolean {
  return reason !== "unreachable" && reason !== "transport";
}

/** An AI agent in a shell pane, found by the PTY host (native/src/host/agent.rs). */
export type AgentKind = "claude" | "codex" | "gemini";
/** waiting: the agent asks the user, such as a permission or approval question. */
export type AgentStatus = "working" | "waiting" | "idle";

/** The agent, its state, and what its info card shows; null for what the host does not know. */
export interface AgentState {
  kind: AgentKind;
  status: AgentStatus;
  /** When this status began, in milliseconds since the epoch. */
  since: number | null;
  /** When the host first saw this agent, in milliseconds since the epoch. */
  started: number | null;
  conversation: string | null;
  /** The command a restored shell types to resume it. */
  resume: string | null;
  cwd: string | null;
  version: string | null;
  /** The model its command line names. */
  model: string | null;
}

/** The host's agent field, or null for anything this page does not know (a newer host). */
export function parseAgentState(value: unknown): AgentState | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const kinds: unknown[] = ["claude", "codex", "gemini"];
  const statuses: unknown[] = ["working", "waiting", "idle"];
  if (!kinds.includes(raw.kind) || !statuses.includes(raw.status)) return null;
  // The details only fill the info card: a wrong one is left out, not the agent.
  const time = (field: unknown) => (typeof field === "number" && Number.isFinite(field) && field > 0 ? field : null);
  const text = (field: unknown) => (typeof field === "string" && field.length > 0 && field.length <= 4096 ? field : null);
  return {
    kind: raw.kind as AgentKind,
    status: raw.status as AgentStatus,
    since: time(raw.since),
    started: time(raw.started),
    conversation: text(raw.conversation),
    resume: text(raw.resume),
    cwd: text(raw.cwd),
    version: text(raw.version),
    model: text(raw.model),
  };
}

/** What an agent launch asks the host for: the agent to start in a new shell, and its conversation to continue. */
export interface AgentLaunch {
  kind: AgentKind;
  conversation: string | null;
}

/**
 * What a plugin launch asks the host for: the plugin, its action, and the
 * thing it acts on. The host checks all three and builds the command itself.
 */
export interface PluginLaunch {
  plugin: string;
  action: string;
  target: string;
}

/** The action of an external plugin's shell pane. Its target is the argv as a JSON array. */
export const PLUGIN_RUN_ACTION = "run";
/** Most words in an external launch, the program included. */
export const MAX_PLUGIN_ARGS = 32;
/** Longest word in an external launch, in UTF-8 bytes. */
export const MAX_PLUGIN_ARG_BYTES = 1024;

/**
 * An argv the host accepts for an external launch: 1 to MAX_PLUGIN_ARGS
 * strings, each at most MAX_PLUGIN_ARG_BYTES in UTF-8, free of control
 * characters (C0, DEL, C1) and of lone surrogates (serde refuses them in the
 * JSON target), whose first word is a program name, not a path or
 * an option. The same rules as `parse_argv` in native/src/plugins/external.rs.
 */
export function isLaunchArgv(value: unknown): value is string[] {
  if (!Array.isArray(value) || value.length < 1 || value.length > MAX_PLUGIN_ARGS) return false;
  const encoder = new TextEncoder();
  return (
    value.every(
      (word) =>
        typeof word === "string" &&
        encoder.encode(word).length <= MAX_PLUGIN_ARG_BYTES &&
        !/[\u0000-\u001f\u007f-\u009f]/.test(word) &&
        !/\p{Cs}/u.test(word),
    ) && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value[0])
  );
}

/**
 * The launch that opens a shell pane running `argv` for the external plugin
 * `id`. This is the only encoding of an external launch: check the argv with
 * isLaunchArgv first, or the host refuses the session.
 */
export function externalLaunch(id: string, argv: readonly string[]): PluginLaunch {
  return { plugin: id, action: PLUGIN_RUN_ACTION, target: JSON.stringify(argv) };
}

/** A shell's current folder and the git branch there. */
export interface Place {
  cwd: string;
  branch: string | null;
}

/** The host's place field, or null for anything this page does not know. */
export function parsePlace(value: unknown): Place | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const text = (field: unknown) => (typeof field === "string" && field.length > 0 && field.length <= 4096 ? field : null);
  const cwd = text(raw.cwd);
  return cwd === null ? null : { cwd, branch: text(raw.branch) };
}

export type ServerMessage =
  /** `pixels`: the host sends pixel updates (see KIND_TEXT). */
  | { type: "ready"; pid: number; mode: AttachMode; pixels?: boolean }
  /** After "ready" and on every change in a shell: the agent in its foreground, or null. */
  | { type: "agent"; agent: AgentState | null }
  /** After "ready" and when the folder or branch changes in a shell: where it is, or null. */
  | { type: "place"; place: Place | null }
  | { type: "exit"; code: number; signal: number }
  | { type: "error"; message: string }
  /** First on a remote window's status stream: the local westerm's version line. */
  | { type: "hello"; version: string }
  /** On a remote window's status stream: where the link to the server is. */
  | ({ type: "remote" } & RemoteStage);

export const WS_PATH = "/ws/session";
/** Another window attached to the same session. Do not reconnect automatically. */
export const CLOSE_TAKEN_OVER = 4001;
/**
 * WebSocket ?target=: a remote window's status stream ("hello", then each stage).
 * The dashboard keeps the link to the server up while a window watches it.
 */
export const REMOTE_LINK_PATH = "/api/remote/link";
/** POST {target}: try again after a permanent failure. 409 when nothing waits. */
export const REMOTE_RETRY_PATH = "/api/remote/retry";
/** POST {target, replace}: the answer to a remote host replacement question. */
export const REMOTE_DECISION_PATH = "/api/remote/decision";
/** POST {target, session}: end a remote session whose pane closed while disconnected. */
export const REMOTE_CLOSE_PATH = "/api/remote/close";
/** POST {target}: replace the server's westerm host now. */
export const REMOTE_UPDATE_HOST_PATH = "/api/remote/update-host";
/** WebSocket (dashboard origin only): the tunnel's status as JSON, now and on every change. */
export const TUNNEL_STATUS_PATH = "/api/tunnel/status";
/** POST {} (dashboard origin only): start, stop, or look for cloudflared again. */
export const TUNNEL_START_PATH = "/api/tunnel/start";
export const TUNNEL_STOP_PATH = "/api/tunnel/stop";
export const TUNNEL_CHECK_PATH = "/api/tunnel/check";
/** GET: whether this page came through a tunnel, signed in, locked. Local dashboards say {tunnel: false}. */
export const TUNNEL_AUTH_PATH = "/api/tunnel-auth";
/** POST {code}: the access code for the tunnel cookie. */
export const TUNNEL_LOGIN_PATH = "/api/tunnel-auth/login";
/** The tunnel ended on purpose: a tunnel page stops reconnecting. Same value in native/src/server/cloudflare/mod.rs. */
export const CLOSE_TUNNEL_ENDED = 4010;

export type TunnelPhase = "off" | "starting" | "on" | "reconnecting" | "missing" | "failed";
export type TunnelReason = "stopped" | "idle" | "tooManyCodes" | "timeout" | "exited" | "listen";
export type TunnelOs = "macos" | "linux" | "windows";

export interface TunnelDevice {
  /** "iPhone · Safari" */
  label: string;
  /** Milliseconds since the epoch. */
  lastSeen: number;
  connected: boolean;
}

/** native/src/server/cloudflare/manager.rs `Status`. Times are milliseconds since the epoch. */
export interface TunnelStatus {
  phase: TunnelPhase;
  os: TunnelOs;
  url: string | null;
  code: string | null;
  startedAt: number | null;
  devices: TunnelDevice[];
  idleDeadline: number | null;
  keepAwake: boolean;
  reason: TunnelReason | null;
  /** cloudflared's last lines when it failed. */
  log: string[];
  localPort: number | null;
}

export interface TunnelAuth {
  tunnel: boolean;
  signedIn: boolean;
  /** Seconds until logins open again. */
  lockedFor: number | null;
}

const TUNNEL_PHASES: readonly string[] = ["off", "starting", "on", "reconnecting", "missing", "failed"];
const TUNNEL_REASONS: readonly string[] = ["stopped", "idle", "tooManyCodes", "timeout", "exited", "listen"];
const TUNNEL_OSES: readonly string[] = ["macos", "linux", "windows"];

const textOrNull = (value: unknown) => (typeof value === "string" ? value : null);
const numberOrNull = (value: unknown) => (typeof value === "number" && Number.isFinite(value) ? value : null);

/** The status stream's message, or null for anything this page does not know. */
export function parseTunnelStatus(value: unknown): TunnelStatus | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (typeof raw.phase !== "string" || !TUNNEL_PHASES.includes(raw.phase)) return null;
  if (typeof raw.os !== "string" || !TUNNEL_OSES.includes(raw.os)) return null;
  const devices = (Array.isArray(raw.devices) ? raw.devices : []).flatMap((item): TunnelDevice[] => {
    if (!item || typeof item !== "object") return [];
    const { label, lastSeen, connected } = item as Record<string, unknown>;
    return typeof label === "string" && typeof lastSeen === "number" && typeof connected === "boolean"
      ? [{ label, lastSeen, connected }]
      : [];
  });
  const reason = typeof raw.reason === "string" && TUNNEL_REASONS.includes(raw.reason) ? (raw.reason as TunnelReason) : null;
  return {
    phase: raw.phase as TunnelPhase,
    os: raw.os as TunnelOs,
    url: textOrNull(raw.url),
    code: textOrNull(raw.code),
    startedAt: numberOrNull(raw.startedAt),
    devices,
    idleDeadline: numberOrNull(raw.idleDeadline),
    keepAwake: raw.keepAwake === true,
    reason,
    log: Array.isArray(raw.log) ? raw.log.filter((line): line is string => typeof line === "string") : [],
    localPort: numberOrNull(raw.localPort),
  };
}

/** The answer of TUNNEL_AUTH_PATH. A local dashboard sends only `tunnel: false`. */
export function parseTunnelAuth(value: unknown): TunnelAuth | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (typeof raw.tunnel !== "boolean") return null;
  return { tunnel: raw.tunnel, signedIn: raw.signedIn === true, lockedFor: numberOrNull(raw.lockedFor) };
}

export const CONFIG_PATH = "/api/terminal-config";
/** Font families installed on the computer that runs the server (and the browser). */
export const SYSTEM_FONTS_PATH = "/api/system-fonts";
/** Concrete Host aliases from ~/.ssh/config. */
export const SSH_HOSTS_PATH = "/api/ssh-hosts";
/** GET: whether a newer release exists. POST (dashboard origin only): install it and restart. */
export const UPDATE_PATH = "/api/update";

/** binary: an installed executable · app: inside the westerm desktop app · source: built from source */
export type UpdateChannel = "binary" | "app" | "source";

export interface UpdateStatus {
  current: string;
  channel: UpdateChannel;
  latest?: string;
  newer?: boolean;
  /** The release page with its notes. */
  page?: string;
  /** Why the check failed. */
  error?: string;
  /** WESTERM_NO_UPDATE_CHECK is set. */
  disabled?: boolean;
}

/** Terminal typography read from the user's Ghostty config. */
export interface TerminalConfig {
  /** Ghostty `font-family` values in fallback order; empty when unset. */
  fontFamilies: string[];
  /** Ghostty `font-size` in points; null when unset. */
  fontSize: number | null;
}

/** One installed font family, for Settings ▸ Terminal ▸ Font. */
export interface SystemFont {
  /** The family name CSS `font-family` takes. */
  family: string;
  /** Latin letters and digits have one width, as a terminal needs. */
  mono: boolean;
  /** Has Korean Hangul. */
  hangul: boolean;
  /** Has Japanese kana. */
  kana: boolean;
  /** Has Chinese characters. */
  han: boolean;
  /** Has the Nerd Font icons that shell prompts use. */
  icons: boolean;
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
