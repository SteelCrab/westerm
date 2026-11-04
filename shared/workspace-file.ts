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
  /** ssh destination, for ssh sessions and remote browsers. */
  target?: string | null;
}

export interface SavedTab {
  id: string;
  /** User-set name, or null. */
  name: string | null;
  /** The tab's panes in reading order. */
  paneIds: string[];
  /** Split tree; files from before split layouts have none and use grid order. */
  layout?: LayoutNode | null;
  focusedId: string | null;
  maximizedId: string | null;
}

export interface SavedWorkspace {
  version: 1;
  activeTabId: string;
  tabs: SavedTab[];
  sessions: SavedSession[];
}

export const WORKSPACE_PATH = "/api/workspace";
export const WORKSPACE_MAX_BYTES = 256 * 1024;
const MAX_TABS = 64;
const MAX_SESSIONS = 64;
const MAX_TEXT = 512;

function text(value: unknown, max = MAX_TEXT): string | null {
  return typeof value === "string" && value.length <= max ? value : null;
}

/**
 * Validates untrusted JSON into a consistent workspace, or null. Every pane id
 * must name a session, every session must sit in exactly one tab, and focus
 * and maximize must point inside their tab.
 */
export function parseSavedWorkspace(value: unknown): SavedWorkspace | null {
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
    if ((session.target != null && target === null) || (session.kind === "ssh" && !target)) return null;
    sessionIds.add(session.id);
    sessions.push({ id: session.id, kind: session.kind, title, name, url, target });
  }

  const tabs: SavedTab[] = [];
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
  }
  if (placed.size !== sessionIds.size) return null;
  if (!isId(raw.activeTabId) || !tabIds.has(raw.activeTabId)) return null;
  return { version: 1, activeTabId: raw.activeTabId, tabs, sessions };
}
