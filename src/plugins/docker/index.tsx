// The Docker plugin: its description for the plugin manager, and a running
// instance that asks the server for the containers on a timer.

import { useSyncExternalStore } from "react";
import { Container } from "lucide-react";
import { LOCALES, type Locale } from "../../i18n/messages.ts";
import type { PluginContext, PluginInstance, PluginStatus, PluginText, WestermPlugin } from "../types.ts";
import { fetchDockerState } from "./api.ts";
import { DockerPanel } from "./DockerPanel.tsx";
import { DOCKER_TEXT } from "./i18n.ts";
import { newFailures, runningCount, type DockerState } from "./model.ts";
import { createPoller } from "./poller.ts";

const OPEN_INTERVAL = 2000;
const CLOSED_INTERVAL = 15_000;

function start(ctx: PluginContext): PluginInstance {
  const listeners = new Set<() => void>();
  // The last answer; null until the first one.
  let state: DockerState | null = null;
  let status: PluginStatus = { badge: null, alert: false };
  let open = false;
  // The next request asks the server to look at the engine afresh (Retry).
  let retryNext = false;

  const emit = () => {
    for (const listener of [...listeners]) listener();
  };

  const setStatus = (badge: number | null, alert: boolean) => {
    if (status.badge !== badge || status.alert !== alert) status = { badge, alert };
  };

  // The badge counts running containers; the red dot shows a new failure
  // while the panel is closed. Neither shows while the engine is not ok.
  const apply = (next: DockerState) => {
    if (next.engine.status === "ok") {
      const before = state?.engine.status === "ok" ? state.containers : null;
      const running = runningCount(next.containers);
      setStatus(running > 0 ? running : null, status.alert || (!open && newFailures(before, next.containers)));
    } else {
      setStatus(null, false);
    }
    state = next;
    emit();
  };

  const poller = createPoller({
    tick: async () => {
      const retry = retryNext;
      retryNext = false;
      const next = await fetchDockerState(ctx, retry);
      if (next === "gone") {
        // Uninstalled in another window: stop quietly and let westerm read the list again.
        poller.stop();
        ctx.refreshInstalled();
      } else if (next !== null) {
        apply(next);
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
  const snapshot = () => state;
  const retry = () => {
    retryNext = true;
    poller.poke();
  };

  function Panel() {
    const current = useSyncExternalStore(subscribe, snapshot, snapshot);
    return <DockerPanel ctx={ctx} state={current} refresh={poller.poke} retry={retry} />;
  }

  return {
    subscribe,
    status: () => status,
    setPanelOpen(next) {
      if (next === open) return;
      open = next;
      if (!open) return;
      if (status.alert) {
        setStatus(status.badge, false);
        emit();
      }
      poller.poke();
    },
    dispose() {
      poller.stop();
      document.removeEventListener("visibilitychange", onVisibility);
      listeners.clear();
    },
    Panel,
  };
}

export const dockerPlugin: WestermPlugin = {
  id: "docker",
  version: "0.1.0",
  icon: Container,
  text: Object.fromEntries(
    LOCALES.map((locale): [Locale, PluginText] => {
      const { name, description, permissions } = DOCKER_TEXT[locale];
      return [locale, { name, description, permissions }];
    }),
  ) as Record<Locale, PluginText>,
  start,
};
