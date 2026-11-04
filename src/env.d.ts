/// <reference types="vite/client" />
declare module "@wterm/react/css";

/** Vite alias for @wterm/dom 0.5.9 dist/kitty-keys.js (see vite.config.ts). */
declare module "@wterm-internal/kitty-keys" {
  export const KITTY_REPORT_EVENTS: number;
  export const KITTY_REPORT_ALL: number;
  export function encodeKittyKey(
    event: KeyboardEvent,
    flags: number,
    action: "press" | "repeat" | "release",
    pressedModifiers: ReadonlySet<string>,
    cursorKeysApp?: boolean,
  ): string | null;
}
