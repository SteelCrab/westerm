// Settings ▸ Terminal ▸ Font: the installed fonts to pick from, and the
// ordered list the user builds. The list is saved as comma separated text.

import type { SystemFont } from "../../shared/protocol.ts";
import type { Locale } from "../i18n/messages.ts";
import { FONT_FAMILY_MAX, MAX_FONT_FAMILIES } from "../settings.ts";

/** Lower case without spaces: "jetbrainsmono" finds "JetBrains Mono". */
function searchKey(text: string): string {
  return text.toLowerCase().replace(/\s+/g, "");
}

/**
 * Fonts whose name has `query` ignoring case and spaces, or has each of its
 * words ("fira mono"); names that start with it come first. Otherwise the
 * server's order (by name) stays.
 */
export function searchFonts(fonts: SystemFont[], query: string, monoOnly: boolean): SystemFont[] {
  const key = searchKey(query);
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = monoOnly ? fonts.filter((font) => font.mono) : fonts;
  if (!key) return shown;
  const starts: SystemFont[] = [];
  const contains: SystemFont[] = [];
  for (const font of shown) {
    const name = searchKey(font.family);
    const lower = font.family.toLowerCase();
    if (name.startsWith(key)) starts.push(font);
    else if (name.includes(key) || words.every((word) => lower.includes(word))) contains.push(font);
  }
  return [...starts, ...contains];
}

/** The tags beside a font: the UI language's script, and Nerd Font icons. */
export interface FontTags {
  script: boolean;
  icons: boolean;
}

export function fontTags(font: SystemFont, locale: Locale): FontTags {
  const script = {
    ko: font.hangul,
    ja: font.kana,
    zh: font.han,
    en: font.hangul || font.kana || font.han,
  }[locale];
  return { script, icons: font.icons };
}

/**
 * `name` added last. Unchanged when it is blank, already there (in any
 * case), has a comma (the saved text splits on commas), or the list is at
 * its count or text length limit.
 */
export function addFamily(list: string[], name: string): string[] {
  const family = name.trim();
  if (!family || family.includes(",")) return list;
  if (list.length >= MAX_FONT_FAMILIES) return list;
  if (list.some((item) => item.toLowerCase() === family.toLowerCase())) return list;
  const next = [...list, family];
  return next.join(", ").length > FONT_FAMILY_MAX ? list : next;
}

export function removeFamily(list: string[], index: number): string[] {
  if (index < 0 || index >= list.length) return list;
  return list.filter((_, at) => at !== index);
}

/** Moves item `from` to index `to`, counted without the moved item (see `insertionIndex`). */
export function moveFamily(list: string[], from: number, to: number): string[] {
  if (from < 0 || from >= list.length) return list;
  const rest = removeFamily(list, from);
  const at = Math.min(Math.max(to, 0), rest.length);
  if (at === from) return list;
  return [...rest.slice(0, at), list[from], ...rest.slice(at)];
}

/** The server's answer with every entry checked; a bad entry is dropped, a bad flag is false. */
export function parseSystemFonts(value: unknown): SystemFont[] {
  if (!Array.isArray(value)) return [];
  const fonts: SystemFont[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object") continue;
    const item = entry as Record<string, unknown>;
    if (typeof item.family !== "string" || !item.family.trim()) continue;
    const flag = (key: string) => item[key] === true;
    fonts.push({
      family: item.family,
      mono: flag("mono"),
      hangul: flag("hangul"),
      kana: flag("kana"),
      han: flag("han"),
      icons: flag("icons"),
    });
  }
  return fonts;
}
