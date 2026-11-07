import { KIND_PIXELS, KIND_TEXT, PIXEL_FLAG_DEFLATE, type PixelCodec, type SessionKind } from "../../shared/protocol.ts";

/**
 * Pixel updates from the PTY host (native/src/pixels/mod.rs): the frame of a
 * pixel app, sent as the rectangles that changed. Little endian:
 *
 *   kind u8 = 1, flags u8, frame width u16, frame height u16, count u16,
 *   then per rectangle: x, y, w, h u16, byte length u32, data
 *
 * Data is RGBA, w * h * 4 bytes, or raw deflate of it with PIXEL_FLAG_DEFLATE.
 * Only DOM-free code lives here: the worker and the tests use it too.
 */

export interface PixelRect {
  x: number;
  y: number;
  w: number;
  h: number;
  data: Uint8Array<ArrayBuffer>;
}

export interface PixelUpdate {
  width: number;
  height: number;
  deflate: boolean;
  rects: PixelRect[];
}

/** localStorage key; "off" makes pixel app panes take Kitty images again. */
export const PIXELS_SETTING = "westerm.pixels";

/**
 * What a pane asks the host for. Pixel apps only: a remote window gets
 * deflated rectangles, since raw ones are large for a network.
 */
export function pixelCodecFor(kind: SessionKind, remote: boolean, setting: string | null): PixelCodec | null {
  if (setting === "off" || (kind !== "browser" && kind !== "editor")) return null;
  return remote ? "deflate" : "raw";
}

export type BinaryPart ={ kind: "text"; bytes: Uint8Array } | { kind: "pixels"; buffer: ArrayBuffer };

/** One binary message. `framed`: the host said pixels in "ready". */
export function splitBinary(data: ArrayBuffer, framed: boolean): BinaryPart {
  const bytes = new Uint8Array(data);
  if (!framed) return { kind: "text", bytes };
  if (bytes[0] === KIND_PIXELS) return { kind: "pixels", buffer: data };
  return { kind: "text", bytes: bytes[0] === KIND_TEXT ? bytes.subarray(1) : bytes };
}

/** Null for anything that is not a whole, in-bounds update. */
export function parsePixelUpdate(buffer: ArrayBuffer): PixelUpdate | null {
  const view = new DataView(buffer);
  if (buffer.byteLength < 8 || view.getUint8(0) !== KIND_PIXELS) return null;
  const deflate = (view.getUint8(1) & PIXEL_FLAG_DEFLATE) !== 0;
  const width = view.getUint16(2, true);
  const height = view.getUint16(4, true);
  const count = view.getUint16(6, true);
  const rects: PixelRect[] = [];
  let at = 8;
  for (let i = 0; i < count; i++) {
    if (at + 12 > buffer.byteLength) return null;
    const x = view.getUint16(at, true);
    const y = view.getUint16(at + 2, true);
    const w = view.getUint16(at + 4, true);
    const h = view.getUint16(at + 6, true);
    const length = view.getUint32(at + 8, true);
    at += 12;
    if (at + length > buffer.byteLength) return null;
    if (w === 0 || h === 0 || x + w > width || y + h > height) return null;
    if (!deflate && length !== w * h * 4) return null;
    rects.push({ x, y, w, h, data: new Uint8Array(buffer, at, length) });
    at += length;
  }
  return at === buffer.byteLength ? { width, height, deflate, rects } : null;
}

/** RGBA bytes of a rectangle, inflated if the update is deflated. */
export async function rectPixels(rect: PixelRect, deflate: boolean): Promise<Uint8Array<ArrayBuffer>> {
  if (!deflate) return rect.data;
  const stream = new Blob([rect.data as BlobPart]).stream().pipeThrough(new DecompressionStream("deflate-raw"));
  const pixels = new Uint8Array(await new Response(stream).arrayBuffer());
  if (pixels.length !== rect.w * rect.h * 4) throw new Error("pixel update: inflated size does not match");
  return pixels;
}

interface ImageLike {
  width: number;
  height: number;
}

/** A 2D canvas, or just enough of one for the tests. */
export interface PaintTarget<I extends ImageLike> {
  canvas: { width: number; height: number };
  context: { putImageData(image: I, x: number, y: number): void };
  /** new ImageData(data, width, height) in a browser. */
  image(data: Uint8ClampedArray<ArrayBuffer>, width: number, height: number): I;
}

/**
 * Puts every rectangle. A new frame size resizes the canvas, which clears it;
 * the host sends the whole frame after a resize.
 */
export async function paintPixelUpdate<I extends ImageLike>(target: PaintTarget<I>, update: PixelUpdate): Promise<void> {
  // Inflate everything first, so the canvas changes in one go.
  const pixels = await Promise.all(update.rects.map((rect) => rectPixels(rect, update.deflate)));
  if (target.canvas.width !== update.width) target.canvas.width = update.width;
  if (target.canvas.height !== update.height) target.canvas.height = update.height;
  update.rects.forEach((rect, i) => {
    const data = new Uint8ClampedArray(pixels[i].buffer, pixels[i].byteOffset, pixels[i].byteLength);
    target.context.putImageData(target.image(data, rect.w, rect.h), rect.x, rect.y);
  });
}
