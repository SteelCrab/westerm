import assert from "node:assert/strict";
import { test } from "node:test";
import { deflateRawSync } from "node:zlib";
import { KIND_PIXELS, KIND_TEXT, PIXEL_FLAG_DEFLATE } from "../shared/protocol.ts";
import { paintPixelUpdate, parsePixelUpdate, pixelCodecFor, rectPixels, splitBinary } from "../src/terminal/pixel-update.ts";

// The host side of this format lives in native/src/pixels/mod.rs (cargo test).

/** A pixel update as the host encodes it. */
function update(width: number, height: number, flags: number, rects: [number, number, number, number, Uint8Array][]) {
  const size = 8 + rects.reduce((sum, rect) => sum + 12 + rect[4].length, 0);
  const bytes = new Uint8Array(size);
  const view = new DataView(bytes.buffer);
  bytes[0] = KIND_PIXELS;
  bytes[1] = flags;
  view.setUint16(2, width, true);
  view.setUint16(4, height, true);
  view.setUint16(6, rects.length, true);
  let at = 8;
  for (const [x, y, w, h, data] of rects) {
    view.setUint16(at, x, true);
    view.setUint16(at + 2, y, true);
    view.setUint16(at + 4, w, true);
    view.setUint16(at + 6, h, true);
    view.setUint32(at + 8, data.length, true);
    bytes.set(data, at + 12);
    at += 12 + data.length;
  }
  return bytes.buffer;
}

const red = (pixels: number) => new Uint8Array(Array.from({ length: pixels }, () => [255, 0, 0, 255]).flat());

test("pixel app panes ask for pixel updates, deflated in a remote window", () => {
  assert.equal(pixelCodecFor("browser", false, null), "raw");
  assert.equal(pixelCodecFor("editor", true, null), "deflate");
  assert.equal(pixelCodecFor("shell", false, null), null, "shells draw text");
  assert.equal(pixelCodecFor("ssh", true, null), null);
  // localStorage "westerm.pixels" = "off" brings back the Kitty images.
  assert.equal(pixelCodecFor("browser", false, "off"), null);
});

test("a pixel update is read with its frame size and rectangles", () => {
  const parsed = parsePixelUpdate(update(40, 20, 0, [[3, 4, 2, 1, red(2)]]));
  assert.ok(parsed);
  assert.equal(parsed.width, 40);
  assert.equal(parsed.height, 20);
  assert.equal(parsed.deflate, false);
  assert.equal(parsed.rects.length, 1);
  assert.deepEqual({ ...parsed.rects[0], data: [...parsed.rects[0].data] }, { x: 3, y: 4, w: 2, h: 1, data: [...red(2)] });
});

test("a broken pixel update is refused", () => {
  const good = new Uint8Array(update(40, 20, 0, [[0, 0, 2, 1, red(2)]]));
  assert.equal(parsePixelUpdate(good.slice(0, good.length - 1).buffer), null, "cut short");
  assert.equal(parsePixelUpdate(update(40, 20, 0, [[39, 0, 2, 1, red(2)]])), null, "past the frame edge");
  assert.equal(parsePixelUpdate(update(40, 20, 0, [[0, 0, 2, 2, red(2)]])), null, "raw data of the wrong size");
  assert.equal(parsePixelUpdate(new Uint8Array([KIND_TEXT, 0, 0, 0]).buffer), null, "not a pixel update");
});

test("binary messages split into text and pixel updates once the host frames them", () => {
  const text = new Uint8Array([KIND_TEXT, 104, 105]).buffer;
  const framed = splitBinary(text, true);
  assert.equal(framed.kind, "text");
  assert.deepEqual([...(framed as { bytes: Uint8Array }).bytes], [104, 105]);
  const pixels = update(1, 1, 0, []);
  assert.equal(splitBinary(pixels, true).kind, "pixels");
  // Before "ready" says pixels, every binary message is plain output.
  const plain = splitBinary(text, false);
  assert.equal(plain.kind, "text");
  assert.deepEqual([...(plain as { bytes: Uint8Array }).bytes], [KIND_TEXT, 104, 105]);
});

test("deflated rectangles inflate to their pixels", async () => {
  const pixels = red(400);
  const parsed = parsePixelUpdate(update(20, 20, PIXEL_FLAG_DEFLATE, [[0, 0, 20, 20, deflateRawSync(pixels)]]));
  assert.ok(parsed);
  assert.equal(parsed.deflate, true);
  assert.deepEqual([...(await rectPixels(parsed.rects[0], parsed.deflate))], [...pixels]);
});

test("painting resizes the canvas for a new frame size and puts each rectangle", async () => {
  const puts: [number, number, number, number][] = [];
  const canvas = { width: 0, height: 0 };
  const target = {
    canvas,
    context: { putImageData: (image: { width: number; height: number }, x: number, y: number) => puts.push([x, y, image.width, image.height]) },
    image: (data: Uint8ClampedArray, width: number, height: number) => ({ data, width, height }),
  };
  await paintPixelUpdate(target, parsePixelUpdate(update(40, 20, 0, [[1, 2, 2, 1, red(2)], [5, 6, 1, 1, red(1)]]))!);
  assert.deepEqual([canvas.width, canvas.height], [40, 20]);
  assert.deepEqual(puts, [[1, 2, 2, 1], [5, 6, 1, 1]]);
});
