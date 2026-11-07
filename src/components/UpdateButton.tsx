import { useEffect, useRef } from "react";
import { CircleArrowUp, ExternalLink, LoaderCircle } from "lucide-react";
import { useI18n } from "../i18n/index.tsx";
import type { UpdateStatus } from "../../shared/protocol.ts";
import { appBridge } from "../appBridge.ts";
import type { UpdateState } from "../useUpdate.ts";

interface UpdateButtonProps {
  update: UpdateState;
  /** The details panel; also opened from the palette's "Check for updates". */
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** Whether the button shows: a newer release exists, or the panel was opened (from the palette). */
export function updateShown(status: UpdateStatus | null, open: boolean): boolean {
  return open || Boolean(status?.newer && status.latest);
}

/** Header button that appears when a newer release exists, with its details panel. */
export function UpdateButton({ update, open, onOpenChange }: UpdateButtonProps) {
  const { t } = useI18n();
  const { status, phase, error } = update;
  const root = useRef<HTMLDivElement>(null);
  const newer = Boolean(status?.newer && status.latest);
  const busy = phase === "installing" || phase === "restarting";

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!busy && !root.current?.contains(event.target as Node)) onOpenChange(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onOpenChange(false);
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, busy, onOpenChange]);

  if (!updateShown(status, open)) return null;

  const body = () => {
    if (!status) return <p>{t.update.checkFailed}</p>;
    if (status.disabled) return <p>{t.update.disabled}</p>;
    if (!status.latest) {
      return (
        <>
          <p>{t.update.checkFailed}</p>
          {status.error && <p className="update-detail">{status.error}</p>}
        </>
      );
    }
    if (!newer) return <p>{t.update.upToDate(status.current)}</p>;

    let action;
    if (status.channel === "source") {
      action = <p>{t.update.source}</p>;
    } else if (status.channel === "app" && !appBridge()) {
      action = <p>{t.update.app}</p>;
    } else if (phase === "manual") {
      action = <p className="update-note">{t.update.manual}</p>;
    } else if (phase === "installing" || phase === "restarting") {
      action = (
        <p className="update-progress" role="status">
          <LoaderCircle size={14} className="spin" aria-hidden="true" />
          {phase === "installing" ? t.update.installing : t.update.restarting}
        </p>
      );
    } else {
      action = (
        <>
          {phase === "failed" && error && <p className="update-error" role="alert">{t.update.failed(error)}</p>}
          {phase === "timeout" && <p className="update-error" role="alert">{t.update.timeout}</p>}
          <p className="update-note">{t.update.restartNote}</p>
          <div className="update-actions">
            <button type="button" className="button primary" onClick={update.install}>
              {status.channel === "app" ? t.update.appInstall : t.update.install}
            </button>
            <button type="button" className="button" onClick={() => onOpenChange(false)}>
              {t.update.later}
            </button>
          </div>
        </>
      );
    }
    return (
      <>
        <h2>{t.update.heading(status.latest)}</h2>
        <p className="update-detail">{t.update.current(status.current)}</p>
        {status.page && (
          <a className="update-link" href={status.page} target="_blank" rel="noreferrer">
            {t.update.notes} <ExternalLink size={12} aria-hidden="true" />
          </a>
        )}
        {action}
      </>
    );
  };

  return (
    <div className="update" ref={root}>
      {newer && status?.latest && (
        <button
          type="button"
          className="update-button"
          aria-expanded={open}
          aria-haspopup="dialog"
          onClick={() => onOpenChange(!open)}
        >
          <CircleArrowUp size={13} aria-hidden="true" />
          {t.update.button(status.latest)}
        </button>
      )}
      {open && (
        <div className="update-panel" role="dialog" aria-label={newer && status?.latest ? t.update.heading(status.latest) : t.palette.checkUpdates}>
          {body()}
          {!newer && (
            <div className="update-actions">
              <button type="button" className="button" onClick={() => onOpenChange(false)}>
                {t.update.close}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
