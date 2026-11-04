import {
  ACK_THRESHOLD,
  CLOSE_TAKEN_OVER,
  WS_PATH,
  type AttachMode,
  type ClientMessage,
  type PtySize,
  type ServerMessage,
  type SessionKind,
} from "../../shared/protocol.ts";
import type { Locale } from "../i18n/messages.ts";

export type ConnectionEvent =
  | { type: "ready"; pid: number; mode: AttachMode }
  | { type: "exit"; code: number; signal: number }
  | { type: "error"; message: string }
  /** The socket closed without an exit. `takenOver`: another window has the session. */
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
  size: PtySize;
  /** Parse output synchronously; bytes are acknowledged after this returns. */
  onOutput: (bytes: Uint8Array) => void;
  onEvent: (event: ConnectionEvent) => void;
}

const ACK_DELAY_MS = 30;

/** One WebSocket ↔ one PTY on the local server. */
export class PtyConnection {
  private readonly ws: WebSocket;
  private unacked = 0;
  private ackTimer: ReturnType<typeof setTimeout> | null = null;
  private finished = false;
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
    });
    if (options.url) query.set("url", options.url);
    if (options.cwdFrom) query.set("cwdFrom", options.cwdFrom);
    if (options.target) query.set("target", options.target);
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    this.ws = new WebSocket(`${scheme}//${location.host}${WS_PATH}?${query}`);
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
    if (end && this.ws.readyState === WebSocket.CONNECTING) {
      // Closing now would leave the new process running unseen.
      this.ws.onopen = () => {
        this.send({ type: "close" });
        this.ws.close();
      };
      return;
    }
    if (end) this.send({ type: "close" });
    this.ws.close();
  }

  private receive(data: unknown): void {
    if (data instanceof ArrayBuffer) {
      const bytes = new Uint8Array(data);
      this.options.onOutput(bytes);
      this.consumed(bytes.length);
      return;
    }
    let message: ServerMessage;
    try {
      message = JSON.parse(String(data));
    } catch {
      return;
    }
    if (message.type === "exit" || message.type === "error") this.finish(message);
    else this.options.onEvent(message);
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
