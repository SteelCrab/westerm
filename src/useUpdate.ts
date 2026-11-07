import { useCallback, useEffect, useRef, useState } from "react";
import { UPDATE_PATH, type UpdateStatus } from "../shared/protocol.ts";
import { appBridge, type AppBridge, type AppUpdateResult } from "./appBridge.ts";

/**
 * installing: downloading and checking · restarting: waiting for the new
 * server or app · manual: the app opened the release page for the person
 */
export type UpdatePhase = "idle" | "installing" | "restarting" | "manual" | "failed" | "timeout";

export interface UpdateState {
  status: UpdateStatus | null;
  phase: UpdatePhase;
  /** The server's message when an install failed. */
  error: string | null;
  /** `refresh` asks GitHub again instead of using the server's recent answer. */
  check: (refresh?: boolean) => Promise<UpdateStatus | null>;
  install: () => void;
}

const RECHECK_MS = 6 * 60 * 60 * 1000;
const RESTART_WAIT_MS = 90_000;

/** The update status from the desktop app, or from the dashboard in a browser. */
export async function fetchUpdateStatus(refresh: boolean, bridge: AppBridge | null = appBridge()): Promise<UpdateStatus | null> {
  if (bridge) return (await bridge.request({ type: "updateStatus", refresh })) as UpdateStatus;
  const response = await fetch(refresh ? `${UPDATE_PATH}?refresh=1` : UPDATE_PATH, { cache: "no-store" });
  return response.ok ? ((await response.json()) as UpdateStatus) : null;
}

/**
 * Release updates through /api/update; inside the desktop app, of the app,
 * through the app. With `automatic` off (Settings), nothing is checked until asked.
 */
export function useUpdate(automatic = true): UpdateState {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [phase, setPhase] = useState<UpdatePhase>("idle");
  const [error, setError] = useState<string | null>(null);
  const statusRef = useRef(status);
  statusRef.current = status;

  const check = useCallback(async (refresh = false) => {
    try {
      const next = await fetchUpdateStatus(refresh);
      setStatus(next);
      return next;
    } catch {
      setStatus(null);
      return null;
    }
  }, []);

  useEffect(() => {
    if (!automatic) return;
    void check();
    const timer = setInterval(() => void check(), RECHECK_MS);
    return () => clearInterval(timer);
  }, [automatic, check]);

  const install = useCallback(() => {
    const current = statusRef.current;
    const target = current?.latest;
    if (!current?.newer || !target) return;
    if (current.channel === "app") {
      const bridge = appBridge();
      if (!bridge) return;
      setPhase("installing");
      setError(null);
      bridge.request({ type: "update" }).then(
        (result) => setPhase((result as AppUpdateResult) === "manual" ? "manual" : "restarting"),
        (reason: unknown) => {
          setError(reason instanceof Error ? reason.message : String(reason));
          setPhase("failed");
        },
      );
      return;
    }
    if (current.channel !== "binary") return;
    setPhase("installing");
    setError(null);
    void (async () => {
      try {
        const response = await fetch(UPDATE_PATH, { method: "POST" });
        if (!response.ok) {
          const body = (await response.json().catch(() => ({}))) as { error?: string };
          setError(body.error ?? `HTTP ${response.status}`);
          setPhase("failed");
          return;
        }
      } catch (reason) {
        setError(String(reason));
        setPhase("failed");
        return;
      }
      // The server restarts on the new executable. Reload once it answers
      // with the new version, so the page also gets the new UI.
      setPhase("restarting");
      const deadline = Date.now() + RESTART_WAIT_MS;
      while (Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 1000));
        try {
          const response = await fetch(UPDATE_PATH, { cache: "no-store" });
          if (response.ok && ((await response.json()) as UpdateStatus).current === target) {
            location.reload();
            return;
          }
        } catch {
          // Not back yet.
        }
      }
      setPhase("timeout");
    })();
  }, []);

  return { status, phase, error, check, install };
}
