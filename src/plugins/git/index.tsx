// The Git plugin: its description for the plugin manager, and a running
// instance whose panel reads the repository while it shows. It keeps no
// badge and runs nothing while the panel is closed.

import { FolderGit2 } from "lucide-react";
import { LOCALES, type Locale } from "../../i18n/messages.ts";
import type { PluginContext, PluginInstance, PluginPanelProps, PluginStatus, PluginText, WestermPlugin } from "../types.ts";
import { GitPanel } from "./GitPanel.tsx";
import { GIT_TEXT } from "./i18n.ts";

const NO_STATUS: PluginStatus = { badge: null, alert: false };

function start(ctx: PluginContext): PluginInstance {
  // A new workspace starts the panel over: its folder, selection, and message.
  function Panel({ compact, place }: PluginPanelProps) {
    return <GitPanel key={place.workspace} ctx={ctx} compact={compact} place={place} />;
  }
  return {
    subscribe: () => () => {},
    status: () => NO_STATUS,
    setPanelOpen() {},
    dispose() {},
    Panel,
  };
}

export const gitPlugin: WestermPlugin = {
  id: "git",
  version: "0.1.0",
  icon: FolderGit2,
  everywhere: true,
  text: Object.fromEntries(
    LOCALES.map((locale): [Locale, PluginText] => {
      const { name, description, permissions } = GIT_TEXT[locale];
      return [locale, { name, description, permissions }];
    }),
  ) as Record<Locale, PluginText>,
  start,
};
