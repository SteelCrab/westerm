import type { WTerm } from "@wterm/dom";

/**
 * Full-screen Kitty clients on the alternate screen.
 *
 * @wterm/dom treats a Kitty placement without a cell size ("implicit") as an
 * inline image in shell output: it pushes the following rows down by the
 * image height so the next prompt stays visible, and pins the image to the
 * left edge. That suits `kitten icat` on the primary screen.
 *
 * terminal-browser instead switches to the alternate screen, homes the cursor,
 * and sends every frame as an implicit placement with C=1 (cursor does not
 * move). There is no following prompt to protect, so pushing rows down only
 * scrolls the frame out of view.
 *
 * On the alternate screen this patch keeps implicit placements at their cell
 * and reserves no row space. The primary screen keeps the library behavior.
 *
 * It reaches into GraphicsLayer internals (TypeScript-private, not #private)
 * of @wterm/dom 0.5.9 and disables itself if their shape changes.
 */

interface Placement {
  rows: number;
  col: number;
  offsetX: number;
}

interface Viewport {
  charWidth: number;
}

interface CoreLike {
  usingAltScreen?: () => boolean;
}

interface GraphicsLayerLike {
  reconcileFlowReservations: (core: CoreLike, ...rest: unknown[]) => Map<number, number>;
  clearFlowReservations: () => void;
  position: (canvas: HTMLCanvasElement, placement: Placement, viewport: Viewport, flowOffset?: number) => void;
  __altScreenImages?: { alt: boolean };
}

function graphicsLayer(wt: WTerm): GraphicsLayerLike | null {
  const layer = (wt as unknown as { renderer?: { graphics?: Partial<GraphicsLayerLike> } }).renderer
    ?.graphics;
  if (
    !layer ||
    typeof layer.reconcileFlowReservations !== "function" ||
    typeof layer.clearFlowReservations !== "function" ||
    typeof layer.position !== "function"
  ) {
    return null;
  }
  return layer as GraphicsLayerLike;
}

/** Returns false when this @wterm/dom build has a different internal shape. */
export function installAltScreenImages(wt: WTerm): boolean {
  const layer = graphicsLayer(wt);
  if (!layer) return false;
  if (layer.__altScreenImages) return true;

  const state = { alt: false };
  layer.__altScreenImages = state;
  const reconcileFlow = layer.reconcileFlowReservations;
  const position = layer.position;

  layer.reconcileFlowReservations = function (core, ...rest) {
    // reconcile() calls this before positioning canvases, so `alt` is current
    // for every position() call in the same pass.
    state.alt = core.usingAltScreen?.() === true;
    if (!state.alt) return reconcileFlow.call(this, core, ...rest);
    this.clearFlowReservations();
    return new Map();
  };

  layer.position = function (canvas, placement, viewport, flowOffset = 0) {
    position.call(this, canvas, placement, viewport, flowOffset);
    if (state.alt && placement.rows === 0) {
      const left = placement.col * viewport.charWidth + placement.offsetX;
      if (Number.isFinite(left)) canvas.style.left = `${left}px`;
    }
  };
  return true;
}
