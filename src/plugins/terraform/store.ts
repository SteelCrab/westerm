// The Terraform plugin's running part without the page: it asks the server
// for the projects on a timer, counts the badge, starts runs, and opens the
// plan pane when a plan this window started is done.

import { createPoller } from "../poller.ts";
import type { PluginContext, PluginStatus } from "../types.ts";
import { fetchState, startRun } from "./api.ts";
import { badgeCount } from "./model.ts";
import type { Action, PlanPaneData, ProjectInfo, StateAnswer } from "./types.ts";

/** While the panel shows. */
const OPEN_INTERVAL = 2000;
/** While the panel is closed and a run is running. */
const RUNNING_INTERVAL = 15_000;
/** Closed and nothing runs: no asking (the longest wait a timer takes). */
const IDLE_INTERVAL = 2_147_483_647;

export interface TerraformStore {
  /** The last answer; null until the first one. The same object until it changes. */
  state(): StateAnswer | null;
  subscribe(listener: () => void): () => void;
  /** The same object until it changes. */
  status(): PluginStatus;
  /**
   * Starts a run and opens its pane. The refusal's code or error line, or
   * null when it started. `planAt`: for an apply, the time of the plan the
   * user confirmed.
   */
  run(projectId: string, action: Action, target: string | null, planAt?: number): Promise<string | null>;
  /** Asks the server now; `scan` also walks the folders again. */
  refresh(scan?: boolean): void;
  setPanelOpen(open: boolean): void;
  dispose(): void;
}

const isRunning = (project: ProjectInfo | undefined) => project?.run?.state === "running";
const hasFailed = (project: ProjectInfo) => project.run?.state === "failed" || project.run?.state === "interrupted";

export const planPaneKey = (projectId: string) => `plan:${projectId}`;
export const planPaneTitle = (name: string) => `Plan · ${name}`;

export function createStore(ctx: PluginContext): TerraformStore {
  const listeners = new Set<() => void>();
  let snapshot: StateAnswer | null = null;
  let status: PluginStatus = { badge: null, alert: false };
  let open = false;
  // The next request walks the folders.
  let scanNext = true;
  // Plans this window started, by project: the time of the run the project showed before.
  const planned = new Map<string, number | null>();

  const emit = () => {
    for (const listener of [...listeners]) listener();
  };

  const setStatus = (badge: number | null, alert: boolean) => {
    if (status.badge !== badge || status.alert !== alert) status = { badge, alert };
  };

  const apply = (next: StateAnswer) => {
    const before = new Map((snapshot?.projects ?? []).map((project) => [project.id, project]));
    let alert = status.alert;
    let ended = false;
    for (const project of next.projects) {
      const was = before.get(project.id);
      if (isRunning(was) && !isRunning(project)) {
        ended = true;
        // A failure the user is not looking at.
        if (hasFailed(project) && !open) alert = true;
      }
      const since = planned.get(project.id);
      if (since === undefined || project.run === null || isRunning(project) || project.run.at === since) continue;
      // The first run that ended after this window asked for a plan.
      planned.delete(project.id);
      if (project.run.action === "plan" && project.run.state === "ok" && !project.remote) {
        const data: PlanPaneData = { projectId: project.id, path: project.path };
        ctx.openPane({ view: "plan", title: planPaneTitle(project.name), data, reuseKey: planPaneKey(project.id) });
      }
    }
    const badge = badgeCount(next.projects);
    setStatus(badge > 0 ? badge : null, alert);
    snapshot = next;
    // A run can add folders, modules, and a data folder: walk again.
    if (ended) {
      scanNext = true;
      poller.poke();
    }
    emit();
  };

  const poller = createPoller({
    tick: async () => {
      const scan = scanNext;
      scanNext = false;
      const answer = await fetchState(ctx, ctx.folders(), scan);
      if (answer.ok) {
        apply(answer.value);
      } else if (answer.failure.type === "gone") {
        // Uninstalled in another window: stop quietly and let westerm read the list again.
        poller.stop();
        ctx.refreshInstalled();
      } else if (scan) {
        scanNext = true;
      }
    },
    interval: () => (open ? OPEN_INTERVAL : (snapshot?.projects ?? []).some(isRunning) ? RUNNING_INTERVAL : IDLE_INTERVAL),
    hidden: () => document.visibilityState === "hidden",
  });

  const onVisibility = () => {
    if (document.visibilityState !== "visible") return;
    scanNext = true;
    poller.poke();
  };
  document.addEventListener("visibilitychange", onVisibility);
  poller.start();

  return {
    state: () => snapshot,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    status: () => status,
    async run(projectId, action, target, planAt) {
      const project = snapshot?.projects.find((each) => each.id === projectId);
      if (!project) return "removed";
      const answer = await startRun(ctx, projectId, action, target, planAt);
      if (!answer.ok) return answer.failure.type === "refused" || answer.failure.type === "error" ? answer.failure.error : answer.failure.type;
      if (action === "plan") planned.set(projectId, project.run?.at ?? null);
      ctx.openShell({ action: "run", target: answer.value.token, cwd: project.path, name: answer.value.name, reuse: false, replaceKey: `run:${projectId}` });
      poller.poke();
      return null;
    },
    refresh(scan = false) {
      if (scan) scanNext = true;
      poller.poke();
    },
    setPanelOpen(next) {
      if (next === open) return;
      open = next;
      if (!open) return;
      if (status.alert) {
        setStatus(status.badge, false);
        emit();
      }
      scanNext = true;
      poller.poke();
    },
    dispose() {
      poller.stop();
      document.removeEventListener("visibilitychange", onVisibility);
      listeners.clear();
    },
  };
}
