import { LoaderCircle, RotateCcw } from "lucide-react";
import {
  isPermanentFailure,
  REMOTE_DECISION_PATH,
  REMOTE_RETRY_PATH,
  type RemoteStage,
} from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import type { Messages } from "../i18n/messages.ts";
import { RemoteWindowIcon } from "../kinds.tsx";
import { postRemote } from "../remoteApi.ts";

/** What a stage of the link says: a step, the way it connected, or why it failed. */
export function stageText(stage: RemoteStage | null, t: Messages): string {
  switch (stage?.stage) {
    case undefined:
    case "connecting":
      return t.remote.connecting;
    case "checking":
      return t.remote.checking;
    case "installing":
      return t.remote.installing(stage.version);
    case "starting":
      return t.remote.starting;
    case "confirm":
      return t.remote.confirm(stage.sessions);
    case "connected":
      return [t.remote.via[stage.via], stage.killOnLogout && t.remote.killOnLogout].filter(Boolean).join(" · ");
    case "failed":
      return t.remote.failed[stage.reason];
  }
}

interface RemoteStatusProps {
  target: string;
  stage: RemoteStage | null;
  /** Text before the stage while the dashboard keeps trying, e.g. "Reconnecting". */
  waiting?: string;
}

/**
 * The link's stage with what the user can do about it: answer the host
 * replacement question, or try again after a failure that does not pass on
 * its own. Anything else is progress; the dashboard keeps trying.
 */
function RemoteStatus({ target, stage, waiting }: RemoteStatusProps) {
  const { t } = useI18n();
  if (stage?.stage === "confirm") {
    return (
      <>
        <p className="remote-message">{stageText(stage, t)}</p>
        <div className="remote-actions">
          <button
            type="button"
            className="button primary"
            onClick={() => void postRemote(REMOTE_DECISION_PATH, { target, replace: true })}
          >
            {t.remote.replace}
          </button>
          <button
            type="button"
            className="button"
            onClick={() => void postRemote(REMOTE_DECISION_PATH, { target, replace: false })}
          >
            {t.remote.keep}
          </button>
        </div>
      </>
    );
  }
  if (stage?.stage === "failed" && isPermanentFailure(stage.reason)) {
    return (
      <>
        <p className="remote-message remote-error" role="alert">
          {stageText(stage, t)}
        </p>
        {stage.detail && <p className="remote-detail">{stage.detail}</p>}
        <div className="remote-actions">
          <button
            type="button"
            className="button primary"
            onClick={() => void postRemote(REMOTE_RETRY_PATH, { target })}
          >
            <RotateCcw size={14} /> {t.remote.retry}
          </button>
        </div>
      </>
    );
  }
  const text = waiting ? [waiting, stage && stageText(stage, t)].filter(Boolean).join(" · ") : stageText(stage, t);
  return (
    <>
      <p className="remote-message remote-progress" role="status">
        <LoaderCircle size={14} className="spin" aria-hidden="true" />
        {text}
      </p>
      {stage?.stage === "failed" && stage.detail && <p className="remote-detail">{stage.detail}</p>}
    </>
  );
}

/** Covers the whole window until the link to the server is up for the first time. */
export function RemoteConnect({ target, stage }: { target: string; stage: RemoteStage | null }) {
  const { t } = useI18n();
  return (
    <div className="remote-connect" role="dialog" aria-modal="true" aria-labelledby="remote-connect-title">
      <p id="remote-connect-title" className="remote-connect-title">
        <RemoteWindowIcon size={16} />
        {t.remote.connectingTo(target)}
      </p>
      <RemoteStatus target={target} stage={stage} />
    </div>
  );
}

/** A strip above the panes while a link that was up is down. The panes keep reconnecting. */
export function RemoteBanner({ target, stage }: { target: string; stage: RemoteStage | null }) {
  const { t } = useI18n();
  return (
    <div className="remote-banner">
      <RemoteStatus target={target} stage={stage} waiting={t.remote.reconnecting} />
    </div>
  );
}
