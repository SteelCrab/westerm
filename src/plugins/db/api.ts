// Requests to the Databases plugin's server routes.

import type { PluginContext } from "../types.ts";
import type { Clients, Connection, ConnectionInput, ConnectionList, Found, QueryResult, TreeNode } from "./types.ts";

export type Failure =
  /** The plugin is not installed any more (404 without a body). */
  | { type: "gone" }
  /** The database wants a password. */
  | { type: "needPassword" }
  /** The connection was deleted. */
  | { type: "removed" }
  /** `kind`: "db", "timeout", "cancelled" from the database; "request" for anything else. */
  | { type: "error"; error: string; kind: string };

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

/** Sends a request and reads its answer; `read` turns a 2xx body into the value. */
async function request<T>(ctx: PluginContext, path: string, init: RequestInit | undefined, read: (body: Record<string, unknown> | null) => T): Promise<Answer<T>> {
  let response: Response;
  try {
    response = await ctx.fetch(path, init);
  } catch {
    return fail({ type: "error", error: REQUEST_FAILED, kind: "request" });
  }
  const body = await bodyOf(response);
  if (response.ok) return { ok: true, value: read(body) };
  if (response.status === 404) return fail(body?.removed === true ? { type: "removed" } : { type: "gone" });
  if (response.status === 409 && body?.needPassword === true) return fail({ type: "needPassword" });
  const error = typeof body?.error === "string" && body.error !== "" ? body.error : REQUEST_FAILED;
  const kind = response.status === 502 && typeof body?.kind === "string" ? body.kind : "request";
  return fail({ type: "error", error, kind });
}

const post = (body: unknown, method = "POST"): RequestInit => ({
  method,
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

const none = () => null;
const at = (id: string) => `/connections/${encodeURIComponent(id)}`;

export function listConnections(ctx: PluginContext): Promise<Answer<ConnectionList>> {
  return request(ctx, "/connections", undefined, (body) => ({
    connections: Array.isArray(body?.connections) ? (body.connections as Connection[]) : [],
    keychain: body?.keychain !== false,
    open: typeof body?.open === "number" ? body.open : 0,
  }));
}

/** What saving answers: the keychain's error when it kept the password in memory instead. */
export interface Saved {
  keychainError: string | null;
}

const keychainErrorOf = (answer: Record<string, unknown> | null): string | null =>
  typeof answer?.keychainError === "string" && answer.keychainError !== "" ? answer.keychainError : null;

/** Saves a new connection; a password (if any) goes to the keychain. */
export function createConnection(ctx: PluginContext, input: ConnectionInput, password: string | null): Promise<Answer<Connection & Saved>> {
  const body = password ? { ...input, password } : input;
  return request(ctx, "/connections", post(body), (answer) => ({ ...(answer as unknown as Connection), keychainError: keychainErrorOf(answer) }));
}

/** Saves changed fields: a new password, `clear` to forget it, or neither to keep it. */
export function updateConnection(
  ctx: PluginContext,
  id: string,
  input: ConnectionInput,
  password: string | null,
  clear: boolean,
): Promise<Answer<Saved>> {
  const body = password ? { ...input, password } : clear ? { ...input, clearPassword: true } : input;
  return request(ctx, at(id), post(body, "PUT"), (answer) => ({ keychainError: keychainErrorOf(answer) }));
}

export function deleteConnection(ctx: PluginContext, id: string): Promise<Answer<null>> {
  return request(ctx, at(id), { method: "DELETE" }, none);
}

export function forgetAll(ctx: PluginContext): Promise<Answer<null>> {
  return request(ctx, "/connections", { method: "DELETE" }, none);
}

export interface TestOk {
  ms: number;
  version: string;
}

/** Connects without saving. A failed test is `ok: true` with `error` set in the value. */
export function testConnection(ctx: PluginContext, input: ConnectionInput, password: string | null): Promise<Answer<TestOk | { error: string }>> {
  const body = password ? { ...input, password } : input;
  return request(ctx, "/connections/test", post(body), (answer) =>
    answer?.ok === true
      ? { ms: Number(answer.ms ?? 0), version: String(answer.version ?? "") }
      : { error: typeof answer?.error === "string" ? answer.error : REQUEST_FAILED },
  );
}

export function connect(ctx: PluginContext, id: string, password?: string): Promise<Answer<null>> {
  return request(ctx, `${at(id)}/connect`, post(password ? { password } : {}), none);
}

export function disconnect(ctx: PluginContext, id: string): Promise<Answer<null>> {
  return request(ctx, `${at(id)}/disconnect`, { method: "POST" }, none);
}

/** One level of the tree: databases (no db), schemas (PostgreSQL db), or tables. */
export function readTree(ctx: PluginContext, id: string, db: string | null, schema: string | null): Promise<Answer<TreeNode[]>> {
  const query = new URLSearchParams();
  if (db) query.set("db", db);
  if (schema) query.set("schema", schema);
  const suffix = query.size > 0 ? `?${query}` : "";
  return request(ctx, `${at(id)}/tree${suffix}`, undefined, (body) => (Array.isArray(body?.nodes) ? (body.nodes as TreeNode[]) : []));
}

export interface QueryRequest {
  db: string;
  schema: string | null;
  sql: string;
  queryId: string;
}

export function runQuery(ctx: PluginContext, id: string, query: QueryRequest): Promise<Answer<QueryResult>> {
  return request(ctx, `${at(id)}/query`, post(query), (body) => body as unknown as QueryResult);
}

export function cancelQuery(ctx: PluginContext, queryId: string): Promise<Answer<null>> {
  return request(ctx, `/queries/${encodeURIComponent(queryId)}/cancel`, { method: "POST" }, none);
}

export interface ConsoleTicket {
  token: string;
  name: string;
}

/** Prepares a console; `ctx.openShell` then opens it with the token. */
export function openConsole(ctx: PluginContext, id: string, db: string): Promise<Answer<ConsoleTicket>> {
  return request(ctx, `${at(id)}/console`, post({ db }), (body) => ({ token: String(body?.token ?? ""), name: String(body?.name ?? "") }));
}

export function readClients(ctx: PluginContext): Promise<Answer<Clients>> {
  return request(ctx, "/clients", undefined, (body) => ({
    psql: body?.psql === true,
    mysql: body?.mysql === "mysql" || body?.mysql === "mariadb" ? body.mysql : null,
    sqlite3: body?.sqlite3 === true,
    consoles: body?.consoles === true,
  }));
}

/** Connections found in the session folders and in Docker. */
export function detect(ctx: PluginContext, folders: readonly string[]): Promise<Answer<Found[]>> {
  const query = new URLSearchParams();
  for (const folder of folders) query.append("folder", folder);
  const suffix = folders.length > 0 ? `?${query}` : "";
  return request(ctx, `/detect${suffix}`, undefined, (body) => (Array.isArray(body?.found) ? (body.found as Found[]) : []));
}
