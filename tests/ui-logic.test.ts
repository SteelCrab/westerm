import assert from "node:assert/strict";
import { test } from "node:test";
import { fuzzyScore } from "../src/components/CommandPalette.tsx";
import { clampSidebar } from "../src/components/SidebarResizer.tsx";
import { insertionIndex } from "../src/dnd.ts";
import { MESSAGES } from "../src/i18n/messages.ts";
import { urlCommands } from "../src/paletteCommands.tsx";

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
