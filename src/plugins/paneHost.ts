// What a plugin pane needs to find its plugin: the running instances and the
// registry (for the plugin's name when it is not installed). App provides it.

import { createContext } from "react";
import type { PluginInstance, WestermPlugin } from "./types.ts";

export interface PaneHost {
  instances: ReadonlyMap<string, PluginInstance>;
  registry: readonly WestermPlugin[];
}

export const PaneHostContext = createContext<PaneHost>({ instances: new Map(), registry: [] });
