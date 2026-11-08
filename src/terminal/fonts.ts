import type { TerminalConfig } from "../../shared/protocol.ts";
import type { Locale } from "../i18n/messages.ts";

/**
 * Ghostty ships its own Nerd Font symbols, so a plain font such as D2Coding
 * still shows prompt icons there. A browser has no such built-in fallback, so
 * the stack names installed Nerd Fonts after the user's font. Missing fonts
 * are skipped by the browser.
 */
export const NERD_FONT_FALLBACKS = [
  "Symbols Nerd Font Mono",
  "D2KodingLigature Nerd Font Mono",
  "D2CodingLigature Nerd Font Mono",
  "JetBrainsMono Nerd Font Mono",
  "FiraCode Nerd Font Mono",
  "MesloLGS NF",
  "MesloLGS Nerd Font Mono",
  "Hack Nerd Font Mono",
  "Maple Mono NF",
];

/** CJK glyphs for fonts without them, per script. */
const CJK_FALLBACKS = {
  ko: ["D2Coding", "Apple SD Gothic Neo", "Malgun Gothic", "Noto Sans Mono CJK KR"],
  ja: ["Noto Sans Mono CJK JP", "Hiragino Sans", "Hiragino Kaku Gothic ProN", "MS Gothic"],
  zh: ["Noto Sans Mono CJK SC", "PingFang SC", "Microsoft YaHei"],
};

/**
 * All CJK fallbacks, the UI language's first: Han characters shared by the
 * three languages then take that language's glyph shapes. English keeps
 * Korean first, as before.
 */
export function cjkFallbacks(locale: Locale): string[] {
  const first = locale === "en" ? "ko" : locale;
  const order = [first, ...(["ko", "ja", "zh"] as const).filter((script) => script !== first)];
  return order.flatMap((script) => CJK_FALLBACKS[script]);
}
const DEFAULT_FAMILIES = ["Menlo", "Monaco", "Cascadia Mono", "Consolas", "DejaVu Sans Mono"];
const EMOJI_FALLBACKS = ["Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji"];
const GENERIC = new Set(["monospace", "ui-monospace", "sans-serif", "serif", "system-ui"]);

export const DEFAULT_FONT_SIZE = 14;
export const MIN_FONT_SIZE = 10;
export const MAX_FONT_SIZE = 28;

function cssFamily(name: string): string {
  if (GENERIC.has(name)) return name;
  return `"${name.replace(/["\\]/g, "\\$&")}"`;
}

/**
 * CSS font-family list: user fonts, Nerd Font icons, defaults, CJK, emoji.
 * The monospace defaults come before the CJK fonts: Malgun Gothic, Apple SD
 * Gothic Neo, and others are proportional for Latin, and WTerm sizes the grid
 * from "W", so Latin in them leaves gaps. The defaults have no CJK glyphs,
 * which still come from the CJK fonts.
 */
export function fontStack(primary: string[], locale: Locale = "en"): string {
  const seen = new Set<string>();
  const names: string[] = [];
  for (const name of [
    ...primary,
    ...NERD_FONT_FALLBACKS,
    ...DEFAULT_FAMILIES,
    ...cjkFallbacks(locale),
    ...EMOJI_FALLBACKS,
    "monospace",
  ]) {
    const trimmed = name.trim();
    const key = trimmed.toLowerCase();
    if (!trimmed || seen.has(key)) continue;
    seen.add(key);
    names.push(cssFamily(trimmed));
  }
  return names.join(", ");
}

export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return DEFAULT_FONT_SIZE;
  return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(size)));
}

export interface TerminalFont {
  /** Name shown in the UI: the first configured family, or the default. */
  label: string;
  stack: string;
  size: number;
}

/** `familyOverride` (Settings ▸ Font) replaces the Ghostty font families when not empty. */
export function terminalFont(
  config: TerminalConfig | null,
  sizeOverride: number | null,
  locale: Locale = "en",
  familyOverride: string[] = [],
): TerminalFont {
  const families = familyOverride.length ? familyOverride : (config?.fontFamilies ?? []);
  // On macOS Ghostty's points equal CSS pixels.
  const size = clampFontSize(sizeOverride ?? config?.fontSize ?? DEFAULT_FONT_SIZE);
  return { label: families[0] ?? DEFAULT_FAMILIES[0], stack: fontStack(families, locale), size };
}

/** CSS variables that WTerm reads for typography. */
export function fontStyle(font: TerminalFont): React.CSSProperties {
  return {
    "--term-font-family": font.stack,
    "--term-font-size": `${font.size}px`,
    "--term-row-height": `${Math.ceil(font.size * 1.2)}px`,
  } as React.CSSProperties;
}
