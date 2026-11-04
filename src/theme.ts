import { useEffect, useState } from "react";
import type { TerminalThemeColors } from "@wterm/core";

export type ColorScheme = "dark" | "light";
/** "system" follows the OS; the others pin the theme (Settings ▸ Theme). */
export type ThemeChoice = "system" | ColorScheme;

const QUERY = "(prefers-color-scheme: light)";

function current(): ColorScheme {
  return typeof matchMedia === "function" && matchMedia(QUERY).matches ? "light" : "dark";
}

/**
 * The scheme in use: the OS appearance, followed live, unless `choice` pins
 * one. A pinned choice sets <html data-theme>, which the stylesheet follows.
 */
export function useColorScheme(choice: ThemeChoice = "system"): ColorScheme {
  const [system, setSystem] = useState<ColorScheme>(current);
  useEffect(() => {
    const media = matchMedia(QUERY);
    const update = () => setSystem(media.matches ? "light" : "dark");
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  useEffect(() => {
    const root = document.documentElement;
    if (choice === "system") delete root.dataset.theme;
    else root.dataset.theme = choice;
  }, [choice]);
  return choice === "system" ? system : choice;
}

/**
 * Terminal defaults for each scheme. WTerm.setThemeColors() hands them to the
 * Ghostty core too, so programs that ask for the background (OSC 11), such as
 * terminal-browser and Neovim, pick a matching theme.
 */
export const TERMINAL_COLORS: Record<ColorScheme, TerminalThemeColors> = {
  dark: {
    foreground: 0xd4d4d4,
    background: 0x1e1e1e,
    cursor: 0xaeafad,
    palette: [
      0x1e1e1e, 0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xd4d4d4,
      0x808080, 0xf44747, 0x6a9955, 0xd7ba7d, 0x569cd6, 0xc586c0, 0x4ec9b0, 0xffffff,
    ],
  },
  light: {
    foreground: 0x383a42,
    background: 0xfafafa,
    cursor: 0x526fff,
    palette: [
      0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7,
      0x4f525e, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xffffff,
    ],
  },
};
