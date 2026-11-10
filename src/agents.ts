// Pure logic behind the agents sidebar: a workspace's shells grouped by
// folder, which view the sidebar slot shows, and past conversations.

import type { AgentKind } from "../shared/protocol.ts";
import { homeShort } from "./uiHelpers.ts";
import { REMOTE_TARGET } from "./remoteWindow.ts";
import {
  activeWorkspace,
  agentDisplay,
  tabSessions,
  workspaceTabs,
  type AgentDisplay,
  type Dashboard,
  type Session,
} from "./workspace.ts";

export interface AgentFolder {
  /** The folder's path; "" for shells whose folder is not known. */
  key: string;
  path: string | null;
  name: string;
  parent: string;
  /** The branch of the first shell that sits in this folder itself and reports one. */
  branch: string | null;
  shells: number;
  /** The shells here that run an agent: most urgent first, then pane order. */
  agents: Session[];
  waiting: boolean;
}

/** How soon an agent needs the user: lower comes first. */
const URGENCY: Record<AgentDisplay, number> = { waiting: 0, done: 1, working: 2, idle: 3 };

/** `/Users/p/westerm` is `{ name: "westerm", parent: "~/" }`; the parent is the folder one level up. */
export function folderLabel(path: string): { name: string; parent: string } {
  const parts = homeShort(path).split("/").filter(Boolean);
  if (parts.length === 0) return { name: "/", parent: "" };
  const name = parts[parts.length - 1];
  return { name, parent: parts.length > 1 ? `${parts[parts.length - 2]}/` : "" };
}

/** The shells of the workspace in view, grouped by folder in the order panes first show each. */
export function agentFolders(state: Dashboard): AgentFolder[] {
  const folders = new Map<string, AgentFolder>();
  for (const tab of workspaceTabs(state, activeWorkspace(state).id)) {
    for (const session of tabSessions(state, tab.id)) {
      if (session.kind !== "shell") continue;
      const path = session.agent?.cwd ?? session.place?.cwd ?? null;
      const key = path ?? "";
      let folder = folders.get(key);
      if (!folder) {
        const label = path === null ? { name: "", parent: "" } : folderLabel(path);
        folder = { key, path, ...label, branch: null, shells: 0, agents: [], waiting: false };
        folders.set(key, folder);
      }
      folder.shells += 1;
      // A shell grouped here by its agent's folder may itself sit in another folder.
      if (session.place?.cwd === key) folder.branch ??= session.place.branch;
      if (session.agent) folder.agents.push(session);
    }
  }
  for (const folder of folders.values()) {
    // Array.sort is stable, so equal urgency keeps pane order.
    folder.agents.sort((a, b) => URGENCY[agentDisplay(a)!] - URGENCY[agentDisplay(b)!]);
    folder.waiting = folder.agents.some((session) => agentDisplay(session) === "waiting");
  }
  return [...folders.values()];
}

/**
 * A panel that can take the sidebar slot instead of the sessions list:
 * agents, Git, the tunnel, or one plugin's.
 */
export type SidebarPanel = "agents" | "git" | "tunnel" | `plugin:${string}`;

export type SidebarView = "sessions" | SidebarPanel | null;

export interface SidebarFlags {
  layout: "top" | "sidebar";
  /** The window is narrow: the sidebar is a drawer. */
  narrow: boolean;
  sidebarOpen: boolean;
  drawerOpen: boolean;
  /** The panel in the slot. Top layout: the slot opens only for one. Sidebar layout: null shows the sessions list. */
  panel: SidebarPanel | null;
}

/** What the sidebar slot shows now; null when it is closed. */
export function sidebarView(o: SidebarFlags): SidebarView {
  const open = o.narrow ? o.drawerOpen : o.layout === "top" ? o.panel !== null : o.sidebarOpen;
  if (!open) return null;
  return o.panel ?? "sessions";
}

/** The flags after the button of panel `which` is pressed. The panels share the slot. */
export function pressPanel(o: SidebarFlags, which: SidebarPanel): SidebarFlags {
  if (sidebarView(o) === which) {
    // Close a drawer; in the sidebar layout go back to the sessions list, still open.
    return o.narrow ? { ...o, drawerOpen: false } : { ...o, panel: null };
  }
  return {
    ...o,
    panel: which,
    sidebarOpen: o.sidebarOpen || o.layout === "sidebar",
    drawerOpen: o.narrow || o.drawerOpen,
  };
}

/** A built-in plugin's id (`docker`) or an external one's (`<author>.<name>`). */
const PLUGIN_PANEL = /^plugin:[a-z0-9-]{1,32}(\.[a-z0-9-]{1,32})?$/;

/** `saved` names a plugin panel that can exist. */
export function isPluginPanel(saved: string): saved is `plugin:${string}` {
  return PLUGIN_PANEL.test(saved);
}

/**
 * The panel to show: a plugin panel needs its plugin in the installed list.
 * `installed` is null while the list is being read.
 */
export function effectivePanel(panel: SidebarPanel | null, installed: readonly string[] | null): SidebarPanel | null {
  if (panel === null || !panel.startsWith("plugin:")) return panel;
  return installed?.includes(panel.slice("plugin:".length)) ? panel : null;
}

const isWaiting = (session: Session) => session.kind === "shell" && agentDisplay(session) === "waiting";

/** Agents that wait for the user in the workspaces out of view. */
export function elsewhereWaiting(state: Dashboard): number {
  const shown = new Set(workspaceTabs(state, activeWorkspace(state).id).flatMap((tab) => tab.paneIds));
  return state.sessions.filter((session) => !shown.has(session.id) && isWaiting(session)).length;
}

/** Any agent in any workspace waits for the user. */
export function anyWaiting(state: Dashboard): boolean {
  return state.sessions.some(isWaiting);
}

export interface PastConversation {
  kind: AgentKind;
  id: string;
  title: string | null;
  /** Milliseconds since the epoch. */
  updated: number;
}

/**
 * The conversations of folder `path` that no session runs now. An agent in
 * this folder that has not told its conversation (Codex, Gemini CLI, a new
 * Claude Code) hides those of its kind written since it started: one of them
 * is its own.
 */
export function pastConversations(list: PastConversation[], state: Dashboard, path: string): PastConversation[] {
  const running = new Set(state.sessions.flatMap((session) => session.agent?.conversation ?? []));
  // Both times are milliseconds since the epoch.
  const untold = state.sessions.flatMap(({ agent, place }) =>
    agent && agent.conversation === null && agent.started !== null && (agent.cwd ?? place?.cwd) === path
      ? [{ kind: agent.kind, started: agent.started }]
      : [],
  );
  return list.filter(
    (item) => !running.has(item.id) && !untold.some((agent) => agent.kind === item.kind && item.updated >= agent.started),
  );
}

/** A kept list of past conversations is read again when it is this old (ms). */
export const HISTORY_MAX_AGE = 30_000;

export type HistoryReader = (dir: string) => Promise<PastConversation[] | null>;

export interface HistoryCache {
  /** The list kept for `path`, however old: what its folder shows until a read ends. */
  shown(path: string): PastConversation[];
  /**
   * The list for `path`: the kept one while it is younger than 30 s and was
   * read with the same `running` key, else a new read. null when the read
   * fails: the folder keeps what it shows, and the next call reads again.
   */
  load(path: string, running: string): Promise<PastConversation[] | null>;
}

interface Kept {
  list: PastConversation[];
  running: string;
  at: number;
}

/** Past-conversation lists by folder, kept outside the panel so a folder that folds and unfolds reads nothing new. */
export function createHistoryCache(read: HistoryReader, clock: () => number = Date.now): HistoryCache {
  const kept = new Map<string, Kept>();
  const reading = new Map<string, { running: string; done: Promise<PastConversation[] | null> }>();
  return {
    shown: (path) => kept.get(path)?.list ?? [],
    load(path, running) {
      const entry = kept.get(path);
      if (entry && entry.running === running && clock() - entry.at < HISTORY_MAX_AGE) return Promise.resolve(entry.list);
      const pending = reading.get(path);
      if (pending && pending.running === running) return pending.done;
      const done = read(path).catch(() => null).then((list) => {
        if (reading.get(path)?.done === done) reading.delete(path);
        if (list) kept.set(path, { list, running, at: clock() });
        return list;
      });
      reading.set(path, { running, done });
      return done;
    },
  };
}

const caches = new WeakMap<HistoryReader, HistoryCache>();

/** The one cache of a reader; tests pass their own reader and get a fresh cache. */
export function historyCache(read: HistoryReader): HistoryCache {
  let cache = caches.get(read);
  if (!cache) {
    cache = createHistoryCache(read);
    caches.set(read, cache);
  }
  return cache;
}

const AGENT_KINDS: readonly string[] = ["claude", "codex", "gemini"];

/** Where this window asks for a folder's past conversations: through `/remote/<target>` in a remote window. */
export function historyPath(dir: string, target: string | null = REMOTE_TARGET): string {
  const base = target !== null ? `/remote/${encodeURIComponent(target)}/api/agents/history` : "/api/agents/history";
  return `${base}?${new URLSearchParams({ dir })}`;
}

function parseConversation(value: unknown): PastConversation | null {
  if (!value || typeof value !== "object") return null;
  const { kind, id, title, updated } = value as Record<string, unknown>;
  if (typeof kind !== "string" || !AGENT_KINDS.includes(kind) || typeof id !== "string") return null;
  if (title !== null && typeof title !== "string") return null;
  if (typeof updated !== "number" || !Number.isFinite(updated)) return null;
  return { kind: kind as AgentKind, id, title, updated };
}

/** A folder's past conversations, in the server's order; null when the call fails. */
export async function fetchHistory(dir: string): Promise<PastConversation[] | null> {
  try {
    const response = await fetch(historyPath(dir), { cache: "no-store" });
    if (!response.ok) return null;
    const body = (await response.json()) as { conversations?: unknown } | null;
    if (!Array.isArray(body?.conversations)) return null;
    return body.conversations.flatMap((item) => parseConversation(item) ?? []);
  } catch {
    return null;
  }
}
