import { useCallback, useEffect, useMemo, useState } from "react";
import {
  parseTunnelStatus,
  TUNNEL_CHECK_PATH,
  TUNNEL_START_PATH,
  TUNNEL_STATUS_PATH,
  TUNNEL_STOP_PATH,
  type TunnelStatus,
} from "../shared/protocol.ts";

export interface Tunnel {
  /** Null before the first status and while the stream is down. */
  status: TunnelStatus | null;
  start(): void;
  stop(): void;
  check(): void;
}

const REOPEN_MS = 1000;

/**
 * The tunnel's status from the local dashboard's stream, and its controls.
 * Off (`enabled` false) in a remote window and on a tunnel page: neither has
 * the controls. A closed stream opens again after a second.
 */
export function useTunnel(enabled: boolean): Tunnel {
  const [status, setStatus] = useState<TunnelStatus | null>(null);
  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      return;
    }
    let socket: WebSocket | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let stopped = false;
    const connect = () => {
      const scheme = location.protocol === "https:" ? "wss:" : "ws:";
      const ws = new WebSocket(`${scheme}//${location.host}${TUNNEL_STATUS_PATH}`);
      socket = ws;
      ws.onmessage = (event) => {
        try {
          const next = parseTunnelStatus(JSON.parse(String(event.data)));
          if (next) setStatus(next);
        } catch {
          // Not JSON: ignore it.
        }
      };
      ws.onclose = () => {
        if (stopped) return;
        setStatus(null);
        timer = setTimeout(connect, REOPEN_MS);
      };
    };
    connect();
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
      socket?.close();
    };
  }, [enabled]);
  const post = useCallback((path: string) => {
    void fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" }).catch(() => {
      // The stream shows what happened; a lost request changes nothing.
    });
  }, []);
  return useMemo(
    () => ({
      status,
      start: () => post(TUNNEL_START_PATH),
      stop: () => post(TUNNEL_STOP_PATH),
      check: () => post(TUNNEL_CHECK_PATH),
    }),
    [status, post],
  );
}
