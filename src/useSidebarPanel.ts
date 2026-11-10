import { useCallback, useState } from "react";
import { isPluginPanel, type SidebarPanel } from "./agents.ts";

const KEY = "westerm.sidebarPanel";
/** Before the tunnel panel, the slot remembered only "agents or not". */
const OLD_KEY = "westerm.agentsView";

/**
 * The remembered panel; the old agents choice counts when there is none. A
 * plugin panel comes back as it was saved: App shows it only while its plugin
 * is installed (agents.ts effectivePanel).
 */
export function readSidebarPanel(storage: Pick<Storage, "getItem"> | null): SidebarPanel | null {
  if (!storage) return null;
  const saved = storage.getItem(KEY);
  if (saved !== null) return saved === "agents" || saved === "tunnel" || isPluginPanel(saved) ? saved : null;
  return storage.getItem(OLD_KEY) === "true" ? "agents" : null;
}

function storage(): Storage | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/** The panel in the sidebar slot (agents.ts SidebarFlags.panel), remembered in this browser. */
export function useSidebarPanel(): [SidebarPanel | null, (panel: SidebarPanel | null) => void] {
  const [panel, setPanel] = useState(() => readSidebarPanel(storage()));
  const update = useCallback((next: SidebarPanel | null) => {
    setPanel(next);
    try {
      localStorage.setItem(KEY, next ?? "");
    } catch {
      // Storage can be unavailable; the choice still applies to this page.
    }
  }, []);
  return [panel, update];
}
