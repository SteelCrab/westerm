import { useCallback, useState } from "react";
import { readSidebarPanel, type SidebarPanel } from "./agents.ts";

const KEY = "westerm.sidebarPanel";
/** The key from before plugin panels existed: "true" meant the agents panel. */
const LEGACY_KEY = "westerm.agentsView";

function read(): SidebarPanel | null {
  try {
    return readSidebarPanel(localStorage.getItem(KEY), localStorage.getItem(LEGACY_KEY));
  } catch {
    return null;
  }
}

/**
 * The panel the sidebar slot shows instead of the sessions list (sidebar
 * layout) or the one that is open (top layout); remembered in this browser.
 */
export function useSidebarPanel(): [SidebarPanel | null, (next: SidebarPanel | null) => void] {
  const [panel, setPanel] = useState(read);
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
