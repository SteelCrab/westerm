// Unsaved edits of docs panes, one draft per file. Every docs pane reads the
// same draft, and drafts are kept in localStorage, so a reload, a closed pane,
// or a quit app loses nothing. A draft goes when it is saved or discarded.
//
// All windows of the dashboard share one localStorage (a remote window has
// the same origin). So each draft has its own storage key, and a window writes
// only the drafts it changed: it never erases another window's drafts. A
// `storage` event brings another window's change in.

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

/** A storage whose keys can be listed and removed, like localStorage. */
export type KeyedStorage = Storage & { readonly length: number; key(index: number): string | null; removeItem(key: string): void };

/** Runs `task` a little later; a store asks once for all the drafts that wait. */
export type Schedule = (task: () => void) => void;

/** The first version kept all drafts under this one key. */
const OLD_KEY = "westerm.docs.drafts";
const PREFIX = "westerm.docs.draft:";
const SEPARATOR = "\u0000";
/** Typing writes a draft at most this often; a hidden page writes at once. */
const WRITE_DELAY_MS = 250;

/** One file of one window's machine: the server of a remote window, or this computer. */
export function draftKey(target: string | null, root: string, path: string): string {
  return [target ?? "", root, path].join(SEPARATOR);
}

const storageKey = (key: string) => PREFIX + encodeURIComponent(key);

function keyOf(stored: string): string | null {
  if (!stored.startsWith(PREFIX)) return null;
  try {
    return decodeURIComponent(stored.slice(PREFIX.length));
  } catch {
    return null;
  }
}

export interface DraftStore {
  get(key: string): Draft | null;
  set(key: string, draft: Draft): void;
  clear(key: string): void;
  /** True when the last write of `key` failed (a full storage): a reload loses it. */
  unkept(key: string): boolean;
  /** Paths under `root` that have a draft. */
  paths(target: string | null, root: string): Set<string>;
  /** Drafts of the same machine in other roots, by root and path. */
  elsewhere(target: string | null, root: string): { root: string; path: string }[];
  /** Reads a storage key another window changed; null reads them all. */
  sync(stored: string | null): void;
  /** Writes the drafts that wait for their delay. */
  flush(): void;
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

function parse(text: string | null): Draft | null {
  if (text === null) return null;
  try {
    const value: unknown = JSON.parse(text);
    return isDraft(value) ? value : null;
  } catch {
    return null;
  }
}

/** Every draft in `storage`. Junk or no access: no drafts. */
function readAll(storage: KeyedStorage | null): Map<string, Draft> {
  const drafts = new Map<string, Draft>();
  try {
    if (!storage) return drafts;
    for (let index = 0; index < storage.length; index++) {
      const stored = storage.key(index);
      const key = stored === null ? null : keyOf(stored);
      const draft = key === null || stored === null ? null : parse(storage.getItem(stored));
      if (key !== null && draft) drafts.set(key, draft);
    }
  } catch {
    // No access: start without saved drafts.
  }
  return drafts;
}

/** Moves the drafts of the first version to one key each. */
function moveOldDrafts(storage: KeyedStorage | null) {
  try {
    const old = storage?.getItem(OLD_KEY);
    if (!storage || old == null) return;
    const saved: unknown = JSON.parse(old);
    if (typeof saved === "object" && saved !== null) {
      for (const [key, draft] of Object.entries(saved)) {
        if (isDraft(draft) && storage.getItem(storageKey(key)) === null) storage.setItem(storageKey(key), JSON.stringify(draft));
      }
    }
    storage.removeItem(OLD_KEY);
  } catch {
    // Junk, full, or no access: the old key stays as it is.
  }
}

const later: Schedule = (task) => void setTimeout(task, WRITE_DELAY_MS);

/**
 * A store over `storage`; null keeps drafts in memory only. A storage failure
 * costs only the persistence, and `unkept` tells which drafts it cost.
 */
export function createDraftStore(storage: KeyedStorage | null, schedule: Schedule = later): DraftStore {
  moveOldDrafts(storage);
  const drafts = readAll(storage);
  /** Keys changed here and not written yet. */
  const waiting = new Set<string>();
  const unkept = new Set<string>();
  const listeners = new Set<() => void>();
  let count = 0;

  const changed = () => {
    count += 1;
    for (const listener of listeners) listener();
  };

  /** Writes `key` as this window has it. A draft that does not fit leaves no older copy behind. */
  const write = (key: string): boolean => {
    if (!storage) return true;
    const draft = drafts.get(key);
    try {
      if (!draft) {
        storage.removeItem(storageKey(key));
        return true;
      }
      const text = JSON.stringify(draft);
      try {
        storage.setItem(storageKey(key), text);
      } catch {
        // The old copy takes room too: drop it and try once more.
        storage.removeItem(storageKey(key));
        storage.setItem(storageKey(key), text);
      }
      return true;
    } catch {
      return false;
    }
  };

  const flush = () => {
    if (waiting.size === 0) return;
    const keys = [...waiting];
    waiting.clear();
    let shown = false;
    for (const key of keys) {
      const kept = write(key) || !drafts.has(key);
      if (kept === unkept.has(key)) shown = true;
      if (kept) unkept.delete(key);
      else unkept.add(key);
    }
    if (shown) changed();
  };

  const wait = (key: string) => {
    const first = waiting.size === 0;
    waiting.add(key);
    if (first) schedule(flush);
  };

  /** Takes `key` as storage has it, unless this window changed it since. True when that changed something. */
  const take = (key: string): boolean => {
    if (waiting.has(key)) return false;
    const draft = parse(storage?.getItem(storageKey(key)) ?? null);
    const before = drafts.get(key);
    if (!draft) {
      unkept.delete(key);
      return drafts.delete(key);
    }
    drafts.set(key, draft);
    unkept.delete(key);
    return JSON.stringify(before) !== JSON.stringify(draft);
  };

  return {
    get: (key) => drafts.get(key) ?? null,
    set(key, draft) {
      drafts.set(key, draft);
      wait(key);
      changed();
    },
    clear(key) {
      if (!drafts.delete(key)) return;
      wait(key);
      changed();
    },
    unkept: (key) => unkept.has(key),
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
    sync(stored) {
      let moved = false;
      try {
        if (stored === null) {
          const keys = new Set([...drafts.keys(), ...readAll(storage).keys()]);
          for (const key of keys) moved = take(key) || moved;
        } else {
          const key = keyOf(stored);
          if (key !== null) moved = take(key);
        }
      } catch {
        // No access: keep what this window has.
      }
      if (moved) changed();
    },
    flush,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    version: () => count,
  };
}

export function pageStorage(): KeyedStorage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

export const drafts: DraftStore = createDraftStore(pageStorage());

if (typeof window !== "undefined") {
  window.addEventListener("storage", (event) => {
    if (event.storageArea === pageStorage()) drafts.sync(event.key);
  });
  // A closing or hidden page writes at once what waits for the delay.
  window.addEventListener("pagehide", () => drafts.flush());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") drafts.flush();
  });
}

/** Re-renders on every draft change; returns the change count. */
export function useDrafts(): number {
  return useSyncExternalStore(drafts.subscribe, drafts.version);
}
