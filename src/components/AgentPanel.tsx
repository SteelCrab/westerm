import { useEffect, useState } from "react";
import { Check, ChevronRight, Folder, GitBranch, History } from "lucide-react";
import type { AgentKind, AgentLaunch } from "../../shared/protocol.ts";
import type { WorkspaceColor, WorkspaceIcon } from "../../shared/workspace-file.ts";
import {
  agentFolders,
  elsewhereWaiting,
  fetchHistory,
  historyCache,
  pastConversations,
  type AgentFolder,
  type HistoryCache,
  type HistoryReader,
  type PastConversation,
} from "../agents.ts";
import { AgentIcon } from "../agentIcons.tsx";
import { useI18n } from "../i18n/index.tsx";
import { HTML_LANG } from "../i18n/messages.ts";
import { minutesSince } from "../uiHelpers.ts";
import { activeTab, agentDisplay, sessionLabel, type AgentDisplay, type Dashboard, type Session } from "../workspace.ts";
import { useNow } from "./AgentCard.tsx";
import { AgentMark } from "./AgentMark.tsx";
import type { RenameTarget } from "./InlineRename.tsx";
import { WorkspaceMenu } from "./WorkspaceMenu.tsx";

const AGENT_KINDS: readonly AgentKind[] = ["claude", "codex", "gemini"];
/** A folder shows up to this many past conversations. The server lists more, so some are left after the running ones go. */
const PAST_MAX = 5;

interface AgentPanelProps {
  state: Dashboard;
  renaming: RenameTarget | null;
  onRename: (target: RenameTarget | null) => void;
  onSelectWorkspace: (id: string) => void;
  onNewWorkspace: () => void;
  onCloseWorkspace: (id: string) => void;
  onRenameWorkspace: (id: string, name: string) => void;
  onSetWorkspaceIcon: (id: string, icon: WorkspaceIcon) => void;
  onSetWorkspaceColor: (id: string, color: WorkspaceColor) => void;
  /** Shows the agent's pane. */
  onFocus: (id: string) => void;
  /** Opens a new shell in `folder` that starts or resumes an agent. */
  onStart: (folder: string, launch: AgentLaunch) => void;
  /** Icons only: folder icons and agent marks, names in tooltips. */
  compact: boolean;
  /** Reads a folder's past conversations; tests pass their own. */
  history?: HistoryReader;
}

/**
 * The agents view of the sidebar slot: the workspace menu, then the
 * workspace's shells grouped by folder. Each folder lists its agents, start
 * buttons, and the conversations that ended there.
 */
export function AgentPanel(props: AgentPanelProps) {
  const { state, compact } = props;
  const { t } = useI18n();
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const now = useNow(15_000);
  const folders = agentFolders(state);
  const elsewhere = elsewhereWaiting(state);
  const agentCount = folders.reduce((count, folder) => count + folder.agents.length, 0);

  const toggle = (key: string) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <aside className={`session-panel agents-panel${compact ? " compact" : ""}`} aria-label={t.agents.title}>
      <div className="sidebar-workspace">
        <WorkspaceMenu
          where="sidebar"
          labelled={!compact}
          state={state}
          renaming={props.renaming}
          onRename={props.onRename}
          onSelect={props.onSelectWorkspace}
          onNew={props.onNewWorkspace}
          onClose={props.onCloseWorkspace}
          onRenameWorkspace={props.onRenameWorkspace}
          onSetIcon={props.onSetWorkspaceIcon}
          onSetColor={props.onSetWorkspaceColor}
        />
        {!compact && elsewhere > 0 && (
          <span className="agents-elsewhere" role="img" aria-label={t.agents.elsewhereHint(elsewhere)} title={t.agents.elsewhereHint(elsewhere)}>
            <span className="agents-dot" />
            {t.agents.elsewhere(elsewhere)}
          </span>
        )}
      </div>
      {!compact && (
        <header className="agents-header">
          <span>{t.agents.folders}</span>
          <span className="agents-header-count">{folders.length}</span>
          <span className="agents-header-agents">{t.agents.agentCount(agentCount)}</span>
        </header>
      )}
      {folders.length === 0 ? (
        <p className="agents-empty">{!compact && t.agents.empty}</p>
      ) : (
        <ul className="tree" role="tree" aria-label={t.agents.tree}>
          {folders.map((folder) => (
            <FolderNode
              key={folder.key}
              {...props}
              cache={historyCache(props.history ?? fetchHistory)}
              folder={folder}
              now={now}
              expanded={!collapsed.has(folder.key)}
              onToggle={() => toggle(folder.key)}
            />
          ))}
        </ul>
      )}
    </aside>
  );
}

interface FolderNodeProps extends AgentPanelProps {
  cache: HistoryCache;
  folder: AgentFolder;
  now: number;
  expanded: boolean;
  onToggle: () => void;
}

function FolderNode(props: FolderNodeProps) {
  const { folder, expanded, compact, onToggle } = props;
  const { t } = useI18n();
  const name = folder.path === null ? t.agents.unknownFolder : folder.name;
  const title = folder.path ?? t.agents.unknownFolder;
  const focusedId = activeTab(props.state).focusedId;
  const busy = folder.agents.length > 0;
  const icon = <Folder size={15} className={`agents-folder-icon${busy ? " busy" : ""}`} aria-hidden="true" />;
  const waitingDot = folder.waiting && <span className="agents-dot" role="img" aria-label={t.agent.tab.waiting} />;

  return (
    <li role="treeitem" aria-expanded={expanded} className="agents-folder-node">
      <div className="tree-row agents-folder">
        {compact ? (
          <button type="button" className="tree-select" title={title} aria-label={name} onClick={onToggle}>
            {icon}
            {waitingDot}
          </button>
        ) : (
          <>
            <button type="button" className="tree-select agents-folder-toggle" title={title} aria-expanded={expanded} onClick={onToggle}>
              <ChevronRight size={13} className={`agents-chevron${expanded ? " expanded" : ""}`} aria-hidden="true" />
              {icon}
              <span className="agents-folder-text">
                <span className="tree-label">
                  <span className="agents-parent">{folder.parent}</span>
                  <span className="agents-folder-name">{name}</span>
                </span>
                <span className="agents-folder-meta">
                  {folder.branch && (
                    <>
                      <GitBranch size={11} aria-hidden="true" />
                      <span className="tree-label">{folder.branch}</span>
                      <span aria-hidden="true">·</span>
                    </>
                  )}
                  <span className="agents-shells">{t.agents.shellCount(folder.shells)}</span>
                </span>
              </span>
            </button>
            {busy && folder.path !== null && <StartButtons path={folder.path} onStart={props.onStart} onHover />}
            {waitingDot}
            <span className="tree-count" title={t.agents.agentCount(folder.agents.length)}>
              {folder.agents.length}
            </span>
          </>
        )}
      </div>
      {expanded && (
        <ul className="tree-children" role="group">
          {folder.agents.map((session) => (
            <AgentNode
              key={session.id}
              session={session}
              current={session.id === focusedId}
              now={props.now}
              compact={compact}
              onFocus={props.onFocus}
            />
          ))}
          {!compact && !busy && (
            <li className="agents-none">
              <span className="agents-none-text">{t.agents.none}</span>
              {folder.path !== null && (
                <>
                  {/* Hidden when the panel is narrow; the buttons' group keeps it as its name. */}
                  <span className="agents-start-label" aria-hidden="true">
                    {t.agents.startHere}
                  </span>
                  <StartButtons path={folder.path} onStart={props.onStart} />
                </>
              )}
            </li>
          )}
          {!compact && folder.path !== null && (
            <FolderHistory
              path={folder.path}
              agents={folder.agents}
              state={props.state}
              cache={props.cache}
              onStart={props.onStart}
            />
          )}
        </ul>
      )}
    </li>
  );
}

interface AgentNodeProps {
  session: Session;
  /** The focused pane of the tab in view. */
  current: boolean;
  now: number;
  compact: boolean;
  onFocus: (id: string) => void;
}

function AgentNode({ session, current, now, compact, onFocus }: AgentNodeProps) {
  const { t } = useI18n();
  const agent = session.agent!;
  const display = agentDisplay(session) ?? "idle";
  const label = sessionLabel(session);
  // A finished agent tells when it finished; the others how long they have been at it.
  const card = t.agent.card;
  const elapsed = agent.since !== null && (display === "done" ? card.ago : card.for)(minutesSince(agent.since, now));

  return (
    <li role="treeitem" aria-selected={current}>
      <div className={`tree-row tree-session agents-agent agent-${display}${current ? " active" : ""}`}>
        {compact ? (
          <button type="button" className="tree-select" title={label} aria-label={label} onClick={() => onFocus(session.id)}>
            <AgentMark agent={agent} display={display} size={15} detail={label} />
          </button>
        ) : (
          <button type="button" className="tree-select agents-agent-select" onClick={() => onFocus(session.id)}>
            <AgentIcon kind={agent.kind} size={16} />
            <span className="agents-agent-text">
              <span className="agents-agent-line">
                <span className="tree-label">{label}</span>
                <StateSign display={display} />
                {elapsed && <span className="agents-time">{elapsed}</span>}
              </span>
              {display === "waiting" && <span className="agents-agent-note">{t.agents.needsInput}</span>}
            </span>
          </button>
        )}
      </div>
    </li>
  );
}

/** Asks: a pulsing yellow dot. Works: a turning blue ring. Done: a green check. Idle: a gray dot. */
function StateSign({ display }: { display: AgentDisplay }) {
  const { t } = useI18n();
  return (
    <span className={`agents-state agents-state-${display}`} role="img" aria-label={t.agent.states[display]}>
      {display === "done" && <Check size={12} strokeWidth={2.5} aria-hidden="true" />}
    </span>
  );
}

/** One button per agent that opens a new shell in the folder and starts it there. */
function StartButtons({ path, onStart, onHover }: { path: string; onStart: AgentPanelProps["onStart"]; onHover?: boolean }) {
  const { t } = useI18n();
  return (
    <span className={`agents-start${onHover ? " on-hover" : ""}`} role="group" aria-label={t.agents.startHere}>
      {AGENT_KINDS.map((kind) => {
        const label = t.agents.start(t.agent.names[kind]);
        return (
          <button
            key={kind}
            type="button"
            className="icon-button"
            aria-label={label}
            title={label}
            onClick={() => onStart(path, { kind, conversation: null })}
          >
            <AgentIcon kind={kind} size={15} />
          </button>
        );
      })}
    </span>
  );
}

interface FolderHistoryProps {
  path: string;
  agents: Session[];
  state: Dashboard;
  cache: HistoryCache;
  onStart: AgentPanelProps["onStart"];
}

/**
 * Reads the folder's past conversations when the panel opens, and again when
 * the conversations running here change: one that ends shows up as past. The
 * cache outlives this component, so a mount within 30 s of the last read
 * shows the kept list and reads nothing.
 */
function FolderHistory({ path, agents, state, cache, onStart }: FolderHistoryProps) {
  const [list, setList] = useState(() => cache.shown(path));
  // Sorted, so a change of order by urgency does not read again; "?" counts an agent with no known conversation.
  const running = agents
    .map((session) => session.agent?.conversation ?? "?")
    .sort()
    .join(" ");
  useEffect(() => {
    let live = true;
    void cache.load(path, running).then((items) => {
      // A failed read keeps what the folder showed.
      if (live && items) setList(items);
    });
    return () => {
      live = false;
    };
  }, [path, running, cache]);
  const items = pastConversations(list, state, path).slice(0, PAST_MAX);
  return items.length > 0 ? <PastList path={path} items={items} onStart={onStart} /> : null;
}

interface PastListProps {
  path: string;
  items: PastConversation[];
  onStart: AgentPanelProps["onStart"];
}

/** A folder's past conversations, expanded at first, each with a resume button. */
export function PastList({ path, items, onStart }: PastListProps) {
  const { t, locale } = useI18n();
  const [open, setOpen] = useState(true);
  const now = useNow(60_000);
  // Within a day: "5 min ago". Older: the date.
  const when = (time: number) => {
    const minutes = minutesSince(time, now);
    return minutes < 24 * 60
      ? t.agent.card.ago(minutes)
      : new Date(time).toLocaleDateString(HTML_LANG[locale], { month: "short", day: "numeric" });
  };

  return (
    <li role="treeitem" aria-expanded={open} className="agents-past">
      <button type="button" className="tree-row agents-past-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
        <History size={12} aria-hidden="true" />
        <span>{t.agents.past(items.length)}</span>
        <ChevronRight size={12} className={`agents-chevron${open ? " expanded" : ""}`} aria-hidden="true" />
      </button>
      {open && (
        <ul role="group">
          {items.map((item) => {
            const title = item.title ?? t.agents.untitled;
            return (
              // Two lines, so the title gets the row's width: logo and title; then when, and Resume.
              <li key={`${item.kind}:${item.id}`} role="treeitem" className="agents-past-row">
                <span className="agents-past-line">
                  <AgentIcon kind={item.kind} size={14} />
                  <span className={`tree-label agents-past-title${item.title ? "" : " untitled"}`} title={title}>
                    {title}
                  </span>
                </span>
                <span className="agents-past-meta">
                  <span className="agents-time">{when(item.updated)}</span>
                  <button
                    type="button"
                    className="agents-resume"
                    aria-label={t.agents.resumeLabel(title)}
                    title={t.agents.resumeLabel(title)}
                    onClick={() => onStart(path, { kind: item.kind, conversation: item.id })}
                  >
                    {t.agents.resume}
                  </button>
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </li>
  );
}
