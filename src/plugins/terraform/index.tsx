// The Terraform plugin: its description for the plugin manager, and a
// running instance over the store (the projects, the badge, the runs) that
// draws the sidebar panel and the plan panes.

import { Blocks } from "lucide-react";
import { LOCALES, type Locale } from "../../i18n/messages.ts";
import type { PluginContext, PluginInstance, PluginPaneProps, PluginText, WestermPlugin } from "../types.ts";
import { useApply } from "./ApplyDialog.tsx";
import { TERRAFORM_ICON_URL } from "./icon.tsx";
import { TF_TEXT } from "./i18n.ts";
import { PlanPane } from "./PlanPane.tsx";
import { createStore } from "./store.ts";
import { TerraformPanel } from "./TerraformPanel.tsx";

function start(ctx: PluginContext): PluginInstance {
  const store = createStore(ctx);

  // The panel and each plan pane show the confirmation where it was asked for.
  function Panel() {
    const { openApply, dialog } = useApply(ctx, store);
    return (
      <>
        <TerraformPanel ctx={ctx} store={store} openApply={openApply} />
        {dialog}
      </>
    );
  }

  function PaneView(props: PluginPaneProps) {
    const { openApply, dialog } = useApply(ctx, store);
    if (props.view !== "plan") return null;
    return (
      <>
        <PlanPane {...props} ctx={ctx} store={store} openApply={openApply} />
        {dialog}
      </>
    );
  }

  return {
    subscribe: store.subscribe,
    status: store.status,
    setPanelOpen: store.setPanelOpen,
    dispose: store.dispose,
    Panel,
    PaneView,
  };
}

export const terraformPlugin: WestermPlugin = {
  id: "terraform",
  version: "0.1.0",
  // The plugin bar and the manager draw `iconUrl`: the logo in its brand color.
  icon: Blocks,
  iconUrl: TERRAFORM_ICON_URL,
  text: Object.fromEntries(
    LOCALES.map((locale): [Locale, PluginText] => {
      const { name, description, permissions } = TF_TEXT[locale];
      return [locale, { name, description, permissions }];
    }),
  ) as Record<Locale, PluginText>,
  start,
};
