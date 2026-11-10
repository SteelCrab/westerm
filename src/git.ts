import { REMOTE_TARGET } from "./remoteWindow.ts";
import { activeTab, activeWorkspace, workspaceTabs, type Dashboard, type Session } from "./workspace.ts";

export interface GitFile {
  path: string;
  oldPath: string | null;
  index: string;
  worktree: string;
  conflict: boolean;
}

export interface GitStatus {
  root: string;
  branch: string | null;
  head: string | null;
  branches: string[];
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFile[];
  commits: { hash: string; message: string; author: string; time: string }[];
}

export interface GitEntry extends GitFile {
  staged: boolean;
  status: string;
}

export interface GitCommitDetail {
  hash: string;
  message: string;
  author: string;
  email: string;
  time: string;
  parents: string[];
  files: { path: string; oldPath: string | null; status: string }[];
}

export type GitOperation =
  | { action: "stage" | "unstage"; paths: string[] }
  | { action: "commit"; message: string }
  | { action: "switch"; branch: string }
  | { action: "pull" | "push" };

export type GitResult<T> = { ok: true; value: T } | { ok: false; error: string };

export function gitPath(route: "status" | "diff" | "action" | "commit" | "commit-diff", query?: Record<string, string>, target: string | null = REMOTE_TARGET) {
  const prefix = target === null ? "" : `/remote/${encodeURIComponent(target)}`;
  return `${prefix}/api/git/${route}${query ? `?${new URLSearchParams(query)}` : ""}`;
}

async function call<T>(path: string, init?: RequestInit): Promise<GitResult<T>> {
  try {
    const response = await fetch(path, { cache: "no-store", ...init });
    const body = await response.json().catch(() => null);
    if (!response.ok) return { ok: false, error: typeof body?.error === "string" ? body.error : `HTTP ${response.status}` };
    if (body === null) return { ok: false, error: "The Git server returned an invalid response. Restart westerm with the latest build." };
    return { ok: true, value: body as T };
  } catch {
    return { ok: false, error: "Cannot reach the Git server" };
  }
}

export const fetchGitStatus = (dir: string, signal?: AbortSignal) => call<GitStatus>(gitPath("status", { dir }), { signal });
export const fetchGitDiff = (dir: string, entry: Pick<GitEntry, "path" | "staged">, signal?: AbortSignal) =>
  call<{ text: string }>(gitPath("diff", { dir, path: entry.path, staged: String(entry.staged) }), { signal });
export const fetchGitCommit = (dir: string, hash: string, signal?: AbortSignal) =>
  call<GitCommitDetail>(gitPath("commit", { dir, hash }), { signal });
export const fetchGitCommitDiff = (dir: string, hash: string, path: string, signal?: AbortSignal) =>
  call<{ text: string }>(gitPath("commit-diff", { dir, hash, path }), { signal });
export const runGitOperation = (dir: string, operation: GitOperation) => call<Record<string, never>>(gitPath("action"), {
  method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ dir, ...operation }),
});

/** A partially staged file appears in both groups, with independent diffs. */
export function gitEntries(files: GitFile[]): { staged: GitEntry[]; changes: GitEntry[] } {
  const staged: GitEntry[] = [];
  const changes: GitEntry[] = [];
  for (const file of files) {
    if (!file.conflict && file.index !== " " && file.index !== "?") staged.push({ ...file, staged: true, status: file.index });
    if (file.conflict || (file.worktree !== " " && file.worktree !== "")) {
      changes.push({ ...file, staged: false, status: file.conflict ? "U" : file.worktree });
    }
  }
  return { staged, changes };
}

const sessionFolder = (session: Session) => session.place?.cwd ?? session.agent?.cwd ?? session.cwd ?? session.docs?.root ?? null;

export function gitFolders(state: Dashboard): string[] {
  const ids = new Set(workspaceTabs(state, activeWorkspace(state).id).flatMap((tab) => tab.paneIds));
  return [...new Set(state.sessions.filter((session) => ids.has(session.id) && (session.kind === "shell" || session.kind === "docs"))
    .map(sessionFolder).filter((path): path is string => path !== null))];
}

export function focusedGitFolder(state: Dashboard): string | null {
  const focused = state.sessions.find((session) => session.id === activeTab(state).focusedId);
  return (focused && (focused.kind === "shell" || focused.kind === "docs") ? sessionFolder(focused) : null) ?? gitFolders(state)[0] ?? null;
}

export const sameGitEntry = (a: Pick<GitEntry, "path" | "staged"> | null, b: Pick<GitEntry, "path" | "staged">) =>
  a?.path === b.path && a.staged === b.staged;
