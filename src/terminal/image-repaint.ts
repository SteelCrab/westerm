import type { WTerm } from "@wterm/dom";

/**
 * Kitty images in panes that still draw them with wterm: a shell running
 * `kitten icat`, or a pixel app on a host without pixel updates.
 *
 * @wterm/dom 0.5.9 repaints every visible image canvas whenever any image
 * changes (graphics-layer.js: `if (changed || !existingCanvas)`), so one
 * small new image costs a putImageData of every image on screen. A canvas's
 * key already names its image and version, so it only needs painting again
 * when its size or source area changes. This patch skips the other paints
 * and only moves the canvas.
 *
 * It wraps GraphicsLayer internals (TypeScript-private, not #private) and
 * turns itself off if their shape changes.
 */

interface Placement {
  imageId: number;
  imageVersion: number;
  sourceX: number;
  sourceY: number;
  sourceWidth: number;
  sourceHeight: number;
  columns: number;
  rows: number;
  z: number;
}

interface Size {
  width: number;
  height: number;
}

interface Viewport {
  charWidth: number;
  rowHeight: number;
}

interface CanvasLike {
  style: { zIndex: string };
  __westermPainted?: string;
}

interface GraphicsLayerLike {
  paint: (
    canvas: CanvasLike,
    key: string,
    image: unknown,
    placement: Placement,
    viewport: Viewport,
    bounds: Size | null,
    flowOffset?: number,
  ) => boolean;
  position: (canvas: CanvasLike, placement: Placement, viewport: Viewport, flowOffset?: number) => void;
  __imageRepaintCache?: true;
}

function graphicsLayer(wt: WTerm): GraphicsLayerLike | null {
  const layer = (wt as unknown as { renderer?: { graphics?: Partial<GraphicsLayerLike> } }).renderer?.graphics;
  if (!layer || typeof layer.paint !== "function" || typeof layer.position !== "function") return null;
  return layer as GraphicsLayerLike;
}

/** What decides a canvas's pixels besides its image, which its key names. */
function geometry(placement: Placement, viewport: Viewport, bounds: Size | null): string {
  return [
    placement.imageId,
    placement.imageVersion,
    placement.sourceX,
    placement.sourceY,
    placement.sourceWidth,
    placement.sourceHeight,
    placement.columns,
    placement.rows,
    viewport.charWidth,
    viewport.rowHeight,
    bounds?.width,
    bounds?.height,
  ].join(",");
}

/** Returns false when this @wterm/dom build has a different internal shape. */
export function installImageRepaintCache(wt: WTerm): boolean {
  const layer = graphicsLayer(wt);
  if (!layer) return false;
  if (layer.__imageRepaintCache) return true;
  layer.__imageRepaintCache = true;
  const paint = layer.paint;
  layer.paint = function (canvas, key, image, placement, viewport, bounds, flowOffset = 0) {
    const painted = geometry(placement, viewport, bounds);
    if (canvas.__westermPainted === painted) {
      this.position(canvas, placement, viewport, flowOffset);
      canvas.style.zIndex = String(placement.z);
      return true;
    }
    const ok = paint.call(this, canvas, key, image, placement, viewport, bounds, flowOffset);
    canvas.__westermPainted = ok ? painted : undefined;
    return ok;
  };
  return true;
}
