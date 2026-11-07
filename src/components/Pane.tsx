import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, GripVertical, Maximize2, Minimize2, RotateCcw, X } from "lucide-react";
import { carries, dropZone, PANE_MIME, startDrag, type DropZone } from "../dnd.ts";
import { DocsView } from "../docs/DocsView.tsx";
import { useI18n } from "../i18n/index.tsx";
import { installHint, type InstallHint } from "../installHints.ts";
import { KindIcon } from "../kinds.tsx";
import { AgentCard } from "./AgentCard.tsx";
import { AgentMark } from "./AgentMark.tsx";
import { REMOTE_TARGET } from "../remoteWindow.ts";
import type { TerminalThemeColors } from "@wterm/core";
import type { ConnectionEvent } from "../terminal/connection.ts";
import type { TerminalFont } from "../terminal/fonts.ts";
import { TerminalView } from "../terminal/TerminalView.tsx";
import { useCopy } from "../useCopy.ts";
import { agentDisplay, sessionLabel, type Session, type DashboardAction } from "../workspace.ts";
import { InlineRename, type RenameTarget } from "./InlineRename.tsx";
import { StatusDot } from "./StatusDot.tsx";

interface PaneProps {
  session: Session;
  font: TerminalFont;
  /** A restored shell resumes its AI agent (Settings ▸ Terminal). */
  resumeAgents: boolean;
  colors: TerminalThemeColors;
  focused: boolean;
  maximized: boolean;
  /** Covered by a maximized pane: stays mounted, stops painting. */
  covered: boolean;
  gridStyle: React.CSSProperties;
  /** Changes when the dashboard hands keyboard focus back to the terminal. */
  focusNonce: number;
  renaming: RenameTarget | null;
  onRename: (target: RenameTarget | null) => void;
  /** Id of the pane being dragged anywhere in the dashboard. */
  dragging: string | null;
  /** Another window has the sessions: do not reconnect until this one is used. */
  away: boolean;
  /** False while a remote window's link is down or its server's layout is not read yet. */
  mayReattach: boolean;
  onDragPane: (id: string | null) => void;
  dispatch: (action: DashboardAction) => void;
  /** Docs panes: the followed shell's pid while it runs, whether it exists, and a shell to follow. */
  followPid: number | null;
  followAlive: boolean;
  followCandidate: string | null;
  /** Docs panes open web links in a terminal-browser pane. */
  onOpenUrl: (url: string) => void;
}

function exitDetail(event: Extract<ConnectionEvent, { type: "exit" }>): string {
  if (event.signal) return `signal ${event.signal}`;
  return `exit ${event.code}`;
}

/** The install command of a missing program, with a Copy button. westerm never runs it. */
function InstallCommand({ command }: { command: string }) {
  const { t } = useI18n();
  // The label goes back to "Copy" after a moment. A failed copy asks the user to select the text.
  const [result, copy] = useCopy();
  return (
    <div className="pane-install">
      <code>{command}</code>
      <button type="button" className="button" onClick={() => copy(command)}>
        {result === "copied" ? <Check size={14} /> : <Copy size={14} />} {result === "copied" ? t.pane.hint.copied : t.pane.hint.copy}
      </button>
      {result === "failed" && <span role="alert">{t.pane.hint.copyFailed}</span>}
    </div>
  );
}

function InstallHintView({ hint }: { hint: InstallHint }) {
  const { t } = useI18n();
  if (hint.type === "failed") return <p>{t.pane.hint.failed(hint.program)}</p>;
  return (
    <div className="pane-hint">
      <p>{t.pane.hint.missing(hint.program, hint.remote)}</p>
      <InstallCommand command={hint.command} />
      <p>{t.pane.hint.installThenRestart}</p>
      {hint.remote && <p>{t.pane.hint.runOnServer}</p>}
    </div>
  );
}

export function Pane(props: PaneProps) {
  const { session, font, focused, maximized, covered, gridStyle, focusNonce, renaming, onRename, dispatch } = props;
  const { dragging, onDragPane, colors, away, mayReattach } = props;
  const { id } = session;
  const { t } = useI18n();
  // Where a dragged pane would land: a side (split there) or the center (swap).
  const [zone, setZone] = useState<DropZone | null>(null);
  const label = sessionLabel(session);
  const editing = renaming?.kind === "session" && renaming.id === id && renaming.where === "pane";
  // The AI mark opens the agent's info card; the card goes with the agent.
  const [cardOpen, setCardOpen] = useState(false);
  const mark = useRef<HTMLButtonElement>(null);
  const closeCard = useCallback(() => setCardOpen(false), []);
  const hasAgent = session.agent !== null;
  useEffect(() => {
    if (!hasAgent) setCardOpen(false);
  }, [hasAgent]);

  const handleEvent = (event: ConnectionEvent) => {
    switch (event.type) {
      case "ready":
        dispatch({ type: "ready", id, pid: event.pid, mode: event.mode });
        break;
      case "agent":
        dispatch({ type: "agent", id, agent: event.agent });
        break;
      case "exit":
        dispatch({ type: "status", id, status: "exited", detail: exitDetail(event), exitCode: event.signal ? null : event.code });
        break;
      case "error":
        dispatch({ type: "status", id, status: "error", detail: event.message });
        break;
      case "closed":
        if (event.takenOver) {
          dispatch({ type: "status", id, status: "error", notice: "takenOver" });
        } else {
          dispatch({ type: "disconnected", id, keepTrying: REMOTE_TARGET !== null });
        }
        break;
    }
  };

  // Reconnect to the same session with backoff: 0.5 s, 1 s, 2 s … up to 8 s.
  // The PTY host keeps the shell running meanwhile. A reconnect from a window
  // that is not in use would take every session from the one that is.
  // A remote window's pane also waits for its link and the server's layout.
  useEffect(() => {
    if (session.status !== "disconnected" || away || !mayReattach) return;
    const delay = Math.min(8000, 500 * 2 ** (session.retries - 1));
    const timer = setTimeout(() => dispatch({ type: "restart", id }), delay);
    return () => clearTimeout(timer);
  }, [session.status, session.retries, away, mayReattach, id, dispatch]);

  const ended = session.status === "exited" || session.status === "error";
  const hint =
    session.status === "exited" && session.kind !== "docs" ? installHint(session.kind, session.exitCode, REMOTE_TARGET !== null) : null;
  // "pid 7 · reconnected", "exit 1", or a server message, in the UI language.
  const detail = [session.detail, session.notice && t.notice[session.notice]].filter(Boolean).join(" · ") || null;
  const classes = [
    "pane",
    focused && "focused",
    maximized && "maximized",
    covered && "covered",
    dragging === id && "drag-source",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <section
      className={classes}
      style={gridStyle}
      aria-label={label}
      aria-hidden={covered || undefined}
      inert={covered || undefined}
      onMouseDown={() => !focused && dispatch({ type: "focus", id })}
      // Drop another pane on an edge to split this pane that way, or in the
      // middle to swap the two.
      onDragOver={(event) => {
        if (!carries(event, PANE_MIME) || dragging === id) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "move";
        const next = dropZone(event);
        if (next !== zone) setZone(next);
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setZone(null);
      }}
      onDrop={(event) => {
        const where = dropZone(event);
        setZone(null);
        const from = event.dataTransfer.getData(PANE_MIME);
        if (!from || from === id) return;
        event.preventDefault();
        if (where === "center") dispatch({ type: "swapPanes", id: from, withId: id });
        else dispatch({ type: "dockPane", id: from, targetId: id, side: where });
        onDragPane(null);
      }}
    >
      {zone && <div className={`drop-preview ${zone}`} aria-hidden="true" />}
      <header
        className="pane-header"
        draggable={!editing}
        title={t.pane.dragHint}
        onDragStart={(event) => {
          startDrag(event, PANE_MIME, id);
          onDragPane(id);
        }}
        onDragEnd={() => onDragPane(null)}
        onDoubleClick={() => dispatch({ type: "toggleMaximize", id })}
      >
        <GripVertical size={13} className="pane-grip" aria-hidden="true" />
        {session.agent ? (
          <button
            type="button"
            ref={mark}
            className="agent-button"
            aria-label={t.agent.card.open(t.agent.names[session.agent.kind])}
            aria-haspopup="dialog"
            aria-expanded={cardOpen}
            onClick={() => setCardOpen((open) => !open)}
            // The header's double-click maximizes; two presses here only toggle the card.
            onDoubleClick={(event) => event.stopPropagation()}
          >
            <AgentMark agent={session.agent} display={agentDisplay(session) ?? "idle"} size={14} detail={session.title} />
          </button>
        ) : (
          <KindIcon kind={session.kind} size={14} />
        )}
        {editing ? (
          <InlineRename
            value={session.name ?? ""}
            label={t.rename.sessionName}
            className="pane-rename"
            onCommit={(name) => {
              dispatch({ type: "renameSession", id, name });
              onRename(null);
            }}
            onCancel={() => onRename(null)}
          />
        ) : (
          <span
            className={`pane-title${session.name ? " custom" : ""}`}
            title={t.pane.renameHint(label)}
            onDoubleClick={(event) => {
              // The header's own double-click maximizes; the title renames.
              event.stopPropagation();
              onRename({ kind: "session", id, where: "pane" });
            }}
          >
            {label}
          </span>
        )}
        <span className="pane-kind">{t.kinds[session.kind].name}</span>
        <StatusDot status={session.status} />
        {detail && <span className="pane-detail">{detail}</span>}
        <span className="pane-spacer" />
        <button
          type="button"
          className="icon-button"
          aria-label={maximized ? t.pane.restore : t.pane.maximize}
          title={maximized ? t.pane.restore : t.pane.maximize}
          onClick={() => dispatch({ type: "toggleMaximize", id })}
        >
          {maximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t.sidebar.closeSession(label)}
          title={t.sidebar.closeSessionHint}
          onClick={() => dispatch({ type: "close", id })}
        >
          <X size={14} />
        </button>
      </header>
      {cardOpen && session.agent && (
        <AgentCard
          agent={session.agent}
          display={agentDisplay(session) ?? "idle"}
          resumeAllowed={props.resumeAgents}
          autoResume={session.autoResume}
          onAutoResume={(on) => dispatch({ type: "autoResume", id, on })}
          onClose={closeCard}
          anchor={mark}
        />
      )}
      <div className="pane-body">
        {session.kind === "docs" ? (
          <DocsView
            session={session}
            focused={focused && !covered}
            visible={!covered}
            focusNonce={focusNonce}
            followPid={props.followPid}
            followAlive={props.followAlive}
            followCandidate={props.followCandidate}
            dispatch={dispatch}
            onOpenUrl={props.onOpenUrl}
          />
        ) : (
        <TerminalView
          key={session.generation}
          kind={session.kind}
          url={session.url}
          sessionId={id}
          cwdFrom={session.cwdFrom}
          target={session.target}
          label={label}
          font={font}
          colors={colors}
          resumeAgents={props.resumeAgents && session.autoResume}
          focusNonce={focusNonce}
          focused={focused && !covered}
          visible={!covered}
          onEvent={handleEvent}
          onTitle={(title) => dispatch({ type: "title", id, title })}
          onFocus={() => !focused && dispatch({ type: "focus", id })}
        />
        )}
        {ended && (
          // A program that failed to start printed its own message: keep it readable.
          <div className={hint?.type === "failed" ? "pane-overlay beside-text" : "pane-overlay"} role="status">
            <p>
              {session.status === "exited" ? t.pane.exited : t.pane.failed}
              {detail ? ` (${detail})` : ""}
            </p>
            {hint && <InstallHintView hint={hint} />}
            <div className="pane-overlay-actions">
              <button type="button" className="button primary" onClick={() => dispatch({ type: "restart", id, manual: true })}>
                <RotateCcw size={14} /> {t.pane.restart}
              </button>
              <button type="button" className="button" onClick={() => dispatch({ type: "close", id })}>
                {t.pane.close}
              </button>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
