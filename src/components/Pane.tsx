import { useEffect, useState } from "react";
import { GripVertical, Maximize2, Minimize2, RotateCcw, X } from "lucide-react";
import { carries, dropZone, PANE_MIME, startDrag, type DropZone } from "../dnd.ts";
import { KindIcon } from "../kinds.tsx";
import type { ConnectionEvent } from "../terminal/connection.ts";
import type { TerminalFont } from "../terminal/fonts.ts";
import type { ColorScheme } from "../theme.ts";
import { TerminalView } from "../terminal/TerminalView.tsx";
import { KIND_LABEL, sessionLabel, type Session, type WorkspaceAction } from "../workspace.ts";
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
  onDragPane: (id: string | null) => void;
  dispatch: (action: WorkspaceAction) => void;
}

function exitDetail(event: Extract<ConnectionEvent, { type: "exit" }>): string {
  if (event.signal) return `signal ${event.signal}`;
  return `exit ${event.code}`;
}

export function Pane(props: PaneProps) {
  const { session, font, focused, maximized, covered, gridStyle, focusNonce, renaming, onRename, dispatch } = props;
  const { dragging, onDragPane, scheme } = props;
  const { id } = session;
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
          dispatch({ type: "status", id, status: "error", detail: "다른 창에서 이 세션을 열었습니다" });
        } else {
          dispatch({ type: "disconnected", id });
        }
        break;
    }
  };

  // Reconnect to the same session with backoff: 0.5 s, 1 s, 2 s … up to 8 s.
  // The PTY host keeps the shell running meanwhile.
  useEffect(() => {
    if (session.status !== "disconnected") return;
    const delay = Math.min(8000, 500 * 2 ** (session.retries - 1));
    const timer = setTimeout(() => dispatch({ type: "restart", id }), delay);
    return () => clearTimeout(timer);
  }, [session.status, session.retries, id, dispatch]);

  const ended = session.status === "exited" || session.status === "error";
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
        title="끌어서 이동: 다른 패널 가장자리에 놓으면 그쪽으로 분할, 가운데는 자리 바꾸기, 탭 위는 그 탭으로"
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
            label="Session name"
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
            title={`${label} — 더블클릭하여 이름 변경`}
            onDoubleClick={(event) => {
              // The header's own double-click maximizes; the title renames.
              event.stopPropagation();
              onRename({ kind: "session", id, where: "pane" });
            }}
          >
            {label}
          </span>
        )}
        <span className="pane-kind">{KIND_LABEL[session.kind]}</span>
        <StatusDot status={session.status} />
        {session.detail && <span className="pane-detail">{session.detail}</span>}
        <span className="pane-spacer" />
        <button
          type="button"
          className="icon-button"
          aria-label={maximized ? "Restore panes" : "Maximize pane"}
          title={maximized ? "그리드로 돌아가기" : "패널 최대화"}
          onClick={() => dispatch({ type: "toggleMaximize", id })}
        >
          {maximized ? <Minimize2 size={14} /> : <Maximize2 size={14} />}
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={`Close ${label}`}
          title="세션 닫기"
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
              {session.status === "exited" ? "프로세스가 종료되었습니다" : "세션 오류"}
              {session.detail ? ` (${session.detail})` : ""}
            </p>
            <div className="pane-overlay-actions">
              <button type="button" className="button primary" onClick={() => dispatch({ type: "restart", id, manual: true })}>
                <RotateCcw size={14} /> 다시 시작
              </button>
              <button type="button" className="button" onClick={() => dispatch({ type: "close", id })}>
                닫기
              </button>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
