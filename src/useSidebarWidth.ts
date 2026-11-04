import { useCallback, useState } from "react";
import { clampSidebar, SIDEBAR_DEFAULT } from "./components/SidebarResizer.tsx";

const KEY = "westerm.sidebarWidth";
/** Key used before the rename; read once so the setting carries over. */
const LEGACY_KEY = "wterm-dashboard.sidebarWidth";

function read(): number {
  try {
    const saved = localStorage.getItem(KEY) ?? localStorage.getItem(LEGACY_KEY);
    return saved ? clampSidebar(Number(saved)) : SIDEBAR_DEFAULT;
  } catch {
    return SIDEBAR_DEFAULT;
  }
}

/** Sidebar width in CSS pixels, remembered in this browser. */
export function useSidebarWidth(): [number, (width: number) => void] {
  const [width, setWidth] = useState(read);
  const update = useCallback((next: number) => {
    const value = clampSidebar(next);
    setWidth(value);
    try {
      localStorage.setItem(KEY, String(value));
    } catch {
      // Storage can be unavailable; the width still applies to this page.
    }
  }, []);
  return [width, update];
}
