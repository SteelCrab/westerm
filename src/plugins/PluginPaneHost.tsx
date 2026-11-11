// The body of a plugin pane: the plugin's PaneView with this pane's state, or
// a notice when the plugin is not installed (or has no pane views).

import { useContext, useMemo } from "react";
import { useI18n } from "../i18n/index.tsx";
import type { DashboardAction, Session } from "../workspace.ts";
import { PaneHostContext } from "./paneHost.ts";

interface PluginPaneHostProps {
  session: Session;
  focused: boolean;
  dispatch: (action: DashboardAction) => void;
}

export function PluginPaneHost({ session, focused, dispatch }: PluginPaneHostProps) {
  const { t, locale } = useI18n();
  const { instances, registry } = useContext(PaneHostContext);
  const pane = session.pluginPane;
  const id = session.id;
  const actions = useMemo(
    () => ({
      setData: (data: unknown) => dispatch({ type: "pluginPaneData", id, data }),
      setTitle: (title: string) => dispatch({ type: "title", id, title }),
      close: () => dispatch({ type: "close", id }),
    }),
    [dispatch, id],
  );
  if (!pane) return null;
  const View = instances.get(pane.plugin)?.PaneView;
  if (View) {
    return <View view={pane.view} data={pane.data} focused={focused} {...actions} />;
  }
  const name = registry.find((plugin) => plugin.id === pane.plugin)?.text[locale].name ?? pane.plugin;
  return (
    <div className="plugin-pane-missing" role="status">
      <p>{t.pluginPane.missing(name)}</p>
      <button type="button" className="button" onClick={actions.close}>
        {t.pluginPane.close}
      </button>
    </div>
  );
}
