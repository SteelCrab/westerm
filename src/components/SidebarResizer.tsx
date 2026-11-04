import { useRef } from "react";
import { onDragEnd } from "../dragEnd.ts";
import { useI18n } from "../i18n/index.tsx";

export const SIDEBAR_MIN = 160;
export const SIDEBAR_MAX = 480;
export const SIDEBAR_DEFAULT = 240;
/** Icons only: tab numbers and session icons. */
export const SIDEBAR_COMPACT = 56;
/** Dragging the edge left of this snaps the sidebar to icons only. */
const COMPACT_BELOW = 120;
const STEP = 16;

export function clampSidebar(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT;
  if (width < COMPACT_BELOW) return SIDEBAR_COMPACT;
  return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width)));
}

export function isCompact(width: number): boolean {
  return width <= SIDEBAR_COMPACT;
}

interface SidebarResizerProps {
  width: number;
  onResize: (width: number) => void;
}

/**
 * Drag handle on the sidebar's right edge. Narrower than 120 px it snaps to
 * icons only. Arrow keys move it by 16 px (and in and out of icons only),
 * Home/End jump to the limits, and double-click or Enter resets it.
 */
export function SidebarResizer({ width, onResize }: SidebarResizerProps) {
  const { t } = useI18n();
  const drag = useRef<{ startX: number; startWidth: number; frame: number } | null>(null);

  return (
    <div
      className="sidebar-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label={t.sidebar.resize}
      aria-valuemin={SIDEBAR_COMPACT}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={width}
      tabIndex={0}
      title={t.sidebar.resizeHint}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { startX: event.clientX, startWidth: width, frame: 0 };
        document.body.classList.add("resizing-sidebar");
        onDragEnd(() => {
          drag.current = null;
          document.body.classList.remove("resizing-sidebar");
        });
      }}
      onPointerMove={(event) => {
        const state = drag.current;
        if (!state) return;
        const next = clampSidebar(state.startWidth + event.clientX - state.startX);
        // One layout per frame: every pane resizes its terminal and PTY.
        cancelAnimationFrame(state.frame);
        state.frame = requestAnimationFrame(() => onResize(next));
      }}
      onPointerUp={(event) => {
        if (!drag.current) return;
        event.currentTarget.releasePointerCapture(event.pointerId);
        drag.current = null;
        document.body.classList.remove("resizing-sidebar");
      }}
      onPointerCancel={() => {
        drag.current = null;
        document.body.classList.remove("resizing-sidebar");
      }}
      // Also when the pointer is lost without an up event, e.g. released
      // outside the window: never leave the page in resizing mode.
      onLostPointerCapture={() => {
        drag.current = null;
        document.body.classList.remove("resizing-sidebar");
      }}
      onDoubleClick={() => onResize(SIDEBAR_DEFAULT)}
      onKeyDown={(event) => {
        const keys: Record<string, number> = {
          ArrowLeft: width <= SIDEBAR_MIN ? SIDEBAR_COMPACT : width - STEP,
          ArrowRight: isCompact(width) ? SIDEBAR_MIN : width + STEP,
          Home: SIDEBAR_COMPACT,
          End: SIDEBAR_MAX,
          Enter: SIDEBAR_DEFAULT,
        };
        if (!(event.key in keys)) return;
        event.preventDefault();
        onResize(clampSidebar(keys[event.key]));
      }}
    />
  );
}
