import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { parsePluginList, type ExternalInfo } from "./external/info.ts";
import { externalPlugin } from "./external/plugin.tsx";
import { instanceKey, parseInstalled, statusStore, syncInstances, withPlugin } from "./installed.ts";
import { BUILTIN_PLUGINS } from "./registry.ts";
import type { PluginContext, PluginInstance, PluginStatus, WestermPlugin } from "./types.ts";

const PATH = "/api/plugins";

export interface Plugins {
  /** The installed ids in registry order; null until the first read answers. */
  installed: string[] | null;
  /** The built-in plugins, then the external plugins the server knows. */
  registry: readonly WestermPlugin[];
  /** The installed external plugins, as the server describes them. */
  external: readonly ExternalInfo[];
  instances: ReadonlyMap<string, PluginInstance>;
  statuses: Record<string, PluginStatus>;
  /** Installs or uninstalls a built-in plugin on the server; false when the server did not take it. */
  setInstalled(id: string, on: boolean): Promise<boolean>;
  /** Reads the installed list again; resolves when the answer is in (or failed). */
  refresh(): Promise<void>;
}

interface List {
  installed: string[] | null;
  external: ExternalInfo[];
}

const sameIds = (a: readonly string[] | null, b: readonly string[]) =>
  a !== null && a.length === b.length && a.every((id, index) => id === b[index]);

const keyOf = (info: ExternalInfo) => instanceKey({ id: info.id, version: info.version, external: info });

const sameExternal = (a: readonly ExternalInfo[], b: readonly ExternalInfo[]) => sameIds(a.map(keyOf), b.map(keyOf));

/**
 * The installed plugins and their running instances. The list is read when
 * the page opens, when it shows again or gets focus, and on `refresh()`; it
 * is the same for every window of this westerm.
 *
 * `contextFor` is called once per instance, when it starts. `allowed` is
 * false in a remote window and on a tunnel page (tunnel.ts pageControls):
 * neither has the plugin routes, so the list stays empty. It does not change
 * while the page is open.
 */
export function usePlugins(contextFor: (id: string) => PluginContext, allowed: boolean): Plugins {
  const [list, setList] = useState<List>({ installed: allowed ? null : [], external: [] });
  const contextForRef = useRef(contextFor);
  contextForRef.current = contextFor;
  // Bumped by each request, so an older answer does not overwrite a newer one.
  const sequence = useRef(0);

  // Whether the server can open plugin panes; the bridges read it per call.
  const shellPanes = useRef(false);
  // One plugin object per external plugin's files (instanceKey), so a new
  // read of the same list keeps the running instances.
  const made = useRef(new Map<string, WestermPlugin>());
  const registry = useMemo(() => {
    const next = new Map<string, WestermPlugin>();
    const external = list.external.map((info) => {
      const key = keyOf(info);
      const plugin = made.current.get(key) ?? externalPlugin(info, undefined, () => shellPanes.current);
      next.set(key, plugin);
      return plugin;
    });
    made.current = next;
    return [...BUILTIN_PLUGINS, ...external];
  }, [list.external]);
  const listRef = useRef(list);
  listRef.current = list;
  const registryRef = useRef(registry);
  registryRef.current = registry;

  const refresh = useCallback(async () => {
    if (!allowed) return;
    const mine = ++sequence.current;
    try {
      const response = await fetch(PATH);
      if (!response.ok) return;
      const json: unknown = await response.json();
      if (mine !== sequence.current) return;
      const { installed, external, shellPanes: panes } = parsePluginList(json);
      shellPanes.current = panes;
      const known = [...BUILTIN_PLUGINS.map((plugin) => plugin.id), ...external.map((info) => info.id)];
      const ids = parseInstalled({ installed }, known);
      setList((previous) => {
        const sameList = sameIds(previous.installed, ids);
        const sameInfo = sameExternal(previous.external, external);
        if (sameList && sameInfo) return previous;
        return { installed: sameList ? previous.installed : ids, external: sameInfo ? previous.external : external };
      });
    } catch {
      // Keep the list as it is; the next focus reads it again.
    }
  }, [allowed]);

  useEffect(() => {
    if (!allowed) return;
    const read = () => void refresh();
    read();
    const onVisible = () => {
      if (document.visibilityState === "visible") read();
    };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener("focus", read);
    return () => {
      document.removeEventListener("visibilitychange", onVisible);
      window.removeEventListener("focus", read);
    };
  }, [allowed, refresh]);

  const setInstalled = useCallback(async (id: string, on: boolean) => {
    if (!allowed) return false;
    // Every plugin of the registry counts, so the external ones stay in the list.
    const known = registryRef.current.map((plugin) => plugin.id);
    const next = withPlugin(listRef.current.installed ?? [], id, on, known);
    ++sequence.current;
    try {
      const response = await fetch(PATH, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ installed: next }),
      });
      if (!response.ok) return false;
    } catch {
      return false;
    }
    setList((previous) => ({ ...previous, installed: next }));
    return true;
  }, [allowed]);

  // Start and dispose before paint, so an installed plugin's panel shows at once.
  const instancesRef = useRef<ReadonlyMap<string, PluginInstance>>(new Map());
  const [instances, setInstances] = useState(instancesRef.current);
  useLayoutEffect(() => {
    const next = syncInstances(instancesRef.current, list.installed ?? [], registry, (id) => contextForRef.current(id));
    instancesRef.current = next;
    setInstances(next);
  }, [list.installed, registry]);
  useEffect(
    () => () => {
      for (const instance of instancesRef.current.values()) instance.dispose();
      instancesRef.current = new Map();
    },
    [],
  );

  const store = useMemo(() => statusStore(instances), [instances]);
  const statuses = useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot);

  return { installed: list.installed, registry, external: list.external, instances, statuses, setInstalled, refresh };
}
