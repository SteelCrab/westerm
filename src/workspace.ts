import type { AttachMode, SessionKind } from "../shared/protocol.ts";
import type { SavedWorkspace } from "../shared/workspace-file.ts";
import type { Notice } from "./i18n/messages.ts";
import {
  dockPane,
  fromGrid,
  insertPane,
  leaves,
  neighbor,
  type Direction,
  removePane,
  setSizesAt,
  swapPanes,
  type LayoutNode,
  type Side,
} from "../shared/layout.ts";

/** disconnected: the dashboard lost the connection and is reconnecting. */
export type SessionStatus = "connecting" | "running" | "disconnected" | "exited" | "error";

/** Reconnect attempts before a pane asks the user to retry. */
export const MAX_RECONNECTS = 8;

export interface Session {
  id: string;
  kind: SessionKind;
  /** Latest title the program reported (OSC 0/2), or the default name. */
  title: string;
  /** Name set by the user; null follows `title`. */
  name: string | null;
  /** Start URL for terminal-browser sessions; null opens its start page. */
  url: string | null;
  /** ssh destination: the remote host, or the proxy host for a remote browser. */
  target: string | null;
  status: SessionStatus;
  /** Untranslated detail: pid, exit code, or a server message (already localized). */
  detail: string | null;
  /** Connection state the UI shows in its language. */
  notice: Notice | null;
  /** PTY process id once the server reports it. */
  pid: number | null;
  /** terminal-code opens the current directory of this session's process. */
  cwdFrom: string | null;
  /** Reconnect attempts since the last successful attach. */
  retries: number;
  /** Bumped on restart so the pane remounts with a fresh terminal and PTY. */
  generation: number;
}

/** A tab owns a split layout of panes. Focus and maximize are remembered per tab. */
export interface Tab {
  id: string;
  /** Name set by the user; null follows the focused pane's title. */
  name: string | null;
  /** Split tree of panes; null for an empty tab. */
  layout: LayoutNode | null;
  /** The layout's panes in reading order, kept in step with `layout`. */
  paneIds: string[];
  focusedId: string | null;
  maximizedId: string | null;
}

/**
 * `sessions` stays in creation order so pane DOM nodes never move; placement
 * lives in each tab's `layout`.
 */
export interface Workspace {
  sessions: Session[];
  /** Never empty. */
  tabs: Tab[];
  activeTabId: string;
}

export type WorkspaceAction =
  | {
      type: "open";
      id: string;
      kind: SessionKind;
      url?: string | null;
      cwdFrom?: string | null;
      target?: string | null;
      /** Layer width / height, to split the focused pane along its longer side. */
      aspect?: number;
    }
  | { type: "ready"; id: string; pid: number; mode: AttachMode }
  /** The connection dropped; the pane reconnects to the same session. */
  | { type: "disconnected"; id: string }
  /** Replaces the whole workspace with a saved one, e.g. on page load. */
  | { type: "hydrate"; saved: SavedWorkspace }
  | { type: "close"; id: string }
  | { type: "focus"; id: string }
  | { type: "toggleMaximize"; id: string }
  | { type: "status"; id: string; status: SessionStatus; detail?: string | null; notice?: Notice | null }
  | { type: "title"; id: string; title: string }
  /** An empty name returns the session to the program's title. */
  | { type: "renameSession"; id: string; name: string }
  /** `manual`: the user pressed restart, so the reconnect budget starts over. */
  | { type: "restart"; id: string; manual?: boolean }
  | { type: "newTab"; id: string }
  | { type: "selectTab"; id: string }
  /** An empty name returns the tab to its automatic name. */
  | { type: "renameTab"; id: string; name: string }
  /** Closes the tab and its sessions. `fallbackId` names the empty tab that replaces the last one. */
  | { type: "closeTab"; id: string; fallbackId: string }
  /** Puts the tab at `toIndex` among the tabs. */
  | { type: "moveTab"; id: string; toIndex: number }
  /** Moves the pane into another tab, beside that tab's focused pane. */
  | { type: "movePane"; id: string; toTabId: string; aspect?: number }
  /** Puts the pane on one side of another pane, splitting it; also across tabs. */
  | { type: "dockPane"; id: string; targetId: string; side: Side }
  /** Swaps two panes' places, also across tabs. */
  | { type: "swapPanes"; id: string; withId: string }
  /** Divider drag result for the split at `path`. */
  | { type: "setSplitSizes"; tabId: string; path: number[]; sizes: number[] };

function emptyTab(id: string): Tab {
  return { id, name: null, layout: null, paneIds: [], focusedId: null, maximizedId: null };
}

export function createWorkspace(tabId: string): Workspace {
  return { sessions: [], tabs: [emptyTab(tabId)], activeTabId: tabId };
}

const TITLE_PREFIX: Record<SessionKind, string> = {
  shell: "Shell",
  browser: "Browser",
  editor: "Code",
  ssh: "SSH",
};

export const NAME_MAX = 60;

/** Collapses whitespace and bounds length; empty means "no custom name". */
export function cleanName(name: string): string | null {
  return name.replace(/\s+/g, " ").trim().slice(0, NAME_MAX) || null;
}

export function sessionLabel(session: Session): string {
  return session.name ?? session.title;
}

function defaultTitle(sessions: Session[], kind: SessionKind): string {
  const count = sessions.filter((session) => session.kind === kind).length + 1;
  return `${TITLE_PREFIX[kind]} ${count}`;
}

export function activeTab(state: Workspace): Tab {
  return state.tabs.find((tab) => tab.id === state.activeTabId) ?? state.tabs[0];
}

/** A tab's sessions in grid order. */
export function tabSessions(state: Workspace, tabId: string): Session[] {
  const tab = state.tabs.find((item) => item.id === tabId);
  if (!tab) return [];
  const byId = new Map(state.sessions.map((session) => [session.id, session]));
  return tab.paneIds.flatMap((id) => byId.get(id) ?? []);
}

export function sessionTab(state: Workspace, sessionId: string): Tab | null {
  return state.tabs.find((tab) => tab.paneIds.includes(sessionId)) ?? null;
}

/** The session a tab shows as its name: its focused pane, else its first pane. */
export function tabLead(state: Workspace, tab: Tab): Session | null {
  const sessions = tabSessions(state, tab.id);
  return sessions.find((session) => session.id === tab.focusedId) ?? sessions[0] ?? null;
}

/**
 * What the tab bar and sidebar show: the user's name, else the lead pane's
 * title, else `untitled` (the UI's word for an empty tab).
 */
export function tabLabel(state: Workspace, tab: Tab, untitled: string): string {
  const lead = tabLead(state, tab);
  return tab.name ?? (lead ? sessionLabel(lead) : untitled);
}

/** Tab `number` (1–9) as browsers count: 9 is always the last tab. */
export function tabByNumber(state: Workspace, number: number): Tab | null {
  if (number === 9) return state.tabs[state.tabs.length - 1] ?? null;
  return state.tabs[number - 1] ?? null;
}

/** The tab `delta` steps from the active one, wrapping around. */
export function stepTab(state: Workspace, delta: number): Tab {
  const index = state.tabs.findIndex((tab) => tab.id === state.activeTabId);
  const count = state.tabs.length;
  return state.tabs[(((index + delta) % count) + count) % count];
}

/** The pane `delta` steps from the focused one in the active tab, in reading order, wrapping around. */
export function stepPane(state: Workspace, delta: number): string | null {
  const ids = activeTab(state).paneIds;
  if (ids.length === 0) return null;
  const index = Math.max(0, ids.indexOf(activeTab(state).focusedId ?? ""));
  return ids[(((index + delta) % ids.length) + ids.length) % ids.length];
}

/** The pane beside the focused one in `direction`, in the active tab. */
export function paneToward(state: Workspace, direction: Direction): string | null {
  const tab = activeTab(state);
  if (!tab.focusedId) return tab.paneIds[0] ?? null;
  return neighbor(tab.layout, tab.focusedId, direction);
}

/** The focused shell in the active tab, whose directory a new editor opens. */
export function focusedShellId(state: Workspace): string | null {
  const focusedId = activeTab(state).focusedId;
  const focused = state.sessions.find((session) => session.id === focusedId);
  return focused?.kind === "shell" && focused.status === "running" ? focused.id : null;
}

function newSession(
  id: string,
  kind: SessionKind,
  title: string,
  url: string | null,
  target: string | null = null,
): Session {
  return {
    id,
    kind,
    title,
    name: null,
    url,
    target,
    status: "connecting",
    detail: null,
    notice: null,
    pid: null,
    cwdFrom: null,
    retries: 0,
    generation: 0,
  };
}

/** Layout and names only; processes and screens live in the PTY host. */
export function toSaved(state: Workspace): SavedWorkspace {
  return {
    version: 1,
    activeTabId: state.activeTabId,
    tabs: state.tabs.map(({ id, name, layout, paneIds, focusedId, maximizedId }) => ({
      id,
      name,
      layout,
      paneIds,
      focusedId,
      maximizedId,
    })),
    sessions: state.sessions.map(({ id, kind, title, name, url, target }) => ({ id, kind, title, name, url, target })),
  };
}

const ATTACH_NOTICE: Record<AttachMode, Notice | null> = {
  new: null,
  reattached: "reattached",
  revived: "revived",
};

function patch(state: Workspace, id: string, change: Partial<Session>): Workspace {
  if (!state.sessions.some((session) => session.id === id)) return state;
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === id ? { ...session, ...change } : session,
    ),
  };
}

function patchTab(state: Workspace, tabId: string, change: Partial<Tab>): Workspace {
  return { ...state, tabs: state.tabs.map((tab) => (tab.id === tabId ? { ...tab, ...change } : tab)) };
}

function clampIndex(index: number, length: number): number {
  return Math.max(0, Math.min(length, Math.trunc(index)));
}

const DEFAULT_ASPECT = 16 / 10;

function withLayout(tab: Tab, layout: LayoutNode | null): Tab {
  return { ...tab, layout, paneIds: leaves(layout) };
}

/** Removes a pane from its tab, moving that tab's focus to a neighbor. */
function detachPane(tab: Tab, paneId: string): Tab {
  const index = tab.paneIds.indexOf(paneId);
  if (index < 0) return tab;
  const next = withLayout(tab, removePane(tab.layout, paneId));
  const neighbor = next.paneIds[Math.min(index, next.paneIds.length - 1)] ?? null;
  return {
    ...next,
    focusedId: tab.focusedId === paneId ? neighbor : tab.focusedId,
    maximizedId: tab.maximizedId === paneId ? null : tab.maximizedId,
  };
}

export function workspaceReducer(state: Workspace, action: WorkspaceAction): Workspace {
  switch (action.type) {
    case "open": {
      const tab = activeTab(state);
      const session: Session = {
        ...newSession(
          action.id,
          action.kind,
          action.target ? `${action.kind === "ssh" ? "ssh" : "browser"} ${action.target}` : defaultTitle(state.sessions, action.kind),
          action.url ?? null,
          action.target ?? null,
        ),
        cwdFrom: action.cwdFrom ?? null,
      };
      // A new session leaves maximize so it is visible, beside the focused pane.
      const next = { ...state, sessions: [...state.sessions, session] };
      const placed = withLayout(tab, insertPane(tab.layout, session.id, tab.focusedId, action.aspect ?? DEFAULT_ASPECT));
      return patchTab(next, tab.id, { ...placed, focusedId: session.id, maximizedId: null });
    }
    case "close": {
      const tab = sessionTab(state, action.id);
      if (!tab) return state;
      const next = { ...state, sessions: state.sessions.filter((session) => session.id !== action.id) };
      return { ...next, tabs: next.tabs.map((item) => (item.id === tab.id ? detachPane(item, action.id) : item)) };
    }
    case "focus": {
      // Focusing a session in another tab, e.g. from the sidebar, switches to it.
      const tab = sessionTab(state, action.id);
      if (!tab) return state;
      // Focusing another pane while one is maximized shows that pane instead.
      return patchTab({ ...state, activeTabId: tab.id }, tab.id, {
        focusedId: action.id,
        maximizedId: tab.maximizedId ? action.id : null,
      });
    }
    case "toggleMaximize": {
      const tab = sessionTab(state, action.id);
      if (!tab) return state;
      return patchTab({ ...state, activeTabId: tab.id }, tab.id, {
        focusedId: action.id,
        maximizedId: tab.maximizedId === action.id ? null : action.id,
      });
    }
    case "ready":
      return patch(state, action.id, {
        status: "running",
        detail: `pid ${action.pid}`,
        notice: ATTACH_NOTICE[action.mode],
        pid: action.pid,
        retries: 0,
      });
    case "disconnected": {
      const session = state.sessions.find((item) => item.id === action.id);
      if (!session) return state;
      const retries = session.retries + 1;
      return retries > MAX_RECONNECTS
        ? patch(state, action.id, { status: "error", detail: null, notice: "unreachable", retries })
        : patch(state, action.id, { status: "disconnected", detail: null, notice: "reconnecting", retries });
    }
    case "hydrate": {
      const { saved } = action;
      return {
        sessions: saved.sessions.map((item) => ({
          ...newSession(item.id, item.kind, item.title, item.url, item.target ?? null),
          name: item.name,
        })),
        // Layouts saved before split trees were a plain grid order.
        tabs: saved.tabs.map((tab) => ({
          ...withLayout(emptyTab(tab.id), tab.layout ?? fromGrid(tab.paneIds)),
          name: tab.name,
          focusedId: tab.focusedId,
          maximizedId: tab.maximizedId,
        })),
        activeTabId: saved.activeTabId,
      };
    }
    case "status":
      return patch(state, action.id, {
        status: action.status,
        detail: action.detail ?? null,
        notice: action.notice ?? null,
      });
    case "title": {
      const title = action.title.trim();
      return title ? patch(state, action.id, { title }) : state;
    }
    case "renameSession":
      return patch(state, action.id, { name: cleanName(action.name) });
    case "restart": {
      const session = state.sessions.find((item) => item.id === action.id);
      if (!session) return state;
      return patch(state, action.id, {
        status: "connecting",
        detail: null,
        notice: null,
        pid: null,
        retries: action.manual ? 0 : session.retries,
        generation: session.generation + 1,
      });
    }
    case "newTab":
      if (state.tabs.some((tab) => tab.id === action.id)) return state;
      return { ...state, tabs: [...state.tabs, emptyTab(action.id)], activeTabId: action.id };
    case "selectTab":
      return state.tabs.some((tab) => tab.id === action.id) ? { ...state, activeTabId: action.id } : state;
    case "renameTab":
      if (!state.tabs.some((tab) => tab.id === action.id)) return state;
      return patchTab(state, action.id, { name: cleanName(action.name) });
    case "closeTab": {
      const index = state.tabs.findIndex((tab) => tab.id === action.id);
      if (index < 0) return state;
      const closing = new Set(state.tabs[index].paneIds);
      const sessions = state.sessions.filter((session) => !closing.has(session.id));
      const tabs = state.tabs.filter((tab) => tab.id !== action.id);
      if (tabs.length === 0) return { sessions, tabs: [emptyTab(action.fallbackId)], activeTabId: action.fallbackId };
      // Like a browser: the tab to the right takes over, else the one to the left.
      const activeTabId =
        state.activeTabId === action.id ? tabs[Math.min(index, tabs.length - 1)].id : state.activeTabId;
      return { sessions, tabs, activeTabId };
    }
    case "moveTab": {
      const moving = state.tabs.find((tab) => tab.id === action.id);
      if (!moving) return state;
      const rest = state.tabs.filter((tab) => tab.id !== action.id);
      const at = clampIndex(action.toIndex, rest.length);
      const tabs = [...rest.slice(0, at), moving, ...rest.slice(at)];
      return tabs.every((tab, index) => tab === state.tabs[index]) ? state : { ...state, tabs };
    }
    case "movePane": {
      const from = sessionTab(state, action.id);
      if (!from || from.id === action.toTabId || !state.tabs.some((tab) => tab.id === action.toTabId)) return state;
      const tabs = state.tabs.map((tab) => {
        if (tab.id === from.id) return detachPane(tab, action.id);
        if (tab.id !== action.toTabId) return tab;
        const placed = withLayout(tab, insertPane(tab.layout, action.id, tab.focusedId, action.aspect ?? DEFAULT_ASPECT));
        // A pane dropped into a tab should be visible there.
        return { ...placed, focusedId: action.id, maximizedId: null };
      });
      return { ...state, tabs, activeTabId: action.toTabId };
    }
    case "dockPane": {
      const from = sessionTab(state, action.id);
      const to = sessionTab(state, action.targetId);
      if (!from || !to || action.id === action.targetId) return state;
      const tabs = state.tabs.map((tab) => {
        const detached = tab.id === from.id ? detachPane(tab, action.id) : tab;
        if (tab.id !== to.id) return detached;
        const docked = withLayout(detached, dockPane(detached.layout, action.id, action.targetId, action.side));
        return { ...docked, focusedId: action.id, maximizedId: null };
      });
      return { ...state, tabs, activeTabId: to.id };
    }
    case "setSplitSizes": {
      const tab = state.tabs.find((item) => item.id === action.tabId);
      if (!tab) return state;
      return patchTab(state, tab.id, { layout: setSizesAt(tab.layout, action.path, action.sizes) });
    }
    case "swapPanes": {
      const a = sessionTab(state, action.id);
      const b = sessionTab(state, action.withId);
      if (!a || !b || action.id === action.withId) return state;
      const swap = (id: string) => (id === action.id ? action.withId : id === action.withId ? action.id : id);
      const tabs = state.tabs.map((tab) => {
        if (tab.id !== a.id && tab.id !== b.id) return tab;
        const swapped = withLayout(tab, swapPanes(tab.layout, action.id, action.withId));
        if (a.id === b.id) return { ...swapped, focusedId: action.id };
        // Across tabs each pane takes the other's place, including focus and maximize.
        return {
          ...swapped,
          focusedId: tab.focusedId ? swap(tab.focusedId) : null,
          maximizedId: tab.maximizedId ? swap(tab.maximizedId) : null,
        };
      });
      return { ...state, tabs, activeTabId: b.id };
    }
  }
}
