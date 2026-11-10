import { Fragment, useEffect, useLayoutEffect, useRef } from "react";
import type { LucideIcon } from "lucide-react";
import { stepIndex } from "../../uiHelpers.ts";

export interface MenuItem {
  label: string;
  icon?: LucideIcon;
  run: () => void;
  disabled?: boolean;
  /** Red, for an item that deletes. */
  danger?: boolean;
  /** A line above the item. */
  separated?: boolean;
}

interface PopupMenuProps {
  label: string;
  /** Where it opens, in viewport pixels; it moves left or up to stay on screen. */
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/**
 * A menu at a point: focus goes to its first item, the arrow keys move,
 * Escape closes it and gives focus back, and a click outside closes it.
 */
export function PopupMenu({ label, x, y, items, onClose }: PopupMenuProps) {
  const root = useRef<HTMLDivElement>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  // The element that opened the menu; Escape gives focus back to it.
  const opener = useRef<HTMLElement | null>(null);

  useLayoutEffect(() => {
    const menu = root.current;
    if (!menu) return;
    const rect = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 4))}px`;
  }, [x, y]);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    root.current?.querySelector<HTMLElement>('[role="menuitem"]:not([disabled])')?.focus();
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) closeRef.current();
    };
    window.addEventListener("pointerdown", onPointer, true);
    return () => window.removeEventListener("pointerdown", onPointer, true);
  }, []);

  const onKeyDown = (event: React.KeyboardEvent) => {
    // Keep keys away from the terminal and the app shortcuts.
    event.stopPropagation();
    if (event.key === "Escape" || event.key === "Tab") {
      event.preventDefault();
      onClose();
      opener.current?.focus();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    const enabled = Array.from(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([disabled])') ?? []);
    const index = enabled.indexOf(document.activeElement as HTMLElement);
    enabled[stepIndex(index, event.key === "ArrowDown" ? 1 : -1, enabled.length)]?.focus();
  };

  return (
    <div className="docker-menu" role="menu" aria-label={label} ref={root} style={{ left: x, top: y }} onKeyDown={onKeyDown}>
      {items.map(({ label: text, icon: Icon, run, disabled, danger, separated }) => (
        <Fragment key={text}>
          {separated && <div className="docker-menu-separator" role="separator" />}
          <button
            type="button"
            role="menuitem"
            className={`docker-menu-item${danger ? " danger" : ""}`}
            disabled={disabled}
            onClick={() => {
              onClose();
              run();
            }}
          >
            {Icon && <Icon size={14} aria-hidden="true" />}
            {text}
          </button>
        </Fragment>
      ))}
    </div>
  );
}
