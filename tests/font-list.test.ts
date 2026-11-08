import assert from "node:assert/strict";
import { test } from "node:test";
import type { SystemFont } from "../shared/protocol.ts";
import { FONT_FAMILY_MAX, MAX_FONT_FAMILIES } from "../src/settings.ts";
import {
  addFamily,
  fontTags,
  moveFamily,
  parseSystemFonts,
  removeFamily,
  searchFonts,
} from "../src/terminal/fontList.ts";

function font(family: string, flags: Partial<SystemFont> = {}): SystemFont {
  return { family, mono: false, hangul: false, kana: false, han: false, icons: false, ...flags };
}

const FONTS = [
  font("Apple SD Gothic Neo", { hangul: true }),
  font("D2Coding", { mono: true, hangul: true, kana: true, han: true }),
  font("FiraCode Nerd Font Mono", { mono: true, icons: true }),
  font("Hiragino Sans", { kana: true, han: true }),
  font("JetBrains Mono", { mono: true }),
  font("Maple Mono NF", { mono: true, icons: true }),
  font("Menlo", { mono: true }),
];

const names = (fonts: SystemFont[]) => fonts.map((item) => item.family);

test("the search shows monospaced fonts unless asked for all", () => {
  assert.deepEqual(names(searchFonts(FONTS, "", true)), [
    "D2Coding",
    "FiraCode Nerd Font Mono",
    "JetBrains Mono",
    "Maple Mono NF",
    "Menlo",
  ]);
  assert.equal(searchFonts(FONTS, "", false).length, FONTS.length);
});

test("the search ignores case and spaces, and puts names that start with it first", () => {
  assert.deepEqual(names(searchFonts(FONTS, "jetbrainsmono", true)), ["JetBrains Mono"]);
  assert.deepEqual(names(searchFonts(FONTS, "MONO", true)), ["FiraCode Nerd Font Mono", "JetBrains Mono", "Maple Mono NF"]);
  assert.deepEqual(names(searchFonts(FONTS, "m", true)), ["Maple Mono NF", "Menlo", "FiraCode Nerd Font Mono", "JetBrains Mono"]);
  assert.deepEqual(names(searchFonts(FONTS, "  ", false)), names(FONTS), "blank is no filter");
});

test("each word of the search can match a different part of the name", () => {
  assert.deepEqual(names(searchFonts(FONTS, "fira mono", true)), ["FiraCode Nerd Font Mono"]);
  assert.deepEqual(names(searchFonts(FONTS, "mono nf", true)), ["Maple Mono NF"]);
  assert.deepEqual(names(searchFonts(FONTS, "nerd jet", true)), []);
});

test("tags name the UI language's script and Nerd Font icons", () => {
  const d2 = FONTS[1];
  const hiragino = FONTS[3];
  assert.deepEqual(fontTags(d2, "ko"), { script: true, icons: false });
  assert.deepEqual(fontTags(hiragino, "ko"), { script: false, icons: false }, "no Hangul");
  assert.deepEqual(fontTags(hiragino, "ja"), { script: true, icons: false });
  assert.deepEqual(fontTags(hiragino, "zh"), { script: true, icons: false });
  assert.deepEqual(fontTags(hiragino, "en"), { script: true, icons: false }, "English tags any CJK font");
  assert.deepEqual(fontTags(FONTS[2], "en"), { script: false, icons: true });
});

test("adding a font skips repeats and stops at the limits", () => {
  assert.deepEqual(addFamily(["D2Coding"], " Menlo "), ["D2Coding", "Menlo"]);
  assert.deepEqual(addFamily(["D2Coding"], "d2coding"), ["D2Coding"], "same name in other case");
  assert.deepEqual(addFamily(["D2Coding"], "  "), ["D2Coding"]);
  assert.deepEqual(addFamily(["D2Coding"], "Bad, Name"), ["D2Coding"], "a comma would split the saved text");
  const full = Array.from({ length: MAX_FONT_FAMILIES }, (_, index) => `F${index}`);
  assert.deepEqual(addFamily(full, "Menlo"), full);
  const long = ["x".repeat(FONT_FAMILY_MAX - 10)];
  assert.deepEqual(addFamily(long, "JetBrains Mono"), long, "the saved text has a length limit");
});

test("fonts are removed and moved by index", () => {
  const list = ["A", "B", "C", "D"];
  assert.deepEqual(removeFamily(list, 1), ["A", "C", "D"]);
  assert.deepEqual(removeFamily(list, 9), list);
  assert.deepEqual(moveFamily(list, 0, 2), ["B", "C", "A", "D"], "the index counts without the moved font");
  assert.deepEqual(moveFamily(list, 3, 0), ["D", "A", "B", "C"]);
  assert.deepEqual(moveFamily(list, 1, 1), list);
  assert.deepEqual(moveFamily(list, 2, 99), ["A", "B", "D", "C"], "past the end goes last");
  assert.deepEqual(moveFamily(list, -1, 0), list);
});

test("the server's font list is checked entry by entry", () => {
  assert.deepEqual(parseSystemFonts(null), []);
  assert.deepEqual(parseSystemFonts({ fonts: [] }), [], "not an array");
  assert.deepEqual(
    parseSystemFonts([
      { family: "Menlo", mono: true, hangul: false, kana: false, han: false, icons: false },
      { family: 42, mono: true },
      { family: "Odd", mono: "yes" },
      "Monaco",
    ]),
    [font("Menlo", { mono: true }), font("Odd")],
  );
});
