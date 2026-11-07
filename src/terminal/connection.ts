import {
  ACK_THRESHOLD,
  CLOSE_TAKEN_OVER,
  REMOTE_CLOSE_PATH,
  WS_PATH,
  parseAgentState,
  type AgentState,
  type AttachMode,
  type ClientMessage,
  type PixelCodec,
  type PtySize,
  type ServerMessage,
  type SessionKind,
} from "../../shared/protocol.ts";
import type { Locale } from "../i18n/messages.ts";
import { postRemote } from "../remoteApi.ts";
import { REMOTE_TARGET, serverPath, shouldPostRemoteClose } from "../remoteWindow.ts";
import { splitBinary } from "./pixel-update.ts";

export type ConnectionEvent =
  | { type: "ready"; pid: number; mode: AttachMode; pixels?: boolean }
  /** The AI agent in a shell's foreground, or null. */
  | { type: "agent"; agent: AgentState | null }
  | { type: "exit"; code: number; signal: number }
  | { type: "error"; message: string }
  /** The socket closed without an exit. `takenOver`: another window has the sessions. */
  | { type: "closed"; takenOver: boolean };

export interface ConnectionOptions {
  /** Stable id: reconnecting with it reaches the same process in the PTY host. */
  session: string;
  kind: SessionKind;
  url: string | null;
  /** Start in this session's directory, for a new editor. */
  cwdFrom: string | null;
  /** ssh destination. */
  target: string | null;
  /** Language of text the server writes: the restore notice and errors. */
  lang: Locale;
  /** A shell that comes back as "restored" resumes its AI agent. */
  resumeAgents: boolean;
  size: PtySize;
  /** Ask for pixel updates instead of Kitty images (pixel apps only). */
  pixels: PixelCodec | null;
  /** Parse output synchronously; bytes are acknowledged after this returns. */
  onOutput: (bytes: Uint8Array) => void;
  /** A pixel update; call sendPainted() once it is drawn. */
  onPixels: (buffer: ArrayBuffer) => void;
  onEvent: (event: ConnectionEvent) => void;
}

const ACK_DELAY_MS = 30;

/**
 * Names this page to the PTY host. One window at a time owns every session:
 * a pane from another window takes them all (see native/src/host/registry.rs).
 */
const WINDOW_ID = `w${Array.from(crypto.getRandomValues(new Uint8Array(8)), (byte) => byte.toString(16).padStart(2, "0")).join("")}`;

/** One WebSocket ↔ one PTY on the local server, or on a remote window's server. */
export class PtyConnection {
  private readonly ws: WebSocket;
  private unacked = 0;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private finished = false;
  /** "ready" said pixels: binary messages start with a kind byte. */
  private framed = false;
  /** Latest size; sent first when the socket opens. */
  private pendingSize: PtySize;

  constructor(private readonly options: ConnectionOptions) {
    this.pendingSize = options.size;
    const query = new URLSearchParams({
      session: options.session,
      kind: options.kind,
      cols: String(options.size.cols),
      rows: String(options.size.rows),
      width: String(options.size.width),
      height: String(options.size.height),
      lang: options.lang,
      window: WINDOW_ID,
    });
    if (options.url) query.set("url", options.url);
    if (options.cwdFrom) query.set("cwdFrom", options.cwdFrom);
    if (options.target) query.set("target", options.target);
    if (!options.resumeAgents) query.set("resume", "off");
    if (options.pixels) query.set("pixels", options.pixels);
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    this.ws = new WebSocket(`${scheme}//${location.host}${serverPath(WS_PATH)}?${query}`);
    this.ws.binaryType = "arraybuffer";
    this.ws.onopen = () => {
      // Always send the fitted size first: the terminal usually fits its pane
      // while the socket connects, and a reattach replays at this size.
      this.send({ type: "resize", ...this.pendingSize });
    };
    this.ws.onmessage = (event) => this.receive(event.data);
    this.ws.onclose = (event) => this.finish({ type: "closed", takenOver: event.code === CLOSE_TAKEN_OVER });
  }

  sendInput(data: string): void {
    this.send({ type: "input", data });
  }

  /** One pixel update is drawn: the host may send the next. */
  sendPainted(): void {
    this.send({ type: "painted" });
  }

  sendBinary(data: Uint8Array): void {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(new Uint8Array(data));
  }

  resize(size: PtySize): void {
    this.pendingSize = size;
    if (this.ws.readyState === WebSocket.OPEN) this.send({ type: "resize", ...size });
  }

  /**
   * `end`: the user closed the pane, so the process stops and its saved screen
   * is deleted. Otherwise this only detaches and the process keeps running.
   */
  close({ end }: { end: boolean }): void {
    this.finished = true;
    this.clearAckTimer();
    // A socket that is down cannot carry the close. In a remote window the
    // local dashboard keeps the id and ends the session on the server at the
    // next connection.
    const reportClose = () => {
      if (shouldPostRemoteClose(REMOTE_TARGET, end, this.ws.readyState)) {
        void postRemote(REMOTE_CLOSE_PATH, { target: REMOTE_TARGET, session: this.options.session });
      }
    };
    if (end && this.ws.readyState === WebSocket.CONNECTING) {
      // Closing now would leave the new process running unseen.
      this.ws.onopen = () => {
        this.ws.onclose = null;
        this.send({ type: "close" });
        this.ws.close();
      };
      // While a remote window's link is down, the socket never opens.
      this.ws.onclose = reportClose;
      return;
    }
    reportClose();
    if (end) this.send({ type: "close" });
    this.ws.close();
  }

  private receive(data: unknown): void {
    if (data instanceof ArrayBuffer) {
      const part = splitBinary(data, this.framed);
      if (part.kind === "pixels") {
        this.options.onPixels(part.buffer);
        return;
      }
      this.options.onOutput(part.bytes);
      // The host counts output without the kind byte.
      this.consumed(part.bytes.length);
      return;
    }
    let message: ServerMessage;
    try {
      message = JSON.parse(String(data));
    } catch {
      return;
    }
    if (message.type === "exit" || message.type === "error") this.finish(message);
    else if (message.type === "ready") {
      this.framed = message.pixels === true;
      this.options.onEvent(message);
    } else if (message.type === "agent") this.options.onEvent({ type: "agent", agent: parseAgentState(message.agent) });
  }

  /** Acknowledge parsed bytes in batches so the server can keep the PTY flowing. */
  private consumed(bytes: number): void {
    this.unacked += bytes;
    if (this.unacked >= ACK_THRESHOLD) {
      this.flushAck();
    } else if (!this.ackTimer) {
      this.ackTimer = setTimeout(() => this.flushAck(), ACK_DELAY_MS);
    }
  }

  private flushAck(): void {
    this.clearAckTimer();
    if (this.unacked === 0) return;
    this.send({ type: "ack", bytes: this.unacked });
    this.unacked = 0;
  }

  private clearAckTimer(): void {
    if (this.ackTimer) clearTimeout(this.ackTimer);
    this.ackTimer = null;
  }

  private send(message: ClientMessage): void {
    if (this.ws.readyState === WebSocket.OPEN) this.ws.send(JSON.stringify(message));
  }

  /** Report the first terminal event only; "exit" wins over the close that follows it. */
  private finish(event: ConnectionEvent): void {
    if (this.finished) return;
    this.finished = true;
    this.clearAckTimer();
    this.options.onEvent(event);
  }
}
