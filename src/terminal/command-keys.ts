import type { WTerm } from "@wterm/dom";
import {
  encodeKittyKey,
  KITTY_REPORT_ALL,
  KITTY_REPORT_EVENTS,
} from "@wterm-internal/kitty-keys";

/**
 * Cmd shortcuts for full Kitty-keyboard apps (terminal-code, terminal-browser).
 *
 * @wterm/dom leaves every Cmd+key to the browser, so Cmd+P or Cmd+S never
 * reach VS Code running in a pane. A native terminal such as Ghostty sends
 * those keys to an application that asked for "report all keys as escape
 * codes" (Kitty flag 8). This does the same, with wterm's own encoder.
 *
 * Kept for the browser:
 * - Cmd+V, so the paste event still delivers clipboard text.
 * - Shortcuts Chrome reserves (Cmd+T, Cmd+W, Cmd+N, Cmd+Q). They never reach
 *   the page.
 * - Cmd+Q, Cmd+H, and Cmd+M. In the macOS app (desktop/) the page sees keys
 *   before the menu, so forwarding these would stop Quit, Hide, and Minimize.
 *
 * Apps that do not set flag 8 (shells, Neovim) keep wterm's behavior.
 */

const MODIFIER_CODE = /^(Shift|Control|Alt|Meta)(Left|Right)$/;

function isNativePaste(event: KeyboardEvent): boolean {
  return event.code === "KeyV" && !event.altKey && !event.shiftKey;
}

const SYSTEM_KEYS = new Set(["KeyQ", "KeyH", "KeyM"]);

export function installCommandKeys(wt: WTerm, send: (sequence: string) => void): () => void {
  const element = wt.element;
  const pressed = new Set<string>();
  // Keydown events we sent, by key code, so their releases go to the app too.
  const forwarded = new Map<string, KeyboardEvent>();

  const state = () => {
    const bridge = (wt as unknown as { bridge?: { kittyKeyboardFlags?: () => number; cursorKeysApp?: () => boolean } })
      .bridge;
    return { flags: bridge?.kittyKeyboardFlags?.() ?? 0, cursorKeysApp: bridge?.cursorKeysApp?.() ?? false };
  };

  const release = (event: KeyboardEvent) => {
    const { flags, cursorKeysApp } = state();
    if (!(flags & KITTY_REPORT_EVENTS)) return;
    const sequence = encodeKittyKey(event, flags, "release", pressed, cursorKeysApp);
    if (sequence) send(sequence);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (MODIFIER_CODE.test(event.code)) {
      pressed.add(event.code);
      return; // wterm reports modifier presses itself.
    }
    if (!event.metaKey || event.ctrlKey || event.isComposing || isNativePaste(event) || SYSTEM_KEYS.has(event.code)) return;
    const { flags, cursorKeysApp } = state();
    if (!(flags & KITTY_REPORT_ALL)) return;
    const sequence = encodeKittyKey(event, flags, event.repeat ? "repeat" : "press", pressed, cursorKeysApp);
    if (!sequence) return;
    event.preventDefault();
    event.stopPropagation();
    forwarded.set(event.code, event);
    send(sequence);
  };

  const onKeyUp = (event: KeyboardEvent) => {
    if (MODIFIER_CODE.test(event.code)) {
      pressed.delete(event.code);
      // macOS browsers drop keyup for keys released while Cmd is held.
      // Release them with Cmd so the app does not see a stuck key.
      if (event.code.startsWith("Meta")) {
        for (const down of forwarded.values()) release(down);
        forwarded.clear();
      }
      return;
    }
    const down = forwarded.get(event.code);
    if (!down) return;
    forwarded.delete(event.code);
    event.preventDefault();
    event.stopPropagation();
    release(event);
  };

  const onBlur = () => {
    pressed.clear();
    forwarded.clear();
  };

  // Capture on the terminal element runs before wterm's textarea listeners.
  element.addEventListener("keydown", onKeyDown, true);
  element.addEventListener("keyup", onKeyUp, true);
  element.addEventListener("focusout", onBlur);
  return () => {
    element.removeEventListener("keydown", onKeyDown, true);
    element.removeEventListener("keyup", onKeyUp, true);
    element.removeEventListener("focusout", onBlur);
  };
}
