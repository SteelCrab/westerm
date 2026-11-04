import { useRef } from "react";
import { useI18n } from "../i18n/index.tsx";

export const SIDEBAR_MIN = 160;
export const SIDEBAR_MAX = 480;
export const SIDEBAR_DEFAULT = 240;
const STEP = 16;

export function clampSidebar(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT;
  return Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width)));
}

interface SidebarResizerProps {
  width: number;
  onResize: (width: number) => void;
}

/**
 * Drag handle on the sidebar's right edge. Arrow keys move it by 16 px,
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
      aria-valuemin={SIDEBAR_MIN}
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
      onDoubleClick={() => onResize(SIDEBAR_DEFAULT)}
      onKeyDown={(event) => {
        const keys: Record<string, number> = {
          ArrowLeft: width - STEP,
          ArrowRight: width + STEP,
          Home: SIDEBAR_MIN,
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
