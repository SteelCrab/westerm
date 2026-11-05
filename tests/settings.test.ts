import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_SETTINGS, parseFamilies, parseSettings } from "../src/settings.ts";

test("saved settings are checked field by field", () => {
  assert.deepEqual(parseSettings(null), DEFAULT_SETTINGS);
  assert.deepEqual(parseSettings("not json"), DEFAULT_SETTINGS);
  assert.deepEqual(
    parseSettings('{"theme":"dark","fontFamily":"D2Coding","autoUpdate":false,"layout":"sidebar"}'),
    { theme: "dark", fontFamily: "D2Coding", autoUpdate: false, layout: "sidebar" },
  );
  const odd = parseSettings('{"theme":"purple","fontFamily":42,"autoUpdate":"yes","layout":"left"}');
  assert.deepEqual(odd, DEFAULT_SETTINGS, "unknown values fall back one by one");
  assert.equal(parseSettings(JSON.stringify({ fontFamily: "x".repeat(500) })).fontFamily.length, 200);
});

test("workspaces and tabs are at the top unless settings pick the sidebar", () => {
  assert.equal(DEFAULT_SETTINGS.layout, "top");
  assert.equal(parseSettings('{"theme":"dark"}').layout, "top", "settings saved before the choice");
  assert.equal(parseSettings('{"layout":"top"}').layout, "top");
});

test("font names are split on commas and unquoted", () => {
  assert.deepEqual(parseFamilies(` D2Coding, "Noto Sans Mono" ,'Menlo',, `), ["D2Coding", "Noto Sans Mono", "Menlo"]);
  assert.deepEqual(parseFamilies(""), []);
  assert.equal(parseFamilies(Array.from({ length: 12 }, (_, i) => `F${i}`).join(",")).length, 8);
});
