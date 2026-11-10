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

// A sidebar list taller than the window (many folders and past conversations)
// must scroll inside the sidebar. With an auto-sized grid row, the list's
// height stretched the whole app below the window, cutting off the lowest
// panes and the activity bar's bottom buttons.
test("a long sidebar list scrolls within the window instead of expanding the grid row", () => {
  assert.match(rule(".app") ?? "", /grid-template-rows:\s*minmax\(0,\s*1fr\)/);
  assert.match(rule(".sidebar-slot") ?? "", /min-height:\s*0/);
});
