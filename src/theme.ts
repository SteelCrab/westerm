import { useEffect, useState } from "react";
import { uiColors, type ColorThemeId } from "./colorThemes.ts";

export type ColorScheme = "dark" | "light";
/** "system" follows the OS; the others pin the scheme (Settings ▸ Appearance ▸ Mode). */
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
 * Colors the window with the color theme (Settings ▸ Appearance ▸ Color theme): its
 * colors go on <html> and override the stylesheet's. The default theme
 * keeps the stylesheet's.
 */
export function useWindowColors(theme: ColorThemeId, scheme: ColorScheme): void {
  useEffect(() => {
    const colors = uiColors(theme, scheme);
    if (!colors) return;
    const style = document.documentElement.style;
    for (const [name, value] of Object.entries(colors)) style.setProperty(name, value);
    return () => {
      for (const name of Object.keys(colors)) style.removeProperty(name);
    };
  }, [theme, scheme]);
}
