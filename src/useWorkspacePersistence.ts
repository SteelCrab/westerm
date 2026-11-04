import { useEffect, useRef, useState } from "react";
import { WORKSPACE_PATH, parseSavedWorkspace } from "../shared/workspace-file.ts";
import { toSaved, type Workspace, type WorkspaceAction } from "./workspace.ts";

const SAVE_DELAY_MS = 400;

function save(state: Workspace, keepalive = false): void {
  void fetch(WORKSPACE_PATH, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(toSaved(state)),
    keepalive,
  }).catch(() => {
    // The next change retries; the PTY host keeps the processes regardless.
  });
}

/**
 * Loads the saved layout once, then saves every change. Returns false until
 * the load settles, so panes do not attach before their saved ids are known.
 * `fresh` runs when nothing was saved.
 */
export function useWorkspacePersistence(
  state: Workspace,
  dispatch: (action: WorkspaceAction) => void,
  fresh: () => void,
): boolean {
  const [loaded, setLoaded] = useState(false);
  const latest = useRef(state);
  latest.current = state;

  useEffect(() => {
    let cancelled = false;
    fetch(WORKSPACE_PATH, { cache: "no-store" })
      .then((response) => (response.ok ? response.json() : null))
      .catch(() => null)
      .then((value: unknown) => {
        if (cancelled) return;
        const saved = parseSavedWorkspace(value);
        if (saved && saved.sessions.length > 0) dispatch({ type: "hydrate", saved });
        else fresh();
        setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
    // Load once per page.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!loaded) return;
    const timer = setTimeout(() => save(state), SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [loaded, state]);

  // A reload or closed tab must not lose the last change inside the delay.
  useEffect(() => {
    if (!loaded) return;
    const flush = () => save(latest.current, true);
    window.addEventListener("pagehide", flush);
    return () => window.removeEventListener("pagehide", flush);
  }, [loaded]);

  return loaded;
}
