import { useEffect, useState } from "react";
import { SYSTEM_FONTS_PATH, type SystemFont } from "../shared/protocol.ts";
import { parseSystemFonts } from "./terminal/fontList.ts";

export type SystemFontsState =
  | { status: "loading" }
  | { status: "ready"; fonts: SystemFont[] }
  /** The request failed, or the server is older than the route. */
  | { status: "failed" };

/** The fonts installed on this computer, asked for once per mount. */
export function useSystemFonts(): SystemFontsState {
  const [state, setState] = useState<SystemFontsState>({ status: "loading" });
  useEffect(() => {
    let cancelled = false;
    fetch(SYSTEM_FONTS_PATH)
      .then((response) => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
      .then((value: unknown) => {
        if (!cancelled) setState({ status: "ready", fonts: parseSystemFonts(value) });
      })
      .catch(() => {
        if (!cancelled) setState({ status: "failed" });
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return state;
}
