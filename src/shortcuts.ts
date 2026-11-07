import type { Direction } from "../shared/layout.ts";

/** Moving between terminals from the keyboard. */
export type SwitchCommand =
  /** 1–9; 9 is the last tab. */
  | { type: "tab"; number: number }
  | { type: "tabStep"; delta: 1 | -1 }
  | { type: "paneStep"; delta: 1 | -1 }
  | { type: "paneToward"; direction: Direction };

type Keys = Pick<KeyboardEvent, "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

const ARROWS: Record<string, Direction> = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };

/**
 * The switch a key press asks for, by physical key (any layout or IME).
 *
 * - Everywhere: Ctrl+Shift with 1–9, ] [ (tabs) and . , (panes). Browsers
 *   leave these to the page.
 * - With `appKeys` (the macOS app, see appCommandKeys) also the terminal-app
 *   keys that browsers keep for themselves: ⌘1–9, ⌘⇧] ⌘⇧[, ⌘] ⌘[, and ⌥⌘ arrows.
 */
export function switchCommand(event: Keys, appKeys: boolean): SwitchCommand | null {
  const { code, ctrlKey: ctrl, shiftKey: shift, altKey: alt, metaKey: meta } = event;
  const digit = /^Digit([1-9])$/.exec(code);
  if (ctrl && shift && !alt && !meta) {
    if (digit) return { type: "tab", number: Number(digit[1]) };
    if (code === "BracketRight") return { type: "tabStep", delta: 1 };
    if (code === "BracketLeft") return { type: "tabStep", delta: -1 };
    if (code === "Period") return { type: "paneStep", delta: 1 };
    if (code === "Comma") return { type: "paneStep", delta: -1 };
    return null;
  }
  if (!appKeys || !meta || ctrl) return null;
  if (alt && !shift) return code in ARROWS ? { type: "paneToward", direction: ARROWS[code] } : null;
  if (alt) return null;
  if (digit && !shift) return { type: "tab", number: Number(digit[1]) };
  if (code === "BracketRight") return shift ? { type: "tabStep", delta: 1 } : { type: "paneStep", delta: 1 };
  if (code === "BracketLeft") return shift ? { type: "tabStep", delta: -1 } : { type: "paneStep", delta: -1 };
  return null;
}

/** The key to show for a command: the macOS app's ⌘ key, or the Ctrl+Shift one. */
export function shortcutLabel(command: SwitchCommand, appKeys: boolean, mac: boolean): string {
  const key =
    command.type === "tab"
      ? String(command.number)
      : command.type === "tabStep"
        ? command.delta > 0 ? "]" : "["
        : command.type === "paneStep"
          ? command.delta > 0 ? (appKeys ? "]" : ".") : appKeys ? "[" : ","
          : { left: "←", right: "→", up: "↑", down: "↓" }[command.direction];
  if (appKeys) {
    if (command.type === "tabStep") return `⌘⇧${key}`;
    if (command.type === "paneToward") return `⌥⌘${key}`;
    return `⌘${key}`;
  }
  return mac ? `⌃⇧${key}` : `Ctrl+Shift+${key}`;
}

/** True while the user types in a field other than a terminal (palette, rename, settings). */
export function typingElsewhere(target: EventTarget | null): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;
  const field = target.closest("input, textarea, select, [contenteditable='true']");
  return field !== null && target.closest(".wterm") === null;
}
