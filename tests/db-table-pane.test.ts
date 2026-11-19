import "./dom-page.ts";
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { TablePane } from "../src/plugins/db/TablePane.tsx";
import type { Connection, ConnectionList, QueryResult, TablePaneData } from "../src/plugins/db/types.ts";
import type { PluginContext } from "../src/plugins/types.ts";
import { PLUGIN_PANE_DATA_MAX } from "../shared/workspace-file.ts";
import { click, mount, settle, text, type, type Mounted } from "./dom-page.ts";

let mounted: Mounted | null = null;
afterEach(async () => {
  await mounted?.unmount();
  mounted = null;
});

const copied: string[] = [];
Object.defineProperty(navigator, "clipboard", {
  value: { writeText: async (value: string) => void copied.push(value) },
  configurable: true,
});

const PG: Connection = {
  id: "0123456789abcdef",
  name: "local-pg",
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
  passwordSaved: true,
  status: "open",
  error: null,
};

const LIST: ConnectionList = { connections: [PG], keychain: true, open: 1 };

const DATA: TablePaneData = {
  connectionId: PG.id,
  db: "app",
  schema: "public",
  table: "orders",
  sql: 'SELECT * FROM "public"."orders" LIMIT 100',
  run: true,
  estimate: 48213,
};

const RESULT: QueryResult = {
  columns: [
    { name: "id", type: "INT8" },
    { name: "status", type: "TEXT" },
    { name: "note", type: "TEXT" },
  ],
  rows: [
    ["1", "paid", null],
    ["2", "pending", "long text"],
  ],
  cut: [[1, 2]],
  more: true,
  changed: null,
  ms: 18,
};

const json = (status: number, body?: unknown) => new Response(body === undefined ? null : JSON.stringify(body), { status });

interface Fake {
  ctx: PluginContext;
  calls: { method: string; path: string; body: unknown }[];
  data: TablePaneData[];
  closed: number;
}

function fake(answer: (method: string, path: string, body: unknown) => Response | Promise<Response>): Fake {
  const f: Fake = { calls: [], data: [], closed: 0, ctx: null as unknown as PluginContext };
  f.ctx = {
    openShell() {},
    openBrowser() {},
    openPane() {},
    openFolder() {},
    folders: () => [],
    fetch: async (path, init) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : null;
      f.calls.push({ method, path, body });
      return answer(method, path, body);
    },
    refreshInstalled() {},
    openManager() {},
  };
  return f;
}

async function pane(f: Fake, data: TablePaneData, list: ConnectionList = LIST): Promise<Mounted> {
  mounted = await mount(
    createElement(TablePane, {
      ctx: f.ctx,
      list,
      view: "table",
      data,
      focused: true,
      setData: (next: unknown) => f.data.push(next as TablePaneData),
      setTitle: () => {},
      close: () => {
        f.closed += 1;
      },
    }),
  );
  await settle(6);
  return mounted;
}

const queries = (f: Fake) => f.calls.filter((call) => call.path.endsWith("/query"));
const button = (root: Element, label: string) =>
  [...root.querySelectorAll("button")].find((each) => each.getAttribute("aria-label") === label || text(each) === label);

test("a restored table pane waits for Run and sends nothing", async () => {
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, { ...DATA, run: false });
  assert.match(text(container), /Press Run to load/);
  assert.equal(queries(f).length, 0);
  await click(button(container, "Run"));
  assert.equal(queries(f).length, 1);
});

test("a fresh table pane runs once and clears its run flag", async () => {
  const f = fake(() => json(200, RESULT));
  await pane(f, DATA);
  assert.equal(queries(f).length, 1);
  const body = queries(f)[0].body as Record<string, unknown>;
  assert.equal(body.db, "app");
  assert.equal(body.schema, "public");
  assert.equal(body.sql, DATA.sql);
  assert.match(String(body.queryId), /^[0-9a-f]{16}$/);
  assert.deepEqual(f.data[0], { ...DATA, run: false });
});

test("results show NULL, numbers on the right, cut cells, and the row-limit note", async () => {
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, DATA);
  const headers = [...container.querySelectorAll("th")].map((th) => text(th));
  assert.deepEqual(headers, ["id INT8", "status TEXT", "note TEXT"]);
  const cells = [...container.querySelectorAll("tbody tr")].map((tr) => [...tr.querySelectorAll("td")]);
  assert.ok(cells[0][0].classList.contains("db-num"));
  assert.ok(cells[0][2].classList.contains("db-null"));
  assert.equal(text(cells[0][2]), "NULL");
  assert.match(text(cells[1][2]), /long text \(truncated\)/);
  assert.match(text(container), /Showing the first 2 rows/, "the rows shown: the limit, or fewer at the byte budget");
  assert.match(text(container), /2 rows · 18 ms/);
  assert.match(text(container), /about 48k rows total/);
  assert.match(text(container), /Read-only/);
});

test("a write shows the changed count", async () => {
  const f = fake(() => json(200, { ...RESULT, columns: [], rows: [], cut: [], more: false, changed: 3, ms: 4 }));
  const { container } = await pane(f, { ...DATA, sql: "UPDATE t SET a = 1" });
  assert.match(text(container), /3 rows changed · 4 ms/);
});

test("Cancel sends the query id", async () => {
  let release: (response: Response) => void = () => {};
  const f = fake((method, path) => {
    if (path.endsWith("/query")) return new Promise<Response>((resolve) => (release = resolve));
    if (path.endsWith("/cancel")) {
      release(json(502, { error: "Cancelled", kind: "cancelled" }));
      return json(204);
    }
    return json(404);
  });
  const { container } = await pane(f, DATA);
  const queryId = (queries(f)[0].body as Record<string, unknown>).queryId;
  await click(button(container, "Cancel"));
  assert.deepEqual(f.calls.at(-1), { method: "POST", path: `/queries/${queryId}/cancel`, body: null });
  await settle();
  assert.match(text(container), /Cancelled/);
  assert.ok(button(container, "Run"), "Run comes back");
});

test("Copy as CSV copies the whole result", async () => {
  copied.length = 0;
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, DATA);
  await click(button(container, "Copy as CSV"));
  assert.deepEqual(copied, ["id,status,note\r\n1,paid,\r\n2,pending,long text\r\n"]);
});

test("a clicked cell is copied with the copy keys", async () => {
  copied.length = 0;
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, DATA);
  const cell = container.querySelectorAll("tbody tr")[1].querySelectorAll("td")[1];
  await click(cell);
  const grid = container.querySelector(".db-grid")!;
  grid.dispatchEvent(new window.KeyboardEvent("keydown", { key: "c", metaKey: true, ctrlKey: true, bubbles: true }) as unknown as Event);
  await settle();
  assert.deepEqual(copied, ["pending"]);
});

test("a removed connection shows the removed notice", async () => {
  const f = fake(() => json(404, { removed: true }));
  const { container } = await pane(f, DATA);
  assert.match(text(container), /This connection was removed/);
  await click(button(container, "Close"));
  assert.equal(f.closed, 1);
});

test("a database error shows its first line strongly", async () => {
  const f = fake(() => json(502, { error: 'relation "orders" does not exist', kind: "db" }));
  const { container } = await pane(f, DATA);
  assert.match(text(container.querySelector(".db-result-error")!), /relation "orders" does not exist/);
});

test("a connection that needs a password asks for it and runs again", async () => {
  let connected = false;
  const f = fake((_method, path, body) => {
    if (path.endsWith("/connect")) {
      connected = (body as { password?: string }).password === "pw";
      return json(204);
    }
    if (path.endsWith("/query")) return connected ? json(200, RESULT) : json(409, { needPassword: true });
    return json(404);
  });
  const { container } = await pane(f, DATA);
  assert.match(text(container), /Enter the password/);
  await type(container.querySelector('input[type="password"]'), "pw");
  await click(button(container, "Connect"));
  await settle();
  assert.ok(connected);
  assert.equal(queries(f).length, 2);
  assert.ok(container.querySelector("tbody tr"));
});

test("a red connection draws the warning line", async () => {
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, DATA, { ...LIST, connections: [{ ...PG, color: "red" }] });
  assert.ok(container.querySelector(".db-table-pane.db-danger"));
});

test("SQL too long to keep with the layout is not saved and says so", async () => {
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, { ...DATA, run: false });
  await type(container.querySelector("textarea"), `SELECT '${"x".repeat(PLUGIN_PANE_DATA_MAX)}'`);
  await new Promise((resolve) => setTimeout(resolve, 450));
  await settle();
  assert.equal(f.data.length, 0);
  assert.match(text(container), /too long to keep with the layout/);
});

test("long cells show a short part, and the popover and copy keep all of it", async () => {
  copied.length = 0;
  const long = "y".repeat(10_000);
  const f = fake(() => json(200, { ...RESULT, rows: [["1", "paid", long]], cut: [], more: false }));
  const { container } = await pane(f, DATA);
  const cell = container.querySelectorAll("tbody td")[2];
  assert.ok(text(cell).length < 600, `${text(cell).length}`);
  cell.dispatchEvent(new window.MouseEvent("dblclick", { bubbles: true }) as unknown as Event);
  await settle();
  assert.equal(container.querySelector(".db-cell-popover pre")?.textContent, long);
  await click(cell);
  container.querySelector(".db-grid")!.dispatchEvent(new window.KeyboardEvent("keydown", { key: "c", ctrlKey: true, bubbles: true }) as unknown as Event);
  await settle();
  assert.deepEqual(copied, [long]);
});

test("editing the SQL saves it after a pause", async () => {
  const f = fake(() => json(200, RESULT));
  const { container } = await pane(f, { ...DATA, run: false });
  await type(container.querySelector("textarea"), "SELECT 2");
  await new Promise((resolve) => setTimeout(resolve, 450));
  await settle();
  assert.deepEqual(f.data.at(-1), { ...DATA, run: false, sql: "SELECT 2" });
});
