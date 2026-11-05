// The dashboard layout saved on the server so it survives reloads, program
// restarts, and reboots. Process state lives in the PTY host, not here.

import { fromGrid, leaves, parseLayout, type LayoutNode } from "./layout.ts";
import { isId, isSessionKind, normalizeSshTarget, type SessionKind } from "./protocol.ts";

export { isId };

export interface SavedSession {
  id: string;
  kind: SessionKind;
  title: string;
  /** User-set name, or null. */
  name: string | null;
  url: string | null;
  /** ssh destination, for ssh and remote sessions and remote browsers. */
  target?: string | null;
}

export interface SavedTab {
  id: string;
  /** The workspace that shows this tab. */
  workspaceId: string;
  /** User-set name, or null. */
  name: string | null;
  /** The tab's panes in reading order. */
  paneIds: string[];
  /** Split tree; files from before split layouts have none and use grid order. */
  layout?: LayoutNode | null;
  focusedId: string | null;
  maximizedId: string | null;
}

/** Icons a workspace can show. The first is the default. */
export const WORKSPACE_ICONS = [
  "layers",
  "folder",
  "code",
  "terminal",
  "globe",
  "server",
  "database",
  "book",
  "flask",
  "rocket",
  "bug",
  "star",
] as const;
export type WorkspaceIcon = (typeof WORKSPACE_ICONS)[number];
export const DEFAULT_WORKSPACE_ICON: WorkspaceIcon = WORKSPACE_ICONS[0];

export function isWorkspaceIcon(value: unknown): value is WorkspaceIcon {
  return (WORKSPACE_ICONS as readonly unknown[]).includes(value);
}

export interface SavedWorkspace {
  id: string;
  /** User-set name, or null. */
  name: string | null;
  /** The tab the workspace shows; one of its own. */
  activeTabId: string;
  /** Files from before icons, and icons this version does not know, get the default. */
  icon: WorkspaceIcon;
}

/**
 * Workspaces were added without a version bump: files from before them have
 * no `workspaces` and open as one workspace, and older versions read the
 * tabs and `activeTabId` and ignore the rest.
 */
export interface SavedDashboard {
  version: 1;
  /** The tab in view. */
  activeTabId: string;
  tabs: SavedTab[];
  sessions: SavedSession[];
  workspaces: SavedWorkspace[];
  activeWorkspaceId: string;
}

export const WORKSPACE_PATH = "/api/workspace";
export const WORKSPACE_MAX_BYTES = 256 * 1024;
// Totals across workspaces. A file over a limit does not load at all.
const MAX_TABS = 128;
const MAX_SESSIONS = 128;
/** The one workspace of files from before workspaces, or with broken workspace data. */
const DEFAULT_WORKSPACE_ID = "w0";
const MAX_TEXT = 512;

function text(value: unknown, max = MAX_TEXT): string | null {
  return typeof value === "string" && value.length <= max ? value : null;
}

/**
 * Validates untrusted JSON into a consistent dashboard, or null. Every pane id
 * must name a session, every session must sit in exactly one tab, and focus
 * and maximize must point inside their tab. Workspace data that does not add
 * up only costs the grouping: the tabs then open as one workspace.
 */
export function parseSavedDashboard(value: unknown): SavedDashboard | null {
  if (typeof value !== "object" || value === null) return null;
  const raw = value as Record<string, unknown>;
  if (raw.version !== 1 || !Array.isArray(raw.tabs) || !Array.isArray(raw.sessions)) return null;
  if (raw.tabs.length === 0 || raw.tabs.length > MAX_TABS || raw.sessions.length > MAX_SESSIONS) return null;

  const sessions: SavedSession[] = [];
  const sessionIds = new Set<string>();
  for (const item of raw.sessions as unknown[]) {
    if (typeof item !== "object" || item === null) return null;
    const session = item as Record<string, unknown>;
    const title = text(session.title);
    const url = session.url === null ? null : text(session.url, 2048);
    const name = session.name == null ? null : text(session.name);
    if (!isId(session.id) || sessionIds.has(session.id) || !isSessionKind(session.kind)) return null;
    if (title === null || (session.url !== null && url === null)) return null;
    if (session.name != null && name === null) return null;
    const target = session.target == null ? null : normalizeSshTarget(String(session.target));
    const needsTarget = session.kind === "ssh" || session.kind === "remote";
    if ((session.target != null && target === null) || (needsTarget && !target)) return null;
    sessionIds.add(session.id);
    sessions.push({ id: session.id, kind: session.kind, title, name, url, target });
  }

  const tabs: Omit<SavedTab, "workspaceId">[] = [];
  /** Each tab's `workspaceId` as written, still unchecked. */
  const owners: unknown[] = [];
  const tabIds = new Set<string>();
  const placed = new Set<string>();
  for (const item of raw.tabs as unknown[]) {
    if (typeof item !== "object" || item === null) return null;
    const tab = item as Record<string, unknown>;
    if (!isId(tab.id) || tabIds.has(tab.id) || !Array.isArray(tab.paneIds)) return null;
    const paneIds: string[] = [];
    for (const paneId of tab.paneIds as unknown[]) {
      if (!isId(paneId) || !sessionIds.has(paneId) || placed.has(paneId)) return null;
      placed.add(paneId);
      paneIds.push(paneId);
    }
    const inTab = (id: unknown): string | null => (isId(id) && paneIds.includes(id) ? id : null);
    const name = tab.name == null ? null : text(tab.name);
    if (tab.name != null && name === null) return null;
    tabIds.add(tab.id);
    // The tree must hold exactly this tab's panes; otherwise use grid order.
    const inThisTab = (id: unknown): id is string => isId(id) && paneIds.includes(id);
    const parsed = tab.layout == null ? null : parseLayout(tab.layout, inThisTab);
    const sameLeaves =
      parsed !== null && leaves(parsed).length === paneIds.length && new Set(leaves(parsed)).size === paneIds.length;
    const layout = sameLeaves ? parsed : fromGrid(paneIds);
    tabs.push({
      id: tab.id,
      name,
      paneIds: leaves(layout),
      layout,
      focusedId: inTab(tab.focusedId),
      maximizedId: inTab(tab.maximizedId),
    });
    owners.push(tab.workspaceId);
  }
  if (placed.size !== sessionIds.size) return null;
  if (!isId(raw.activeTabId) || !tabIds.has(raw.activeTabId)) return null;
  const activeTabId = raw.activeTabId;

  // Broken workspace data costs the grouping, not the tabs.
  const grouped = parseWorkspaces(raw.workspaces, raw.activeWorkspaceId, tabs, owners);
  if (grouped) return { version: 1, activeTabId, sessions, ...grouped };
  return {
    version: 1,
    activeTabId,
    tabs: tabs.map((tab) => ({ ...tab, workspaceId: DEFAULT_WORKSPACE_ID })),
    sessions,
    workspaces: [{ id: DEFAULT_WORKSPACE_ID, name: null, activeTabId, icon: DEFAULT_WORKSPACE_ICON }],
    activeWorkspaceId: DEFAULT_WORKSPACE_ID,
  };
}

/**
 * The file's workspaces with each tab's owner when they are consistent, else
 * null: unique ids, every tab in one of them, and each showing a tab of its
 * own (so none is empty).
 */
function parseWorkspaces(
  value: unknown,
  activeId: unknown,
  tabs: ReadonlyArray<Omit<SavedTab, "workspaceId">>,
  owners: readonly unknown[],
): Pick<SavedDashboard, "tabs" | "workspaces" | "activeWorkspaceId"> | null {
  if (!Array.isArray(value) || value.length === 0 || value.length > MAX_TABS) return null;
  const workspaces: SavedWorkspace[] = [];
  for (const item of value as unknown[]) {
    if (typeof item !== "object" || item === null) return null;
    const workspace = item as Record<string, unknown>;
    const name = workspace.name == null ? null : text(workspace.name);
    if (!isId(workspace.id) || !isId(workspace.activeTabId)) return null;
    if (workspaces.some((other) => other.id === workspace.id)) return null;
    if (workspace.name != null && name === null) return null;
    const icon = isWorkspaceIcon(workspace.icon) ? workspace.icon : DEFAULT_WORKSPACE_ICON;
    workspaces.push({ id: workspace.id, name, activeTabId: workspace.activeTabId, icon });
  }
  const known = (id: unknown): id is string => workspaces.some((workspace) => workspace.id === id);
  const owned: SavedTab[] = [];
  for (const [index, tab] of tabs.entries()) {
    const owner = owners[index];
    if (!known(owner)) return null;
    owned.push({ ...tab, workspaceId: owner });
  }
  const showsOwnTab = (workspace: SavedWorkspace) =>
    owned.some((tab) => tab.id === workspace.activeTabId && tab.workspaceId === workspace.id);
  if (!workspaces.every(showsOwnTab) || !known(activeId)) return null;
  return { tabs: owned, workspaces, activeWorkspaceId: activeId };
}
