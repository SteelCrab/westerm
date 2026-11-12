// What a plugin's panel learns about where the user is. Plugins do not read
// the dashboard themselves (tests/plugin-boundary.test.ts); App hands them
// this instead.

import type { PluginPlace } from "./plugins/types.ts";
import { activeTab, activeWorkspace, workspaceTabs, type Dashboard, type Session } from "./workspace.ts";

const hasFolder = (session: Session) => session.kind === "shell" || session.kind === "docs";

const folderOf = (session: Session) => session.place?.cwd ?? session.agent?.cwd ?? session.cwd ?? session.docs?.root ?? null;

/** The workspace in view, its panes' folders, and the folder of the focused pane. */
export function pluginPlace(state: Dashboard): PluginPlace {
  const workspace = activeWorkspace(state).id;
  const ids = new Set(workspaceTabs(state, workspace).flatMap((tab) => tab.paneIds));
  const folders = [
    ...new Set(
      state.sessions
        .filter((session) => ids.has(session.id) && hasFolder(session))
        .map(folderOf)
        .filter((path): path is string => path !== null),
    ),
  ];
  const focused = state.sessions.find((session) => session.id === activeTab(state).focusedId);
  return { workspace, focused: (focused && hasFolder(focused) ? folderOf(focused) : null) ?? folders[0] ?? null, folders };
}
