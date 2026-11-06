import { useCallback, useRef } from "react";
import { rects, stackRects, type Rect } from "../../shared/layout.ts";
import type { PaneKind } from "../../shared/protocol.ts";
import type { TerminalThemeColors } from "@wterm/core";
import { useI18n } from "../i18n/index.tsx";
import { KindIcon, LAUNCHERS } from "../kinds.tsx";
import type { TerminalFont } from "../terminal/fonts.ts";
import { activeTab, sessionTab, tabSessions, takenOver, type Dashboard, type DashboardAction, type Session } from "../workspace.ts";
import type { RenameTarget } from "./InlineRename.tsx";
import { Pane } from "./Pane.tsx";
import { SplitDividers } from "./SplitDividers.tsx";

interface PaneLayerProps {
  state: Dashboard;
  font: TerminalFont;
  colors: TerminalThemeColors;
  focusNonce: number;
  renaming: RenameTarget | null;
  dragging: string | null;
  onDragPane: (id: string | null) => void;
  onRename: (target: RenameTarget | null) => void;
  onLaunch: (kind: PaneKind) => void;
  /** Opens a web page in a terminal-browser pane (a link in a docs pane). */
  onOpenUrl: (url: string) => void;
  /** Disconnected panes may connect again (see `mayReattach`). */
  mayReattach: boolean;
  /** Phone-wide: panes stack top to bottom, without split dividers. */
  narrow: boolean;
  dispatch: (action: DashboardAction) => void;
}

/**
 * For a docs pane: the shell it follows (its pid while running; whether it
 * still exists) and the shell it may start to follow, the first running
 * shell of its tab.
 */
function followInfo(state: Dashboard, session: Session, tabId: string) {
  const followed = state.sessions.find((item) => item.id === session.docs?.followFrom);
  const candidate = tabSessions(state, tabId).find((item) => item.kind === "shell" && item.status === "running");
  return {
    followPid: followed?.status === "running" ? followed.pid : null,
    followAlive: followed !== undefined,
    followCandidate: candidate?.id ?? null,
  };
}

const NO_FOLLOW = { followPid: null, followAlive: false, followCandidate: null };

/** Matches .pane-layer padding: the space split fractions refer to. */
const LAYER_PADDING = 4;

/**
 * Every pane of every tab, in one layer, in creation order. A pane's DOM node
 * never moves: switching tabs, docking, swapping, and moving a pane to another
 * tab only change its CSS rectangle. Remounting would destroy the terminal and
 * end its shell. Panes of inactive tabs keep their size, so their PTYs are not
 * resized while hidden.
 */
export function PaneLayer(props: PaneLayerProps) {
  const { state, dragging } = props;
  const { t } = useI18n();
  const current = activeTab(state);
  const layerRef = useRef<HTMLDivElement>(null);
  const tabRects = new Map(
    state.tabs.map((tab) => [tab.id, props.narrow ? stackRects(tab.paneIds) : rects(tab.layout)]),
  );
  const away = takenOver(state);

  const inner = useCallback(() => {
    const rect = layerRef.current?.getBoundingClientRect();
    if (!rect) return { width: 0, height: 0 };
    return { width: rect.width - 2 * LAYER_PADDING, height: rect.height - 2 * LAYER_PADDING };
  }, []);

  return (
    <div
      ref={layerRef}
      className={`pane-layer${dragging ? " dragging" : ""}`}
      role="tabpanel"
      aria-labelledby={`tab-${current.id}`}
    >
      {state.sessions.map((session) => {
        const tab = sessionTab(state, session.id);
        const rect: Rect | undefined = tab ? tabRects.get(tab.id)?.get(session.id) : undefined;
        if (!tab || !rect) return null;
        const active = tab.id === current.id;
        const maximized = tab.maximizedId === session.id;
        const cellStyle = { "--x": rect.x, "--y": rect.y, "--w": rect.w, "--h": rect.h } as React.CSSProperties;
        return (
          <Pane
            key={session.id}
            session={session}
            font={props.font}
            colors={props.colors}
            focused={active && session.id === tab.focusedId}
            maximized={maximized}
            covered={!active || (tab.maximizedId !== null && !maximized)}
            gridStyle={cellStyle}
            focusNonce={props.focusNonce}
            renaming={props.renaming}
            onRename={props.onRename}
            dragging={dragging}
            away={away}
            mayReattach={props.mayReattach}
            onDragPane={props.onDragPane}
            dispatch={props.dispatch}
            onOpenUrl={props.onOpenUrl}
            {...(session.kind === "docs" ? followInfo(state, session, tab.id) : NO_FOLLOW)}
          />
        );
      })}
      {!props.narrow && current.layout && current.paneIds.length > 1 && current.maximizedId === null && (
        <div className="split-dividers">
          <SplitDividers
            layout={current.layout}
            inner={inner}
            onResize={(path, sizes) => props.dispatch({ type: "setSplitSizes", tabId: current.id, path, sizes })}
          />
        </div>
      )}
      {current.paneIds.length === 0 && (
        <div className="workspace-empty layer-empty">
          <p>{t.pane.emptyTab}</p>
          <div className="workspace-empty-actions">
            {LAUNCHERS.map((kind) => (
              <button key={kind} type="button" className="button" onClick={() => props.onLaunch(kind)}>
                <KindIcon kind={kind} size={15} /> {t.kinds[kind].label}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
