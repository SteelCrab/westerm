// The shape of a plugin and everything it may ask of westerm. The core knows
// plugins only through these types, `registry.ts`, and the external runtime
// in `external/`.

import type { ComponentType } from "react";
import type { LucideIcon } from "lucide-react";
import type { Locale } from "../i18n/messages.ts";
import type { ExternalPermissions, ExternalSource } from "./external/info.ts";

export interface PluginText {
  name: string;
  description: string;
  /** What the plugin can do, one line each; shown in the plugin manager. */
  permissions: string[];
}

/** What the plugin's button in the plugin bar shows. */
export interface PluginStatus {
  /** A number on the button, or none. */
  badge: number | null;
  /** A red dot on the button. */
  alert: boolean;
  /** Why the plugin is not running, shown with its name (an external plugin that stopped). */
  note?: string;
}

export interface OpenShellOptions {
  action: string;
  target: string;
  cwd: string | null;
  name: string;
  reuse: boolean;
}

export interface OpenPaneOptions {
  /** Which of the plugin's views the pane shows (`PluginPaneProps.view`). */
  view: string;
  title: string;
  /** The pane's first state, as JSON; saved with the layout. */
  data: unknown;
  /** Focus the plugin's pane with this key in the workspace in view instead of opening one; null always opens. */
  reuseKey: string | null;
}

/** What westerm hands a plugin's pane view. */
export interface PluginPaneProps {
  view: string;
  data: unknown;
  /** Replaces the pane's state; it is saved with the layout. */
  setData(data: unknown): void;
  setTitle(title: string): void;
  focused: boolean;
  /** Closes the pane. */
  close(): void;
}

/** Where the user is: the folders of the workspace in view. */
export interface PluginPlace {
  /** The id of the workspace in view; a panel may start over when it changes. */
  workspace: string;
  /** The folder of the focused pane, else the first of `folders`; null without one. */
  focused: string | null;
  /** The folders of the workspace's shell and docs panes, once each. */
  folders: readonly string[];
}

/** What westerm hands a plugin's panel each time it draws it. */
export interface PluginPanelProps {
  /** The sidebar is at its narrowest: room for an icon and a number. */
  compact: boolean;
  place: PluginPlace;
}

/** Everything a plugin may ask of westerm. Stable for the life of the plugin. */
export interface PluginContext {
  /** reuse: focus the pane this launch opened before in this workspace, if any. */
  openShell(options: OpenShellOptions): void;
  openBrowser(url: string): void;
  /** Opens a pane the plugin draws (`PluginInstance.PaneView`), beside the focused pane. */
  openPane(options: OpenPaneOptions): void;
  /** The folders of the shells in every workspace, once each, at most 32. */
  folders(): string[];
  /** fetch() under /api/plugins/<id>, on the server of a remote window; `path` starts with "/". */
  fetch(path: string, init?: RequestInit): Promise<Response>;
  /** Read the installed list again (a plugin route answered 404). */
  refreshInstalled(): void;
  /** Opens the plugin manager with this plugin selected, e.g. for its Uninstall button. */
  openManager(): void;
}

/** A running plugin: started on install (or page load), disposed on uninstall. */
export interface PluginInstance {
  subscribe(listener: () => void): () => void;
  /** The same object until it changes (useSyncExternalStore). */
  status(): PluginStatus;
  setPanelOpen(open: boolean): void;
  dispose(): void;
  Panel: ComponentType<PluginPanelProps>;
  /** Draws the plugin's panes; a plugin without panes leaves it out. */
  PaneView?: ComponentType<PluginPaneProps>;
}

export interface WestermPlugin {
  id: string;
  version: string;
  icon: LucideIcon;
  /** An image to show instead of `icon` (an external plugin's SVG, drawn with <img>). */
  iconUrl?: string;
  text: Record<Locale, PluginText>;
  /**
   * Also runs in a remote window and on a tunnel page, always on: those pages
   * have no plugin manager and no other plugin. Its server routes must be
   * served there too.
   */
  everywhere?: boolean;
  /** Set for a plugin installed from outside westerm. */
  external?: { source: ExternalSource; granted: ExternalPermissions; installedAt: number };
  start(ctx: PluginContext): PluginInstance;
}
