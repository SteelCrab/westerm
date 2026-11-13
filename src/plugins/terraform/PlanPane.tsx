// The plan pane: a project's plan as a list of the resources that change,
// grouped by module, with the attributes that change. The pane only shows a
// plan; a plan is made and applied by runs in a terminal pane.

import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { ChevronRight } from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import type { PluginContext, PluginPaneProps } from "../types.ts";
import { fetchPlan } from "./api.ts";
import { TerraformLogo } from "./icon.tsx";
import { TF_TEXT, type TfMessages } from "./i18n.ts";
import { age, canApply, canRun, groupChanges, isProd, opensAtFirst } from "./model.ts";
import { planPaneTitle, type TerraformStore } from "./store.ts";
import type { AttrValue, PlanAnswer, PlanChange, PlanPaneData, ProjectInfo } from "./types.ts";

export interface PlanPaneProps extends PluginPaneProps {
  ctx: PluginContext;
  store: TerraformStore;
  /** Opens the confirmation before an apply. */
  openApply(projectId: string): void;
}

const MARKS: Record<PlanChange["action"], string> = { create: "+", update: "~", delete: "-", replace: "±", import: "↓", forget: "×" };

function paneData(data: unknown): PlanPaneData | null {
  if (typeof data !== "object" || data === null) return null;
  const { projectId, path, focus } = data as Record<string, unknown>;
  if (typeof projectId !== "string" || typeof path !== "string") return null;
  return typeof focus === "string" ? { projectId, path, focus } : { projectId, path };
}

/** What makes a project's plan worth reading again. */
const signature = (project: ProjectInfo | undefined) => (project ? [project.run?.state, project.run?.at, project.plan?.state, project.plan?.at, project.workspace].join("|") : "");

function valueText(value: AttrValue | null, t: TfMessages): string {
  if (value === null) return t.absent;
  if (value.kind === "unknown") return t.unknown;
  if (value.kind === "sensitive") return t.sensitive;
  return value.cut ? `${value.text}…` : value.text;
}

/** The line a plan without a list shows; null when the list shows. */
function stateLine(project: ProjectInfo, plan: PlanAnswer | null, t: TfMessages): string | null {
  if (project.run?.state === "running") return project.run.action === "plan" ? t.planRunning : t.running[project.run.action];
  if (plan === null) {
    const failed = project.run?.action === "plan" && project.run.state !== "ok";
    return failed ? t.planFailed : t.noPlan;
  }
  if (plan.state === "unreadable") return t.planUnreadable;
  if (plan.state === "applyFailed") return t.applyFailed;
  if (plan.state === "applied") {
    const { add = 0, change = 0, destroy = 0, import: imported = 0, forget = 0 } = plan.counts ?? {};
    return t.applied(add + change + destroy + imported + forget);
  }
  if (plan.state === "stale") return t.stale;
  if (plan.changes.length > 0) return null;
  return plan.moved + plan.outputs > 0 ? t.stateOnly(plan.moved, plan.outputs) : t.noChanges;
}

export function PlanPane({ data, focused, setData, setTitle, close, ctx, store, openApply }: PlanPaneProps) {
  const { locale } = useI18n();
  const t = TF_TEXT[locale];
  const state = useSyncExternalStore(store.subscribe, store.state, store.state);
  const pane = paneData(data);
  const project = pane ? state?.projects.find((each) => each.id === pane.projectId) : undefined;
  // undefined until the first answer; null when the project has no plan.
  const [plan, setPlan] = useState<PlanAnswer | null | undefined>(undefined);
  // What the user opened or closed; the rest follows `opensAtFirst`.
  const [toggled, setToggled] = useState<ReadonlyMap<string, boolean>>(new Map());
  const [notice, setNotice] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);

  const projectId = project?.id ?? null;
  const moved = signature(project);
  useEffect(() => {
    if (projectId === null) return;
    let stale = false;
    void fetchPlan(ctx, projectId).then((answer) => {
      if (!stale && answer.ok) setPlan(answer.value);
    });
    return () => {
      stale = true;
    };
    // Read again when the run or the plan moves on, and when the pane gets the focus.
  }, [ctx, projectId, moved, focused]);

  const name = project?.name ?? null;
  useEffect(() => {
    if (name !== null) setTitle(planPaneTitle(name));
  }, [name, setTitle]);

  // The change the tree asked for: open it, show it, and forget the request.
  const focus = pane?.focus ?? null;
  const loaded = plan !== undefined && plan !== null;
  useEffect(() => {
    if (focus === null || !loaded || pane === null) return;
    setToggled((current) => new Map(current).set(focus, true));
    const row = [...(list.current?.querySelectorAll<HTMLElement>(".tf-change") ?? [])].find((each) => each.dataset.address === focus);
    row?.scrollIntoView?.({ block: "center" });
    setData({ projectId: pane.projectId, path: pane.path });
    // `pane` is rebuilt on every render; its focus is what matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, loaded]);

  const planAgain = useCallback(async () => {
    if (projectId === null) return;
    setNotice(null);
    const refused = await store.run(projectId, "plan", plan?.target ?? null);
    if (refused !== null) setNotice(t.refusals[refused] ?? refused);
  }, [projectId, plan, store, t]);

  if (pane === null || (state !== null && project === undefined)) {
    return (
      <div className="plugin-pane-missing" role="status">
        <p>{t.projectNotFound}</p>
        <button type="button" className="button" onClick={close}>
          {t.close}
        </button>
      </div>
    );
  }
  if (state === null || project === undefined || plan === undefined) {
    return (
      <div className="tf-plan-pane">
        <p className="tf-quiet">{t.loading}</p>
      </div>
    );
  }

  const line = stateLine(project, plan, t);
  const listed = plan !== null && (plan.state === "current" || plan.state === "stale") && plan.changes.length > 0 && project.run?.state !== "running";
  const counts = plan !== null && (plan.state === "current" || plan.state === "stale") ? plan.counts : null;
  const since = plan !== null ? age(Date.now(), plan.at) : null;
  const total = plan !== null ? plan.changes.length + plan.moreChanges : 0;
  const isOpen = (change: PlanChange) => toggled.get(change.address) ?? opensAtFirst(change);

  return (
    <div className={`tf-plan-pane${isProd(project.name, plan?.workspace ?? project.workspace) ? " tf-plan-prod" : ""}`}>
      <header className="tf-pane-header">
        <TerraformLogo size={13} />
        <span className="tf-title">{t.plan}</span>
        <span className="tf-sub">{`${project.name} · ${t.workspaceOf(plan?.workspace ?? project.workspace)}`}</span>
        {since && <span className="tf-age">{t.ago(since.n, since.unit)}</span>}
      </header>
      <div className="tf-plan-bar">
        {counts && (
          <>
            <span className="tf-chip tf-mark-create">{t.toAdd(counts.add)}</span>
            <span className="tf-chip tf-mark-update">{t.toChange(counts.change)}</span>
            <span className={`tf-chip ${counts.destroy > 0 ? "tf-mark-delete" : "tf-mark-none"}`}>{t.toDestroy(counts.destroy)}</span>
            {counts.import > 0 && <span className="tf-chip">{t.toImport(counts.import)}</span>}
            {counts.forget > 0 && <span className="tf-chip">{t.toForget(counts.forget)}</span>}
          </>
        )}
        {plan?.target != null && <span className="tf-tag tf-target">{t.target(plan.target)}</span>}
        <span className="tf-spacer" />
        <button type="button" className="button" disabled={!canRun(state, project)} onClick={() => void planAgain()}>
          {t.planAgain}
        </button>
        <button type="button" className="button primary" disabled={!canApply(state, project)} title={project.remote ? t.remoteApply : undefined} onClick={() => openApply(project.id)}>
          {t.actions.apply}
        </button>
      </div>
      {notice && <p className="tf-notice">{notice}</p>}
      {line !== null && <p className="tf-plan-state">{line}</p>}
      {listed && plan !== null && (
        <div className={`tf-plan-list${plan.state === "stale" ? " stale" : ""}`} ref={list}>
          {groupChanges(plan.changes).map((group) => (
            <section key={group.module} className="tf-plan-group">
              <h3>{group.module === "" ? t.root : group.module}</h3>
              {group.changes.map((change) => {
                const expanded = isOpen(change);
                const shown = change.module === "" ? change.address : change.address.slice(change.module.length + 1);
                return (
                  <div key={change.address} className={`tf-change tf-change-${change.action}`} data-address={change.address}>
                    <button
                      type="button"
                      className="tf-change-head"
                      aria-expanded={expanded}
                      onClick={() => setToggled((current) => new Map(current).set(change.address, !expanded))}
                    >
                      <span className={`tf-mark tf-mark-${change.action}`}>{MARKS[change.action]}</span>
                      <span className="tf-name tf-mono">{shown}</span>
                      <span className="tf-change-action">{t.changeActions[change.action]}</span>
                      <ChevronRight size={12} className={expanded ? "tf-chevron open" : "tf-chevron"} aria-hidden="true" />
                    </button>
                    {expanded && (change.attrs.length > 0 || change.moreAttrs > 0) && (
                      <div className="tf-attrs tf-mono">
                        {change.attrs.map((attr) => (
                          <div key={attr.path} className="tf-attr">
                            <span className="tf-attr-path">{attr.path}</span> <span className="tf-attr-before">{valueText(attr.before, t)}</span>
                            {" → "}
                            <span className="tf-attr-after">{valueText(attr.after, t)}</span>
                            {attr.forces && <span className="tf-forces"> {t.forces}</span>}
                          </div>
                        ))}
                        {change.moreAttrs > 0 && <div className="tf-attr tf-quiet-row">{t.moreAttrs(change.moreAttrs)}</div>}
                      </div>
                    )}
                  </div>
                );
              })}
            </section>
          ))}
          {plan.moreChanges > 0 && <p className="tf-quiet">{t.moreChanges(plan.changes.length)}</p>}
        </div>
      )}
      {plan !== null && (plan.state === "current" || plan.state === "stale") && (
        <footer className="tf-plan-footer">
          <span>{`${t.changes(total)} · ${t.seconds(plan.seconds)}`}</span>
          <span>{t.saved}</span>
          <span className="tf-spacer" />
          <button type="button" className="tf-link" onClick={() => void navigator.clipboard?.writeText(JSON.stringify(plan, null, 2)).catch(() => {})}>
            {t.copyJson}
          </button>
        </footer>
      )}
    </div>
  );
}
