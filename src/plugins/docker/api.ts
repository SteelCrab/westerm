// Requests to the Docker plugin's server routes.

import type { PluginContext } from "../types.ts";
import { parseDockerState, type DockerState } from "./model.ts";

export type ContainerAction = "start" | "stop" | "restart" | "remove";

/** What `containerAction` returns when Docker gave no error text; the panel shows it in the UI language. */
export const GENERIC_FAILURE = "Request failed";

/** The state, "gone" when the plugin is no longer installed (404), or null when the request failed. */
export async function fetchDockerState(ctx: PluginContext, retry: boolean): Promise<DockerState | "gone" | null> {
  try {
    const res = await ctx.fetch(retry ? "/state?retry=1" : "/state");
    if (res.status === 404) return "gone";
    if (!res.ok) return null;
    return parseDockerState(await res.json());
  } catch {
    return null;
  }
}

/** null on success; otherwise the text to show under the row. */
export async function containerAction(
  ctx: PluginContext,
  id: string,
  action: ContainerAction,
): Promise<string | null> {
  try {
    const res = await ctx.fetch(`/containers/${encodeURIComponent(id)}/${action}`, { method: "POST" });
    if (res.ok) return null;
    if (res.status === 502) {
      const body: unknown = await res.json();
      const error = typeof body === "object" && body !== null ? (body as { error?: unknown }).error : undefined;
      if (typeof error === "string" && error !== "") return error;
    }
    return GENERIC_FAILURE;
  } catch {
    return GENERIC_FAILURE;
  }
}
