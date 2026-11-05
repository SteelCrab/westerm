import type { AttachMode, RemoteStage, SessionKind } from "../shared/protocol.ts";
import type { SavedDashboard } from "../shared/workspace-file.ts";
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
  /** Remote panes: the dashboard's last stage (connecting, connected via …); null before one arrives. */
  remote: RemoteStage | null;
  /** Bumped on restart so the pane remounts with a fresh terminal and PTY. */
  generation: number;
}

/** A tab owns a split layout of panes. Focus and maximize are remembered per tab. */
export interface Tab {
  id: string;
  /** The workspace that shows this tab. */
  workspaceId: string;
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
 * A set of tabs shown together. Only the workspace in view shows its tabs;
 * the sessions of the others keep running.
 */
export interface Workspace {
  id: string;
  /** Name set by the user; null shows the workspace's number. */
  name: string | null;
  /** The tab this workspace shows; always one of its own. */
  activeTabId: string;
}

/**
 * `sessions` stays in creation order so pane DOM nodes never move; placement
 * lives in each tab's `layout`. `tabs` holds the tabs of every workspace, in
 * order within each workspace.
 */
export interface Dashboard {
  sessions: Session[];
  /** Every workspace has at least one. */
  tabs: Tab[];
  /** Never empty. */
  workspaces: Workspace[];
  activeWorkspaceId: string;
}

export type DashboardAction =
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
  /** A remote pane's connection stage, from the dashboard. */
  | { type: "remoteStage"; id: string; stage: RemoteStage }
  /** The connection dropped; the pane reconnects to the same session. */
  | { type: "disconnected"; id: string }
  /** Replaces the whole workspace with a saved one, e.g. on page load. */
  | { type: "hydrate"; saved: SavedDashboard }
  | { type: "close"; id: string }
  | { type: "focus"; id: string }
  | { type: "toggleMaximize"; id: string }
  | { type: "status"; id: string; status: SessionStatus; detail?: string | null; notice?: Notice | null }
  | { type: "title"; id: string; title: string }
  /** An empty name returns the session to the program's title. */
  | { type: "renameSession"; id: string; name: string }
  /** `manual`: the user pressed restart, so the reconnect budget starts over. */
  | { type: "restart"; id: string; manual?: boolean }
  /**
   * This window takes every session back from another window. That window
   * may have changed the layout, so the saved one replaces this one when
   * there is one; then every pane connects again.
   */
  | { type: "reclaim"; saved: SavedDashboard | null }
  | { type: "newTab"; id: string }
  | { type: "selectTab"; id: string }
  /** An empty name returns the tab to its automatic name. */
  | { type: "renameTab"; id: string; name: string }
  /** Closes the tab and its sessions. `fallbackId` names the empty tab that replaces a workspace's last one. */
  | { type: "closeTab"; id: string; fallbackId: string }
  /** Puts the tab at `toIndex` among its workspace's tabs. */
  | { type: "moveTab"; id: string; toIndex: number }
  /** A workspace with one empty tab, shown at once. */
  | { type: "newWorkspace"; id: string; tabId: string }
  | { type: "selectWorkspace"; id: string }
  /** An empty name returns the workspace to its number. */
  | { type: "renameWorkspace"; id: string; name: string }
  /**
   * Closes the workspace, its tabs, and their sessions. `fallbackId` and
   * `fallbackTabId` name the empty workspace that replaces the last one.
   */
  | { type: "closeWorkspace"; id: string; fallbackId: string; fallbackTabId: string }
  /**
   * Moves the tab to the end of another workspace and shows it there.
   * `fallbackId` names the empty tab left behind when it was the source's only tab.
   */
  | { type: "moveTabToWorkspace"; id: string; workspaceId: string; fallbackId: string }
  /** Moves the pane into another tab, beside that tab's focused pane. */
  | { type: "movePane"; id: string; toTabId: string; aspect?: number }
  /** Puts the pane on one side of another pane, splitting it; also across tabs. */
  | { type: "dockPane"; id: string; targetId: string; side: Side }
  /** Swaps two panes' places, also across tabs. */
  | { type: "swapPanes"; id: string; withId: string }
  /** Divider drag result for the split at `path`. */
  | { type: "setSplitSizes"; tabId: string; path: number[]; sizes: number[] };

function emptyTab(id: string, workspaceId: string): Tab {
  return { id, workspaceId, name: null, layout: null, paneIds: [], focusedId: null, maximizedId: null };
}

export function createDashboard(workspaceId: string, tabId: string): Dashboard {
  return {
    sessions: [],
    tabs: [emptyTab(tabId, workspaceId)],
    workspaces: [{ id: workspaceId, name: null, activeTabId: tabId }],
    activeWorkspaceId: workspaceId,
  };
}

const TITLE_PREFIX: Record<SessionKind, string> = {
  shell: "Shell",
  browser: "Browser",
  editor: "Code",
  ssh: "SSH",
  remote: "Remote",
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

export function activeWorkspace(state: Dashboard): Workspace {
  return state.workspaces.find((workspace) => workspace.id === state.activeWorkspaceId) ?? state.workspaces[0];
}

/** A workspace's tabs in order. */
export function workspaceTabs(state: Dashboard, workspaceId: string): Tab[] {
  return state.tabs.filter((tab) => tab.workspaceId === workspaceId);
}

/** The tabs of the workspace in view: what the tab bar shows. */
export function visibleTabs(state: Dashboard): Tab[] {
  return workspaceTabs(state, activeWorkspace(state).id);
}

export function activeTab(state: Dashboard): Tab {
  const workspace = activeWorkspace(state);
  return state.tabs.find((tab) => tab.id === workspace.activeTabId) ?? visibleTabs(state)[0] ?? state.tabs[0];
}

/** The user's name for a workspace, else `untitled` with its number (1-based). */
export function workspaceLabel(state: Dashboard, workspace: Workspace, untitled: (number: number) => string): string {
  return workspace.name ?? untitled(state.workspaces.findIndex((item) => item.id === workspace.id) + 1);
}

/** A tab's sessions in grid order. */
export function tabSessions(state: Dashboard, tabId: string): Session[] {
  const tab = state.tabs.find((item) => item.id === tabId);
  if (!tab) return [];
  const byId = new Map(state.sessions.map((session) => [session.id, session]));
  return tab.paneIds.flatMap((id) => byId.get(id) ?? []);
}

/**
 * Another window took every session (close code CLOSE_TAKEN_OVER). The host
 * tells each pane, so one pane told stands for the whole window.
 */
export function takenOver(state: Dashboard): boolean {
  return state.sessions.some((session) => session.notice === "takenOver");
}

export function sessionTab(state: Dashboard, sessionId: string): Tab | null {
  return state.tabs.find((tab) => tab.paneIds.includes(sessionId)) ?? null;
}

/** The session a tab shows as its name: its focused pane, else its first pane. */
export function tabLead(state: Dashboard, tab: Tab): Session | null {
  const sessions = tabSessions(state, tab.id);
  return sessions.find((session) => session.id === tab.focusedId) ?? sessions[0] ?? null;
}

/**
 * What the tab bar and sidebar show: the user's name, else the lead pane's
 * title, else `untitled` (the UI's word for an empty tab).
 */
export function tabLabel(state: Dashboard, tab: Tab, untitled: string): string {
  const lead = tabLead(state, tab);
  return tab.name ?? (lead ? sessionLabel(lead) : untitled);
}

/** Tab `number` (1–9) of the workspace in view, as browsers count: 9 is always the last tab. */
export function tabByNumber(state: Dashboard, number: number): Tab | null {
  const tabs = visibleTabs(state);
  if (number === 9) return tabs[tabs.length - 1] ?? null;
  return tabs[number - 1] ?? null;
}

/** The tab `delta` steps from the active one in the workspace in view, wrapping around. */
export function stepTab(state: Dashboard, delta: number): Tab {
  const tabs = visibleTabs(state);
  const index = tabs.findIndex((tab) => tab.id === activeTab(state).id);
  const count = tabs.length;
  return tabs[(((index + delta) % count) + count) % count];
}

/** The pane `delta` steps from the focused one in the active tab, in reading order, wrapping around. */
export function stepPane(state: Dashboard, delta: number): string | null {
  const ids = activeTab(state).paneIds;
  if (ids.length === 0) return null;
  const index = Math.max(0, ids.indexOf(activeTab(state).focusedId ?? ""));
  return ids[(((index + delta) % ids.length) + ids.length) % ids.length];
}

/** The pane beside the focused one in `direction`, in the active tab. */
export function paneToward(state: Dashboard, direction: Direction): string | null {
  const tab = activeTab(state);
  if (!tab.focusedId) return tab.paneIds[0] ?? null;
  return neighbor(tab.layout, tab.focusedId, direction);
}

/** The focused shell in the active tab, whose directory a new editor opens. */
export function focusedShellId(state: Dashboard): string | null {
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
    remote: null,
  };
}

const TARGET_TITLE: Partial<Record<SessionKind, string>> = { ssh: "ssh", remote: "westerm", browser: "browser" };

/** Layout and names only; processes and screens live in the PTY host. */
export function toSaved(state: Dashboard): SavedDashboard {
  return {
    version: 1,
    activeTabId: activeTab(state).id,
    tabs: state.tabs.map(({ id, workspaceId, name, layout, paneIds, focusedId, maximizedId }) => ({
      id,
      workspaceId,
      name,
      layout,
      paneIds,
      focusedId,
      maximizedId,
    })),
    sessions: state.sessions.map(({ id, kind, title, name, url, target }) => ({ id, kind, title, name, url, target })),
    workspaces: state.workspaces.map(({ id, name, activeTabId }) => ({ id, name, activeTabId })),
    activeWorkspaceId: state.activeWorkspaceId,
  };
}

const ATTACH_NOTICE: Record<AttachMode, Notice | null> = {
  new: null,
  reattached: "reattached",
  revived: "revived",
};

function patch(state: Dashboard, id: string, change: Partial<Session>): Dashboard {
  if (!state.sessions.some((session) => session.id === id)) return state;
  return {
    ...state,
    sessions: state.sessions.map((session) =>
      session.id === id ? { ...session, ...change } : session,
    ),
  };
}

function patchTab(state: Dashboard, tabId: string, change: Partial<Tab>): Dashboard {
  return { ...state, tabs: state.tabs.map((tab) => (tab.id === tabId ? { ...tab, ...change } : tab)) };
}

function patchWorkspace(state: Dashboard, workspaceId: string, change: Partial<Workspace>): Dashboard {
  return {
    ...state,
    workspaces: state.workspaces.map((workspace) => (workspace.id === workspaceId ? { ...workspace, ...change } : workspace)),
  };
}

/** Brings the tab into view: its workspace is shown, showing this tab. */
function showTab(state: Dashboard, tabId: string): Dashboard {
  const tab = state.tabs.find((item) => item.id === tabId);
  if (!tab) return state;
  return { ...patchWorkspace(state, tab.workspaceId, { activeTabId: tab.id }), activeWorkspaceId: tab.workspaceId };
}

/**
 * Takes the tab out of its workspace. A workspace left without tabs gets the
 * empty tab `fallbackId`. One that showed the tab shows a neighbor, like a
 * browser: the tab to the right, else the one to the left.
 */
function takeTab(state: Dashboard, tab: Tab, fallbackId: string): Dashboard {
  const siblings = workspaceTabs(state, tab.workspaceId);
  const rest = siblings.filter((item) => item !== tab);
  const replacement = rest.length === 0 ? emptyTab(fallbackId, tab.workspaceId) : null;
  const next = { ...state, tabs: state.tabs.flatMap((item) => (item !== tab ? [item] : replacement ? [replacement] : [])) };
  const showed = state.workspaces.some((workspace) => workspace.id === tab.workspaceId && workspace.activeTabId === tab.id);
  if (!showed) return next;
  const shown = replacement ?? rest[Math.min(siblings.indexOf(tab), rest.length - 1)];
  return patchWorkspace(next, tab.workspaceId, { activeTabId: shown.id });
}

function clampIndex(index: number, length: number): number {
  return Math.max(0, Math.min(length, Math.trunc(index)));
}

/** The list with `item` at `toIndex` among the others, or the same list when nothing moves. */
function moveTo<T>(list: T[], item: T, toIndex: number): T[] {
  const rest = list.filter((other) => other !== item);
  const at = clampIndex(toIndex, rest.length);
  const moved = [...rest.slice(0, at), item, ...rest.slice(at)];
  return moved.every((other, index) => other === list[index]) ? list : moved;
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

export function dashboardReducer(state: Dashboard, action: DashboardAction): Dashboard {
  switch (action.type) {
    case "open": {
      const tab = activeTab(state);
      const session: Session = {
        ...newSession(
          action.id,
          action.kind,
          action.target ? `${TARGET_TITLE[action.kind] ?? action.kind} ${action.target}` : defaultTitle(state.sessions, action.kind),
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
      return patchTab(showTab(state, tab.id), tab.id, {
        focusedId: action.id,
        maximizedId: tab.maximizedId ? action.id : null,
      });
    }
    case "toggleMaximize": {
      const tab = sessionTab(state, action.id);
      if (!tab) return state;
      return patchTab(showTab(state, tab.id), tab.id, {
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
      // A remote shell keeps running on its server, so its pane keeps trying.
      return retries > MAX_RECONNECTS && session.kind !== "remote"
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
          ...withLayout(emptyTab(tab.id, tab.workspaceId), tab.layout ?? fromGrid(tab.paneIds)),
          name: tab.name,
          focusedId: tab.focusedId,
          maximizedId: tab.maximizedId,
        })),
        workspaces: saved.workspaces.map(({ id, name, activeTabId }) => ({ id, name, activeTabId })),
        activeWorkspaceId: saved.activeWorkspaceId,
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
        remote: null,
      });
    }
    case "remoteStage":
      return patch(state, action.id, { remote: action.stage });
    case "reclaim": {
      if (!takenOver(state)) return state;
      const next = action.saved ? dashboardReducer(state, { type: "hydrate", saved: action.saved }) : state;
      // A new generation mounts a new terminal, which connects again.
      const generations = new Map(state.sessions.map((session) => [session.id, session.generation]));
      return {
        ...next,
        sessions: next.sessions.map((session) => ({
          ...session,
          status: "connecting",
          detail: null,
          notice: null,
          pid: null,
          retries: 0,
          generation: (generations.get(session.id) ?? session.generation) + 1,
          remote: null,
        })),
      };
    }
    case "newTab": {
      if (state.tabs.some((tab) => tab.id === action.id)) return state;
      const tab = emptyTab(action.id, activeWorkspace(state).id);
      return showTab({ ...state, tabs: [...state.tabs, tab] }, tab.id);
    }
    case "selectTab":
      return showTab(state, action.id);
    case "renameTab":
      if (!state.tabs.some((tab) => tab.id === action.id)) return state;
      return patchTab(state, action.id, { name: cleanName(action.name) });
    case "closeTab": {
      const tab = state.tabs.find((item) => item.id === action.id);
      if (!tab) return state;
      const closing = new Set(tab.paneIds);
      const sessions = state.sessions.filter((session) => !closing.has(session.id));
      return takeTab({ ...state, sessions }, tab, action.fallbackId);
    }
    case "moveTab": {
      const moving = state.tabs.find((tab) => tab.id === action.id);
      if (!moving) return state;
      const siblings = workspaceTabs(state, moving.workspaceId);
      const order = moveTo(siblings, moving, action.toIndex);
      if (order === siblings) return state;
      // The workspace's tabs take the new order in their own slots; other workspaces' tabs stay put.
      let slot = 0;
      return { ...state, tabs: state.tabs.map((tab) => (tab.workspaceId === moving.workspaceId ? order[slot++] : tab)) };
    }
    case "newWorkspace": {
      if (state.workspaces.some((workspace) => workspace.id === action.id)) return state;
      if (state.tabs.some((tab) => tab.id === action.tabId)) return state;
      return {
        ...state,
        tabs: [...state.tabs, emptyTab(action.tabId, action.id)],
        workspaces: [...state.workspaces, { id: action.id, name: null, activeTabId: action.tabId }],
        activeWorkspaceId: action.id,
      };
    }
    case "selectWorkspace":
      if (!state.workspaces.some((workspace) => workspace.id === action.id)) return state;
      return { ...state, activeWorkspaceId: action.id };
    case "renameWorkspace":
      if (!state.workspaces.some((workspace) => workspace.id === action.id)) return state;
      return patchWorkspace(state, action.id, { name: cleanName(action.name) });
    case "closeWorkspace": {
      const index = state.workspaces.findIndex((workspace) => workspace.id === action.id);
      if (index < 0) return state;
      const workspaces = state.workspaces.filter((workspace) => workspace.id !== action.id);
      if (workspaces.length === 0) return createDashboard(action.fallbackId, action.fallbackTabId);
      const closing = new Set(workspaceTabs(state, action.id).flatMap((tab) => tab.paneIds));
      // Like tabs: the workspace below takes over, else the one above.
      const activeWorkspaceId =
        state.activeWorkspaceId === action.id ? workspaces[Math.min(index, workspaces.length - 1)].id : state.activeWorkspaceId;
      return {
        sessions: state.sessions.filter((session) => !closing.has(session.id)),
        tabs: state.tabs.filter((tab) => tab.workspaceId !== action.id),
        workspaces,
        activeWorkspaceId,
      };
    }
    case "moveTabToWorkspace": {
      const moving = state.tabs.find((tab) => tab.id === action.id);
      if (!moving || moving.workspaceId === action.workspaceId) return state;
      if (!state.workspaces.some((workspace) => workspace.id === action.workspaceId)) return state;
      const taken = takeTab(state, moving, action.fallbackId);
      return showTab({ ...taken, tabs: [...taken.tabs, { ...moving, workspaceId: action.workspaceId }] }, moving.id);
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
      return showTab({ ...state, tabs }, action.toTabId);
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
      return showTab({ ...state, tabs }, to.id);
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
      return showTab({ ...state, tabs }, b.id);
    }
  }
}
