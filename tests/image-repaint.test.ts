import assert from "node:assert/strict";
import { test } from "node:test";
import type { WTerm } from "@wterm/dom";
import { installImageRepaintCache } from "../src/terminal/image-repaint.ts";

/** Just the part of @wterm/dom 0.5.9's GraphicsLayer that the patch wraps. */
function fakeTerminal() {
  const calls = { paint: 0, position: 0 };
  const layer = {
    paint(canvas: { style: { zIndex: string } }, _key: string, _image: unknown, placement: { z: number } & Record<string, number>, ..._rest: unknown[]) {
      calls.paint++;
      canvas.style.zIndex = String(placement.z);
      return true;
    },
    position() {
      calls.position++;
    },
  };
  const wt = { renderer: { graphics: layer } } as unknown as WTerm;
  return { wt, layer, calls };
}

const placement = {
  imageId: 4,
  imageVersion: 1,
  sourceX: 0,
  sourceY: 0,
  sourceWidth: 0,
  sourceHeight: 0,
  columns: 0,
  rows: 0,
  z: 3,
};
const viewport = { charWidth: 9, rowHeight: 18 };
const bounds = { width: 1350, height: 780 };

test("an image canvas that already shows its picture is not painted again", () => {
  const { wt, layer, calls } = fakeTerminal();
  assert.equal(installImageRepaintCache(wt), true);
  const canvas = { style: { zIndex: "" } };
  assert.equal(layer.paint(canvas, "k", {}, placement, viewport, bounds, 0), true);
  assert.equal(layer.paint(canvas, "k", {}, { ...placement, z: 5 }, viewport, bounds, 0), true);
  assert.equal(calls.paint, 1, "the second call only moves it");
  assert.equal(calls.position, 1);
  assert.equal(canvas.style.zIndex, "5", "a new z still applies");
});

test("a canvas is painted again when its size or source changes", () => {
  const { wt, layer, calls } = fakeTerminal();
  installImageRepaintCache(wt);
  const canvas = { style: { zIndex: "" } };
  layer.paint(canvas, "k", {}, placement, viewport, bounds, 0);
  layer.paint(canvas, "k", {}, placement, { ...viewport, charWidth: 10 }, bounds, 0);
  layer.paint(canvas, "k", {}, { ...placement, sourceWidth: 2 }, { ...viewport, charWidth: 10 }, bounds, 0);
  layer.paint(canvas, "k", {}, { ...placement, sourceWidth: 2 }, { ...viewport, charWidth: 10 }, { width: 100, height: 780 }, 0);
  assert.equal(calls.paint, 4);
});

test("the patch turns itself off for a GraphicsLayer of another shape", () => {
  assert.equal(installImageRepaintCache({ renderer: { graphics: {} } } as unknown as WTerm), false);
  assert.equal(installImageRepaintCache({} as unknown as WTerm), false);
});
