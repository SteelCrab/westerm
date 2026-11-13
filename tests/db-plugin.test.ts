import assert from "node:assert/strict";
import { test } from "node:test";
import {
  connect,
  createConnection,
  listConnections,
  openConsole,
  readTree,
  runQuery,
  updateConnection,
} from "../src/plugins/db/api.ts";
import { defaultName, filterNodes, isNumericType, newQueryId, parseDbUrl, shortCount, toCsv } from "../src/plugins/db/model.ts";
import type { ConnectionInput, QueryResult, TreeNode } from "../src/plugins/db/types.ts";
import type { PluginContext } from "../src/plugins/types.ts";

// ---- Model ----

test("parseDbUrl reads the parts of a postgres URL", () => {
  assert.deepEqual(parseDbUrl("postgres://dev:pw@db.local:6543/app?sslmode=require"), {
    engine: "postgres",
    user: "dev",
    password: "pw",
    host: "db.local",
    port: 6543,
    database: "app",
    ssl: "require",
  });
  assert.deepEqual(parseDbUrl("  postgresql://db.local/app  "), { engine: "postgres", host: "db.local", database: "app" });
});

test("parseDbUrl decodes percent-encoded parts", () => {
  assert.equal(parseDbUrl("mysql://me:p%40ss%3Aw%2Fd@h/db")?.password, "p@ss:w/d");
  assert.equal(parseDbUrl("postgres://us%20er@h/d%C3%A9")?.user, "us er");
  assert.equal(parseDbUrl("postgres://us%20er@h/d%C3%A9")?.database, "dé");
});

test("parseDbUrl maps mariadb to mysql and paths to sqlite", () => {
  assert.equal(parseDbUrl("mariadb://h/db")?.engine, "mysql");
  assert.deepEqual(parseDbUrl("~/site/app.db"), { engine: "sqlite", path: "~/site/app.db" });
  assert.deepEqual(parseDbUrl("/var/data/app.sqlite3"), { engine: "sqlite", path: "/var/data/app.sqlite3" });
  assert.deepEqual(parseDbUrl("sqlite:///var/data/app.db"), { engine: "sqlite", path: "/var/data/app.db" });
  assert.deepEqual(parseDbUrl("sqlite:~/app.db"), { engine: "sqlite", path: "~/app.db" });
  assert.deepEqual(parseDbUrl("C:\\data\\app.db"), { engine: "sqlite", path: "C:\\data\\app.db" });
});

test("parseDbUrl refuses what it does not know and ignores unknown ssl modes", () => {
  for (const text of ["redis://h", "", "hello", "sqlite:relative.db", "postgres://"]) {
    assert.equal(parseDbUrl(text), null, text);
  }
  assert.equal(parseDbUrl("postgres://h/d?sslmode=allow")?.ssl, undefined);
});

test("shortCount abbreviates", () => {
  assert.deepEqual(
    [0, 999, 1000, 1200, 48213, 131000, 999999, 1234567, 12345678, -1, null].map(shortCount),
    ["0", "999", "1k", "1.2k", "48k", "131k", "999k", "1.2M", "12M", null, null],
  );
});

test("defaultName names a connection by its database and host, or its file", () => {
  assert.equal(defaultName({ engine: "postgres", host: "localhost", database: "app" }), "app@localhost");
  assert.equal(defaultName({ engine: "mysql", host: "db.local", database: "" }), "db.local");
  assert.equal(defaultName({ engine: "sqlite", path: "~/site/app.db" }), "app.db");
  assert.equal(defaultName({ engine: "sqlite", path: "C:\\data\\shop.sqlite" }), "shop.sqlite");
});

const node = (name: string, kind: TreeNode["kind"]): TreeNode => ({ name, kind, rows: null, preview: null });

test("filterNodes keeps tables and views whose name matches", () => {
  const nodes = [node("public", "schema"), node("orders", "table"), node("Order_Items", "table"), node("users", "table"), node("active_orders", "view")];
  assert.deepEqual(filterNodes(nodes, "  ORDER ").map((each) => each.name), ["public", "orders", "Order_Items", "active_orders"]);
  assert.equal(filterNodes(nodes, ""), nodes);
});

test("toCsv quotes fields and leaves NULL empty", () => {
  const result: QueryResult = {
    columns: [{ name: "a", type: "TEXT" }, { name: "b", type: "TEXT" }],
    rows: [["x,y", null], ['say "hi"', "1"], ["line\nbreak", ""]],
    cut: [],
    more: false,
    changed: null,
    ms: 1,
  };
  assert.equal(toCsv(result), 'a,b\r\n"x,y",\r\n"say ""hi""",1\r\n"line\nbreak",\r\n');
});

test("isNumericType knows the number types of the three engines", () => {
  for (const type of ["INT4", "INT8", "INTEGER", "NUMERIC", "FLOAT8", "REAL", "BIGINT UNSIGNED", "DECIMAL", "DOUBLE"]) {
    assert.ok(isNumericType(type), type);
  }
  for (const type of ["TEXT", "VARCHAR", "TIMESTAMPTZ", "INTERVAL", "POINT", ""]) {
    assert.ok(!isNumericType(type), type);
  }
});

test("newQueryId makes 16 lowercase hex digits", () => {
  const ids = new Set(Array.from({ length: 50 }, newQueryId));
  assert.equal(ids.size, 50);
  for (const id of ids) assert.match(id, /^[0-9a-f]{16}$/);
});

// ---- Requests ----

interface Call {
  path: string;
  method: string;
  body: unknown;
}

function fakeCtx(answer: (path: string) => Response): { ctx: PluginContext; calls: Call[] } {
  const calls: Call[] = [];
  const ctx: PluginContext = {
    openShell() {},
    openBrowser() {},
    openPane() {},
    openFolder() {},
    folders: () => [],
    fetch: async (path, init) => {
      calls.push({ path, method: init?.method ?? "GET", body: init?.body ? JSON.parse(String(init.body)) : null });
      return answer(path);
    },
    refreshInstalled() {},
    openManager() {},
  };
  return { ctx, calls };
}

const json = (status: number, body: unknown) => new Response(body === undefined ? null : JSON.stringify(body), { status });

const INPUT: ConnectionInput = {
  name: "local",
  engine: "postgres",
  host: "localhost",
  port: 5432,
  user: "dev",
  database: "app",
  ssl: "prefer",
  path: null,
  ssh: null,
  color: "gray",
  readOnly: true,
};

test("a 404 from a db route reads as gone", async () => {
  const { ctx } = fakeCtx(() => json(404, undefined));
  assert.deepEqual(await listConnections(ctx), { ok: false, failure: { type: "gone" } });
});

test("a 404 with removed reads as removed", async () => {
  const { ctx } = fakeCtx(() => json(404, { removed: true }));
  assert.deepEqual(await runQuery(ctx, "0123456789abcdef", { db: "", schema: null, sql: "SELECT 1", queryId: "a".repeat(16) }), {
    ok: false,
    failure: { type: "removed" },
  });
});

test("a 409 reads as needPassword", async () => {
  const { ctx } = fakeCtx(() => json(409, { needPassword: true }));
  assert.deepEqual(await readTree(ctx, "0123456789abcdef", null, null), { ok: false, failure: { type: "needPassword" } });
});

test("a 502 carries the error and its kind", async () => {
  const { ctx } = fakeCtx(() => json(502, { error: "relation \"x\" does not exist", kind: "db" }));
  assert.deepEqual(await runQuery(ctx, "0123456789abcdef", { db: "", schema: null, sql: "SELECT * FROM x", queryId: "a".repeat(16) }), {
    ok: false,
    failure: { type: "error", error: 'relation "x" does not exist', kind: "db" },
  });
});

test("a 400 with a field name reads as invalid", async () => {
  const { ctx } = fakeCtx(() => json(400, { error: "nameUsed" }));
  assert.deepEqual(await createConnection(ctx, INPUT, null), { ok: false, failure: { type: "error", error: "nameUsed", kind: "request" } });
});

test("a failed fetch reads as a request error", async () => {
  const ctx = { ...fakeCtx(() => json(200, {})).ctx, fetch: () => Promise.reject(new Error("offline")) };
  assert.deepEqual(await listConnections(ctx), { ok: false, failure: { type: "error", error: "Request failed", kind: "request" } });
});

test("requests go to the right routes with JSON bodies", async () => {
  const { ctx, calls } = fakeCtx((path) => {
    if (path.endsWith("/console")) return json(200, { token: "t".repeat(32), name: "app console" });
    if (path.includes("/tree")) return json(200, { nodes: [] });
    if (path === "/connections") return json(201, { id: "0123456789abcdef", ...INPUT });
    return json(204, undefined);
  });
  assert.deepEqual(await createConnection(ctx, INPUT, "pw"), { ok: true, value: { id: "0123456789abcdef", ...INPUT, keychainError: null } });
  assert.deepEqual(await updateConnection(ctx, "0123456789abcdef", INPUT, null, true), { ok: true, value: { keychainError: null } });
  assert.deepEqual(await readTree(ctx, "0123456789abcdef", "app", "public"), { ok: true, value: [] });
  assert.deepEqual(await connect(ctx, "0123456789abcdef", "pw"), { ok: true, value: null });
  assert.deepEqual(await openConsole(ctx, "0123456789abcdef", "app"), { ok: true, value: { token: "t".repeat(32), name: "app console" } });
  assert.deepEqual(
    calls.map(({ path, method }) => `${method} ${path}`),
    [
      "POST /connections",
      "PUT /connections/0123456789abcdef",
      "GET /connections/0123456789abcdef/tree?db=app&schema=public",
      "POST /connections/0123456789abcdef/connect",
      "POST /connections/0123456789abcdef/console",
    ],
  );
  assert.deepEqual(calls[0].body, { ...INPUT, password: "pw" });
  assert.deepEqual(calls[1].body, { ...INPUT, clearPassword: true });
  assert.deepEqual(calls[3].body, { password: "pw" });
  assert.deepEqual(calls[4].body, { db: "app" });
});
