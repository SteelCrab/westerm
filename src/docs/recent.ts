// Folders a docs pane showed lately, newest first, for the root field's list.
// Kept per machine: a remote window's folders are the server's.

import { pageStorage, type Storage } from "./drafts.ts";

const STORAGE_KEY = "westerm.docs.recentRoots";
const MAX_ROOTS = 8;

export interface RecentRoots {
  list(): string[];
  remember(root: string): void;
}

export function createRecentRoots(storage: Storage | null, target: string | null = null): RecentRoots {
  const key = target === null ? STORAGE_KEY : `${STORAGE_KEY}.${target}`;
  let roots: string[] = [];
  try {
    const saved: unknown = JSON.parse(storage?.getItem(key) ?? "[]");
    if (Array.isArray(saved)) roots = saved.filter((root): root is string => typeof root === "string").slice(0, MAX_ROOTS);
  } catch {
    // Junk or no access: start with none.
  }
  return {
    list: () => [...roots],
    remember(root) {
      if (roots[0] === root) return;
      roots = [root, ...roots.filter((other) => other !== root)].slice(0, MAX_ROOTS);
      try {
        storage?.setItem(key, JSON.stringify(roots));
      } catch {
        // Full or denied: the list lasts until the page closes.
      }
    },
  };
}
