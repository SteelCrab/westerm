import { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import type { AgentState } from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import { HTML_LANG } from "../i18n/messages.ts";
import { useCopy } from "../useCopy.ts";
import type { AgentDisplay } from "../workspace.ts";

/** Whole minutes from `time` to `now`, both in milliseconds since the epoch. */
export function minutesSince(time: number, now: number): number {
  return Math.max(0, Math.floor((now - time) / 60_000));
}

/** "~/westerm" for a folder in the user's home on macOS or Linux; other paths as they are. */
export function homeShort(path: string): string {
  return path.replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, "~");
}

/** The time now, again every `ms`. */
export function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(timer);
  }, [ms]);
  return now;
}

function CopyButton({ text, label, onFailed }: { text: string; label: string; onFailed: () => void }) {
  const { t } = useI18n();
  const [result, copy] = useCopy();
  useEffect(() => {
    if (result === "failed") onFailed();
  }, [result, onFailed]);
  return (
    <button type="button" className="agent-card-copy" aria-label={`${t.agent.card.copy}: ${label}`} onClick={() => copy(text)}>
      {result === "copied" ? <Check size={12} aria-hidden="true" /> : <Copy size={12} aria-hidden="true" />}
      {result === "copied" ? t.agent.card.copied : t.agent.card.copy}
    </button>
  );
}

interface AgentCardProps {
  agent: AgentState;
  display: AgentDisplay;
  /** Settings ▸ Terminal lets restored shells resume their agents. */
  resumeAllowed: boolean;
  /** This pane's own choice. */
  autoResume: boolean;
  onAutoResume: (on: boolean) => void;
  onClose: () => void;
  /** The mark that opened the card: a press there toggles the card, so it does not close it here. */
  anchor: React.RefObject<HTMLElement | null>;
}

/** What the pane's AI mark opens: the agent's state, its conversation, and how it resumes. */
export function AgentCard({ agent, display, resumeAllowed, autoResume, onAutoResume, onClose, anchor }: AgentCardProps) {
  const { t, locale } = useI18n();
  const root = useRef<HTMLDivElement>(null);
  const now = useNow(15_000);
  const [copyFailed, setCopyFailed] = useState(false);
  const failed = useRef(() => setCopyFailed(true)).current;

  useEffect(() => {
    const onPointer = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!root.current?.contains(target) && !anchor.current?.contains(target)) onClose();
    };
    // Capture: the terminal keeps the keys it handles from bubbling.
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose, anchor]);

  const card = t.agent.card;
  const name = t.agent.names[agent.kind];
  // A finished agent tells when it finished; the others how long they have been at it.
  const elapsed = agent.since && (display === "done" ? card.ago : card.for)(minutesSince(agent.since, now));
  const started =
    agent.started &&
    `${new Date(agent.started).toLocaleTimeString(HTML_LANG[locale], { hour: "numeric", minute: "2-digit" })} · ${card.ago(minutesSince(agent.started, now))}`;

  return (
    <div className="agent-card" role="dialog" aria-label={card.label(name)} ref={root}>
      <div className="agent-card-head">
        <strong className={`agent-card-name agent-${agent.kind}`}>{name}</strong>
        {agent.version && <span className="agent-card-version">v{agent.version}</span>}
        <span className={`agent-card-state agent-${display}`}>{elapsed ? `${t.agent.states[display]} · ${elapsed}` : t.agent.states[display]}</span>
      </div>
      <dl className="agent-card-rows">
        <dt>{card.conversation}</dt>
        <dd>
          {agent.conversation ? (
            <>
              <code title={agent.conversation}>{agent.conversation.slice(0, 8)}</code>
              <CopyButton text={agent.conversation} label={card.conversation} onFailed={failed} />
            </>
          ) : (
            <span className="agent-card-muted">{card.latest}</span>
          )}
        </dd>
        {agent.cwd && (
          <>
            <dt>{card.folder}</dt>
            <dd>
              <code className="agent-card-path" title={agent.cwd}>
                {homeShort(agent.cwd)}
              </code>
            </dd>
          </>
        )}
        {agent.model && (
          <>
            <dt>{card.model}</dt>
            <dd>{agent.model}</dd>
          </>
        )}
        {started && (
          <>
            <dt>{card.started}</dt>
            <dd>{started}</dd>
          </>
        )}
      </dl>
      {agent.resume && (
        <div className="agent-card-resume">
          <p className="agent-card-muted">{card.command}</p>
          <div className="agent-card-command">
            <code title={agent.resume}>{agent.resume}</code>
            <CopyButton text={agent.resume} label={card.command} onFailed={failed} />
          </div>
        </div>
      )}
      {copyFailed && (
        <p className="agent-card-error" role="alert">
          {card.copyFailed}
        </p>
      )}
      <label className="agent-card-check">
        <input
          type="checkbox"
          checked={resumeAllowed && autoResume}
          disabled={!resumeAllowed}
          onChange={(event) => onAutoResume(event.target.checked)}
        />
        <span>
          {card.autoResume}
          {!resumeAllowed && <span className="agent-card-muted"> · {card.autoResumeOff}</span>}
        </span>
      </label>
    </div>
  );
}
