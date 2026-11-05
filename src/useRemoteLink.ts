import { useEffect, useReducer } from "react";
import { REMOTE_LINK_PATH, type RemoteStage, type ServerMessage } from "../shared/protocol.ts";

/** A remote window's link to its server, as the local dashboard reports it. */
export interface LinkState {
  /** The latest stage; null before the first one and while the status stream is down. */
  stage: RemoteStage | null;
  /** Sessions and the layout can reach the server now. */
  connected: boolean;
  /** The link was up at least once since the page loaded. */
  connectedOnce: boolean;
  /**
   * How often the link came back after it was up once. It changes in the
   * same update as `connected`, so the render where the link rises already
   * holds saves and pane reattaches until the layout for this count is read.
   */
  reconnects: number;
  /** The local westerm's version line from the stream's `hello`. */
  version: string | null;
  /** The local westerm changed (an update): load the new page files. */
  reload: boolean;
}

/** A message on the status stream, or `closed` when the stream itself drops. */
export type LinkMessage = Extract<ServerMessage, { type: "hello" } | { type: "remote" }> | { type: "closed" };

export const INITIAL_LINK: LinkState = {
  stage: null,
  connected: false,
  connectedOnce: false,
  reconnects: 0,
  version: null,
  reload: false,
};

export function linkReducer(state: LinkState, message: LinkMessage): LinkState {
  switch (message.type) {
    case "hello": {
      const reload = state.reload || (state.version !== null && state.version !== message.version);
      return { ...state, version: message.version, reload };
    }
    case "remote": {
      const { type: _, ...stage } = message;
      const connected = stage.stage === "connected";
      const reconnected = connected && !state.connected && state.connectedOnce;
      return {
        ...state,
        stage,
        connected,
        connectedOnce: state.connectedOnce || connected,
        reconnects: state.reconnects + (reconnected ? 1 : 0),
      };
    }
    case "closed":
      // The local dashboard is gone for now (a restart or an update).
      return { ...state, stage: null, connected: false };
  }
}

const REOPEN_MS = 1000;

/**
 * Watches the link to `target` on the local dashboard's status stream. While a
 * window watches, the dashboard keeps the link up. A closed stream opens
 * again after a second. A null target (a local window) opens nothing.
 */
export function useRemoteLink(target: string | null): LinkState {
  const [link, dispatch] = useReducer(linkReducer, INITIAL_LINK);
  useEffect(() => {
    if (target === null) return;
    let socket: WebSocket | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let stopped = false;
    const connect = () => {
      const scheme = location.protocol === "https:" ? "wss:" : "ws:";
      const ws = new WebSocket(`${scheme}//${location.host}${REMOTE_LINK_PATH}?${new URLSearchParams({ target })}`);
      socket = ws;
      ws.onmessage = (event) => {
        let message: ServerMessage;
        try {
          message = JSON.parse(String(event.data));
        } catch {
          return;
        }
        if (message.type === "hello" || message.type === "remote") dispatch(message);
      };
      ws.onclose = () => {
        if (stopped) return;
        dispatch({ type: "closed" });
        timer = setTimeout(connect, REOPEN_MS);
      };
    };
    connect();
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      socket?.close();
    };
  }, [target]);
  return link;
}
