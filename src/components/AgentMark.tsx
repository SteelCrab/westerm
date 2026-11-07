import type { AgentState } from "../../shared/protocol.ts";
import { AgentIcon } from "../agentIcons.tsx";
import { useI18n } from "../i18n/index.tsx";
import type { AgentDisplay, TabAgentDisplay } from "../workspace.ts";

/**
 * The AI mark of a pane that runs an agent, in place of its kind icon. Its
 * logo names the agent; the ring around it shows the state: turning while
 * the agent works, pulsing while it asks, green once it finished out of view.
 */
export function AgentMark({ agent, display, size, detail }: { agent: AgentState; display: AgentDisplay; size: number; detail?: string }) {
  const { t } = useI18n();
  const label = `${t.agent.names[agent.kind]} · ${t.agent.states[display]}`;
  return (
    <span
      className={`agent-mark agent-${agent.kind} agent-${display}`}
      role="img"
      aria-label={label}
      title={detail ? `${label}\n${detail}` : label}
      style={{ "--agent-mark": `${size + 6}px` } as React.CSSProperties}
    >
      <AgentIcon kind={agent.kind} size={size - 2} />
    </span>
  );
}

/** A tab's sign that one of its agents works, asks, or finished. */
export function TabAgentMark({ display }: { display: TabAgentDisplay }) {
  const { t } = useI18n();
  return <span className={`tab-agent tab-agent-${display}`} role="img" aria-label={t.agent.tab[display]} title={t.agent.tab[display]} />;
}
