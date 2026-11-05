import { useCallback, useEffect, useRef, useState } from "react";
import { WORKSPACE_PATH, parseSavedDashboard, type SavedDashboard } from "../shared/workspace-file.ts";
import { REMOTE_TARGET, serverPath } from "./remoteWindow.ts";
import { toSaved, type Dashboard, type DashboardAction } from "./workspace.ts";

const SAVE_DELAY_MS = 400;
/** A remote window reads its server's layout again this soon after a failed read. */
const RETRY_MS = 2000;

/**
 * Sends one layout; true on a 2xx answer. After a failure the next change
 * tries again; the PTY host keeps the processes regardless.
 */
function save(body: string, keepalive = false): Promise<boolean> {
  return fetch(serverPath(WORKSPACE_PATH), {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body,
    keepalive,
  }).then(
    (response) => response.ok,
    () => false,
  );
}

/** A read of the saved layout. `saved` is null when nothing valid is saved. */
export type LoadResult = { ok: true; saved: SavedDashboard | null } | { ok: false };

/** The saved layout; not `ok` when the dashboard or the server did not answer. */
export function loadSaved(): Promise<LoadResult> {
  return fetch(serverPath(WORKSPACE_PATH), { cache: "no-store" })
    .then(async (response): Promise<LoadResult> => {
      if (!response.ok) return { ok: false };
      return { ok: true, saved: parseSavedDashboard(await response.json()) };
    })
    .catch((): LoadResult => ({ ok: false }));
}

/**
 * What the first read leads to. A remote window never starts fresh after a
 * failed read: the server may hold a layout, and the first save would replace
 * it. A layout with nothing but v0.2.0 remote panes starts fresh, since none
 * of them is left after loading.
 */
export function firstLoad(remote: boolean, result: LoadResult): "hydrate" | "fresh" | "retry" {
  if (!result.ok) return remote ? "retry" : "fresh";
  const { saved } = result;
  return saved && saved.sessions.length > (saved.legacyRemoteIds?.length ?? 0) ? "hydrate" : "fresh";
}

/**
 * What a read leads to when this window takes the sessions back from another
 * window. A remote window stays away after a failed read (usually a 503
 * while its link is down). Its own layout may be stale, and the next save
 * would replace the other window's newer one on the server. The user can
 * click again once the link is up. A local window takes them back as before.
 */
export function reclaimAction(remote: boolean, result: LoadResult): "stay" | { saved: SavedDashboard | null } {
  if (result.ok) return { saved: result.saved };
  return remote ? "stay" : { saved: null };
}

/** A layout as compared between reads and writes: its checked form. */
export function layoutKey(saved: SavedDashboard | null): string {
  return JSON.stringify(saved);
}

/** The key of a layout this window wrote, as the server reads it back. */
export function writtenKey(body: string): string {
  return layoutKey(parseSavedDashboard(JSON.parse(body)));
}

/**
 * What a read leads to when a remote window's link comes back. `synced`: the
 * key of the layout it last read from or wrote to the server. `dirty`: a save
 * failed since. A failed read tries again later, never saving blind. A
 * changed server layout is another window's: this window takes it. With no
 * layout on the server there is nothing to lose, so this one is saved.
 */
export function reconnectAction(result: LoadResult, synced: string, dirty: boolean): "save" | "adopt" | "none" | "retry" {
  if (!result.ok) return "retry";
  if (layoutKey(result.saved) === synced) return dirty ? "save" : "none";
  return result.saved ? "adopt" : "save";
}

/** What the hook gives the window. */
export interface Persistence {
  /** False until the first load settles, so panes do not attach before their saved ids are known. */
  loaded: boolean;
  /** This window took `saved` from the server (null: none was there), as `reclaim` does. */
  adopted: (saved: SavedDashboard | null) => void;
}

/**
 * Loads the saved layout once, then saves every change. `fresh` runs when
 * nothing was saved. `paused`: another window has the sessions, and its
 * layout is the one to keep. `link`: a remote window's link to its server; a
 * local window passes `{ ready: true, reconnects: 0 }`. The first read waits
 * until the link is `ready`. After each rise of `reconnects`, the window
 * reads the server's layout again before it saves anything (see
 * `reconnectAction`): while it was away, another computer may have changed it.
 */
export function useWorkspacePersistence(
  state: Dashboard,
  dispatch: (action: DashboardAction) => void,
  fresh: () => void,
  paused: boolean,
  link: { ready: boolean; reconnects: number },
): Persistence {
  const [loaded, setLoaded] = useState(false);
  // Bumped to read again after a failed read.
  const [attempt, setAttempt] = useState(0);
  const latest = useRef(state);
  latest.current = state;
  const linkNow = useRef(link);
  linkNow.current = link;
  // The layout last read from or written to the server, and whether a save
  // failed (or was held back) since.
  const synced = useRef(layoutKey(null));
  const dirty = useRef(false);
  // The value of `reconnects` whose server layout was read. A remote window
  // saves nothing until it has read the layout after the latest reconnect.
  const checked = useRef(0);

  const persist = useCallback((keepalive = false) => {
    const { ready, reconnects } = linkNow.current;
    if (!ready || checked.current !== reconnects) {
      dirty.current = true;
      return;
    }
    const body = JSON.stringify(toSaved(latest.current));
    void save(body, keepalive).then((ok) => {
      if (ok) synced.current = writtenKey(body);
      dirty.current = !ok;
    });
  }, []);

  const adopted = useCallback((saved: SavedDashboard | null) => {
    synced.current = layoutKey(saved);
    dirty.current = false;
  }, []);

  useEffect(() => {
    if (loaded || !link.ready) return;
    let cancelled = false;
    let retry: ReturnType<typeof setTimeout> | null = null;
    void loadSaved().then((result) => {
      if (cancelled) return;
      const next = firstLoad(REMOTE_TARGET !== null, result);
      if (next === "retry") {
        retry = setTimeout(() => setAttempt((value) => value + 1), RETRY_MS);
        return;
      }
      synced.current = layoutKey(result.ok ? result.saved : null);
      checked.current = linkNow.current.reconnects;
      if (next === "hydrate" && result.ok && result.saved) dispatch({ type: "hydrate", saved: result.saved });
      else fresh();
      setLoaded(true);
    });
    return () => {
      cancelled = true;
      if (retry) clearTimeout(retry);
    };
    // Loads once per page. A failed read tries again after a delay or when
    // the link comes up again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loaded, link.ready, attempt]);

  // The link came back: read the server's layout, then save, take it, or
  // leave both alone. A failed read tries again after a delay.
  useEffect(() => {
    if (!loaded || paused || link.reconnects === checked.current) return;
    const reconnects = link.reconnects;
    let cancelled = false;
    let retry: ReturnType<typeof setTimeout> | null = null;
    const check = () => {
      void loadSaved().then((result) => {
        if (cancelled) return;
        const next = reconnectAction(result, synced.current, dirty.current);
        if (next === "retry") {
          retry = setTimeout(check, RETRY_MS);
          return;
        }
        checked.current = reconnects;
        if (next === "adopt" && result.ok && result.saved) {
          adopted(result.saved);
          dispatch({ type: "adopt", saved: result.saved });
        } else if (next === "save") {
          persist();
        }
      });
    };
    check();
    return () => {
      cancelled = true;
      if (retry) clearTimeout(retry);
    };
  }, [loaded, paused, link.reconnects, dispatch, persist, adopted]);

  useEffect(() => {
    if (!loaded || paused) return;
    const timer = setTimeout(() => persist(), SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [loaded, paused, state, persist]);

  // A reload or closed tab must not lose the last change inside the delay.
  // Neither may leaving the window: another window may take the sessions
  // and load this layout.
  useEffect(() => {
    if (!loaded || paused) return;
    const flush = () => persist(true);
    window.addEventListener("pagehide", flush);
    window.addEventListener("blur", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      window.removeEventListener("blur", flush);
    };
  }, [loaded, paused, persist]);

  return { loaded, adopted };
}
