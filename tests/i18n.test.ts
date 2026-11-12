import assert from "node:assert/strict";
import { test } from "node:test";
import { detectLocale, LOCALES, MESSAGES } from "../src/i18n/messages.ts";

test("the saved language wins; else the first supported browser language; else English", () => {
  assert.equal(detectLocale("ja", ["ko-KR"]), "ja");
  assert.equal(detectLocale("fr", ["ko-KR"]), "ko", "an unknown saved value is ignored");
  assert.equal(detectLocale(null, ["fr-FR", "zh-TW", "ko"]), "zh");
  assert.equal(detectLocale(null, ["JA_jp"]), "ja");
  assert.equal(detectLocale(null, ["fr-FR"]), "en");
  assert.equal(detectLocale(null, []), "en");
});

/** Paths to every leaf with its type, e.g. "pane.renameHint: function". */
function shape(value: unknown, path = ""): string[] {
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, child]) => shape(child, path ? `${path}.${key}` : key));
  }
  return [`${path}: ${typeof value}`];
}

test("every language has every message, with the same kind of value", () => {
  const reference = shape(MESSAGES.en).sort();
  for (const locale of LOCALES) {
    assert.deepEqual(shape(MESSAGES[locale]).sort(), reference, locale);
  }
});

test("parameterized messages include their values in every language", () => {
  for (const locale of LOCALES) {
    const t = MESSAGES[locale];
    assert.ok(t.sidebar.sessionCount(3).includes("3"), locale);
    assert.ok(t.palette.movePaneToTab("빌드").includes("빌드"), locale);
    assert.ok(t.font.sizeLabel(15, true).includes("15"), locale);
    assert.ok(t.tabs.tooltip("vim", 2).includes("vim"), locale);
    assert.ok(t.pane.coreFailed("boom").includes("boom"), locale);
    assert.ok(t.workspaces.untitled(3).includes("3"), locale);
    assert.ok(t.workspaces.close("빌드").includes("빌드"), locale);
    assert.ok(t.workspaces.switcher("빌드").includes("빌드"), locale);
    assert.ok(t.workspaces.rename("빌드").includes("빌드"), locale);
    assert.ok(t.workspaces.changeLook("빌드").includes("빌드"), locale);
    assert.ok(t.palette.moveTabToWorkspace("빌드").includes("빌드"), locale);
    assert.ok(t.remote.connectingTo("pista@msi").includes("pista@msi"), locale);
    assert.ok(t.agents.agentCount(4).includes("4"), locale);
    assert.ok(t.agents.shellCount(4).includes("4"), locale);
    assert.ok(t.agents.elsewhere(4).includes("4"), locale);
    assert.ok(t.agents.elsewhereHint(4).includes("4"), locale);
    assert.ok(t.agents.past(4).includes("4"), locale);
    assert.ok(t.agents.start("Gemini CLI").includes("Gemini CLI"), locale);
    assert.ok(t.agents.resumeLabel("빌드").includes("빌드"), locale);
    assert.ok(t.agents.worktreeCount(4).includes("4"), locale);
    assert.ok(t.agents.idleWorktrees(4).includes("4"), locale);
    assert.ok(t.agents.openPr(54, "빌드").includes("#54") && t.agents.openPr(54, "빌드").includes("빌드"), locale);
    assert.ok(t.agents.prChecks.failing(4).includes("4"), locale);
    assert.ok(t.settings.resumeAgentsHint.includes("Gemini CLI"), locale);
    assert.ok(t.fontPicker.full(8).includes("8"), locale);
    assert.ok(t.fontPicker.addTyped("My Font").includes("My Font"), locale);
    assert.ok(t.fontPicker.count(4, 42).includes("4") && t.fontPicker.count(4, 42).includes("42"), locale);
    assert.ok(t.fontPicker.remove("Menlo").includes("Menlo"), locale);
    assert.ok(t.fontPicker.empty("D2Coding").includes("D2Coding"), locale);
    assert.ok(t.settings.fontFamilyHint("D2Coding").includes("D2Coding"), locale);
  }
});
