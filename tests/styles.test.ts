// Panes of other tabs and panes behind a maximized pane stay mounted (their
// terminals keep running) and must be hidden by CSS, or they paint over the
// visible tab. This rule was once lost in an edit; keep it guarded.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");

function rule(selector: string): string | null {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|\\n)${escaped}\\s*\\{([^}]*)\\}`).exec(css)?.[2] ?? null;
}

test("covered panes are hidden", () => {
  const body = rule(".pane.covered");
  assert.ok(body, ".pane.covered rule exists");
  assert.match(body, /visibility:\s*hidden/);
});

test("a docker row's hover buttons sit left of its port chips, not over them", () => {
  // The buttons hang off the left edge of the box that holds the chips, so the
  // chips stay where they are and stay clickable while the row is hovered.
  const tail = rule(".docker-tail");
  assert.ok(tail, ".docker-tail rule exists");
  assert.match(tail, /position:\s*relative/);
  const actions = rule(".docker-row .docker-actions");
  assert.ok(actions, ".docker-row .docker-actions rule exists");
  assert.match(actions, /right:\s*100%/);
});

const zIndex = (selector: string) => {
  const body = rule(selector);
  assert.ok(body, `${selector} rule exists`);
  const match = /z-index:\s*(\d+)/.exec(body);
  assert.ok(match, `${selector} has a z-index`);
  return Number(match[1]);
};

test("plugin frames sit above the narrow sidebar and below the dialogs; popovers sit above them", () => {
  // The frames live in one layer under <body>, placed over the panel's slot.
  assert.equal(zIndex(".plugin-frames"), 61);
  assert.ok(zIndex(".plugin-frames") > 60, "above the narrow sidebar drawer (.sidebar-slot, 60)");
  for (const dialog of [".remote-connect", ".palette-backdrop", ".settings-backdrop", ".window-away"]) {
    assert.ok(zIndex(dialog) > 61, `${dialog} covers the plugin frames`);
  }
  // Anything that can open over the plugin panel must not slip under its frame.
  for (const popover of [".activity-tooltip", ".ws-panel", ".update-panel", ".docker-menu"]) {
    assert.ok(zIndex(popover) > 61, `${popover} sits above the plugin frames`);
  }
  const frame = rule(".plugin-frame");
  assert.ok(frame, ".plugin-frame rule exists");
  assert.match(frame, /position:\s*fixed/);
});

test("long Git content scrolls within the viewport instead of expanding the grid row", () => {
  assert.match(rule(".app") ?? "", /grid-template-rows:\s*minmax\(0,\s*1fr\)/);
  assert.match(rule(".sidebar-slot") ?? "", /min-height:\s*0/);
  assert.match(rule(".git-panel") ?? "", /min-height:\s*0/);
  assert.match(rule(".git-scroll") ?? "", /min-height:\s*0/);
  assert.match(rule(".git-scroll") ?? "", /overflow-y:\s*auto/);
  assert.match(rule(".git-preview pre") ?? "", /max-height:/);
});

// A sidebar list taller than the window (many folders and past conversations)
// must scroll inside the sidebar. With an auto-sized grid row, the list's
// height stretched the whole app below the window, cutting off the lowest
// panes and the activity bar's bottom buttons.
test("a long sidebar list scrolls within the window instead of expanding the grid row", () => {
  assert.match(rule(".app") ?? "", /grid-template-rows:\s*minmax\(0,\s*1fr\)/);
  assert.match(rule(".sidebar-slot") ?? "", /min-height:\s*0/);
});

// The focused pane needs a ring that is easy to see. Draw it with box-shadow
// outside the 1px border: a thicker border would shrink the terminal grid.
test("the focused pane has a thick accent ring outside its 1px border", () => {
  const body = rule(".pane.focused") ?? "";
  assert.match(body, /border-color:\s*var\(--accent\)/);
  assert.match(body, /box-shadow:\s*0 0 0 1px var\(--accent\),\s*0 0 0 4px var\(--accent-halo\)/);
  assert.doesNotMatch(body, /border(-width)?:/);
  // Dark, light (from the OS) and light (forced) themes each define it.
  assert.equal(css.match(/--accent-halo:/g)?.length, 3);
});

// A folder row has two lines. The pull request is on the first, beside the
// name; the second line is the row's own, so a long folder or branch gets the
// whole width of a narrow sidebar.
test("a folder row's second line takes the row's whole width", () => {
  assert.match(rule(".agents-folder") ?? "", /flex-wrap:\s*wrap/);
  assert.match(rule(".agents-folder-meta") ?? "", /flex-basis:\s*100%/);
});

// A folder row shows start buttons over its right end on hover. A pull
// request chip sits there too, on the first line: the buttons go to the
// second line, or the chip could not be pressed.
test("a folder's hover start buttons do not cover its pull request", () => {
  const body = rule(".agents-folder.with-pr .agents-start.on-hover") ?? "";
  assert.match(body, /top:\s*auto/);
  assert.match(body, /bottom:/);
  assert.match(body, /transform:\s*none/);
});

// In the list of worktrees without a shell, the start buttons open before the
// chip on hover. They keep their place in the tab order while closed.
test("an idle worktree's start buttons open on hover and stay reachable by keyboard", () => {
  const closed = rule(".agents-idle-row .agents-start") ?? "";
  assert.match(closed, /width:\s*0/);
  assert.doesNotMatch(closed, /display:\s*none|visibility:\s*hidden/);
  assert.match(rule(".agents-idle-row:hover .agents-start,\n.agents-idle-row:focus-within .agents-start") ?? "", /width:\s*auto/);
});
