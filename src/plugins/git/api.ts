// Requests to the Git plugin's server routes. `ctx.fetch` sends them to this
// computer, or to the server of a remote window.

import type { PluginContext } from "../types.ts";
import type { GitCommitDetail, GitEntry, GitOperation, GitStatus } from "./model.ts";

export type GitResult<T> = { ok: true; value: T } | { ok: false; error: string };

type GitRoute = "status" | "diff" | "action" | "commit" | "commit-diff";

/** The route's path under the plugin, with its query. */
export function gitPath(route: GitRoute, query?: Record<string, string>): string {
  return `/${route}${query ? `?${new URLSearchParams(query)}` : ""}`;
}

async function call<T>(ctx: PluginContext, path: string, init?: RequestInit): Promise<GitResult<T>> {
  try {
    const response = await ctx.fetch(path, { cache: "no-store", ...init });
    const body = await response.json().catch(() => null);
    if (!response.ok) return { ok: false, error: typeof body?.error === "string" ? body.error : `HTTP ${response.status}` };
    if (body === null) return { ok: false, error: "The Git server returned an invalid response. Restart westerm with the latest build." };
    return { ok: true, value: body as T };
  } catch {
    return { ok: false, error: "Cannot reach the Git server" };
  }
}

export const fetchGitStatus = (ctx: PluginContext, dir: string, signal?: AbortSignal) =>
  call<GitStatus>(ctx, gitPath("status", { dir }), { signal });

export const fetchGitDiff = (ctx: PluginContext, dir: string, entry: Pick<GitEntry, "path" | "staged">, signal?: AbortSignal) =>
  call<{ text: string }>(ctx, gitPath("diff", { dir, path: entry.path, staged: String(entry.staged) }), { signal });

export const fetchGitCommit = (ctx: PluginContext, dir: string, hash: string, signal?: AbortSignal) =>
  call<GitCommitDetail>(ctx, gitPath("commit", { dir, hash }), { signal });

export const fetchGitCommitDiff = (ctx: PluginContext, dir: string, hash: string, path: string, signal?: AbortSignal) =>
  call<{ text: string }>(ctx, gitPath("commit-diff", { dir, hash, path }), { signal });

export const runGitOperation = (ctx: PluginContext, dir: string, operation: GitOperation) =>
  call<Record<string, never>>(ctx, gitPath("action"), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ dir, ...operation }),
  });
