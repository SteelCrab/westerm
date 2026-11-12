// Pure logic behind the repositories of the agents sidebar: a folder's
// worktrees and pull requests from the server, and the folders grouped by
// repository.

import { createReadCache, folderLabel, type AgentFolder, type ReadCache } from "./agents.ts";
import { REMOTE_TARGET } from "./remoteWindow.ts";

export type PrChecks = "passing" | "failing" | "running" | "none";

const PR_CHECKS: readonly string[] = ["passing", "failing", "running", "none"];

/** An open pull request of a worktree's branch. */
export interface PullRequest {
  number: number;
  title: string;
  /** An https link: it opens in a browser pane. */
  url: string;
  draft: boolean;
  checks: PrChecks;
  /** The checks that failed. */
  failed: number;
}

export interface Worktree {
  path: string;
  /** null on a detached HEAD. */
  branch: string | null;
  /** The first 7 characters of the commit. */
  head: string;
  pr: PullRequest | null;
}

/** What the server tells of a folder's repository. */
export interface RepoInfo {
  /** The main worktree: the same for every folder of the repository. */
  root: string;
  /** `owner/repo` on the remote; null without one. */
  name: string | null;
  /** The path of the worktree the folder is in, as `worktrees` lists it. */
  current: string | null;
  /** GitHub answered: a worktree without a pull request has none. */
  prs: boolean;
  worktrees: Worktree[];
}

/** Where this window asks for a folder's worktrees: through `/remote/<target>` in a remote window. */
export function worktreesPath(dir: string, target: string | null = REMOTE_TARGET): string {
  const base = target !== null ? `/remote/${encodeURIComponent(target)}/api/agents/worktrees` : "/api/agents/worktrees";
  return `${base}?${new URLSearchParams({ dir })}`;
}

const textOrNull = (value: unknown): value is string | null => value === null || typeof value === "string";

function parsePullRequest(value: unknown): PullRequest | null {
  if (!value || typeof value !== "object") return null;
  const { number, title, url, draft, checks, failed } = value as Record<string, unknown>;
  if (typeof number !== "number" || !Number.isFinite(number) || typeof title !== "string") return null;
  if (typeof url !== "string" || !url.startsWith("https://") || typeof draft !== "boolean") return null;
  if (typeof checks !== "string" || !PR_CHECKS.includes(checks)) return null;
  if (typeof failed !== "number" || !Number.isFinite(failed)) return null;
  return { number, title, url, draft, checks: checks as PrChecks, failed };
}

function parseWorktree(value: unknown): Worktree | null {
  if (!value || typeof value !== "object") return null;
  const { path, branch, head, pr } = value as Record<string, unknown>;
  if (typeof path !== "string" || !textOrNull(branch) || typeof head !== "string") return null;
  return { path, branch, head, pr: parsePullRequest(pr) };
}

function parseRepoInfo(value: unknown): RepoInfo | null {
  if (!value || typeof value !== "object") return null;
  const { root, name, current, prs, worktrees } = value as Record<string, unknown>;
  if (typeof root !== "string" || !textOrNull(name) || !textOrNull(current)) return null;
  if (typeof prs !== "boolean" || !Array.isArray(worktrees)) return null;
  return { root, name, current, prs, worktrees: worktrees.flatMap((item) => parseWorktree(item) ?? []) };
}

/** The repository of a folder; null when the folder is in none, or the call fails. */
export async function fetchWorktrees(dir: string): Promise<RepoInfo | null> {
  try {
    const response = await fetch(worktreesPath(dir), { cache: "no-store" });
    if (!response.ok) return null;
    return parseRepoInfo(await response.json());
  } catch {
    return null;
  }
}

/** The panel asks about its folders again this often (ms), so a pull request's checks follow GitHub. */
export const WORKTREES_REFRESH = 60_000;

/** A kept answer is used again while it is younger than this (ms): a panel that closes and opens asks nothing new. */
export const WORKTREES_MAX_AGE = 30_000;

export type WorktreeReader = (dir: string) => Promise<RepoInfo | null>;

const caches = new WeakMap<WorktreeReader, ReadCache<RepoInfo>>();

/** The one cache of a reader; tests pass their own reader and get a fresh cache. */
export function worktreeCache(read: WorktreeReader): ReadCache<RepoInfo> {
  let cache = caches.get(read);
  if (!cache) {
    cache = createReadCache(read, WORKTREES_MAX_AGE);
    caches.set(read, cache);
  }
  return cache;
}

/** A folder of the panel with the worktree it is in; null when that is not known. */
export interface RepoFolder {
  folder: AgentFolder;
  worktree: Worktree | null;
}

/** A repository with more than one worktree, and the panel's folders in it. */
export interface AgentRepo {
  /** The main worktree's path. */
  key: string;
  name: string;
  folders: RepoFolder[];
  /** The worktrees no folder of the panel is in: those with a pull request first. */
  idle: Worktree[];
  /** All worktrees. */
  count: number;
  prs: boolean;
  waiting: boolean;
}

export type AgentEntry = ({ kind: "folder"; prs: boolean } & RepoFolder) | { kind: "repo"; repo: AgentRepo };

/**
 * The panel's folders, those of one repository under it. A repository shows
 * as one only with more than one worktree: with one, its folder stays as it
 * is and gets the pull request. `info` gives what is known of a folder's
 * repository.
 */
export function agentEntries(folders: AgentFolder[], info: (path: string) => RepoInfo | undefined): AgentEntry[] {
  const repos = new Map<string, { repo: AgentRepo; worktrees: Worktree[] }>();
  const entries: AgentEntry[] = [];
  for (const folder of folders) {
    const known = folder.path === null ? undefined : info(folder.path);
    const worktree = known?.worktrees.find((item) => item.path === known.current) ?? null;
    if (!known || known.worktrees.length < 2) {
      entries.push({ kind: "folder", folder, worktree, prs: known?.prs ?? false });
      continue;
    }
    let group = repos.get(known.root);
    if (!group) {
      const name = known.name ?? folderLabel(known.root).name;
      const repo: AgentRepo = { key: known.root, name, folders: [], idle: [], count: known.worktrees.length, prs: known.prs, waiting: false };
      group = { repo, worktrees: known.worktrees };
      repos.set(known.root, group);
      entries.push({ kind: "repo", repo });
    }
    group.repo.folders.push({ folder, worktree });
    group.repo.waiting ||= folder.waiting;
  }
  for (const { repo, worktrees } of repos.values()) {
    const busy = new Set(repo.folders.map(({ worktree }) => worktree?.path));
    const idle = worktrees.filter((worktree) => !busy.has(worktree.path));
    repo.idle = [...idle.filter((worktree) => worktree.pr), ...idle.filter((worktree) => !worktree.pr)];
  }
  return entries;
}
