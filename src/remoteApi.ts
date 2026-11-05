// Calls to the dashboard's remote westerm APIs (see shared/protocol.ts).

import type { SessionKind } from "../shared/protocol.ts";

/** POSTs JSON to a dashboard API. True on a 2xx answer, false on anything else. */
export async function postRemote(path: string, body: Record<string, unknown>): Promise<boolean> {
  try {
    const response = await fetch(path, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    return response.ok;
  } catch {
    return false;
  }
}

/** WebSocket readyState values, spelled out so this also runs outside a browser. */
const CONNECTING = 0;
const OPEN = 1;

/**
 * A closed remote pane must end its session on the server. An open socket
 * carries the close itself and a connecting one sends it once open; a closed
 * one cannot, so the pane tells the dashboard through REMOTE_CLOSE_PATH.
 */
export function shouldPostRemoteClose(kind: SessionKind, target: string | null, end: boolean, readyState: number): boolean {
  return kind === "remote" && target !== null && end && readyState !== OPEN && readyState !== CONNECTING;
}
