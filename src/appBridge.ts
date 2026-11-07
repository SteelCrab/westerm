// The desktop app (desktop/, Tauri) puts `window.westermApp` on its pages.
// A browser has none. The app also sends these window events:
// - "westerm:command" with a SwitchCommand (menu: tabs and panes)
// - "westerm:settings" (menu: Settings…)
// - "westerm:update" (menu: Check for Updates…)

/**
 * What a page can ask the app for.
 * - updateStatus: whether a newer app exists (an UpdateStatus with channel
 *   "app"). The app answers for itself: the dashboard it shows may be a
 *   westerm started elsewhere, of another version.
 * - update: install it (AppUpdateResult).
 * - remoteWindow: open or raise a server's window.
 */
export type AppMessage =
  | { type: "updateStatus"; refresh: boolean }
  | { type: "update" }
  | { type: "remoteWindow"; target: string };

/**
 * The answer to `{ type: "update" }`.
 * - relaunching: the app installs the update and opens again.
 * - manual: the app opened the release page; the person installs it.
 */
export type AppUpdateResult = "relaunching" | "manual";

export interface AppBridge {
  /** Rejects with the app's message when it cannot do it. */
  request(message: AppMessage): Promise<unknown>;
}

/** The desktop app's bridge; null in a browser and where there is no page (tests). */
export function appBridge(): AppBridge | null {
  if (typeof window === "undefined") return null;
  const bridge = (window as { westermApp?: Partial<AppBridge> }).westermApp;
  return typeof bridge?.request === "function" ? (bridge as AppBridge) : null;
}

/**
 * Whether ⌘ terminal-app keys (⌘1–9, ⌘] and so on) switch terminals. Only the
 * macOS app gets them: browsers keep those keys, and on Windows and Linux the
 * same key is the system's Windows or Super key.
 */
export function appCommandKeys(bridge: AppBridge | null, mac: boolean): boolean {
  return bridge !== null && mac;
}
