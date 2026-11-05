import { useCallback, useState } from "react";
import type { ThemeChoice } from "./theme.ts";

/**
 * Where the workspaces and tabs are. top: a workspace menu and the tab bar,
 * with no sidebar. sidebar: both in the sidebar, with no tab bar.
 */
export type Layout = "top" | "sidebar";

/** Preferences kept in this browser (or in the macOS app's web view). */
export interface Settings {
  /** "system" follows the OS appearance. */
  theme: ThemeChoice;
  /** Terminal fonts, comma separated. Empty uses the Ghostty config. */
  fontFamily: string;
  /** Look for a new release when the dashboard opens and every few hours. */
  autoUpdate: boolean;
  layout: Layout;
}

export const DEFAULT_SETTINGS: Settings = { theme: "system", fontFamily: "", autoUpdate: true, layout: "top" };

const KEY = "westerm.settings";
export const FONT_FAMILY_MAX = 200;

/** Saved settings with every field checked; anything unusable falls back to the default. */
export function parseSettings(raw: string | null): Settings {
  let value: unknown = null;
  try {
    value = raw ? JSON.parse(raw) : null;
  } catch {
    value = null;
  }
  const saved = (value && typeof value === "object" ? value : {}) as Record<string, unknown>;
  return {
    theme: saved.theme === "light" || saved.theme === "dark" ? saved.theme : DEFAULT_SETTINGS.theme,
    fontFamily: typeof saved.fontFamily === "string" ? saved.fontFamily.slice(0, FONT_FAMILY_MAX) : DEFAULT_SETTINGS.fontFamily,
    autoUpdate: typeof saved.autoUpdate === "boolean" ? saved.autoUpdate : DEFAULT_SETTINGS.autoUpdate,
    layout: saved.layout === "sidebar" ? "sidebar" : DEFAULT_SETTINGS.layout,
  };
}

/** `D2Coding, "Noto Sans Mono"` as ["D2Coding", "Noto Sans Mono"]. */
export function parseFamilies(text: string): string[] {
  return text
    .split(",")
    .map((name) => name.trim().replace(/^["']+|["']+$/g, "").trim())
    .filter(Boolean)
    .slice(0, 8);
}

export function useSettings(): [Settings, (change: Partial<Settings>) => void] {
  const [settings, setSettings] = useState<Settings>(() => {
    try {
      return parseSettings(localStorage.getItem(KEY));
    } catch {
      return DEFAULT_SETTINGS;
    }
  });
  const change = useCallback((patch: Partial<Settings>) => {
    setSettings((current) => {
      const next = { ...current, ...patch };
      try {
        localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        // Storage can be unavailable; the change still applies to this page.
      }
      return next;
    });
  }, []);
  return [settings, change];
}
