// What a plugin's openPane and folders() do to the dashboard, as pure
// functions. Core code: plugin folders never import this file.

import type { OpenPaneOptions } from "./plugins/types.ts";
import { findPluginPane, type Dashboard, type DashboardAction } from "./workspace.ts";

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
