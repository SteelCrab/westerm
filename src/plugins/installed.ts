// The installed list and the running plugin instances, as pure functions.

import type { PluginContext, PluginInstance, PluginStatus, WestermPlugin } from "./types.ts";

/**
 * The built-in plugins a page has: all of them on the computer's own
 * dashboard (`local`), else only the ones that run everywhere. A remote
 * window and a tunnel page have no plugin manager, so those are always on.
 */
export function pagePlugins(all: readonly WestermPlugin[], local: boolean): WestermPlugin[] {
  return local ? [...all] : all.filter((plugin) => plugin.everywhere === true);
}

/** The ids in `GET /api/plugins` that this page knows, in `known` order. Anything else reads as none. */
export function parseInstalled(json: unknown, known: readonly string[]): string[] {
  const list = typeof json === "object" && json !== null ? (json as { installed?: unknown }).installed : null;
  if (!Array.isArray(list)) return [];
  return known.filter((id) => list.includes(id));
}

/** `list` with `id` installed (`on`) or not, in `known` order. */
export function withPlugin(list: readonly string[], id: string, on: boolean, known: readonly string[]): string[] {
  return known.filter((each) => (each === id ? on : list.includes(each)));
}

/** Whether the plugin manager closes after a change: only a successful install, so the new panel shows. */
export function closesManager(ok: boolean, on: boolean): boolean {
  return ok && on;
}

/**
 * Whether an external plugin's change opens its panel and closes the manager:
 * only a plugin that was not installed before. An update, a reload, or a
 * reinstall keeps the manager open (a new version restarts the instance).
 */
export function externalOpensPanel(installed: boolean, known: boolean): boolean {
  return installed && !known;
}

/**
 * What identifies a running instance: a new version, or new files of the same
 * version (a folder Reload), needs a new instance, or old code would mix with
 * new assets.
 */
export function instanceKey(plugin: Pick<WestermPlugin, "id" | "version"> & { external?: { installedAt: number } }): string {
  return `${plugin.id}@${plugin.version}@${plugin.external?.installedAt ?? 0}`;
}

// The key each instance was started with.
const startedAs = new WeakMap<PluginInstance, string>();

/**
 * The instances for `installed`: running ones stay, new ones start with their
 * own context, and ones no longer installed are disposed. One whose plugin
 * changed (`instanceKey`) is disposed and started again.
 */
export function syncInstances(
  current: ReadonlyMap<string, PluginInstance>,
  installed: readonly string[],
  registry: readonly WestermPlugin[],
  contextFor: (id: string) => PluginContext,
): Map<string, PluginInstance> {
  const next = new Map<string, PluginInstance>();
  for (const plugin of registry) {
    if (!installed.includes(plugin.id)) continue;
    const key = instanceKey(plugin);
    const running = current.get(plugin.id);
    if (running !== undefined && startedAs.get(running) === key) {
      next.set(plugin.id, running);
      continue;
    }
    // The old instance ends before its successor starts.
    running?.dispose();
    const started = plugin.start(contextFor(plugin.id));
    startedAs.set(started, key);
    next.set(plugin.id, started);
  }
  for (const [id, instance] of current) {
    if (!next.has(id)) instance.dispose();
  }
  return next;
}

/**
 * One external store over every instance's status, for useSyncExternalStore:
 * the snapshot stays the same object until a status changes.
 */
export function statusStore(instances: ReadonlyMap<string, PluginInstance>): {
  subscribe(listener: () => void): () => void;
  snapshot(): Record<string, PluginStatus>;
} {
  let last: Record<string, PluginStatus> = {};
  return {
    subscribe(listener) {
      const unsubscribes = [...instances.values()].map((instance) => instance.subscribe(listener));
      return () => {
        for (const unsubscribe of unsubscribes) unsubscribe();
      };
    },
    snapshot() {
      const ids = [...instances.keys()];
      const same =
        ids.length === Object.keys(last).length && ids.every((id) => last[id] === instances.get(id)!.status());
      if (!same) last = Object.fromEntries(ids.map((id) => [id, instances.get(id)!.status()]));
      return last;
    },
  };
}
