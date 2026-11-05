import { useCallback, useRef } from "react";
import { rects, type Rect } from "../../shared/layout.ts";
import type { SessionKind } from "../../shared/protocol.ts";
import type { TerminalThemeColors } from "@wterm/core";
import { useI18n } from "../i18n/index.tsx";
import { KindIcon, LAUNCHERS } from "../kinds.tsx";
import type { TerminalFont } from "../terminal/fonts.ts";
import { activeTab, sessionTab, takenOver, type Dashboard, type DashboardAction } from "../workspace.ts";
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
  onLaunch: (kind: SessionKind) => void;
  dispatch: (action: DashboardAction) => void;
}

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
  const tabRects = new Map(state.tabs.map((tab) => [tab.id, rects(tab.layout)]));
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
            onDragPane={props.onDragPane}
            dispatch={props.dispatch}
          />
        );
      })}
      {current.layout && current.paneIds.length > 1 && current.maximizedId === null && (
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
