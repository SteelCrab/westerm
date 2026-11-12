// The agent states the UI shows. Apart from workspace.ts so that the UI text
// (i18n/messages.ts) can name them without importing the workspace state.

import type { AgentStatus } from "../shared/protocol.ts";

/** What an agent's mark shows: its status, or "done" until its pane is seen. */
export type AgentDisplay = AgentStatus | "done";
/** What a tab shows for its agents; an idle agent shows nothing there. */
export type TabAgentDisplay = Exclude<AgentDisplay, "idle">;
