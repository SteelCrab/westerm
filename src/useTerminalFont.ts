import { useCallback, useEffect, useState } from "react";
import { CONFIG_PATH, type TerminalConfig } from "../shared/protocol.ts";
import type { Locale } from "./i18n/messages.ts";
import { clampFontSize, terminalFont, type TerminalFont } from "./terminal/fonts.ts";

const SIZE_KEY = "westerm.fontSize";
/** Key used before the rename; read once so the setting carries over. */
const LEGACY_SIZE_KEY = "wterm-dashboard.fontSize";

function readSavedSize(): number | null {
  try {
    const value = Number(localStorage.getItem(SIZE_KEY) ?? localStorage.getItem(LEGACY_SIZE_KEY));
    return value ? clampFontSize(value) : null;
  } catch {
    return null;
  }
}

function saveSize(size: number | null): void {
  try {
    if (size === null) localStorage.removeItem(SIZE_KEY);
    else localStorage.setItem(SIZE_KEY, String(size));
  } catch {
    // Storage can be unavailable; the size still applies to this page.
  }
}

export interface TerminalFontState {
  /** Null until the Ghostty config request settles. */
  font: TerminalFont | null;
  /** True when the size comes from the user's override, not Ghostty. */
  sizeOverridden: boolean;
  /** The font named in the Ghostty config, shown as the default in Settings. */
  ghosttyFont: string;
  changeSize: (delta: number) => void;
  resetSize: () => void;
}

/**
 * Ghostty config font, with per-browser overrides for the size and the
 * families (Settings), and CJK fallbacks ordered for `locale`.
 */
export function useTerminalFont(locale: Locale, familyOverride: string[] = []): TerminalFontState {
  const [config, setConfig] = useState<TerminalConfig | null | undefined>(undefined);
  const [sizeOverride, setSizeOverride] = useState<number | null>(readSavedSize);

  useEffect(() => {
    let cancelled = false;
    fetch(CONFIG_PATH)
      .then((response) => (response.ok ? (response.json() as Promise<TerminalConfig>) : null))
      .catch(() => null)
      .then((value) => {
        if (!cancelled) setConfig(value);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const font = config === undefined ? null : terminalFont(config, sizeOverride, locale, familyOverride);
  const ghosttyFont = terminalFont(config ?? null, null).label;

  const changeSize = useCallback(
    (delta: number) => {
      if (!font) return;
      const next = clampFontSize(font.size + delta);
      setSizeOverride(next);
      saveSize(next);
    },
    [font],
  );

  const resetSize = useCallback(() => {
    setSizeOverride(null);
    saveSize(null);
  }, []);

  return { font, sizeOverridden: sizeOverride !== null, ghosttyFont, changeSize, resetSize };
}
