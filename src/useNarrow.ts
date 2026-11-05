import { useEffect, useState } from "react";

/**
 * Phone widths. The stylesheet's phone rules use the same query
 * (tests/ui-logic.test.ts checks it): a bottom activity bar, the sidebar as
 * a drawer, and panes stacked top to bottom.
 */
export const PHONE_QUERY = "(max-width: 640px)";

/** True while the window is phone-wide; follows resizes and rotation. */
export function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => typeof matchMedia === "function" && matchMedia(PHONE_QUERY).matches);
  useEffect(() => {
    const media = matchMedia(PHONE_QUERY);
    const update = () => setNarrow(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return narrow;
}
