import { useCallback, useState } from "react";

const KEY = "westerm.agentsView";

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === "true";
  } catch {
    return false;
  }
}

/**
 * The agents panel is open (top layout) or the sidebar shows agents, not
 * sessions (sidebar layout); remembered in this browser.
 */
export function useAgentsView(): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(read);
  const update = useCallback((next: boolean) => {
    setOpen(next);
    try {
      localStorage.setItem(KEY, String(next));
    } catch {
      // Storage can be unavailable; the choice still applies to this page.
    }
  }, []);
  return [open, update];
}
