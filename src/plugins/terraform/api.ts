// Requests to the Terraform plugin's server routes.

import type { PluginContext } from "../types.ts";
import type { Action, PlanAnswer, StateAnswer, Tool, TreeAnswer } from "./types.ts";

export type Failure =
  /** The plugin is not installed any more (404 without a body). */
  | { type: "gone" }
  /** The project is not one of the last scan. */
  | { type: "removed" }
  /** The server said no: a code (`running`, `stale`, …) or a tool's error line. */
  | { type: "refused"; error: string }
  | { type: "error"; error: string };

export type Answer<T> = { ok: true; value: T } | { ok: false; failure: Failure };

/** What `fetch` failing shows; the UI words it in its language. */
export const REQUEST_FAILED = "Request failed";

const fail = (failure: Failure): Answer<never> => ({ ok: false, failure });

async function bodyOf(response: Response): Promise<Record<string, unknown> | null> {
  try {
    const text = await response.text();
    if (text === "") return null;
    const value: unknown = JSON.parse(text);
    return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** Sends a request and reads its answer; `read` turns a 2xx answer into the value. */
async function request<T>(ctx: PluginContext, path: string, init: RequestInit | undefined, read: (body: Record<string, unknown> | null, status: number) => T): Promise<Answer<T>> {
  let response: Response;
  try {
    response = await ctx.fetch(path, init);
  } catch {
    return fail({ type: "error", error: REQUEST_FAILED });
  }
  const body = await bodyOf(response);
  if (response.ok) return { ok: true, value: read(body, response.status) };
  if (response.status === 404) return fail(body?.removed === true ? { type: "removed" } : { type: "gone" });
  if (typeof body?.error === "string" && body.error !== "") return fail({ type: "refused", error: body.error });
  return fail({ type: "error", error: REQUEST_FAILED });
}

const send = (method: string, body: unknown): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

const none = () => null;
const at = (id: string) => `/projects/${encodeURIComponent(id)}`;

/** The tools, the settings, and the projects. `scan` walks `folders`; without it the last scan is read again. */
export function fetchState(ctx: PluginContext, folders: readonly string[], scan: boolean): Promise<Answer<StateAnswer>> {
  const words = [...(scan ? ["scan=1"] : []), ...folders.map((folder) => `folder=${encodeURIComponent(folder)}`)];
  return request(ctx, words.length > 0 ? `/state?${words.join("&")}` : "/state", undefined, (body) => body as unknown as StateAnswer);
}

export function fetchTree(ctx: PluginContext, id: string): Promise<Answer<TreeAnswer>> {
  return request(ctx, `${at(id)}/tree`, undefined, (body) => body as unknown as TreeAnswer);
}

/** The project's plan; null when it has none. */
export function fetchPlan(ctx: PluginContext, id: string): Promise<Answer<PlanAnswer | null>> {
  return request(ctx, `${at(id)}/plan`, undefined, (body, status) => (status === 204 || body === null ? null : (body as unknown as PlanAnswer)));
}

/**
 * Starts a run; the token opens the pane that runs it. An apply names the
 * plan the user confirmed by its time (`planAt`): the server refuses to apply
 * any other plan, such as one another window made since.
 */
export function startRun(ctx: PluginContext, id: string, action: Action, target: string | null, planAt?: number): Promise<Answer<{ token: string; name: string }>> {
  const body = { action, ...(target === null ? {} : { target }), ...(planAt === undefined ? {} : { planAt }) };
  return request(ctx, `${at(id)}/runs`, send("POST", body), (body) => ({
    token: String(body?.token ?? ""),
    name: String(body?.name ?? ""),
  }));
}

/** Asks the project's tool for its workspaces. This can reach the backend. */
export function listWorkspaces(ctx: PluginContext, id: string): Promise<Answer<{ workspaces: string[]; current: string }>> {
  return request(ctx, `${at(id)}/workspaces`, undefined, (body) => ({
    workspaces: Array.isArray(body?.workspaces) ? (body.workspaces as string[]) : [],
    current: String(body?.current ?? ""),
  }));
}

export function selectWorkspace(ctx: PluginContext, id: string, name: string): Promise<Answer<null>> {
  return request(ctx, `${at(id)}/workspace`, send("POST", { name }), none);
}

export function setTool(ctx: PluginContext, id: string, tool: Tool): Promise<Answer<null>> {
  return request(ctx, `${at(id)}/tool`, send("PUT", { tool }), none);
}

export function setCheckVersions(ctx: PluginContext, on: boolean): Promise<Answer<null>> {
  return request(ctx, "/settings", send("PUT", { checkVersions: on }), none);
}

export function deletePlans(ctx: PluginContext): Promise<Answer<null>> {
  return request(ctx, "/plans", { method: "DELETE" }, none);
}
