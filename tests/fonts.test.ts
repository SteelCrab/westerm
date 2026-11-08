import assert from "node:assert/strict";
import { test } from "node:test";
import { cjkFallbacks, clampFontSize, fontStack, terminalFont } from "../src/terminal/fonts.ts";

test("font stack puts the user font first, then Nerd Font icons and Hangul", () => {
  const stack = fontStack(["D2Coding"]);
  assert.ok(stack.startsWith('"D2Coding", "Symbols Nerd Font Mono"'));
  assert.equal(stack.match(/"D2Coding"/g)?.length, 1, "no duplicate families");
  assert.ok(stack.endsWith("monospace"));
  assert.equal(fontStack(['Evil" ; x']).split(",")[0], '"Evil\\" ; x"');
});

test("monospace defaults come before CJK fonts, which are proportional for Latin", () => {
  // Without a Ghostty config (Windows), Latin text took Malgun Gothic: the
  // grid measures "W", so every other letter left a gap.
  for (const locale of ["en", "ko", "ja", "zh"] as const) {
    const stack = fontStack([], locale);
    for (const cjk of ["Malgun Gothic", "Apple SD Gothic Neo", "Hiragino Sans", "PingFang SC", "Microsoft YaHei"]) {
      assert.ok(stack.indexOf('"Consolas"') < stack.indexOf(`"${cjk}"`), `${locale}: Consolas before ${cjk}`);
      assert.ok(stack.indexOf('"Menlo"') < stack.indexOf(`"${cjk}"`), `${locale}: Menlo before ${cjk}`);
    }
  }
});

test("terminal font sizes are clamped and fall back to the Ghostty size", () => {
  assert.equal(clampFontSize(3), 10);
  assert.equal(clampFontSize(99), 28);
  assert.equal(clampFontSize(Number.NaN), 14);
  assert.equal(terminalFont({ fontFamilies: ["D2Coding"], fontSize: 15 }, null).size, 15);
  assert.equal(terminalFont({ fontFamilies: ["D2Coding"], fontSize: 15 }, 18).size, 18);
  assert.equal(terminalFont(null, null).label, "Menlo");
});

test("CJK fallbacks put the UI language's fonts first and keep the others", () => {
  assert.equal(cjkFallbacks("ja")[0], "Noto Sans Mono CJK JP");
  assert.equal(cjkFallbacks("zh")[0], "Noto Sans Mono CJK SC");
  assert.equal(cjkFallbacks("en")[0], "D2Coding");
  assert.deepEqual([...cjkFallbacks("ja")].sort(), [...cjkFallbacks("ko")].sort(), "every script stays covered");
  const stack = fontStack(["Menlo"], "zh");
  assert.ok(stack.indexOf("PingFang SC") < stack.indexOf("Apple SD Gothic Neo"));
});
