// The Cloudflare tunnel's logic that needs no page: the access code input,
// install hints, times on the panel, and the state of a page opened through
// a tunnel (TunnelGate).

import { CLOSE_TUNNEL_ENDED, type TunnelAuth, type TunnelOs } from "../shared/protocol.ts";

/** The 6 digits of an access code from whatever was typed, pasted, or filled in. */
export function codeDigits(input: string): string {
  // NFKC turns full-width digits (a Japanese or Chinese keyboard) into ASCII.
  return input.normalize("NFKC").replace(/[^0-9]/g, "").slice(0, 6);
}

/** Cloudflare's own download page; it lists every way to install cloudflared. */
const DOWNLOADS = "https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/downloads/";

/**
 * The official install command for this computer, or null where it differs
 * by distribution (Linux): then the page has the packages. westerm never
 * installs cloudflared itself (see installHints.ts).
 */
export function installHint(os: TunnelOs): { command: string | null; link: string } {
  switch (os) {
    case "macos":
      return { command: "brew install cloudflared", link: DOWNLOADS };
    case "windows":
      return { command: "winget install --id Cloudflare.cloudflared", link: DOWNLOADS };
    case "linux":
      return { command: null, link: DOWNLOADS };
  }
}

export function elapsedMinutes(startedAt: number, now: number): number {
  return Math.max(0, Math.floor((now - startedAt) / 60_000));
}

export function hoursLeft(deadline: number, now: number): number {
  return Math.max(0, Math.ceil((deadline - now) / 3_600_000));
}

/**
 * A page's state. checking: asking the server. local: the dashboard on this
 * computer. login: a tunnel page without its cookie. in: a tunnel page
 * signed in; `failingSince` is when the server stopped answering.
 */
export type Gate =
  | { phase: "checking" }
  | { phase: "local" }
  | { phase: "login"; lockedFor: number | null }
  | { phase: "in"; failingSince: number | null }
  | { phase: "ended" }
  | { phase: "unreachable" };

export type GateEvent =
  | { type: "auth"; auth: TunnelAuth }
  | { type: "failed"; now: number }
  | { type: "ended" }
  | { type: "retry" };

/** A signed-in page waits this long for an answer before it says the tunnel is unreachable. */
export const UNREACHABLE_AFTER_MS = 120_000;

export function gateReducer(gate: Gate, event: GateEvent): Gate {
  if (gate.phase === "local") return gate;
  switch (event.type) {
    case "auth": {
      const { tunnel, signedIn, lockedFor } = event.auth;
      if (!tunnel) return gate.phase === "checking" ? { phase: "local" } : gate;
      if (signedIn) return gate.phase === "ended" ? gate : { phase: "in", failingSince: null };
      // A page that was in lost its cookie: the tunnel ended.
      if (gate.phase === "in") return { phase: "ended" };
      return gate.phase === "checking" || gate.phase === "login" ? { phase: "login", lockedFor } : gate;
    }
    case "failed":
      if (gate.phase === "checking") return { phase: "unreachable" };
      if (gate.phase !== "in") return gate;
      if (gate.failingSince === null) return { phase: "in", failingSince: event.now };
      return event.now - gate.failingSince >= UNREACHABLE_AFTER_MS ? { phase: "unreachable" } : gate;
    case "ended":
      return { phase: "ended" };
    case "retry":
      return gate.phase === "unreachable" || gate.phase === "ended" ? { phase: "checking" } : gate;
  }
}

/** A terminal socket closed because the tunnel ended on purpose: the page stops for good. */
export function tunnelEnded(code: number): boolean {
  return code === CLOSE_TUNNEL_ENDED;
}

/** Sent on `window` by a terminal that saw CLOSE_TUNNEL_ENDED; TunnelGate shows the end. */
export const TUNNEL_ENDED_EVENT = "westerm:tunnel-ended";

export interface PageControls {
  updates: boolean;
  remoteWindows: boolean;
  tunnel: boolean;
  /** The plugin bar and the plugin manager. */
  plugins: boolean;
}

/**
 * What a page may control. Only the computer's own dashboard updates westerm,
 * opens remote windows, runs tunnels, and has plugins; a remote window follows
 * its local westerm, and a tunnel page belongs to someone who is not at the
 * computer.
 */
export function pageControls(remote: boolean, tunnelPage: boolean): PageControls {
  const local = !remote && !tunnelPage;
  return { updates: local, remoteWindows: local, tunnel: local, plugins: local };
}
