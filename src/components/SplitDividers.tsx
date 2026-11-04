import { useRef } from "react";
import { handles, moveShare, nodeAt, type Handle, type LayoutNode } from "../../shared/layout.ts";
import { onDragEnd } from "../dragEnd.ts";
import { useI18n } from "../i18n/index.tsx";

interface SplitDividersProps {
  layout: LayoutNode;
  /** Layer content size in CSS pixels (without its padding). */
  inner: () => { width: number; height: number };
  onResize: (path: number[], sizes: number[]) => void;
}

const KEY_STEP = 0.02;

function sizesOf(layout: LayoutNode, handle: Handle): number[] {
  const node = nodeAt(layout, handle.path);
  return node?.type === "split" ? node.sizes : [];
}

/**
 * One handle per boundary in the split tree. Dragging moves that boundary
 * only; the two neighbors resize and their terminals refit. Arrow keys nudge;
 * double-click or Enter makes that split's children equal.
 */
export function SplitDividers({ layout, inner, onResize }: SplitDividersProps) {
  const { t } = useI18n();
  const drag = useRef<{ start: number; sizes: number[]; frame: number } | null>(null);

  return (
    <>
      {handles(layout).map((handle) => {
        const vertical = handle.dir === "row";
        const style = {
          "--at": handle.at,
          "--x": handle.rect.x,
          "--y": handle.rect.y,
          "--w": handle.rect.w,
          "--h": handle.rect.h,
        } as React.CSSProperties;
        const apply = (delta: number, base: number[]) => onResize(handle.path, moveShare(base, handle.index, delta));
        const equalize = () => {
          const count = sizesOf(layout, handle).length;
          onResize(handle.path, Array.from({ length: count }, () => 1 / count));
        };
        const end = () => {
          drag.current = null;
          document.body.classList.remove("resizing-columns", "resizing-rows");
        };
        return (
          <div
            key={`${handle.path.join(".")}:${handle.index}`}
            className={`split-divider ${vertical ? "vertical" : "horizontal"}`}
            style={style}
            role="separator"
            aria-orientation={vertical ? "vertical" : "horizontal"}
            aria-label={vertical ? t.pane.splitVertical : t.pane.splitHorizontal}
            aria-valuenow={Math.round(handle.at * 100)}
            tabIndex={0}
            title={t.pane.splitHint}
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              event.preventDefault();
              event.currentTarget.setPointerCapture(event.pointerId);
              drag.current = { start: vertical ? event.clientX : event.clientY, sizes: sizesOf(layout, handle), frame: 0 };
              document.body.classList.add(vertical ? "resizing-columns" : "resizing-rows");
              onDragEnd(end);
            }}
            onPointerMove={(event) => {
              const state = drag.current;
              if (!state) return;
              const space = inner();
              // Pixels along the axis that this split's shares divide.
              const length = vertical ? space.width * handle.rect.w : space.height * handle.rect.h;
              if (length <= 0) return;
              const delta = ((vertical ? event.clientX : event.clientY) - state.start) / length;
              // One layout per frame: each resized pane refits its terminal and PTY.
              cancelAnimationFrame(state.frame);
              state.frame = requestAnimationFrame(() => apply(delta, state.sizes));
            }}
            onPointerUp={(event) => {
              if (!drag.current) return;
              event.currentTarget.releasePointerCapture(event.pointerId);
              end();
            }}
            onPointerCancel={end}
            onLostPointerCapture={end}
            onDoubleClick={equalize}
            onKeyDown={(event) => {
              const back = vertical ? "ArrowLeft" : "ArrowUp";
              const forward = vertical ? "ArrowRight" : "ArrowDown";
              if (event.key === back || event.key === forward) {
                event.preventDefault();
                apply(event.key === back ? -KEY_STEP : KEY_STEP, sizesOf(layout, handle));
              } else if (event.key === "Enter") {
                event.preventDefault();
                equalize();
              }
            }}
          />
        );
      })}
    </>
  );
}
