import { useEffect, useState } from "react";
import { GripVertical, Maximize2, Minimize2, RotateCcw, X } from "lucide-react";
import { carries, dropZone, PANE_MIME, startDrag, type DropZone } from "../dnd.ts";
import { useI18n } from "../i18n/index.tsx";
import { KindIcon } from "../kinds.tsx";
import type { ConnectionEvent } from "../terminal/connection.ts";
import type { TerminalFont } from "../terminal/fonts.ts";
import type { ColorScheme } from "../theme.ts";
import { TerminalView } from "../terminal/TerminalView.tsx";
import { sessionLabel, type Session, type WorkspaceAction } from "../workspace.ts";
import { InlineRename, type RenameTarget } from "./InlineRename.tsx";
import { StatusDot } from "./StatusDot.tsx";

interface PaneProps {
  session: Session;
  font: TerminalFont;
  scheme: ColorScheme;
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
  onDragPane: (id: string | null) => void;
  dispatch: (action: WorkspaceAction) => void;
}

function exitDetail(event: Extract<ConnectionEvent, { type: "exit" }>): string {
  if (event.signal) return `signal ${event.signal}`;
  return `exit ${event.code}`;
}

export function Pane(props: PaneProps) {
  const { session, font, focused, maximized, covered, gridStyle, focusNonce, renaming, onRename, dispatch } = props;
  const { dragging, onDragPane, scheme, away } = props;
  const { id } = session;
  const { t } = useI18n();
  // Where a dragged pane would land: a side (split there) or the center (swap).
  const [zone, setZone] = useState<DropZone | null>(null);
  const label = sessionLabel(session);
  const editing = renaming?.kind === "session" && renaming.id === id && renaming.where === "pane";

  const handleEvent = (event: ConnectionEvent) => {
    switch (event.type) {
      case "ready":
        dispatch({ type: "ready", id, pid: event.pid, mode: event.mode });
        break;
      case "exit":
        dispatch({ type: "status", id, status: "exited", detail: exitDetail(event) });
        break;
      case "error":
        dispatch({ type: "status", id, status: "error", detail: event.message });
        break;
      case "closed":
        if (event.takenOver) {
          dispatch({ type: "status", id, status: "error", notice: "takenOver" });
        } else {
          dispatch({ type: "disconnected", id });
        }
        break;
    }
  };

  // Reconnect to the same session with backoff: 0.5 s, 1 s, 2 s … up to 8 s.
  // The PTY host keeps the shell running meanwhile. A reconnect from a window
  // that is not in use would take every session from the one that is.
  useEffect(() => {
    if (session.status !== "disconnected" || away) return;
    const delay = Math.min(8000, 500 * 2 ** (session.retries - 1));
    const timer = setTimeout(() => dispatch({ type: "restart", id }), delay);
    return () => clearTimeout(timer);
  }, [session.status, session.retries, away, id, dispatch]);

  const ended = session.status === "exited" || session.status === "error";
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
        <KindIcon kind={session.kind} size={14} />
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
      <div className="pane-body">
        <TerminalView
          key={session.generation}
          kind={session.kind}
          url={session.url}
          sessionId={id}
          cwdFrom={session.cwdFrom}
          target={session.target}
          label={label}
          font={font}
          scheme={scheme}
          focusNonce={focusNonce}
          focused={focused && !covered}
          visible={!covered}
          onEvent={handleEvent}
          onTitle={(title) => dispatch({ type: "title", id, title })}
          onFocus={() => !focused && dispatch({ type: "focus", id })}
        />
        {ended && (
          <div className="pane-overlay" role="status">
            <p>
              {session.status === "exited" ? t.pane.exited : t.pane.failed}
              {detail ? ` (${detail})` : ""}
            </p>
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
