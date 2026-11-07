import { paintPixelUpdate, parsePixelUpdate } from "./pixel-update.ts";

/**
 * The picture of a pixel app (terminal-browser, terminal-code), drawn from
 * the host's pixel updates on a canvas over the terminal grid. One CSS pixel
 * per frame pixel, as wterm drew the app's Kitty images.
 *
 * A Worker draws on an OffscreenCanvas, so the page's main thread only hands
 * buffers over. Without OffscreenCanvas it draws here. `onPainted` runs once
 * per update; while the pane is hidden it waits, so the host stops sending
 * and keeps only the newest changes.
 */
export class PixelSurface {
  private readonly canvas: HTMLCanvasElement;
  private readonly observer: MutationObserver;
  private readonly worker: Worker | null;
  private readonly context: CanvasRenderingContext2D | null = null;
  private queue: Promise<void> = Promise.resolve();
  private visible = true;
  private held = 0;
  private width = 0;
  private height = 0;

  constructor(
    container: HTMLElement,
    private readonly onPainted: () => void,
  ) {
    this.canvas = document.createElement("canvas");
    this.canvas.className = "pixel-surface";
    this.canvas.setAttribute("aria-hidden", "true");
    container.appendChild(this.canvas);
    // wterm empties the grid whenever its rows or columns change (its
    // renderer's setup) and puts back only its own layers. Without this the
    // picture is gone after a pane or window resize.
    this.observer = new MutationObserver(() => {
      if (this.canvas.parentNode !== container) container.appendChild(this.canvas);
    });
    this.observer.observe(container, { childList: true });
    this.worker = this.startWorker();
    if (!this.worker) this.context = this.canvas.getContext("2d");
  }

  /** One update from the host; the buffer is handed to the worker. */
  push(buffer: ArrayBuffer): void {
    if (buffer.byteLength >= 6) {
      const view = new DataView(buffer);
      this.fit(view.getUint16(2, true), view.getUint16(4, true));
    }
    if (this.worker) {
      this.worker.postMessage({ type: "update", buffer }, [buffer]);
      return;
    }
    const context = this.context;
    this.queue = this.queue
      .then(async () => {
        const update = parsePixelUpdate(buffer);
        if (update && context) {
          const target = { canvas: this.canvas, context, image: (data: Uint8ClampedArray<ArrayBuffer>, w: number, h: number) => new ImageData(data, w, h) };
          await paintPixelUpdate(target, update);
        }
      })
      .catch((error: unknown) => console.warn("[westerm] pixel update failed", error))
      .then(() => this.painted());
  }

  setVisible(visible: boolean): void {
    this.visible = visible;
    if (!visible) return;
    for (; this.held > 0; this.held--) this.onPainted();
  }

  dispose(): void {
    this.observer.disconnect();
    this.worker?.terminate();
    this.canvas.remove();
  }

  private startWorker(): Worker | null {
    if (typeof Worker === "undefined" || typeof this.canvas.transferControlToOffscreen !== "function") return null;
    let worker: Worker | null = null;
    try {
      worker = new Worker(new URL("./pixel-worker.ts", import.meta.url), { type: "module" });
      const offscreen = this.canvas.transferControlToOffscreen();
      worker.postMessage({ type: "init", canvas: offscreen }, [offscreen]);
      worker.onmessage = () => this.painted();
      return worker;
    } catch (error) {
      worker?.terminate();
      console.warn("[westerm] pixel worker unavailable; drawing on the main thread", error);
      return null;
    }
  }

  private painted(): void {
    if (this.visible) this.onPainted();
    else this.held++;
  }

  /** CSS size of the frame. The canvas's own size follows each update. */
  private fit(width: number, height: number): void {
    if (width === this.width && height === this.height) return;
    this.width = width;
    this.height = height;
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
  }
}
