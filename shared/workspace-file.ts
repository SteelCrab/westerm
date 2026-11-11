// The dashboard layout saved on the server so it survives reloads, program
// restarts, and reboots. Process state lives in the PTY host, not here.

import { fromGrid, leaves, parseLayout, type LayoutNode } from "./layout.ts";
import { isId, isSessionKind, normalizeSshTarget, type SessionKind } from "./protocol.ts";

export { isId };

/** A valid ssh destination exactly as written, not trimmed: what a saved session or a window address may name. */
export function isSshTarget(value: unknown): value is string {
  return typeof value === "string" && normalizeSshTarget(value) === value;
}

export interface SavedSession {
  id: string;
  kind: SessionKind;
  title: string;
  /** User-set name, or null. */
  name: string | null;
  url: string | null;
  /** ssh destination, for ssh sessions and remote browsers. */
  target?: string | null;
  /** False: a restored shell in this pane does not resume its AI agent. Written only when false. */
  autoResume?: false;
}

/** How a docs pane shows its file. */
export type DocsMode = "view" | "edit" | "split";

export const DOCS_MODES: readonly DocsMode[] = ["view", "edit", "split"];

/**
 * A docs pane: no process, only what it shows. Docs panes live outside
 * `sessions` and `paneIds`, so a version from before them still reads the
 * file: it ignores `docs` and `docIds`, and lays such a tab out as a grid.
 */
export interface SavedDocs {
  id: string;
  title: string;
  /** User-set name, or null. */
  name: string | null;
  /** Absolute folder whose Markdown files the pane lists; null until the pane has asked for one. */
  root: string | null;
  /** The shell whose folder the root follows, or null. */
  followFrom: string | null;
  /** The open file, relative to the root with `/`, or null. */
  file: string | null;
  mode: DocsMode;
}

/**
 * A plugin pane: no process, only what its plugin draws. Like docs panes,
 * plugin panes live outside `sessions` and `paneIds`, so a version from before
 * them still reads the file and lays such a tab out as a grid.
 */
export interface SavedPluginPane {
  id: string;
  title: string;
  /** User-set name, or null. */
  name: string | null;
  /** The plugin that draws the pane. */
  plugin: string;
  /** Which of the plugin's views. */
  view: string;
  /** A pane with the same plugin and key is focused instead of opened again. */
  reuseKey: string | null;
  /** The plugin's own state for the pane, as JSON. */
  data: unknown;
}

/** The longest `JSON.stringify(data)` of a saved plugin pane. */
export const PLUGIN_PANE_DATA_MAX = 16 * 1024;

export interface SavedTab {
  id: string;
  /** The workspace that shows this tab. */
  workspaceId: string;
  /** User-set name, or null. */
  name: string | null;
  /** The tab's session panes in reading order; docs panes are in `docIds`. */
  paneIds: string[];
  /** The tab's docs panes in reading order. Files from before docs panes have none. */
  docIds?: string[];
  /** The tab's plugin panes in reading order. Files from before plugin panes have none. */
  pluginIds?: string[];
  /** Split tree of all the tab's panes; files from before split layouts have none and use grid order. */
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

/** Colors a workspace can show, each a `--ws-<name>` theme value in src/styles.css. */
export const WORKSPACE_COLORS = ["blue", "green", "yellow", "orange", "red", "pink", "purple", "gray"] as const;
export type WorkspaceColor = (typeof WORKSPACE_COLORS)[number];

export function isWorkspaceColor(value: unknown): value is WorkspaceColor {
  return (WORKSPACE_COLORS as readonly unknown[]).includes(value);
}

/** The color of the workspace at this position, for files that do not say. */
export function positionColor(index: number): WorkspaceColor {
  return WORKSPACE_COLORS[index % WORKSPACE_COLORS.length];
}

export interface SavedWorkspace {
  id: string;
  /** User-set name, or null. */
  name: string | null;
  /** The tab the workspace shows; one of its own. */
  activeTabId: string;
  /** Files from before icons, and icons this version does not know, get the default. */
  icon: WorkspaceIcon;
  /** Files from before colors, and colors this version does not know, get the position's color. */
  color: WorkspaceColor;
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
  /** Docs panes; a parsed file always has the list. */
  docs?: SavedDocs[];
  /** Plugin panes; a parsed file always has the list. */
  pluginPanes?: SavedPluginPane[];
  workspaces: SavedWorkspace[];
  activeWorkspaceId: string;
  /**
   * Files from v0.2.0 may hold `remote` sessions, which no longer exist. They
   * load as placeholder `ssh` sessions so the rest of the file is checked as
   * usual; the dashboard closes these ids when it loads the file. Only a
   * parsed file that held some has this; it is never written.
   */
  legacyRemoteIds?: string[];
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

const MAX_DOCS = 128;
const MAX_PATH = 4096;

/** An absolute folder on Unix (`/…`) or Windows (`C:\…`, `\\server\…`). */
function isAbsolutePath(value: unknown): value is string {
  return typeof value === "string" && value.length <= MAX_PATH && /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(value) && !value.includes("\0");
}

/** A file path inside a root: `/`-separated parts, none empty, `.` or `..`. */
export function isDocPath(value: unknown): value is string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH) return false;
  if (value.includes("\\") || value.includes("\0")) return false;
  return value.split("/").every((part) => part !== "" && part !== "." && part !== "..");
}

/** The file's docs panes by id, or an empty map when any entry is broken. */
function parseDocs(value: unknown, sessionIds: ReadonlySet<string>): Map<string, SavedDocs> {
  const docs = new Map<string, SavedDocs>();
  if (!Array.isArray(value) || value.length > MAX_DOCS) return docs;
  for (const item of value as unknown[]) {
    if (typeof item !== "object" || item === null) return new Map();
    const raw = item as Record<string, unknown>;
    const title = text(raw.title);
    const name = raw.name == null ? null : text(raw.name);
    const root = raw.root == null ? null : isAbsolutePath(raw.root) ? raw.root : undefined;
    const mode = DOCS_MODES.find((known) => known === raw.mode);
    if (!isId(raw.id) || docs.has(raw.id) || sessionIds.has(raw.id)) return new Map();
    if (title === null || (raw.name != null && name === null) || root === undefined || !mode) return new Map();
    const followFrom = isId(raw.followFrom) && sessionIds.has(raw.followFrom) ? raw.followFrom : null;
    const file = isDocPath(raw.file) ? raw.file : null;
    docs.set(raw.id, { id: raw.id, title, name, root, followFrom, file, mode });
  }
  return docs;
}

const MAX_PLUGIN_PANES = 128;
const MAX_REUSE_KEY = 512;
/** A built-in plugin id, or an external one (`author.name`). */
const PLUGIN_ID = /^(?:[a-z][a-z0-9-]{0,31}|[a-z0-9-]{1,32}\.[a-z0-9-]{1,32})$/;
const PLUGIN_VIEW = /^[a-z][a-z0-9-]{0,31}$/;

/** The file's plugin panes by id, or an empty map when any entry is broken. */
function parsePluginPanes(value: unknown, takenIds: ReadonlySet<string>): Map<string, SavedPluginPane> {
  const panes = new Map<string, SavedPluginPane>();
  if (!Array.isArray(value) || value.length > MAX_PLUGIN_PANES) return panes;
  for (const item of value as unknown[]) {
    if (typeof item !== "object" || item === null) return new Map();
    const raw = item as Record<string, unknown>;
    const title = text(raw.title);
    const name = raw.name == null ? null : text(raw.name);
    const reuseKey = raw.reuseKey == null ? null : text(raw.reuseKey, MAX_REUSE_KEY);
    const data = JSON.stringify(raw.data ?? null);
    if (!isId(raw.id) || panes.has(raw.id) || takenIds.has(raw.id)) return new Map();
    if (title === null || (raw.name != null && name === null) || (raw.reuseKey != null && reuseKey === null)) return new Map();
    if (typeof raw.plugin !== "string" || !PLUGIN_ID.test(raw.plugin)) return new Map();
    if (typeof raw.view !== "string" || !PLUGIN_VIEW.test(raw.view)) return new Map();
    if (data.length > PLUGIN_PANE_DATA_MAX) return new Map();
    panes.set(raw.id, { id: raw.id, title, name, plugin: raw.plugin, view: raw.view, reuseKey, data: raw.data ?? null });
  }
  return panes;
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
  const legacyRemoteIds: string[] = [];
  const sessionIds = new Set<string>();
  for (const item of raw.sessions as unknown[]) {
    if (typeof item !== "object" || item === null) return null;
    const session = item as Record<string, unknown>;
    const title = text(session.title);
    const url = session.url === null ? null : text(session.url, 2048);
    const name = session.name == null ? null : text(session.name);
    const legacyRemote = session.kind === "remote";
    if (!isId(session.id) || sessionIds.has(session.id) || !(legacyRemote || isSessionKind(session.kind))) return null;
    if (title === null || (session.url !== null && url === null)) return null;
    if (session.name != null && name === null) return null;
    const target = session.target == null ? null : normalizeSshTarget(String(session.target));
    const kind = legacyRemote ? "ssh" : (session.kind as SessionKind);
    if ((session.target != null && target === null) || (kind === "ssh" && !target)) return null;
    sessionIds.add(session.id);
    if (legacyRemote) legacyRemoteIds.push(session.id);
    sessions.push({ id: session.id, kind, title, name, url, target, ...(session.autoResume === false && { autoResume: false as const }) });
  }

  const docsById = parseDocs(raw.docs, sessionIds);
  const placedDocs = new Set<string>();
  const pluginsById = parsePluginPanes(raw.pluginPanes, new Set([...sessionIds, ...docsById.keys()]));
  const placedPlugins = new Set<string>();

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
    // Docs ids that do not add up cost this tab its docs panes, not the file.
    const docIds = parseDocIds(tab.docIds, docsById, placedDocs);
    const pluginIds = parseDocIds(tab.pluginIds, pluginsById, placedPlugins);
    const allIds = [...paneIds, ...docIds, ...pluginIds];
    const inTab = (id: unknown): string | null => (isId(id) && allIds.includes(id) ? id : null);
    const name = tab.name == null ? null : text(tab.name);
    if (tab.name != null && name === null) return null;
    tabIds.add(tab.id);
    // The tree must hold exactly this tab's panes; otherwise use grid order.
    const inThisTab = (id: unknown): id is string => isId(id) && allIds.includes(id);
    const parsed = tab.layout == null ? null : parseLayout(tab.layout, inThisTab);
    const sameLeaves =
      parsed !== null && leaves(parsed).length === allIds.length && new Set(leaves(parsed)).size === allIds.length;
    const layout = sameLeaves ? parsed : fromGrid(allIds);
    const order = leaves(layout);
    tabs.push({
      id: tab.id,
      name,
      paneIds: order.filter((id) => sessionIds.has(id)),
      docIds: order.filter((id) => docsById.has(id)),
      pluginIds: order.filter((id) => pluginsById.has(id)),
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
  const legacy = legacyRemoteIds.length > 0 ? { legacyRemoteIds } : {};
  // A docs pane in no tab is dropped.
  const docs = [...docsById.values()].filter((item) => placedDocs.has(item.id));
  const pluginPanes = [...pluginsById.values()].filter((item) => placedPlugins.has(item.id));
  if (grouped) return { version: 1, activeTabId, sessions, docs, pluginPanes, ...grouped, ...legacy };
  return {
    version: 1,
    activeTabId,
    tabs: tabs.map((tab) => ({ ...tab, workspaceId: DEFAULT_WORKSPACE_ID })),
    sessions,
    docs,
    pluginPanes,
    workspaces: [
      { id: DEFAULT_WORKSPACE_ID, name: null, activeTabId, icon: DEFAULT_WORKSPACE_ICON, color: positionColor(0) },
    ],
    activeWorkspaceId: DEFAULT_WORKSPACE_ID,
    ...legacy,
  };
}

/** A tab's docs or plugin panes, each known and in no other tab; none when any is not. Marks them placed. */
function parseDocIds(value: unknown, docs: ReadonlyMap<string, unknown>, placed: Set<string>): string[] {
  if (!Array.isArray(value)) return [];
  const ids = value as unknown[];
  const fits = ids.every((id, index) => isId(id) && docs.has(id) && !placed.has(id) && ids.indexOf(id) === index);
  if (!fits) return [];
  for (const id of ids as string[]) placed.add(id);
  return ids as string[];
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
  for (const [index, item] of (value as unknown[]).entries()) {
    if (typeof item !== "object" || item === null) return null;
    const workspace = item as Record<string, unknown>;
    const name = workspace.name == null ? null : text(workspace.name);
    if (!isId(workspace.id) || !isId(workspace.activeTabId)) return null;
    if (workspaces.some((other) => other.id === workspace.id)) return null;
    if (workspace.name != null && name === null) return null;
    const icon = isWorkspaceIcon(workspace.icon) ? workspace.icon : DEFAULT_WORKSPACE_ICON;
    const color = isWorkspaceColor(workspace.color) ? workspace.color : positionColor(index);
    workspaces.push({ id: workspace.id, name, activeTabId: workspace.activeTabId, icon, color });
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
