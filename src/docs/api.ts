// Calls to the dashboard's `/api/docs/*` routes (native/src/server/docs.rs).
// A remote window reaches its server's routes through `/remote/<target>`.

import { REMOTE_TARGET } from "../remoteWindow.ts";

type Route = "tree" | "file" | "cwd";

/** A failed call: the HTTP status (0 when nothing answered) and, for 409, the file's version on disk. */
export type Failed = { ok: false; status: number; version?: string | null };

export function docsPath(route: Route, query?: Record<string, string>, target: string | null = REMOTE_TARGET): string {
  const base = target !== null ? `/remote/${encodeURIComponent(target)}/api/docs/${route}` : `/api/docs/${route}`;
  return query ? `${base}?${new URLSearchParams(query)}` : base;
}

async function call<T>(path: string, init?: RequestInit): Promise<({ ok: true } & T) | Failed> {
  try {
    const response = await fetch(path, { cache: "no-store", ...init });
    const body = (await response.json().catch(() => ({}))) as Record<string, unknown>;
    if (response.ok) return { ok: true, ...(body as T) };
    const version = typeof body.version === "string" ? body.version : null;
    return { ok: false, status: response.status, version };
  } catch {
    return { ok: false, status: 0 };
  }
}

function sendJson(method: "PUT" | "POST", body: Record<string, unknown>): RequestInit {
  return { method, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
}

/** The folder as the server spells it, and its Markdown files. */
export const fetchTree = (root: string) => call<{ root: string; files: string[]; truncated: boolean }>(docsPath("tree", { root }));

export const fetchFile = (root: string, path: string) =>
  call<{ content: string; version: string }>(docsPath("file", { root, path }));

/** Saves when the file is still at `baseVersion` (null: no file there); 409 with the disk's version otherwise. */
export const saveFile = (root: string, path: string, content: string, baseVersion: string | null) =>
  call<{ version: string }>(docsPath("file"), sendJson("PUT", { root, path, content, baseVersion }));

/** A new file; 409 when one is there. */
export const createFile = (root: string, path: string, content: string) =>
  call<{ version: string }>(docsPath("file"), sendJson("POST", { root, path, content }));

/** The folder of process `pid` (a shell), or the home folder for null. */
export const fetchCwd = (pid: number | null) =>
  call<{ path: string }>(docsPath("cwd", pid === null ? undefined : { pid: String(pid) }));
