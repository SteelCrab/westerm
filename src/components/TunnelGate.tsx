import { useCallback, useEffect, useReducer } from "react";
import { parseTunnelAuth, TUNNEL_AUTH_PATH } from "../../shared/protocol.ts";
import { App } from "../App.tsx";
import { gateReducer, TUNNEL_ENDED_EVENT } from "../tunnel.ts";
import { TunnelEndScreen, TunnelLogin } from "./TunnelLogin.tsx";

/** How often a signed-in tunnel page asks whether the tunnel is still there. */
const ASK_EVERY_MS = 10_000;

/**
 * Decides what this page is before anything else loads: the computer's own
 * dashboard (the app as always), or a page that came through a tunnel (the
 * access code first, then the app without the computer-only controls). The
 * server says which; the address does not.
 */
export function TunnelGate() {
  const [gate, dispatch] = useReducer(gateReducer, { phase: "checking" });
  const ask = useCallback(async () => {
    try {
      const response = await fetch(TUNNEL_AUTH_PATH, { cache: "no-store" });
      const auth = parseTunnelAuth(await response.json());
      dispatch(auth ? { type: "auth", auth } : { type: "failed", now: Date.now() });
    } catch {
      // Cloudflare answers with its own error page when the tunnel is gone.
      dispatch({ type: "failed", now: Date.now() });
    }
  }, []);
  useEffect(() => {
    if (gate.phase === "checking") void ask();
  }, [gate.phase, ask]);
  const signedIn = gate.phase === "in";
  useEffect(() => {
    if (!signedIn) return;
    const timer = setInterval(() => void ask(), ASK_EVERY_MS);
    return () => clearInterval(timer);
  }, [signedIn, ask]);
  useEffect(() => {
    const ended = () => dispatch({ type: "ended" });
    window.addEventListener(TUNNEL_ENDED_EVENT, ended);
    return () => window.removeEventListener(TUNNEL_ENDED_EVENT, ended);
  }, []);

  switch (gate.phase) {
    case "checking":
      return null;
    case "local":
      return <App />;
    case "login":
      return <TunnelLogin lockedFor={gate.lockedFor} onSignedIn={() => void ask()} />;
    case "in":
      return <App tunnelPage />;
    case "ended":
      return <TunnelEndScreen kind="ended" onRetry={() => dispatch({ type: "retry" })} />;
    case "unreachable":
      return <TunnelEndScreen kind="unreachable" onRetry={() => dispatch({ type: "retry" })} />;
  }
}
