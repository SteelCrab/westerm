// The Databases plugin: its description for the plugin manager, and a running
// instance that reads the connection list on a timer (the badge counts open
// connections) and draws the sidebar panel and table panes.

import { useSyncExternalStore } from "react";
import { Database } from "lucide-react";
import { LOCALES, type Locale } from "../../i18n/messages.ts";
import { createPoller } from "../poller.ts";
import type { PluginContext, PluginInstance, PluginPaneProps, PluginStatus, PluginText, WestermPlugin } from "../types.ts";
import { listConnections, readClients } from "./api.ts";
import { DbPanel } from "./DbPanel.tsx";
import { DB_TEXT } from "./i18n.ts";
import { TablePane } from "./TablePane.tsx";
import type { Clients, ConnectionList } from "./types.ts";

const OPEN_INTERVAL = 10_000;
const CLOSED_INTERVAL = 60_000;

interface Snapshot {
  list: ConnectionList | null;
  clients: Clients | null;
}

function start(ctx: PluginContext): PluginInstance {
  const listeners = new Set<() => void>();
  let snapshot: Snapshot = { list: null, clients: null };
  let status: PluginStatus = { badge: null, alert: false };
  let open = false;
  // The console programs are read again when the panel opens.
  let clientsStale = true;

  const emit = () => {
    for (const listener of [...listeners]) listener();
  };

  const poller = createPoller({
    tick: async () => {
      const answer = await listConnections(ctx);
      if (!answer.ok && answer.failure.type === "gone") {
        // Uninstalled in another window: stop quietly and let westerm read the list again.
        poller.stop();
        ctx.refreshInstalled();
        return;
      }
      let next = snapshot;
      if (answer.ok) {
        next = { ...next, list: answer.value };
        const badge = answer.value.open > 0 ? answer.value.open : null;
        if (status.badge !== badge) status = { badge, alert: false };
      }
      if (clientsStale) {
        const clients = await readClients(ctx);
        if (clients.ok) {
          clientsStale = false;
          next = { ...next, clients: clients.value };
        }
      }
      if (next !== snapshot) {
        snapshot = next;
        emit();
      }
    },
    interval: () => (open ? OPEN_INTERVAL : CLOSED_INTERVAL),
    hidden: () => document.visibilityState === "hidden",
  });

  const onVisibility = () => {
    if (document.visibilityState === "visible") poller.poke();
  };
  document.addEventListener("visibilitychange", onVisibility);
  poller.start();

  const subscribe = (listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };
  const read = () => snapshot;

  function Panel() {
    const { list, clients } = useSyncExternalStore(subscribe, read, read);
    return <DbPanel ctx={ctx} list={list} clients={clients} refresh={poller.poke} />;
  }

  function PaneView(props: PluginPaneProps) {
    const { list } = useSyncExternalStore(subscribe, read, read);
    return props.view === "table" ? <TablePane {...props} ctx={ctx} list={list} /> : null;
  }

  return {
    subscribe,
    status: () => status,
    setPanelOpen(next) {
      if (next === open) return;
      open = next;
      if (open) {
        clientsStale = true;
        poller.poke();
      }
    },
    dispose() {
      poller.stop();
      document.removeEventListener("visibilitychange", onVisibility);
      listeners.clear();
    },
    Panel,
    PaneView,
  };
}

export const dbPlugin: WestermPlugin = {
  id: "db",
  version: "0.1.0",
  icon: Database,
  text: Object.fromEntries(
    LOCALES.map((locale): [Locale, PluginText] => {
      const { name, description, permissions } = DB_TEXT[locale];
      return [locale, { name, description, permissions }];
    }),
  ) as Record<Locale, PluginText>,
  start,
};
