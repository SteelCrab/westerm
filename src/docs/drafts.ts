// Unsaved edits of docs panes, one draft per file. Every docs pane of the
// window reads the same draft, and drafts are kept in localStorage, so a
// reload, a closed pane, or a quit app loses nothing. A draft goes when it is
// saved or discarded.

import { useSyncExternalStore } from "react";

export interface Draft {
  /** The editor's text, with `\n` line ends. */
  text: string;
  /** The file version the edit started from; null for a file that was not there. */
  baseVersion: string | null;
  /** The file used `\r\n` line ends. */
  crlf: boolean;
}

export type Storage = { getItem(key: string): string | null; setItem(key: string, value: string): void };

const STORAGE_KEY = "westerm.docs.drafts";
const SEPARATOR = "\u0000";

/** One file of one window's machine: the server of a remote window, or this computer. */
export function draftKey(target: string | null, root: string, path: string): string {
  return [target ?? "", root, path].join(SEPARATOR);
}

export interface DraftStore {
  get(key: string): Draft | null;
  set(key: string, draft: Draft): void;
  clear(key: string): void;
  /** Paths under `root` that have a draft. */
  paths(target: string | null, root: string): Set<string>;
  /** Drafts of the same machine in other roots, by root and path. */
  elsewhere(target: string | null, root: string): { root: string; path: string }[];
  subscribe(listener: () => void): () => void;
  /** Changes on every change; for useSyncExternalStore. */
  version(): number;
}

function isDraft(value: unknown): value is Draft {
  if (typeof value !== "object" || value === null) return false;
  const draft = value as Record<string, unknown>;
  return (
    typeof draft.text === "string" &&
    (draft.baseVersion === null || typeof draft.baseVersion === "string") &&
    typeof draft.crlf === "boolean"
  );
}

function load(storage: Storage | null): Map<string, Draft> {
  const drafts = new Map<string, Draft>();
  try {
    const saved: unknown = JSON.parse(storage?.getItem(STORAGE_KEY) ?? "{}");
    if (typeof saved !== "object" || saved === null) return drafts;
    for (const [key, draft] of Object.entries(saved)) if (isDraft(draft)) drafts.set(key, draft);
  } catch {
    // Junk or no access: start without saved drafts.
  }
  return drafts;
}

/** A store over `storage`; null keeps drafts in memory only. Storage failures cost only the persistence. */
export function createDraftStore(storage: Storage | null): DraftStore {
  const drafts = load(storage);
  const listeners = new Set<() => void>();
  let count = 0;
  const changed = () => {
    count += 1;
    try {
      storage?.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(drafts)));
    } catch {
      // Full or denied: the drafts stay in memory.
    }
    for (const listener of listeners) listener();
  };
  return {
    get: (key) => drafts.get(key) ?? null,
    set(key, draft) {
      drafts.set(key, draft);
      changed();
    },
    clear(key) {
      if (drafts.delete(key)) changed();
    },
    paths(target, root) {
      const prefix = draftKey(target, root, "");
      const found = new Set<string>();
      for (const key of drafts.keys()) if (key.startsWith(prefix)) found.add(key.slice(prefix.length));
      return found;
    },
    elsewhere(target, root) {
      const found: { root: string; path: string }[] = [];
      for (const key of drafts.keys()) {
        const [owner, draftRoot, path] = key.split(SEPARATOR);
        if (owner === (target ?? "") && draftRoot !== root && path) found.push({ root: draftRoot, path });
      }
      return found.sort((a, b) => (a.root === b.root ? (a.path < b.path ? -1 : 1) : a.root < b.root ? -1 : 1));
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    version: () => count,
  };
}

export function pageStorage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export const drafts: DraftStore = createDraftStore(pageStorage());

/** Re-renders on every draft change; returns the change count. */
export function useDrafts(): number {
  return useSyncExternalStore(drafts.subscribe, drafts.version);
}
