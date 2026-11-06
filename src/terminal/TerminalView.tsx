import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal, type TerminalHandle } from "@wterm/react";
import type { TerminalThemeColors } from "@wterm/core";
import type { WTerm } from "@wterm/dom";
import { GhosttyCore } from "@wterm/ghostty";
import ghosttyWasmUrl from "@wterm/ghostty/ghostty-vt.wasm?url";
import type { PtySize, SessionKind } from "../../shared/protocol.ts";
import { KINDS } from "../kinds.tsx";
import { installAltScreenImages } from "./alt-screen-images.ts";
import { installCommandKeys } from "./command-keys.ts";
import { PtyConnection, type ConnectionEvent } from "./connection.ts";
import { useI18n } from "../i18n/index.tsx";
import { takeEnding } from "./ending.ts";
import { keyboardFree } from "./keyboard.ts";
import { fontStyle, type TerminalFont } from "./fonts.ts";


export interface TerminalViewProps {
  sessionId: string;
  kind: SessionKind;
  url: string | null;
  cwdFrom: string | null;
  target: string | null;
  label: string;
  font: TerminalFont;
  /** The color theme's set for the scheme in use (src/colorThemes.ts). */
  colors: TerminalThemeColors;
  focused: boolean;
  /** A new value refocuses the terminal, e.g. after a rename field closes. */
  focusNonce: number;
  /** False while another pane covers this one; parsing continues, painting stops. */
  visible: boolean;
  /** Read at connect time: a restored shell resumes its AI agent. */
  resumeAgents: boolean;
  onEvent: (event: ConnectionEvent) => void;
  onTitle: (title: string) => void;
  onFocus: () => void;
}

/** Content size in CSS pixels, excluding padding, as Kitty clients expect. */
function ptySize(wt: WTerm): PtySize {
  const element = wt.element;
  const style = getComputedStyle(element);
  const width =
    element.clientWidth - (parseFloat(style.paddingLeft) || 0) - (parseFloat(style.paddingRight) || 0);
  const height =
    element.clientHeight - (parseFloat(style.paddingTop) || 0) - (parseFloat(style.paddingBottom) || 0);
  return {
    cols: wt.cols,
    rows: wt.rows,
    width: Math.max(1, Math.min(65535, Math.round(width))),
    height: Math.max(1, Math.min(65535, Math.round(height))),
  };
}

function useGhosttyCore(kind: SessionKind): { core: GhosttyCore | null; error: string | null } {
  const [core, setCore] = useState<GhosttyCore | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let loaded: GhosttyCore | null = null;
    let cancelled = false;
    GhosttyCore.load({ wasmPath: ghosttyWasmUrl, imageStorageLimit: KINDS[kind].imageLimit })
      .then((next) => {
        if (cancelled) next.dispose();
        else setCore((loaded = next));
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      cancelled = true;
      // Let the Terminal child destroy its WTerm before the core goes away.
      const core = loaded;
      setTimeout(() => core?.dispose(), 0);
    };
  }, [kind]);
  return { core, error };
}

export function TerminalView(props: TerminalViewProps) {
  const { sessionId, kind, url, cwdFrom, target, label, font, colors, focused, focusNonce, visible } = props;
  const { core, error } = useGhosttyCore(kind);
  const { t, locale } = useI18n();
  // Read at connect time only: switching language must not reconnect.
  const localeRef = useRef(locale);
  localeRef.current = locale;
  const resumeRef = useRef(props.resumeAgents);
  resumeRef.current = props.resumeAgents;
  const terminalRef = useRef<TerminalHandle>(null);
  const connectionRef = useRef<PtyConnection | null>(null);
  const wtermRef = useRef<WTerm | null>(null);
  const uninstallKeysRef = useRef<(() => void) | null>(null);
  // Callbacks change every render; the connection must always call the latest.
  const callbacks = useRef(props);
  callbacks.current = props;

  useEffect(
    () => () => {
      uninstallKeysRef.current?.();
      connectionRef.current?.close({ end: takeEnding(sessionId) });
    },
    [sessionId],
  );

  useEffect(() => {
    if (focused && keyboardFree()) terminalRef.current?.focus();
  }, [focused, focusNonce]);

  // Live theme and light/dark switch: repaints history and keeps app color overrides.
  const colorsRef = useRef(colors);
  colorsRef.current = colors;
  useEffect(() => {
    wtermRef.current?.setThemeColors(colors);
  }, [colors]);

  const handleReady = useCallback(
    (wt: WTerm) => {
      wtermRef.current = wt;
      // Colors before the process starts, so its first OSC 11 query matches.
      wt.setThemeColors(colorsRef.current);
      if (!installAltScreenImages(wt)) {
        console.warn("[westerm] alternate-screen image patch unavailable; full-screen Kitty apps may scroll");
      }
      uninstallKeysRef.current?.();
      uninstallKeysRef.current = installCommandKeys(wt, (sequence) =>
        connectionRef.current?.sendInput(sequence),
      );
      connectionRef.current?.close({ end: false });
      connectionRef.current = new PtyConnection({
        session: sessionId,
        kind,
        url,
        cwdFrom,
        target,
        lang: localeRef.current,
        resumeAgents: resumeRef.current,
        size: ptySize(wt),
        onOutput: (bytes) => wt.write(bytes),
        onEvent: (event) => callbacks.current.onEvent(event),
      });
      if (callbacks.current.focused && keyboardFree()) wt.focus();
    },
    [sessionId, kind, url, cwdFrom, target],
  );

  const handleResize = useCallback(() => {
    const wt = wtermRef.current;
    if (wt) connectionRef.current?.resize(ptySize(wt));
  }, []);

  if (error) return <div className="pane-message">{t.pane.coreFailed(error)}</div>;
  if (!core) return <div className="pane-message">{t.pane.coreLoading}</div>;

  return (
    <Terminal
      ref={terminalRef}
      className="pane-terminal"
      style={fontStyle(font)}
      core={core}
      autoResize
      aria-label={label}
      renderingPaused={!visible}
      onReady={handleReady}
      onResize={handleResize}
      onData={(data) => connectionRef.current?.sendInput(data)}
      onBinary={(data) => connectionRef.current?.sendBinary(data)}
      onTitle={(title) => callbacks.current.onTitle(title)}
      onFocus={() => callbacks.current.onFocus()}
    />
  );
}
