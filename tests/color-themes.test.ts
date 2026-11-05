import assert from "node:assert/strict";
import { test } from "node:test";
import { COLOR_THEMES, cssColor, isColorTheme, terminalColors, uiColors } from "../src/colorThemes.ts";

/** WCAG contrast ratio of two 0xRRGGBB colors. */
function contrast(a: number, b: number): number {
  const luminance = (color: number) => {
    const [r, g, b] = [16, 8, 0].map((shift) => {
      const c = ((color >> shift) & 0xff) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light + 0.05) / (dark + 0.05);
}

const isColor = (value: number) => Number.isInteger(value) && value >= 0 && value <= 0xffffff;

test("every terminal theme has 16 colors for the dark and the light scheme", () => {
  assert.ok(COLOR_THEMES.length >= 10, "about ten themes");
  for (const id of COLOR_THEMES) {
    for (const scheme of ["dark", "light"] as const) {
      const colors = terminalColors(id, scheme);
      assert.equal(colors.palette?.length, 16, `${id} ${scheme}`);
      for (const value of [colors.foreground, colors.background, colors.cursor, ...(colors.palette ?? [])]) {
        assert.ok(isColor(value as number), `${id} ${scheme}: ${value}`);
      }
    }
  }
});

test("terminal text is readable in every theme", () => {
  for (const id of COLOR_THEMES) {
    for (const scheme of ["dark", "light"] as const) {
      const { foreground, background } = terminalColors(id, scheme);
      const ratio = contrast(foreground as number, background as number);
      assert.ok(ratio >= 4.5, `${id} ${scheme}: ${ratio.toFixed(2)}`);
    }
  }
});

test("a dark scheme gets a dark background, and a light scheme a light one", () => {
  for (const id of COLOR_THEMES) {
    const dark = terminalColors(id, "dark");
    const light = terminalColors(id, "light");
    assert.ok(contrast(dark.background as number, 0x000000) < contrast(light.background as number, 0x000000), id);
  }
});

test("the default theme keeps the colors from before themes", () => {
  assert.equal(COLOR_THEMES[0], "default");
  assert.equal(terminalColors("default", "dark").background, 0x1e1e1e);
  assert.equal(terminalColors("default", "light").background, 0xfafafa);
});

test("theme names are checked, and colors become CSS", () => {
  assert.equal(isColorTheme("gruvbox"), true);
  assert.equal(isColorTheme("purple"), false);
  assert.equal(isColorTheme(undefined), false);
  assert.equal(cssColor(0x1e1e1e), "#1e1e1e");
  assert.equal(cssColor(0x00002b), "#00002b");
});

test("the default theme leaves the window colors to the stylesheet", () => {
  assert.equal(uiColors("default", "dark"), null);
  assert.equal(uiColors("default", "light"), null);
});

/** A "#rrggbb" token as 0xRRGGBB. */
const hex = (value: string | undefined) => {
  assert.match(value ?? "", /^#[0-9a-f]{6}$/);
  return Number.parseInt((value as string).slice(1), 16);
};

test("the other themes color the window, and its text stays readable", () => {
  for (const id of COLOR_THEMES.filter((theme) => theme !== "default")) {
    for (const scheme of ["dark", "light"] as const) {
      const ui = uiColors(id, scheme);
      assert.ok(ui, id);
      const where = `${id} ${scheme}`;
      const grounds = ["--bg", "--surface", "--surface-2"].map((name) => hex(ui[name]));
      const atLeast = (name: string, ratio: number) => {
        for (const ground of grounds) {
          const value = contrast(hex(ui[name]), ground);
          assert.ok(value >= ratio, `${where} ${name}: ${value.toFixed(2)}`);
        }
      };
      // Text, and the status colors that also color text.
      for (const name of ["--text", "--text-muted", "--danger", "--warn"]) atLeast(name, 4.5);
      // Hints, marks, and icons.
      for (const name of ["--text-faint", "--accent", "--ok", "--browser", "--editor", "--ssh"]) atLeast(name, 3);
      const onAccent = contrast(hex(ui["--on-accent"]), hex(ui["--accent"]));
      assert.ok(onAccent >= 4.5, `${where} --on-accent: ${onAccent.toFixed(2)}`);
      assert.equal(hex(ui["--pane-bg"]), terminalColors(id, scheme).background, `${where}: panes match the terminal`);
      for (const name of ["--accent-soft", "--browser-soft", "--editor-soft", "--ssh-soft", "--overlay"]) {
        assert.match(ui[name] ?? "", /^rgba\(\d+, \d+, \d+, 0\.\d+\)$/, `${where} ${name}`);
      }
    }
  }
});
