import assert from "node:assert/strict";
import { test } from "node:test";
import { shortcutLabel, switchCommand } from "../src/shortcuts.ts";

const press = (code: string, mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {}) => ({
  code,
  ctrlKey: Boolean(mods.ctrl),
  shiftKey: Boolean(mods.shift),
  altKey: Boolean(mods.alt),
  metaKey: Boolean(mods.meta),
});

test("Ctrl+Shift keys switch terminals in the browser and the app", () => {
  for (const inApp of [false, true]) {
    assert.deepEqual(switchCommand(press("Digit3", { ctrl: true, shift: true }), inApp), { type: "tab", number: 3 });
    assert.deepEqual(switchCommand(press("BracketRight", { ctrl: true, shift: true }), inApp), { type: "tabStep", delta: 1 });
    assert.deepEqual(switchCommand(press("BracketLeft", { ctrl: true, shift: true }), inApp), { type: "tabStep", delta: -1 });
    assert.deepEqual(switchCommand(press("Period", { ctrl: true, shift: true }), inApp), { type: "paneStep", delta: 1 });
    assert.deepEqual(switchCommand(press("Comma", { ctrl: true, shift: true }), inApp), { type: "paneStep", delta: -1 });
  }
  assert.equal(switchCommand(press("BracketLeft", { ctrl: true }), false), null, "Ctrl+[ stays Escape for the terminal");
  assert.equal(switchCommand(press("KeyC", { ctrl: true, shift: true }), false), null);
});

test("⌘ keys switch terminals only in the macOS app, where the browser does not keep them", () => {
  assert.equal(switchCommand(press("Digit1", { meta: true }), false), null);
  assert.deepEqual(switchCommand(press("Digit1", { meta: true }), true), { type: "tab", number: 1 });
  assert.deepEqual(switchCommand(press("BracketRight", { meta: true, shift: true }), true), { type: "tabStep", delta: 1 });
  assert.deepEqual(switchCommand(press("BracketRight", { meta: true }), true), { type: "paneStep", delta: 1 });
  assert.deepEqual(switchCommand(press("ArrowUp", { meta: true, alt: true }), true), { type: "paneToward", direction: "up" });
  assert.equal(switchCommand(press("KeyK", { meta: true }), true), null, "⌘K stays the palette");
  assert.equal(switchCommand(press("Digit1", { meta: true, shift: true }), true), null);
});

test("shortcut labels match the keys that work where they are shown", () => {
  assert.equal(shortcutLabel({ type: "tabStep", delta: 1 }, true, true), "⌘⇧]");
  assert.equal(shortcutLabel({ type: "paneStep", delta: -1 }, true, true), "⌘[");
  assert.equal(shortcutLabel({ type: "paneStep", delta: -1 }, false, true), "⌃⇧,");
  assert.equal(shortcutLabel({ type: "tab", number: 2 }, false, false), "Ctrl+Shift+2");
  assert.equal(shortcutLabel({ type: "paneToward", direction: "left" }, true, true), "⌥⌘←");
});
