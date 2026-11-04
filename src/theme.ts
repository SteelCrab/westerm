import { useEffect, useState } from "react";
import type { TerminalThemeColors } from "@wterm/core";

export type ColorScheme = "dark" | "light";

const QUERY = "(prefers-color-scheme: light)";

function current(): ColorScheme {
  return typeof matchMedia === "function" && matchMedia(QUERY).matches ? "light" : "dark";
}

/** Follows the OS appearance live, as macOS switches between light and dark. */
export function useColorScheme(): ColorScheme {
  const [scheme, setScheme] = useState<ColorScheme>(current);
  useEffect(() => {
    const media = matchMedia(QUERY);
    const update = () => setScheme(media.matches ? "light" : "dark");
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return scheme;
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
