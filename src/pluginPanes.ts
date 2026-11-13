// What a plugin's openPane and folders() do to the dashboard, as pure
// functions. Core code: plugin folders never import this file.

import type { OpenPaneOptions, OpenShellOptions } from "./plugins/types.ts";
import { findOpenedBy, findPluginPane, findRunPane, type Dashboard, type DashboardAction } from "./workspace.ts";

const MAX_FOLDERS = 32;

/** Focus the plugin's pane with the same key in the workspace in view, or open a new plugin pane. */
export function planOpenPane(
  state: Dashboard,
  plugin: string,
  options: OpenPaneOptions,
  id: string,
  aspect: number,
): DashboardAction {
  const found = options.reuseKey === null ? null : findPluginPane(state, plugin, options.reuseKey);
  if (found) return { type: "focus", id: found };
  return {
    type: "open",
    id,
    kind: "plugin",
    pluginPane: { plugin, view: options.view, reuseKey: options.reuseKey, data: options.data },
    title: options.title,
    aspect,
  };
}

/**
 * What a plugin's openShell does. With a `replaceKey`, the new shell takes the
 * place of the one the plugin opened under that key in the workspace in view.
 * Otherwise `reuse` focuses the pane the same launch opened before.
 */
export function planOpenShell(
  state: Dashboard,
  plugin: string,
  { action, target, cwd, name, reuse, replaceKey }: OpenShellOptions,
  id: string,
  aspect: number,
): DashboardAction {
  const launch = { plugin, action, target };
  const open: DashboardAction = { type: "open", id, kind: "shell", cwd, plugin: launch, name, aspect };
  if (replaceKey != null) {
    const replaceId = findRunPane(state, plugin, replaceKey);
    const runKey = { plugin, key: replaceKey };
    return replaceId === null ? { ...open, runKey } : { ...open, runKey, replaceId };
  }
  const found = reuse ? findOpenedBy(state, launch) : null;
  return found ? { type: "focus", id: found } : open;
}

/** A plain shell in `cwd`, beside the focused pane. */
export function planOpenFolder(cwd: string, name: string | null, id: string, aspect: number): DashboardAction {
  return { type: "open", id, kind: "shell", cwd, name, aspect };
}

/** The folders of the shells in every workspace, in session order, once each. */
export function pluginFolders(state: Dashboard): string[] {
  const folders = new Set<string>();
  for (const session of state.sessions) {
    if (session.kind !== "shell" || !session.place) continue;
    folders.add(session.place.cwd);
    if (folders.size === MAX_FOLDERS) break;
  }
  return [...folders];
}
