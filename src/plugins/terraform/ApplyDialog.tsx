// The confirmation before an apply: what the saved plan does, where, and
// what it destroys. A production project also asks for its folder's name.

import { useCallback, useState, type ReactNode } from "react";
import { useI18n } from "../../i18n/index.tsx";
import type { PluginContext } from "../types.ts";
import { fetchPlan } from "./api.ts";
import { TerraformLogo } from "./icon.tsx";
import { TF_TEXT } from "./i18n.ts";
import { age, confirmWord, destroyList, isProd } from "./model.ts";
import type { TerraformStore } from "./store.ts";
import type { PlanAnswer, ProjectInfo } from "./types.ts";

export interface ApplyDialogProps {
  project: ProjectInfo;
  plan: PlanAnswer;
  onApply(): void;
  onClose(): void;
  /** Why the server refused the apply: a code or an error line. */
  refusal: string | null;
}

export function ApplyDialog({ project, plan, onApply, onClose, refusal }: ApplyDialogProps) {
  const { locale } = useI18n();
  const t = TF_TEXT[locale];
  const [typed, setTyped] = useState("");
  const prod = isProd(project.name, plan.workspace);
  const word = confirmWord(project.path);
  const destroys = destroyList(plan);
  const since = age(Date.now(), plan.at);
  const counts = plan.counts;
  const blocked = refusal === "stale" || (prod && typed !== word);
  return (
    <div className="tf-apply-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div className="tf-apply" role="dialog" aria-modal="true" aria-label={t.apply}>
        <header className="tf-apply-header">
          <TerraformLogo size={14} />
          <span className="tf-title">{t.apply}</span>
          {prod && <span className="tf-tag tf-tag-prod">{t.tagProd}</span>}
        </header>
        <div className="tf-apply-body">
          <dl className="tf-apply-facts">
            <dt>{t.project}</dt>
            <dd className="tf-mono">{project.path}</dd>
            <dt>{t.workspaceLabel}</dt>
            <dd className="tf-mono">{plan.workspace}</dd>
            <dt>{t.plan}</dt>
            <dd>
              {t.ago(since.n, since.unit)}
              {counts && (
                <>
                  {" · "}
                  <span className="tf-mark-create">{`+${counts.add}`}</span> <span className="tf-mark-update">{`~${counts.change}`}</span>{" "}
                  <span className={counts.destroy > 0 ? "tf-mark-delete" : "tf-mark-none"}>{`-${counts.destroy}`}</span>
                </>
              )}
            </dd>
            {plan.target !== null && (
              <>
                <dt />
                <dd className="tf-mono">{t.target(plan.target)}</dd>
              </>
            )}
          </dl>
          {destroys.shown.length > 0 && (
            <div className="tf-destroys">
              <strong>{t.willDestroy(Math.max(counts?.destroy ?? 0, plan.destroys.length))}</strong>
              <ul>
                {destroys.shown.map((address) => (
                  <li key={address} className="tf-mono">
                    {address}
                  </li>
                ))}
              </ul>
              {destroys.more > 0 && <span>{t.andMore(destroys.more)}</span>}
            </div>
          )}
          {prod && (
            <label className="tf-apply-word">
              <span>{t.typeToContinue(word)}</span>
              <input type="text" value={typed} aria-label={t.typeToContinue(word)} spellCheck={false} autoComplete="off" onChange={(event) => setTyped(event.target.value)} />
            </label>
          )}
          {refusal !== null && <p className="tf-notice">{t.refusals[refusal] ?? refusal}</p>}
        </div>
        <footer className="tf-apply-footer">
          <span className="tf-quiet">{t.appliesSaved}</span>
          <button type="button" className="button" onClick={onClose}>
            {t.cancel}
          </button>
          <button type="button" className={`button ${(counts?.destroy ?? 0) > 0 || plan.destroys.length > 0 ? "danger tf-danger" : "primary"}`} disabled={blocked} onClick={onApply}>
            {t.apply}
          </button>
        </footer>
      </div>
    </div>
  );
}

/** What `useApply` keeps while the confirmation shows. */
interface Asked {
  id: string;
  plan: PlanAnswer;
  refusal: string | null;
}

/**
 * The confirmation, for the panel and the plan pane: `openApply` reads the
 * project's plan again and shows the dialog; Apply starts the run. A refusal
 * stays in the dialog.
 */
export function useApply(ctx: PluginContext, store: TerraformStore): { openApply(projectId: string): void; dialog: ReactNode } {
  const [asked, setAsked] = useState<Asked | null>(null);

  const openApply = useCallback(
    (id: string) => {
      void fetchPlan(ctx, id).then((answer) => {
        // Only a plan that can be applied is confirmed; anything else shows in the panel and the pane.
        if (answer.ok && answer.value !== null && (answer.value.state === "current" || answer.value.state === "stale")) {
          setAsked({ id, plan: answer.value, refusal: answer.value.state === "stale" ? "stale" : null });
        }
      });
    },
    [ctx],
  );

  const project = asked ? store.state()?.projects.find((each) => each.id === asked.id) : undefined;
  const dialog =
    asked && project ? (
      <ApplyDialog
        project={project}
        plan={asked.plan}
        refusal={asked.refusal}
        onClose={() => setAsked(null)}
        onApply={() => {
          // The plan the dialog shows, by its time: no other plan is applied.
          void store.run(asked.id, "apply", null, asked.plan.at).then((refused) => setAsked((current) => (refused === null || current === null ? null : { ...current, refusal: refused })));
        }}
      />
    ) : null;
  return { openApply, dialog };
}
