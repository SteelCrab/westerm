// The Terraform panel in the sidebar slot: the projects found in the session
// folders as a tree (project → module → resource), with what the current
// plan does to each resource, and the run buttons of the selected project.
// Runs happen in a terminal pane; the panel only starts them.

import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent, type ReactNode } from "react";
import { Box, ChevronRight, Copy, Ellipsis, Play, RefreshCw, Search, SquareTerminal, Trash2, TriangleAlert } from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import { PopupMenu, type MenuItem } from "../PopupMenu.tsx";
import type { PluginContext } from "../types.ts";
import { deletePlans, fetchTree, listWorkspaces, selectWorkspace, setCheckVersions, setTool } from "./api.ts";
import { TerraformLogo, TofuMark } from "./icon.tsx";
import { TF_TEXT, type TfMessages } from "./i18n.ts";
import { age, canApply, canRun, filterRows, isProd, mergeMarks, primaryAction, projectDot, type Row } from "./model.ts";
import { planPaneKey, planPaneTitle, type TerraformStore } from "./store.ts";
import type { Action, Mark, PlanPaneData, ProjectInfo, StateAnswer, TreeAnswer } from "./types.ts";

export interface TerraformPanelProps {
  ctx: PluginContext;
  store: TerraformStore;
  /** Opens the confirmation before an apply. */
  openApply(projectId: string): void;
  /** How long after a tree's first read it is read once more; tests make it short. */
  secondLookMs?: number;
}

type Menu = { x: number; y: number; items: MenuItem[]; label: string };

/** The workspace picker of one project: its names once the tool listed them. */
interface Picker {
  id: string;
  names: string[] | null;
}

const MARKS: Record<Mark, string> = { create: "+", update: "~", delete: "-", replace: "±" };
/** A tree is read once more after this long: a newer module version is asked for behind the first answer. */
const SECOND_LOOK_MS = 3000;

const below = (element: Element) => {
  const rect = element.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + 2 };
};

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // Without clipboard access there is nothing to do.
  }
}

/** What makes a project's tree worth reading again. */
const signature = ({ tool, workspace, needsInit, run, plan }: ProjectInfo) =>
  [tool, workspace, needsInit, run?.state, run?.at, plan?.state, plan?.at].join("|");

/** The install line for this computer: Homebrew on a Mac, else the download page. */
function installLine(): string {
  const mac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform ?? "");
  return mac ? "brew install hashicorp/tap/terraform" : "https://developer.hashicorp.com/terraform/install";
}

export function TerraformPanel({ ctx, store, openApply, secondLookMs = SECOND_LOOK_MS }: TerraformPanelProps) {
  const { locale } = useI18n();
  const t = TF_TEXT[locale];
  const state = useSyncExternalStore(store.subscribe, store.state, store.state);
  const projects = state?.projects ?? [];
  // The project the run buttons act on; null follows the first one.
  const [chosen, setChosen] = useState<string | null>(null);
  // What the user opened; null until the first click. Until then a single project is open.
  const [opened, setOpened] = useState<ReadonlySet<string> | null>(null);
  const [closedModules, setClosedModules] = useState<ReadonlySet<string>>(new Set());
  const [trees, setTrees] = useState<ReadonlyMap<string, TreeAnswer>>(new Map());
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<Menu | null>(null);
  const [picker, setPicker] = useState<Picker | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  // What each open project's tree was read for.
  const read = useRef(new Map<string, string>());
  // The second looks that wait. They outlive the effect below, which runs on every poll.
  const secondLooks = useRef(new Set<ReturnType<typeof setTimeout>>());
  useEffect(() => {
    const waiting = secondLooks.current;
    return () => {
      for (const timer of waiting) clearTimeout(timer);
      waiting.clear();
    };
  }, []);

  const selected = projects.find((project) => project.id === chosen) ?? projects[0] ?? null;

  const onlyId = projects.length === 1 ? projects[0].id : null;
  const open = useMemo(() => opened ?? new Set(onlyId === null ? [] : [onlyId]), [opened, onlyId]);

  const loadTree = useCallback(
    async (id: string) => {
      const answer = await fetchTree(ctx, id);
      if (answer.ok) setTrees((current) => new Map(current).set(id, answer.value));
    },
    [ctx],
  );

  // An open project's tree is read when it opens and when its run or plan moves on.
  useEffect(() => {
    for (const project of projects) {
      if (!open.has(project.id)) continue;
      const wanted = signature(project);
      const first = !read.current.has(project.id);
      if (read.current.get(project.id) === wanted) continue;
      read.current.set(project.id, wanted);
      void loadTree(project.id);
      if (first && state?.checkVersions) {
        const timer = setTimeout(() => {
          secondLooks.current.delete(timer);
          void loadTree(project.id);
        }, secondLookMs);
        secondLooks.current.add(timer);
      }
    }
    // `projects` is new on every answer; the signatures decide what is read.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, state, loadTree, secondLookMs]);

  const refusalText = useCallback((code: string) => t.refusals[code] ?? code, [t]);

  const run = useCallback(
    async (id: string, action: Action, target: string | null) => {
      setNotice(null);
      const refused = await store.run(id, action, target);
      if (refused !== null) setNotice(refusalText(refused));
    },
    [store, refusalText],
  );

  const refreshAll = () => {
    read.current.clear();
    setNotice(null);
    store.refresh(true);
  };

  const toggleProject = (id: string) => {
    setChosen(id);
    const next = new Set(open);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setOpened(next);
  };

  const toggleModule = (key: string) =>
    setClosedModules((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const openPicker = async (project: ProjectInfo) => {
    if (picker?.id === project.id) {
      setPicker(null);
      return;
    }
    setNotice(null);
    setPicker({ id: project.id, names: null });
    const answer = await listWorkspaces(ctx, project.id);
    if (answer.ok) {
      setPicker({ id: project.id, names: answer.value.workspaces });
    } else {
      setPicker(null);
      const line = answer.failure.type === "refused" ? refusalText(answer.failure.error) : refusalText(answer.failure.type);
      setNotice(`${line} ${t.workspaceFailed(project.tool)}`);
    }
  };

  const pickWorkspace = async (project: ProjectInfo, name: string) => {
    setPicker(null);
    if (name === project.workspace) return;
    const answer = await selectWorkspace(ctx, project.id, name);
    if (answer.ok) store.refresh();
    else setNotice(answer.failure.type === "refused" ? refusalText(answer.failure.error) : refusalText(answer.failure.type));
  };

  const openPlanAt = (project: ProjectInfo, address: string) => {
    const data: PlanPaneData = { projectId: project.id, path: project.path, focus: address };
    ctx.openPane({ view: "plan", title: planPaneTitle(project.name), data, reuseKey: planPaneKey(project.id) });
  };

  const projectMenu = (project: ProjectInfo, at: { x: number; y: number }) => {
    const runnable = state !== null && canRun(state, project);
    const both = state !== null && state.tools.terraform && state.tools.tofu;
    const other = project.tool === "terraform" ? "tofu" : "terraform";
    const items: MenuItem[] = [
      { label: t.actions.init, disabled: !runnable, run: () => void run(project.id, "init", null) },
      { label: t.actions.validate, disabled: !runnable, run: () => void run(project.id, "validate", null) },
      { label: t.actions.plan, icon: Play, disabled: !runnable, run: () => void run(project.id, "plan", null) },
      { label: t.actions.apply, disabled: state === null || !canApply(state, project), run: () => openApply(project.id) },
      { label: t.openTerminalHere, icon: SquareTerminal, separated: true, run: () => ctx.openFolder(project.path, project.name) },
      ...(both ? [{ label: other === "tofu" ? t.useTofu : t.useTerraform, run: () => void setTool(ctx, project.id, other).then(() => store.refresh()) }] : []),
      { label: t.copyPath, icon: Copy, run: () => void copyText(project.path) },
    ];
    setMenu({ ...at, label: project.name, items });
  };

  const moduleMenu = (project: ProjectInfo, address: string, at: { x: number; y: number }) =>
    setMenu({
      ...at,
      label: address,
      items: [
        { label: t.planModule, icon: Play, disabled: state === null || !canRun(state, project), run: () => void run(project.id, "plan", address) },
        { label: t.copyAddress, icon: Copy, run: () => void copyText(address) },
      ],
    });

  const addressMenu = (address: string, at: { x: number; y: number }) =>
    setMenu({ ...at, label: address, items: [{ label: t.copyAddress, icon: Copy, run: () => void copyText(address) }] });

  const onContext = (event: MouseEvent, show: (at: { x: number; y: number }) => void) => {
    event.preventDefault();
    show({ x: event.clientX, y: event.clientY });
  };

  function rows(project: ProjectInfo, list: readonly Row[], depth: number): ReactNode {
    const runnable = state !== null && canRun(state, project);
    return (
      <ul role="group">
        {list.map((row) => {
          if (row.kind === "resource") {
            const { node, mark, count, removed } = row;
            const label = (
              <>
                <span className="tf-diamond" aria-hidden="true" />
                <span className="tf-name tf-mono">{`${node.type}.${node.name}`}</span>
                {count !== null && <span className="tf-count">{`×${count}`}</span>}
                {mark !== null && <span className={`tf-mark tf-mark-${mark}`}>{MARKS[mark]}</span>}
              </>
            );
            return (
              <li key={node.address} role="treeitem" className="tf-item">
                <div
                  className={`tf-row tf-resource${removed ? " removed" : ""}`}
                  style={{ paddingLeft: depth * 12 }}
                  onContextMenu={(event) => onContext(event, (at) => addressMenu(node.address, at))}
                >
                  {mark !== null ? (
                    <button type="button" className="tf-open" title={node.address} onClick={() => openPlanAt(project, node.address)}>
                      {label}
                    </button>
                  ) : (
                    <span className="tf-leaf" title={node.address}>
                      {label}
                    </span>
                  )}
                </div>
              </li>
            );
          }
          const { node, changed, children } = row;
          const key = `${project.id}\n${node.address}`;
          const expanded = !closedModules.has(key) || filter.trim() !== "";
          return (
            <li key={node.address} role="treeitem" aria-expanded={expanded} className="tf-item">
              <div className="tf-row tf-module" style={{ paddingLeft: depth * 12 }} onContextMenu={(event) => onContext(event, (at) => moduleMenu(project, node.address, at))}>
                <button type="button" className="tf-toggle" aria-expanded={expanded} title={node.address} onClick={() => toggleModule(key)}>
                  <ChevronRight size={12} className={expanded ? "tf-chevron open" : "tf-chevron"} aria-hidden="true" />
                  <Box size={13} className="tf-icon" aria-hidden="true" />
                  <span className="tf-name">{`module.${node.name}`}</span>
                  {changed && !expanded && <span className="tf-changed" aria-hidden="true" />}
                  {node.label !== null && <span className={`tf-tag tf-tag-${node.source}`}>{node.label}</span>}
                  {node.newer !== null && (
                    <span className="tf-tag tf-newer" title={t.newer(node.newer, project.tool)}>
                      {`↑ ${node.newer}`}
                    </span>
                  )}
                </button>
                <span className="tf-actions">
                  <button type="button" className="icon-button" aria-label={t.planModule} title={t.planModule} disabled={!runnable} onClick={() => void run(project.id, "plan", node.address)}>
                    <Play size={12} />
                  </button>
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={t.openModuleFolder}
                    title={t.openModuleFolder}
                    disabled={node.dir === null}
                    onClick={() => node.dir !== null && ctx.openFolder(node.dir, node.name)}
                  >
                    <SquareTerminal size={12} />
                  </button>
                </span>
              </div>
              {expanded && rows(project, children, depth + 1)}
            </li>
          );
        })}
      </ul>
    );
  }

  function projectItem(project: ProjectInfo) {
    const expanded = open.has(project.id);
    const tree = trees.get(project.id);
    const dot = projectDot(project);
    const stale = project.plan?.state === "stale";
    // The numbers of a plan that can be applied, or could before the code changed.
    const sums = project.plan?.state === "current" || stale ? (project.plan?.counts ?? null) : null;
    return (
      <li key={project.id} role="treeitem" aria-expanded={expanded} aria-selected={selected?.id === project.id} className="tf-item tf-project" data-id={project.id}>
        <div className={`tf-row tf-project-row${selected?.id === project.id ? " selected" : ""}`} onContextMenu={(event) => onContext(event, (at) => projectMenu(project, at))}>
          <button type="button" className="tf-toggle" aria-expanded={expanded} title={project.path} onClick={() => toggleProject(project.id)}>
            <ChevronRight size={12} className={expanded ? "tf-chevron open" : "tf-chevron"} aria-hidden="true" />
            {project.tool === "tofu" ? <TofuMark size={14} /> : <TerraformLogo size={14} />}
            <span className="tf-name">{project.name}</span>
            <span className="tf-sub">{project.sub}</span>
            {project.unreadable.length > 0 && (
              <span className="tf-warn" title={t.unreadable(project.unreadable.join(", "))}>
                <TriangleAlert size={12} aria-hidden="true" />
              </span>
            )}
            {isProd(project.name, project.workspace) && <span className="tf-tag tf-tag-prod">{t.tagProd}</span>}
            {project.remote && <span className="tf-tag tf-tag-cloud">{t.tagCloud}</span>}
            {project.needsInit && <span className="tf-tag tf-tag-init">{t.tagInit}</span>}
            {stale && <span className="tf-tag tf-tag-init">{t.tagChanged}</span>}
            <span className={`tf-dot tf-dot-${dot}`} role="img" aria-label={t.dots[dot]} />
          </button>
          <button type="button" className="icon-button tf-more" aria-label={`${t.more}: ${project.name}`} onClick={(event) => projectMenu(project, below(event.currentTarget))}>
            <Ellipsis size={13} />
          </button>
        </div>
        {expanded && (
          <>
            <div className="tf-ws">
              <span className="tf-ws-label">{t.workspace}</span>
              <button
                type="button"
                className="tf-ws-pick tf-mono"
                disabled={project.workspaceLocked || project.run?.state === "running"}
                title={project.workspaceLocked ? t.setByEnv : undefined}
                aria-haspopup="listbox"
                aria-expanded={picker?.id === project.id}
                onClick={() => void openPicker(project)}
              >
                {project.workspace}
              </button>
              {sums !== null && (
                <span className={`tf-sum${stale ? " stale" : ""}`}>
                  <span className="tf-mark-create">{`+${sums.add}`}</span> <span className="tf-mark-update">{`~${sums.change}`}</span>{" "}
                  <span className="tf-mark-none">{`-${sums.destroy}`}</span>
                </span>
              )}
            </div>
            {picker?.id === project.id && (
              <ul className="tf-ws-list" role="listbox" aria-label={t.workspace}>
                {picker.names === null ? (
                  <li className="tf-quiet">{t.loading}</li>
                ) : (
                  picker.names.map((name) => (
                    <li key={name} role="option" aria-selected={name === project.workspace}>
                      <button type="button" className={`tf-ws-name tf-mono${name === project.workspace ? " current" : ""}`} onClick={() => void pickWorkspace(project, name)}>
                        {name}
                      </button>
                    </li>
                  ))
                )}
              </ul>
            )}
            {tree === undefined ? <p className="tf-quiet">{t.loading}</p> : rows(project, filterRows(mergeMarks(tree.nodes, tree.marks), filter), 1)}
          </>
        )}
      </li>
    );
  }

  function runBar(project: ProjectInfo, current: StateAnswer) {
    if (project.run?.state === "running") {
      return (
        <div className="tf-runbar">
          <span className="tf-running" role="status">
            {t.running[project.run.action]}
          </span>
        </div>
      );
    }
    const runnable = canRun(current, project);
    const primary = primaryAction(project);
    const plain = (action: "init" | "validate" | "plan") => (
      <button key={action} type="button" className={`button${primary === action ? " primary" : ""}`} disabled={!runnable} onClick={() => void run(project.id, action, null)}>
        {t.actions[action]}
      </button>
    );
    return (
      <div className="tf-runbar">
        {plain("init")}
        {plain("validate")}
        {plain("plan")}
        <button type="button" className="button" disabled={!canApply(current, project)} title={project.remote ? t.remoteApply : undefined} onClick={() => openApply(project.id)}>
          {t.actions.apply}
        </button>
      </div>
    );
  }

  const lastRun = selected?.run && selected.run.state !== "running" ? selected.run : null;
  const lastAge = lastRun ? age(Date.now(), lastRun.at) : null;
  const version = selected ? (trees.get(selected.id)?.version ?? null) : null;
  const noTool = state !== null && !state.tools.terraform && !state.tools.tofu;

  return (
    <aside className="session-panel tf-panel" aria-label={t.name}>
      <header className="tf-header">
        <span className="tf-logo-box" aria-hidden="true">
          <TerraformLogo size={13} />
        </span>
        <span className="tf-title">{t.name}</span>
        <span className="tf-sub">{state ? t.projects(projects.length) : ""}</span>
        <button type="button" className="icon-button" aria-label={t.refresh} title={t.refresh} onClick={refreshAll}>
          <RefreshCw size={14} />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t.more}
          title={t.more}
          aria-haspopup="menu"
          onClick={(event) =>
            setMenu({
              ...below(event.currentTarget),
              label: t.more,
              items: [
                {
                  label: `${state?.checkVersions ? "✓ " : ""}${t.checkVersions}`,
                  run: () => void setCheckVersions(ctx, !(state?.checkVersions ?? true)).then(() => store.refresh()),
                },
                { label: t.deletePlans, icon: Trash2, danger: true, run: () => setConfirm(true) },
                { label: t.uninstall, run: () => ctx.openManager(), separated: true },
              ],
            })
          }
        >
          <Ellipsis size={15} />
        </button>
      </header>
      {noTool && (
        <p className="tf-banner">
          <strong>{t.notFound("terraform")}</strong>
          <span>
            {t.install} <code>{installLine()}</code>
          </span>
        </p>
      )}
      {projects.length > 0 && (
        <label className="tf-filter">
          <Search size={13} aria-hidden="true" />
          <input type="search" value={filter} placeholder={t.filter} aria-label={t.filter} spellCheck={false} onChange={(event) => setFilter(event.target.value)} />
        </label>
      )}
      {confirm && (
        <div className="tf-confirm" role="alertdialog" aria-label={t.deleteConfirm}>
          <p>{t.deleteConfirm}</p>
          <div className="tf-confirm-actions">
            <button
              type="button"
              className="button danger"
              onClick={() => {
                setConfirm(false);
                void deletePlans(ctx).then(() => store.refresh());
              }}
            >
              {t.delete}
            </button>
            <button type="button" className="button" onClick={() => setConfirm(false)}>
              {t.cancel}
            </button>
          </div>
        </div>
      )}
      {notice && <p className="tf-notice">{notice}</p>}
      {state === null ? (
        <p className="tf-quiet">{t.loading}</p>
      ) : projects.length === 0 ? (
        <p className="tf-empty">{t.empty}</p>
      ) : (
        <ul className="tree tf-tree" role="tree" aria-label={t.tree}>
          {projects.map((project) => projectItem(project))}
        </ul>
      )}
      {state?.truncated && <p className="tf-quiet tf-truncated">{t.truncated}</p>}
      {state !== null && selected !== null && runBar(selected, state)}
      {selected !== null && (
        <footer className="tf-footer">
          <span>{`${selected.name} · ${selected.workspace}`}</span>
          {lastRun && lastAge && (
            <span className={lastRun.state === "ok" ? "tf-last ok" : "tf-last failed"}>{`${t.runWords[lastRun.action]} ${lastRun.state === "ok" ? "✓" : "✗"} ${t.ago(lastAge.n, lastAge.unit)}`}</span>
          )}
          {version !== null && <span className="tf-version">{`${selected.tool} ${version}`}</span>}
        </footer>
      )}
      {menu && <PopupMenu label={menu.label} x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
    </aside>
  );
}

export type { TfMessages };
