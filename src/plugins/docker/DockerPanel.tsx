import { useState } from "react";
import {
  Box,
  ChevronRight,
  Container,
  Copy,
  Ellipsis,
  Globe,
  Layers,
  Play,
  RefreshCw,
  RotateCw,
  ScrollText,
  Search,
  Square,
  SquareTerminal,
  Trash2,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import { homeShort, minutesSince } from "../../uiHelpers.ts";
import { useCopy, type CopyResult } from "../../useCopy.ts";
import type { PluginContext } from "../types.ts";
import { containerAction, GENERIC_FAILURE, type ContainerAction } from "./api.ts";
import { DOCKER_TEXT, type DockerMessages } from "./i18n.ts";
import {
  containerDot,
  displayName,
  groupContainers,
  runningCount,
  type ContainerGroup,
  type DockerContainer,
  type DockerState,
} from "./model.ts";
import { detectPlatform, EmptyState, firstLine, InstallHint, type Platform } from "./EmptyStates.tsx";
import { PopupMenu, type MenuItem } from "../PopupMenu.tsx";

const PLATFORM = detectPlatform();

/** The menu that shows: a container's (right-click or the menu key) or the header's ⋯. */
export type MenuTarget = { kind: "container"; id: string; x: number; y: number } | { kind: "header"; x: number; y: number };

/** What the panel shows besides Docker's state. */
export interface PanelUi {
  filter: string;
  /** Group keys folded shut. Not remembered. */
  collapsed: ReadonlySet<string>;
  /** Containers with an action under way. */
  busy: ReadonlySet<string>;
  /** The last failed action's text per container, as Docker gave it. */
  errors: ReadonlyMap<string, string>;
  /** The container whose Remove waits for a yes. */
  confirming: string | null;
  menu: MenuTarget | null;
}

export interface PanelHandlers {
  filter(text: string): void;
  toggleGroup(key: string): void;
  logs(c: DockerContainer): void;
  shell(c: DockerContainer): void;
  openPort(port: number): void;
  act(c: DockerContainer, action: ContainerAction): void;
  actAll(group: ContainerGroup, action: "start" | "stop"): void;
  confirm(id: string | null): void;
  menu(target: MenuTarget | null): void;
  copy(text: string): void;
  refresh(): void;
  retry(): void;
  uninstall(): void;
}

export interface DockerPanelViewProps {
  /** null until the first answer. */
  state: DockerState | null;
  ui: PanelUi;
  now: number;
  platform: Platform;
  /** The install command's Copy button result. */
  copied: CopyResult;
  on: PanelHandlers;
}

const canStop = (c: DockerContainer) => c.state === "running" || c.state === "paused" || c.state === "restarting";

/** Menu coordinates under an element, for the keyboard. */
function below(element: Element): { x: number; y: number } {
  const rect = element.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + 2 };
}

/**
 * The Docker panel in the sidebar slot, from its props alone: the header, then
 * the containers grouped by compose project, or why there are none.
 */
export function DockerPanelView({ state, ui, now, platform, copied, on }: DockerPanelViewProps) {
  const { locale } = useI18n();
  const t = DOCKER_TEXT[locale];
  const engine = state?.engine;

  let menu: MenuItem[] = [];
  let menuLabel = t.more;
  if (ui.menu?.kind === "header") menu = [{ label: t.uninstall, run: on.uninstall }];
  if (ui.menu?.kind === "container") {
    const id = ui.menu.id;
    const c = state?.containers.find((each) => each.id === id);
    if (c) {
      menu = containerMenu(c, ui.busy.has(c.id), t, on);
      menuLabel = t.menu(displayName(c));
    }
  }

  return (
    <aside className="session-panel docker-panel" aria-label={t.name}>
      <header className="docker-header">
        <Container size={15} className="docker-logo" aria-hidden="true" />
        <span className="docker-title">{t.name}</span>
        <span className="docker-engine">{t.local}</span>
        {engine?.version && <span className="docker-version">{engine.version}</span>}
        <button type="button" className="icon-button docker-refresh" aria-label={t.refresh} title={t.refresh} onClick={on.refresh}>
          <RefreshCw size={14} />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t.more}
          title={t.more}
          aria-haspopup="menu"
          aria-expanded={ui.menu?.kind === "header"}
          onClick={(event) => on.menu(ui.menu?.kind === "header" ? null : { kind: "header", ...below(event.currentTarget) })}
        >
          <Ellipsis size={15} />
        </button>
      </header>
      {state === null && <p className="docker-none">{t.loading}</p>}
      {engine?.status === "no-cli" && (
        <EmptyState title={t.noCli} detail={t.noCliHint}>
          <InstallHint t={t} platform={platform} copied={copied} onCopy={on.copy} />
        </EmptyState>
      )}
      {engine?.status === "down" && (
        <EmptyState title={t.notRunning} detail={engine.error}>
          <button type="button" className="button" onClick={on.retry}>
            {t.retry}
          </button>
        </EmptyState>
      )}
      {engine?.status === "ok" && state && <Containers state={state} ui={ui} now={now} t={t} on={on} />}
      {menu.length > 0 && ui.menu && (
        <PopupMenu label={menuLabel} x={ui.menu.x} y={ui.menu.y} items={menu} onClose={() => on.menu(null)} />
      )}
    </aside>
  );
}

function containerMenu(c: DockerContainer, busy: boolean, t: DockerMessages, on: PanelHandlers): MenuItem[] {
  const stoppable = canStop(c);
  return [
    { label: t.logs, icon: ScrollText, run: () => on.logs(c) },
    { label: t.shell, icon: SquareTerminal, run: () => on.shell(c) },
    ...c.ports.map((port) => ({ label: t.openPort(port), icon: Globe, run: () => on.openPort(port) })),
    stoppable
      ? { label: t.stop, icon: Square, run: () => on.act(c, "stop"), disabled: busy, separated: true }
      : { label: t.start, icon: Play, run: () => on.act(c, "start"), disabled: busy, separated: true },
    { label: t.restart, icon: RotateCw, run: () => on.act(c, "restart"), disabled: busy },
    ...(stoppable ? [] : [{ label: t.removeMenu, icon: Trash2, run: () => on.confirm(c.id), disabled: busy, danger: true }]),
    { label: t.copyId, icon: Copy, run: () => on.copy(c.id), separated: true },
  ];
}

interface ContainersProps {
  state: DockerState;
  ui: PanelUi;
  now: number;
  t: DockerMessages;
  on: PanelHandlers;
}

function Containers({ state, ui, now, t, on }: ContainersProps) {
  const { containers } = state;
  if (containers.length === 0) {
    return <EmptyState title={t.noContainers} detail={t.noContainersHint} />;
  }
  const groups = groupContainers(containers, ui.filter);
  return (
    <>
      <label className="docker-filter">
        <Search size={13} aria-hidden="true" />
        <input
          type="search"
          value={ui.filter}
          placeholder={t.filter}
          aria-label={t.filter}
          spellCheck={false}
          onChange={(event) => on.filter(event.target.value)}
        />
        <span className="docker-filter-count">{t.counts(runningCount(containers), containers.length)}</span>
      </label>
      {groups.length === 0 ? (
        <p className="docker-none">{t.noMatch}</p>
      ) : (
        <ul className="tree docker-tree" role="tree" aria-label={t.tree}>
          {groups.map((group) => (
            <GroupNode key={group.key} group={group} ui={ui} now={now} t={t} on={on} />
          ))}
        </ul>
      )}
    </>
  );
}

function GroupNode({ group, ui, now, t, on }: { group: ContainerGroup } & Omit<ContainersProps, "state">) {
  const expanded = !ui.collapsed.has(group.key);
  const name = group.project ?? t.other;
  const total = group.containers.length;
  const Icon = group.project === null ? Box : Layers;
  return (
    <li role="treeitem" aria-expanded={expanded} className="docker-group-node">
      <div className="tree-row docker-group">
        <button
          type="button"
          className="tree-select docker-group-toggle"
          aria-expanded={expanded}
          title={group.workingDir ?? undefined}
          onClick={() => on.toggleGroup(group.key)}
        >
          <ChevronRight size={13} className={`docker-chevron${expanded ? " expanded" : ""}`} aria-hidden="true" />
          <Icon size={14} className="docker-group-icon" aria-hidden="true" />
          <span className="tree-label">
            <span className="docker-project">{name}</span>
            {group.workingDir && <span className="docker-dir">{homeShort(group.workingDir)}</span>}
          </span>
        </button>
        <span className={`tree-count docker-count${group.failed ? " failed" : ""}`}>
          {group.running}/{total}
        </span>
        <span className="docker-actions">
          <IconAction
            icon={Play}
            hint={t.startAll}
            label={t.on(t.startAll, name)}
            disabled={group.running === total}
            onClick={() => on.actAll(group, "start")}
          />
          <IconAction
            icon={Square}
            hint={t.stopAll}
            label={t.on(t.stopAll, name)}
            disabled={!group.containers.some(canStop)}
            onClick={() => on.actAll(group, "stop")}
          />
        </span>
      </div>
      {expanded && (
        <ul className="tree-children" role="group">
          {group.containers.map((c) => (
            <ContainerRow key={c.id} c={c} ui={ui} now={now} t={t} on={on} />
          ))}
        </ul>
      )}
    </li>
  );
}

function ContainerRow({ c, ui, now, t, on }: { c: DockerContainer } & Omit<ContainersProps, "state">) {
  const name = displayName(c);
  const busy = ui.busy.has(c.id);
  const dot = containerDot(c);
  const stoppable = canStop(c);
  const toggle = stoppable ? t.stop : t.start;
  const error = ui.errors.get(c.id);
  const shown = error === GENERIC_FAILURE ? t.requestFailed : error;
  const stateLabel = busy ? t.busy : c.health === "unhealthy" ? `${t.states[c.state]} · ${t.unhealthy}` : t.states[c.state];
  const ago = c.finishedAt === null ? null : t.ago(minutesSince(c.finishedAt, now));

  const openMenu = (x: number, y: number) => on.menu({ kind: "container", id: c.id, x, y });
  return (
    <li role="treeitem" className="docker-container">
      <div
        className={`tree-row tree-session docker-row${busy ? " busy" : ""}`}
        onContextMenu={(event) => {
          event.preventDefault();
          openMenu(event.clientX, event.clientY);
        }}
      >
        <button
          type="button"
          className="tree-select docker-select"
          title={c.name}
          onClick={() => on.logs(c)}
          onKeyDown={(event) => {
            if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
            event.preventDefault();
            const at = below(event.currentTarget);
            openMenu(at.x + 16, at.y);
          }}
        >
          <span className={`docker-dot ${busy ? "busy" : dot}`} role="img" aria-label={stateLabel} />
          <span className="docker-text">
            <span className="docker-line">
              <span className="tree-label">{name}</span>
              {c.state === "running" && c.health === "healthy" && <span className="docker-health">{t.healthy}</span>}
              {c.state === "running" && c.health === "unhealthy" && (
                <span className="docker-health unhealthy">{t.unhealthy}</span>
              )}
              {dot === "failed" && <span className="docker-exit">{t.exited(c.exitCode ?? 0, ago)}</span>}
            </span>
            <span className="docker-image">{c.image}</span>
          </span>
        </button>
        {/* The hover buttons hang off the left of the port chips, so they never
            cover a chip; Stop sits farthest from the chips. The context menu
            (Shift+F10) has these too, so Tab skips them. */}
        <span className="docker-tail">
          <span className="docker-actions">
            <IconAction
              skip
              icon={stoppable ? Square : Play}
              hint={toggle}
              label={t.on(toggle, name)}
              disabled={busy}
              onClick={() => on.act(c, stoppable ? "stop" : "start")}
            />
            <IconAction
              skip
              icon={RotateCw}
              hint={t.restart}
              label={t.on(t.restart, name)}
              disabled={busy}
              onClick={() => on.act(c, "restart")}
            />
            <IconAction skip icon={SquareTerminal} hint={t.shell} label={t.on(t.shell, name)} onClick={() => on.shell(c)} />
            <IconAction skip icon={ScrollText} hint={t.logs} label={t.on(t.logs, name)} onClick={() => on.logs(c)} />
          </span>
          {c.ports.length > 0 && (
            <span className="docker-ports">
              {c.ports.map((port) => (
                <button key={port} type="button" className="docker-port" title={t.openPort(port)} onClick={() => on.openPort(port)}>
                  :{port}
                </button>
              ))}
            </span>
          )}
        </span>
      </div>
      {ui.confirming === c.id && (
        <div className="docker-confirm" onKeyDown={(event) => event.key === "Escape" && on.confirm(null)}>
          <span className="docker-confirm-text">{t.confirmRemove(name)}</span>
          <span className="docker-confirm-buttons">
            <button
              type="button"
              className="docker-confirm-remove"
              onClick={() => {
                on.confirm(null);
                on.act(c, "remove");
              }}
            >
              {t.remove}
            </button>
            <button type="button" className="docker-confirm-cancel" autoFocus onClick={() => on.confirm(null)}>
              {t.cancel}
            </button>
          </span>
        </div>
      )}
      {shown && (
        <p className="docker-error" role="alert" title={shown}>
          {firstLine(shown)}
        </p>
      )}
    </li>
  );
}

interface IconActionProps {
  icon: LucideIcon;
  /** The tooltip. */
  hint: string;
  /** The name for screen readers: the action and what it acts on. */
  label: string;
  disabled?: boolean;
  /** Left out of the Tab order. */
  skip?: boolean;
  onClick: () => void;
}

/** A row's hover button. */
function IconAction({ icon: Icon, hint, label, disabled, skip, onClick }: IconActionProps) {
  return (
    <button
      type="button"
      className="icon-button"
      tabIndex={skip ? -1 : undefined}
      disabled={disabled}
      aria-label={label}
      title={hint}
      onClick={onClick}
    >
      <Icon size={13} />
    </button>
  );
}

interface DockerPanelProps {
  ctx: PluginContext;
  state: DockerState | null;
  /** Asks Docker again now. */
  refresh: () => void;
  /** Asks again, telling the server to look at the engine afresh. */
  retry: () => void;
}

const toggled = (set: ReadonlySet<string>, key: string, on: boolean) => {
  const next = new Set(set);
  if (on) next.add(key);
  else next.delete(key);
  return next;
};

/** The panel with its own UI state; actions go to Docker and ask for the list again when done. */
export function DockerPanel({ ctx, state, refresh, retry }: DockerPanelProps) {
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const [errors, setErrors] = useState<ReadonlyMap<string, string>>(new Map());
  const [confirming, setConfirming] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuTarget | null>(null);
  const [copied, copy] = useCopy();

  const act = async (c: DockerContainer, action: ContainerAction) => {
    if (busy.has(c.id)) return;
    setBusy((current) => toggled(current, c.id, true));
    setErrors((current) => {
      const next = new Map(current);
      next.delete(c.id);
      return next;
    });
    const error = await containerAction(ctx, c.id, action);
    setBusy((current) => toggled(current, c.id, false));
    if (error !== null) setErrors((current) => new Map(current).set(c.id, error));
    refresh();
  };

  const on: PanelHandlers = {
    filter: setFilter,
    toggleGroup: (key) => setCollapsed((current) => toggled(current, key, !current.has(key))),
    logs: (c) =>
      ctx.openShell({ action: "logs", target: c.id, cwd: c.workingDir, name: `${displayName(c)} logs`, reuse: true }),
    shell: (c) =>
      ctx.openShell({ action: "shell", target: c.id, cwd: c.workingDir, name: `${displayName(c)} shell`, reuse: false }),
    openPort: (port) => ctx.openBrowser(`http://localhost:${port}`),
    act: (c, action) => void act(c, action),
    actAll: (group, action) => {
      for (const c of group.containers) {
        if (canStop(c) === (action === "stop")) void act(c, action);
      }
    },
    confirm: setConfirming,
    menu: setMenu,
    copy,
    refresh,
    retry,
    uninstall: () => ctx.openManager(),
  };

  return (
    <DockerPanelView
      state={state}
      ui={{ filter, collapsed, busy, errors, confirming, menu }}
      now={Date.now()}
      platform={PLATFORM}
      copied={copied}
      on={on}
    />
  );
}
