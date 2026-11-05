// A remote window: this page opened as `/?remote=<target>` shows a server's
// tabs, workspaces, and sessions. The page files, the fonts, and the settings
// stay local; the layout, the SSH host list, and the sessions come from the
// server through the local dashboard's `/remote/<target>/…` routes.

import { SSH_HOSTS_PATH, WS_PATH } from "../shared/protocol.ts";
import { isSshTarget, WORKSPACE_PATH } from "../shared/workspace-file.ts";

/** The server a window's address names (`?remote=<target>`), or null for a local window. */
export function remoteTargetFrom(search: string): string | null {
  const target = new URLSearchParams(search).get("remote");
  return isSshTarget(target) ? target : null;
}

/** This window's server; null in a local window and where there is no page (tests). */
export const REMOTE_TARGET: string | null = typeof location === "undefined" ? null : remoteTargetFrom(location.search);

/** The paths that a remote window sends to its server. */
const SERVER_PATHS: readonly string[] = [WORKSPACE_PATH, SSH_HOSTS_PATH, WS_PATH];

/** Where this window reaches `path`: through `/remote/<target>` for the server's own APIs. */
export function serverPath(path: string, target: string | null = REMOTE_TARGET): string {
  return target !== null && SERVER_PATHS.includes(path) ? `/remote/${encodeURIComponent(target)}${path}` : path;
}

/**
 * A pane closed while its socket is down cannot send `close` to the server.
 * The local dashboard then keeps the session id and ends it on the server at
 * the next connection. `readyState`: the WebSocket's; 2 closing, 3 closed.
 */
export function shouldPostRemoteClose(target: string | null, end: boolean, readyState: number): boolean {
  return target !== null && end && readyState >= 2;
}

/** The popups this page opened in a browser, by server. A window that is closed counts as gone. */
const openWindows = new Map<string, Window>();

/** What the macOS app listens for on its "westerm" message handler. */
export type AppBridge = (message: { type: "remoteWindow"; target: string }) => void;

/** The app's message handler, bound; undefined in a browser and where there is no page (tests). */
function appBridge(): AppBridge | undefined {
  if (typeof window === "undefined") return undefined;
  const handler = (window as { webkit?: { messageHandlers?: { westerm?: { postMessage?: AppBridge } } } }).webkit
    ?.messageHandlers?.westerm;
  return handler?.postMessage ? (message) => handler.postMessage?.(message) : undefined;
}

/**
 * Opens `target`'s remote window. In the macOS app the app opens or raises
 * the window: a page cannot raise a window it opened, so every call asks the
 * app. In a browser, this page opens a popup, or focuses the one it opened
 * before when it is still open.
 */
export function openRemoteWindow(
  target: string,
  open: (url: string, name: string, features: string) => Window | null = window.open.bind(window),
  post: AppBridge | undefined = appBridge(),
): void {
  if (post) {
    post({ type: "remoteWindow", target });
    return;
  }
  const existing = openWindows.get(target);
  if (existing && !existing.closed) {
    existing.focus();
    return;
  }
  const name = encodeURIComponent(target);
  const opened = open(`/?remote=${name}`, `westerm-remote-${name}`, "popup,width=1280,height=820");
  if (opened) openWindows.set(target, opened);
  else openWindows.delete(target);
}
