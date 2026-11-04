import { useState } from "react";
import { ChevronRight, Plus, X } from "lucide-react";
import { afterMidpoint, carries, insertionIndex, PANE_MIME, startDrag, TAB_MIME, WORKSPACE_MIME } from "../dnd.ts";
import { useI18n } from "../i18n/index.tsx";
import { KindIcon } from "../kinds.tsx";
import {
  activeTab,
  sessionLabel,
  tabLabel,
  tabSessions,
  visibleTabs,
  workspaceLabel,
  workspaceTabs,
  type Dashboard,
  type Session,
  type Tab,
  type Workspace,
} from "../workspace.ts";
import { InlineRename, type RenameTarget } from "./InlineRename.tsx";
import { StatusDot } from "./StatusDot.tsx";

interface SessionListProps {
  state: Dashboard;
  renaming: RenameTarget | null;
  onSelectTab: (id: string) => void;
  onCloseTab: (id: string) => void;
  onNewTab: () => void;
  onFocus: (id: string) => void;
  onClose: (id: string) => void;
  onRename: (target: RenameTarget | null) => void;
  onRenameTab: (id: string, name: string) => void;
  onRenameSession: (id: string, name: string) => void;
  onMoveTab: (id: string, toIndex: number) => void;
  /** A pane dropped on a tab row joins that tab. */
  onMovePane: (paneId: string, tabId: string) => void;
  /** A pane dropped on another session row swaps places with it. */
  onSwapPanes: (paneId: string, withId: string) => void;
  onDragPane: (id: string | null) => void;
  onNewWorkspace: () => void;
  onSelectWorkspace: (id: string) => void;
  onCloseWorkspace: (id: string) => void;
  onRenameWorkspace: (id: string, name: string) => void;
  onMoveWorkspace: (id: string, toIndex: number) => void;
  /** A tab dropped on a workspace row joins that workspace. */
  onMoveTabToWorkspace: (tabId: string, workspaceId: string) => void;
  /** Icons only: tab numbers and session icons, names in tooltips. */
  compact: boolean;
}

type Place = "before" | "after" | "into";

/** Drop handling shared by tab rows and session rows. */
function useDropMark() {
  const [place, setPlace] = useState<Place | null>(null);
  const leave = (event: React.DragEvent) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setPlace(null);
  };
  const mark = (next: Place) => {
    if (place !== next) setPlace(next);
  };
  return { place, setPlace, leave, mark };
}

/**
 * The workspaces, then the tree of the workspace in view: each tab is a
 * parent row with its panes indented below it, in grid order. Double-click or
 * F2 renames a workspace, a tab, or a session.
 */
export function SessionList(props: SessionListProps) {
  const { state } = props;
  const { t } = useI18n();
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const tabs = visibleTabs(state);
  const activeId = activeTab(state).id;
  const sessionCount = tabs.reduce((count, tab) => count + tab.paneIds.length, 0);

  const toggle = (id: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <aside className={`session-panel${props.compact ? " compact" : ""}`} aria-label={t.sidebar.title}>
      <WorkspaceList {...props} />
      <header className="session-panel-header">
        {!props.compact && (
          <>
            <span>{t.sidebar.tabs}</span>
            <span className="session-counts">
              <span title={t.sidebar.tabs}>{tabs.length}</span>
              <span aria-hidden="true">·</span>
              <span title={t.sidebar.sessions}>{sessionCount}</span>
            </span>
          </>
        )}
        <button
          type="button"
          className="icon-button"
          aria-label={t.sidebar.newTab}
          title={t.sidebar.newTab}
          onClick={props.onNewTab}
        >
          <Plus size={14} />
        </button>
      </header>
      <ul className="tree" role="tree" aria-label={t.sidebar.tree}>
        {tabs.map((tab, index) => (
          <TabNode
            key={tab.id}
            {...props}
            tab={tab}
            tabs={tabs}
            index={index}
            active={tab.id === activeId}
            // Icons only: just the tab in view lists its sessions.
            expanded={props.compact ? tab.id === activeId : !collapsed.has(tab.id)}
            onToggle={() => toggle(tab.id)}
          />
        ))}
      </ul>
    </aside>
  );
}

/** The workspaces: a click shows one, a drop moves a tab or pane into one. */
function WorkspaceList(props: SessionListProps) {
  const { state } = props;
  const { t } = useI18n();
  return (
    <section className="workspace-list" aria-label={t.workspaces.list}>
      <header className="session-panel-header">
        {!props.compact && <span className="workspace-list-title">{t.workspaces.list}</span>}
        <button
          type="button"
          className="icon-button"
          aria-label={t.workspaces.newWorkspace}
          title={t.workspaces.newWorkspace}
          onClick={props.onNewWorkspace}
        >
          <Plus size={14} />
        </button>
      </header>
      <ul className="tree">
        {state.workspaces.map((workspace, index) => (
          <WorkspaceRow key={workspace.id} {...props} workspace={workspace} index={index} />
        ))}
      </ul>
    </section>
  );
}

interface WorkspaceRowProps extends SessionListProps {
  workspace: Workspace;
  index: number;
}

function WorkspaceRow(props: WorkspaceRowProps) {
  const { state, workspace, index, renaming } = props;
  const { t } = useI18n();
  const active = workspace.id === state.activeWorkspaceId;
  const label = workspaceLabel(state, workspace, t.workspaces.untitled);
  const sessions = workspaceTabs(state, workspace.id).reduce((count, tab) => count + tab.paneIds.length, 0);
  const editing = renaming?.kind === "workspace" && renaming.id === workspace.id;
  const startRename = () => props.onRename({ kind: "workspace", id: workspace.id, where: "sidebar" });
  const select = () => props.onSelectWorkspace(workspace.id);
  const drop = useDropMark();

  return (
    <li aria-current={active ? "true" : undefined}>
      <div
        className={`tree-row tree-workspace${active ? " active" : ""}${drop.place ? ` drop-${drop.place}` : ""}`}
        draggable={!editing}
        onDragStart={(event) => startDrag(event, WORKSPACE_MIME, workspace.id)}
        onDragOver={(event) => {
          // Another workspace: reorder. A tab or a pane: move it in.
          if (carries(event, WORKSPACE_MIME)) drop.mark(afterMidpoint(event, "y") ? "after" : "before");
          else if (carries(event, TAB_MIME) || carries(event, PANE_MIME)) drop.mark("into");
          else return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        }}
        onDragLeave={drop.leave}
        onDrop={(event) => {
          drop.setPlace(null);
          const moving = event.dataTransfer.getData(WORKSPACE_MIME);
          const tab = event.dataTransfer.getData(TAB_MIME);
          const pane = event.dataTransfer.getData(PANE_MIME);
          if (moving) {
            event.preventDefault();
            const from = state.workspaces.findIndex((item) => item.id === moving);
            if (moving !== workspace.id) props.onMoveWorkspace(moving, insertionIndex(from, index, afterMidpoint(event, "y")));
          } else if (tab) {
            event.preventDefault();
            props.onMoveTabToWorkspace(tab, workspace.id);
          } else if (pane) {
            // Into the tab the workspace shows.
            event.preventDefault();
            props.onMovePane(pane, workspace.activeTabId);
          }
          props.onDragPane(null);
        }}
      >
        {props.compact ? (
          <button
            type="button"
            className="tree-select"
            title={`${index + 1} · ${label}`}
            aria-label={label}
            onClick={select}
          >
            <span className="workspace-index">{index + 1}</span>
          </button>
        ) : (
          <>
            <span className="workspace-index">{index + 1}</span>
            {editing ? (
              <InlineRename
                value={workspace.name ?? ""}
                label={t.rename.workspaceName}
                className="tree-rename"
                onCommit={(name) => {
                  props.onRenameWorkspace(workspace.id, name);
                  props.onRename(null);
                }}
                onCancel={() => props.onRename(null)}
              />
            ) : (
              <button
                type="button"
                className="tree-select"
                title={t.sidebar.renameHint(label)}
                onClick={select}
                onDoubleClick={startRename}
                onKeyDown={(event) => event.key === "F2" && startRename()}
              >
                <span className={`tree-label${workspace.name ? " custom" : ""}`}>{label}</span>
              </button>
            )}
            <span className="tree-count" title={t.sidebar.sessionCount(sessions)}>
              {sessions}
            </span>
            <button
              type="button"
              className="icon-button subtle"
              aria-label={t.workspaces.close(label)}
              title={t.workspaces.closeHint}
              onClick={() => props.onCloseWorkspace(workspace.id)}
            >
              <X size={13} />
            </button>
          </>
        )}
      </div>
    </li>
  );
}

interface TabNodeProps extends SessionListProps {
  tab: Tab;
  /** The tabs of the workspace in view, which `index` counts in. */
  tabs: Tab[];
  index: number;
  active: boolean;
  expanded: boolean;
  onToggle: () => void;
}

function TabNode(props: TabNodeProps) {
  const { state, tab, index, active, expanded, renaming } = props;
  const { t } = useI18n();
  const sessions = tabSessions(state, tab.id);
  const label = tabLabel(state, tab, t.tabs.untitled);
  const editing = renaming?.kind === "tab" && renaming.id === tab.id && renaming.where === "sidebar";
  const startRename = () => props.onRename({ kind: "tab", id: tab.id, where: "sidebar" });
  const drop = useDropMark();

  return (
    <li role="treeitem" aria-expanded={expanded} aria-selected={active} className="tree-tab-node">
      <div
        className={`tree-row tree-tab${active ? " active" : ""}${drop.place ? ` drop-${drop.place}` : ""}`}
        draggable={!editing}
        onDragStart={(event) => startDrag(event, TAB_MIME, tab.id)}
        onDragOver={(event) => {
          // Another tab: reorder. A pane: move it to the end of this tab.
          if (carries(event, TAB_MIME)) drop.mark(afterMidpoint(event, "y") ? "after" : "before");
          else if (carries(event, PANE_MIME)) drop.mark("into");
          else return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
        }}
        onDragLeave={drop.leave}
        onDrop={(event) => {
          drop.setPlace(null);
          const movingTab = event.dataTransfer.getData(TAB_MIME);
          const pane = event.dataTransfer.getData(PANE_MIME);
          if (movingTab && movingTab !== tab.id) {
            event.preventDefault();
            const from = props.tabs.findIndex((item) => item.id === movingTab);
            props.onMoveTab(movingTab, insertionIndex(from, index, afterMidpoint(event, "y")));
          } else if (pane) {
            event.preventDefault();
            props.onMovePane(pane, tab.id);
          }
          props.onDragPane(null);
        }}
      >
        {props.compact ? (
          <button
            type="button"
            className="tree-select"
            title={`${index + 1} · ${label}`}
            aria-label={label}
            onClick={() => props.onSelectTab(tab.id)}
          >
            <span className="tab-index">{index + 1}</span>
          </button>
        ) : (
          <>
            <button
              type="button"
              className={`tree-chevron${expanded ? " expanded" : ""}`}
              aria-label={expanded ? t.sidebar.collapse : t.sidebar.expand}
              onClick={props.onToggle}
            >
              <ChevronRight size={13} />
            </button>
            <span className="tab-index">{index + 1}</span>
            {editing ? (
              <InlineRename
                value={tab.name ?? ""}
                label={t.rename.tabName}
                className="tree-rename"
                onCommit={(name) => {
                  props.onRenameTab(tab.id, name);
                  props.onRename(null);
                }}
                onCancel={() => props.onRename(null)}
              />
            ) : (
              <button
                type="button"
                className="tree-select"
                title={t.sidebar.renameHint(label)}
                onClick={() => props.onSelectTab(tab.id)}
                onDoubleClick={startRename}
                onKeyDown={(event) => event.key === "F2" && startRename()}
              >
                <span className={`tree-label${tab.name ? " custom" : ""}`}>{label}</span>
              </button>
            )}
            <span className="tree-count" title={t.sidebar.sessionCount(sessions.length)}>
              {sessions.length}
            </span>
            <button
              type="button"
              className="icon-button subtle"
              aria-label={t.sidebar.closeTab(label)}
              title={t.sidebar.closeTabHint}
              onClick={() => props.onCloseTab(tab.id)}
            >
              <X size={13} />
            </button>
          </>
        )}
      </div>
      {expanded && (
        <ul className="tree-children" role="group">
          {sessions.length === 0 ? (
            <li className="tree-empty">{t.sidebar.empty}</li>
          ) : (
            sessions.map((session) => (
              <SessionNode
                key={session.id}
                {...props}
                session={session}
                current={active && session.id === tab.focusedId}
              />
            ))
          )}
        </ul>
      )}
    </li>
  );
}

interface SessionNodeProps extends SessionListProps {
  session: Session;
  current: boolean;
}

function SessionNode(props: SessionNodeProps) {
  const { session, current, renaming, onFocus, onClose, onRename, onRenameSession } = props;
  const { t } = useI18n();
  const label = sessionLabel(session);
  const editing = renaming?.kind === "session" && renaming.id === session.id && renaming.where === "sidebar";
  const startRename = () => onRename({ kind: "session", id: session.id, where: "sidebar" });
  const drop = useDropMark();

  return (
    <li role="treeitem" aria-selected={current}>
      <div
        className={`tree-row tree-session${current ? " active" : ""}${drop.place ? ` drop-${drop.place}` : ""}`}
        draggable={!editing}
        onDragStart={(event) => {
          startDrag(event, PANE_MIME, session.id);
          props.onDragPane(session.id);
        }}
        onDragEnd={() => props.onDragPane(null)}
        onDragOver={(event) => {
          if (!carries(event, PANE_MIME)) return;
          event.preventDefault();
          event.dataTransfer.dropEffect = "move";
          drop.mark("into");
        }}
        onDragLeave={drop.leave}
        onDrop={(event) => {
          drop.setPlace(null);
          const pane = event.dataTransfer.getData(PANE_MIME);
          if (!pane || pane === session.id) return;
          event.preventDefault();
          props.onSwapPanes(pane, session.id);
          props.onDragPane(null);
        }}
      >
        {props.compact ? (
          <button
            type="button"
            className="tree-select"
            title={label}
            aria-label={label}
            onClick={() => onFocus(session.id)}
          >
            <KindIcon kind={session.kind} size={15} />
            <StatusDot status={session.status} />
          </button>
        ) : (
          <>
            <KindIcon kind={session.kind} size={14} />
            {editing ? (
              <InlineRename
                value={session.name ?? ""}
                label={t.rename.sessionName}
                className="tree-rename"
                onCommit={(name) => {
                  onRenameSession(session.id, name);
                  onRename(null);
                }}
                onCancel={() => onRename(null)}
              />
            ) : (
              <button
                type="button"
                className="tree-select"
                title={t.sidebar.renameHint(label)}
                onClick={() => onFocus(session.id)}
                onDoubleClick={startRename}
                onKeyDown={(event) => event.key === "F2" && startRename()}
              >
                <span className={`tree-label${session.name ? " custom" : ""}`}>{label}</span>
              </button>
            )}
            <StatusDot status={session.status} />
            <button
              type="button"
              className="icon-button subtle"
              aria-label={t.sidebar.closeSession(label)}
              title={t.sidebar.closeSessionHint}
              onClick={() => onClose(session.id)}
            >
              <X size={13} />
            </button>
          </>
        )}
      </div>
    </li>
  );
}
