import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { WORKSPACE_COLORS } from "../shared/workspace-file.ts";
import { fuzzyScore } from "../src/components/CommandPalette.tsx";
import { placeRename } from "../src/components/InlineRename.tsx";
import { settingsPages } from "../src/components/SettingsDialog.tsx";
import { clampSidebar } from "../src/components/SidebarResizer.tsx";
import { updateShown } from "../src/components/UpdateButton.tsx";
import { stepIndex, tabColors, workspaceColor } from "../src/components/WorkspaceMenu.tsx";
import { insertionIndex } from "../src/dnd.ts";
import { MESSAGES } from "../src/i18n/messages.ts";
import { urlCommands } from "../src/paletteCommands.tsx";
import { keyboardFree } from "../src/terminal/keyboard.ts";

const en = MESSAGES.en;

test("palette search matches subsequences and prefers tight word-start matches", () => {
  assert.equal(fuzzyScore("새 탭", "xyz"), -1);
  assert.ok(fuzzyScore("새 탭", "탭") > 0);
  assert.ok(fuzzyScore("rename tab", "rt") > 0);
  assert.ok(fuzzyScore("new tab", "new") > fuzzyScore("rename tab", "new"), "word start beats scattered");
  assert.equal(fuzzyScore("anything", ""), 0);
});

test("typing a URL offers terminal-browser; plain words do not", () => {
  const opened: Array<[string, string | null | undefined]> = [];
  const open = (kind: string, url?: string | null) => opened.push([kind, url]);
  for (const text of ["example.com", "https://news.ycombinator.com", "localhost:3000", "localhost"]) {
    const [command] = urlCommands(text, open, en);
    assert.ok(command, text);
    command.run();
  }
  assert.deepEqual(opened.at(-1), ["browser", "localhost"]);
  assert.deepEqual(urlCommands("rename", open, en), []);
  assert.deepEqual(urlCommands("javascript:alert(1)", open, en), []);
  assert.deepEqual(urlCommands("   ", open, en), []);
  assert.equal(urlCommands("example.com", open, MESSAGES.ja)[0].title, "terminal-browser で開く: example.com");
});

test("drop indexes account for the moving item's own slot", () => {
  // [a, b, c]: drag a after b → index 1 in [b, c].
  assert.equal(insertionIndex(0, 1, true), 1);
  // drag c before a → index 0.
  assert.equal(insertionIndex(2, 0, false), 0);
  // drag a before c → index 1 in [b, c].
  assert.equal(insertionIndex(0, 2, false), 1);
  // from another list: no adjustment.
  assert.equal(insertionIndex(-1, 1, true), 2);
});

test("sidebar width is bounded", () => {
  assert.equal(clampSidebar(10), 56, "narrow drags snap to icons only");
  assert.equal(clampSidebar(119), 56);
  assert.equal(clampSidebar(120), 160, "between icons only and the minimum it holds the minimum");
  assert.equal(clampSidebar(9999), 480);
  assert.equal(clampSidebar(Number.NaN), 240);
  assert.equal(clampSidebar(300.6), 301);
});

test("every workspace color has a value in the dark and both light theme blocks", () => {
  const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  for (const color of WORKSPACE_COLORS) {
    assert.equal(css.split(`--ws-${color}:`).length - 1, 3, color);
    assert.equal(css.split(`--ws-${color}-soft:`).length - 1, 3, `${color}-soft`);
    assert.equal(workspaceColor(color), `var(--ws-${color})`);
  }
});

/** The declarations of the first CSS rule whose selector list has `selector`. */
function cssRule(css: string, selector: string): string {
  const plain = css.replace(/\/\*[\s\S]*?\*\//g, "");
  for (const [, selectors, body] of plain.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    if (selectors.split(",").some((part) => part.trim() === selector)) return body;
  }
  return assert.fail(`no rule for ${selector}`);
}

test("the tabs take the color of the workspace in view", () => {
  assert.deepEqual(tabColors("pink"), { "--tab-color": "var(--ws-pink)", "--tab-color-soft": "var(--ws-pink-soft)" });
  const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  const colored = [
    ".tab.active::after",
    ".tab.active .tab-index",
    ".tree-tab.active .tab-index",
    ".session-panel.compact .tree-tab.active .tab-index",
    ".tree-session.active",
  ];
  for (const selector of colored) {
    const rule = cssRule(css, selector);
    assert.match(rule, /var\(--tab-color(-soft)?\)/, selector);
    assert.doesNotMatch(rule, /--accent/, `${selector} keeps the accent color`);
  }
});

test("arrow keys in the workspace menu wrap around", () => {
  assert.equal(stepIndex(0, 1, 3), 1);
  assert.equal(stepIndex(2, 1, 3), 0, "down from the last goes to the first");
  assert.equal(stepIndex(0, -1, 3), 2, "up from the first goes to the last");
  assert.equal(stepIndex(-1, 1, 3), 0, "nothing focused: down goes to the first");
  assert.equal(stepIndex(-1, -1, 3), 2, "nothing focused: up goes to the last");
});

test("the terminal leaves the keyboard to dialogs and rename fields", () => {
  // A palette command that starts a rename also closes the palette, and
  // closing it asks the terminal to take the keyboard back.
  const page = (modal: boolean, active: string | null) => ({
    querySelector: (selector: string) => (modal && selector === '[aria-modal="true"]' ? {} : null),
    activeElement: active === null ? null : { classList: { contains: (name: string) => name === active } },
  });
  assert.equal(keyboardFree(page(false, null)), true);
  assert.equal(keyboardFree(page(false, "palette-input")), true);
  assert.equal(keyboardFree(page(true, null)), false, "a dialog is open");
  assert.equal(keyboardFree(page(false, "inline-rename")), false, "a rename field has the keyboard");
});

test("a rename field opens where the layout shows the item", () => {
  const tab = { kind: "tab", id: "t1", where: "tabbar" } as const;
  const workspace = { kind: "workspace", id: "w1", where: "tabbar" } as const;
  assert.deepEqual(placeRename(tab, "top"), tab);
  assert.deepEqual(placeRename(tab, "sidebar"), { ...tab, where: "sidebar" }, "no tab bar in the sidebar layout");
  assert.deepEqual(placeRename(workspace, "top"), workspace);
  assert.deepEqual(placeRename(workspace, "sidebar"), { ...workspace, where: "sidebar" });
  assert.deepEqual(placeRename({ ...workspace, where: "sidebar" }, "top"), workspace, "no sidebar in the top layout");
  const session = { kind: "session", id: "s1", where: "sidebar" } as const;
  assert.deepEqual(placeRename(session, "top"), { ...session, where: "pane" });
  assert.deepEqual(placeRename(session, "sidebar"), session, "the sidebar layout has both");
  assert.deepEqual(placeRename({ ...session, where: "pane" }, "sidebar"), { ...session, where: "pane" });
});

test("the header shows the update button only when there is something to show", () => {
  const status = (newer: boolean) => ({ newer, latest: newer ? "9.9.9" : null }) as never;
  assert.equal(updateShown(null, false), false);
  assert.equal(updateShown(status(false), false), false);
  assert.equal(updateShown(status(true), false), true);
  assert.equal(updateShown(status(false), true), true, "opened from the palette");
});

test("settings are split into pages, without updates in a remote window", () => {
  assert.deepEqual(settingsPages(true), ["general", "appearance", "terminal", "updates"]);
  assert.deepEqual(settingsPages(false), ["general", "appearance", "terminal"]);
});
