// Draws pixel updates on an OffscreenCanvas, off the page's main thread.
// Messages in: {type: "init", canvas}, then {type: "update", buffer} with the
// buffer transferred. Out: {type: "painted"} once per update, in order.
import { paintPixelUpdate, parsePixelUpdate } from "./pixel-update.ts";

interface WorkerScope {
  onmessage: ((event: MessageEvent) => void) | null;
  postMessage(message: unknown): void;
}

const scope = self as unknown as WorkerScope;
let canvas: OffscreenCanvas | null = null;
let context: OffscreenCanvasRenderingContext2D | null = null;
// Inflating is asynchronous; the chain keeps updates in order.
let queue: Promise<void> = Promise.resolve();

scope.onmessage = (event) => {
  const message = event.data as { type: "init"; canvas: OffscreenCanvas } | { type: "update"; buffer: ArrayBuffer };
  if (message.type === "init") {
    canvas = message.canvas;
    context = canvas.getContext("2d");
    return;
  }
  const buffer = message.buffer;
  queue = queue
    .then(async () => {
      const update = parsePixelUpdate(buffer);
      if (update && canvas && context) {
        const target = { canvas, context, image: (data: Uint8ClampedArray<ArrayBuffer>, w: number, h: number) => new ImageData(data, w, h) };
        await paintPixelUpdate(target, update);
      }
    })
    .catch((error: unknown) => console.warn("[westerm] pixel update failed", error))
    // Always answer, or the host stops sending.
    .then(() => scope.postMessage({ type: "painted" }));
};
