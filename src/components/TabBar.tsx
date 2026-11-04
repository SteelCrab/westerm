import { useState } from "react";
import { Plus, X } from "lucide-react";
import { afterMidpoint, carries, insertionIndex, PANE_MIME, startDrag, TAB_MIME } from "../dnd.ts";
import { useI18n } from "../i18n/index.tsx";
import { KindIcon } from "../kinds.tsx";
import { tabLabel, tabLead, tabSessions, type Workspace } from "../workspace.ts";
import { InlineRename, type RenameTarget } from "./InlineRename.tsx";

interface TabBarProps {
  state: Workspace;
  renaming: RenameTarget | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onNew: () => void;
  onRename: (target: RenameTarget | null) => void;
  onRenameTab: (id: string, name: string) => void;
  onMoveTab: (id: string, toIndex: number) => void;
  /** A pane dropped on a tab joins the end of that tab. */
  onMovePane: (paneId: string, tabId: string) => void;
  onMovePaneToNewTab: (paneId: string) => void;
  onDragEnd: () => void;
}

type DropMark = { tabId: string; place: "before" | "after" | "into" } | null;

/**
 * Dashboard tabs. Each tab is a pane grid; its label follows its focused pane
 * unless the user names it (double-click or F2). Chrome keeps Cmd+T for
 * itself, so new tabs come from the + button or a double-click on the bar.
 */
export function TabBar(props: TabBarProps) {
  const { state, renaming, onSelect, onClose, onNew, onRename, onRenameTab } = props;
  const { t } = useI18n();
  const [mark, setMark] = useState<DropMark>(null);
  const [newTabTarget, setNewTabTarget] = useState(false);

  const dragOver = (event: React.DragEvent, tabId: string) => {
    let place: NonNullable<DropMark>["place"];
    if (carries(event, TAB_MIME)) place = afterMidpoint(event, "x") ? "after" : "before";
    else if (carries(event, PANE_MIME)) place = "into";
    else return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    if (mark?.tabId !== tabId || mark.place !== place) setMark({ tabId, place });
  };

  const drop = (event: React.DragEvent, tabId: string, index: number) => {
    setMark(null);
    const tab = event.dataTransfer.getData(TAB_MIME);
    const pane = event.dataTransfer.getData(PANE_MIME);
    if (tab) {
      event.preventDefault();
      const from = state.tabs.findIndex((item) => item.id === tab);
      if (tab !== tabId) props.onMoveTab(tab, insertionIndex(from, index, afterMidpoint(event, "x")));
    } else if (pane) {
      event.preventDefault();
      props.onMovePane(pane, tabId);
    }
    props.onDragEnd();
  };

  return (
    <div
      className="tab-bar"
      onDoubleClick={(event) => {
        if (event.target === event.currentTarget) onNew();
      }}
    >
      <div className="tab-list" role="tablist" aria-label={t.tabs.list}>
        {state.tabs.map((tab, index) => {
          const lead = tabLead(state, tab);
          const count = tabSessions(state, tab.id).length;
          const active = tab.id === state.activeTabId;
          const label = tabLabel(state, tab, t.tabs.untitled);
          const editing = renaming?.kind === "tab" && renaming.id === tab.id && renaming.where === "tabbar";
          const startRename = () => onRename({ kind: "tab", id: tab.id, where: "tabbar" });
          return (
            <div
              key={tab.id}
              className={[
                "tab",
                active && "active",
                editing && "editing",
                mark?.tabId === tab.id && `drop-${mark.place}`,
              ]
                .filter(Boolean)
                .join(" ")}
              draggable={!editing}
              onDragStart={(event) => startDrag(event, TAB_MIME, tab.id)}
              onDragEnd={() => {
                setMark(null);
                props.onDragEnd();
              }}
              onDragOver={(event) => dragOver(event, tab.id)}
              onDragLeave={(event) => {
                if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setMark(null);
              }}
              onDrop={(event) => drop(event, tab.id, index)}
              onAuxClick={(event) => {
                // Middle click closes, as in browsers.
                if (event.button === 1) onClose(tab.id);
              }}
            >
              {editing ? (
                <InlineRename
                  value={tab.name ?? ""}
                  label={t.rename.tabName}
                  className="tab-rename"
                  onCommit={(name) => {
                    onRenameTab(tab.id, name);
                    onRename(null);
                  }}
                  onCancel={() => onRename(null)}
                />
              ) : (
                <button
                  type="button"
                  role="tab"
                  id={`tab-${tab.id}`}
                  aria-selected={active}
                  aria-controls={`tabpanel-${tab.id}`}
                  className="tab-select"
                  title={t.tabs.tooltip(label, count)}
                  onClick={() => onSelect(tab.id)}
                  onDoubleClick={startRename}
                  onKeyDown={(event) => event.key === "F2" && startRename()}
                >
                  {lead && !tab.name ? (
                    <KindIcon kind={lead.kind} size={13} />
                  ) : (
                    <span className="tab-index">{index + 1}</span>
                  )}
                  <span className="tab-title">{label}</span>
                  {count > 1 && <span className="tab-count">{count}</span>}
                </button>
              )}
              <button
                type="button"
                className="tab-close"
                aria-label={t.sidebar.closeTab(label)}
                title={t.sidebar.closeTabHint}
                onClick={() => onClose(tab.id)}
              >
                <X size={12} />
              </button>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className={`tab-new${newTabTarget ? " drop-into" : ""}`}
        aria-label={t.tabs.newTab}
        title={t.tabs.newTabHint}
        onClick={onNew}
        onDragOver={(event) => {
          if (!carries(event, PANE_MIME)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          setNewTabTarget(true);
        }}
        onDragLeave={() => setNewTabTarget(false)}
        onDrop={(event) => {
          setNewTabTarget(false);
          const pane = event.dataTransfer.getData(PANE_MIME);
          if (!pane) return;
          event.preventDefault();
          props.onMovePaneToNewTab(pane);
          props.onDragEnd();
        }}
      >
        <Plus size={14} />
      </button>
    </div>
  );
}
