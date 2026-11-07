import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { Window } from "happy-dom";

// A DOM for this file only: PixelSurface and wterm's renderer build elements
// through the globals.
const window = new Window();
const globals = ["document", "MutationObserver", "HTMLElement", "HTMLCanvasElement", "Node", "getComputedStyle"] as const;
const saved = new Map<string, unknown>();

before(() => {
  for (const name of globals) {
    saved.set(name, (globalThis as Record<string, unknown>)[name]);
    (globalThis as Record<string, unknown>)[name] = (window as unknown as Record<string, unknown>)[name];
  }
});

after(async () => {
  for (const name of globals) (globalThis as Record<string, unknown>)[name] = saved.get(name);
  await window.happyDOM.close();
});

// Assertions compare booleans: a failing assert.equal on DOM elements makes
// the test runner serialize the whole DOM, and the run hangs.

/** MutationObserver callbacks run after the current task. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A pane's grid, as wterm's renderer owns it, with a pixel surface on it. */
async function pane() {
  const { Renderer } = await import("@wterm/dom");
  const { PixelSurface } = await import("../src/terminal/pixel-surface.ts");
  const grid = window.document.createElement("div") as unknown as HTMLElement;
  grid.className = "term-grid";
  window.document.body.appendChild(grid as never);
  const renderer = new Renderer(grid);
  renderer.setup(80, 24);
  const surface = new PixelSurface(grid, () => {});
  const canvas = () => grid.querySelector("canvas.pixel-surface");
  return { grid, renderer, surface, canvas };
}

test("the pixel canvas comes back when wterm sets the grid up again after a resize", async () => {
  const { renderer, surface, canvas } = await pane();
  const before = canvas();
  assert.ok(before !== null, "the surface draws on the grid");

  // New rows and columns: wterm empties the grid and puts back its own layers.
  renderer.setup(100, 30);
  await settle();

  assert.ok(canvas() === before, "the same canvas is on the grid again, still showing the last picture");
  surface.dispose();
});

test("a disposed surface stays gone when wterm sets the grid up again", async () => {
  const { renderer, surface, canvas } = await pane();
  surface.dispose();
  renderer.setup(100, 30);
  await settle();
  assert.ok(canvas() === null, "no canvas comes back");
});
