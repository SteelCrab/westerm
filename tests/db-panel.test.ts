import "./dom-page.ts";
import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { createElement } from "react";
import { availableBuiltins } from "../src/plugins/installed.ts";
import { ConnectionDialog } from "../src/plugins/db/ConnectionDialog.tsx";
import { DbPanel } from "../src/plugins/db/DbPanel.tsx";
import { dbPlugin } from "../src/plugins/db/index.tsx";
import type { Clients, Connection, ConnectionInput, ConnectionList, TablePaneData, TreeNode } from "../src/plugins/db/types.ts";
import { BUILTIN_PLUGINS } from "../src/plugins/registry.ts";
import type { OpenPaneOptions, OpenShellOptions, PluginContext } from "../src/plugins/types.ts";
import { click, mount, settle, text, type, type Mounted } from "./dom-page.ts";

let mounted: Mounted | null = null;
afterEach(async () => {
  await mounted?.unmount();
  mounted = null;
});

const INPUT: ConnectionInput = {
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
};

const PG: Connection = { ...INPUT, id: "0123456789abcdef", passwordSaved: true, status: "idle", error: null };
const LIST: ConnectionList = { connections: [PG], keychain: true, open: 0 };
const CLIENTS: Clients = { psql: true, mysql: "mysql", sqlite3: true, consoles: true };

const node = (name: string, kind: TreeNode["kind"], extra: Partial<TreeNode> = {}): TreeNode => ({ name, kind, rows: null, preview: null, ...extra });

interface Fake {
  ctx: PluginContext;
  calls: { method: string; path: string; body: unknown }[];
  panes: OpenPaneOptions[];
  shells: OpenShellOptions[];
}

type Route = (method: string, path: string, body: unknown) => Response | undefined;

const json = (status: number, body?: unknown) => new Response(body === undefined ? null : JSON.stringify(body), { status });

function fake(route: Route): Fake {
  const result: Fake = { calls: [], panes: [], shells: [], ctx: null as unknown as PluginContext };
  result.ctx = {
    openShell: (options) => result.shells.push(options),
    openBrowser() {},
    openPane: (options) => result.panes.push(options),
    folders: () => [],
    fetch: async (path, init) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : null;
      result.calls.push({ method, path, body });
      return route(method, path, body) ?? json(404);
    },
    refreshInstalled() {},
    openManager() {},
  };
  return result;
}

/** The tree of PG: app > public > orders, users. */
const PG_TREE: Route = (method, path) => {
  if (method === "GET" && path === "/connections/0123456789abcdef/tree") return json(200, { nodes: [node("app", "database")] });
  if (method === "GET" && path === "/connections/0123456789abcdef/tree?db=app") return json(200, { nodes: [node("public", "schema")] });
  if (method === "GET" && path === "/connections/0123456789abcdef/tree?db=app&schema=public") {
    return json(200, {
      nodes: [
        node("orders", "table", { rows: 48213, preview: 'SELECT * FROM "public"."orders" LIMIT 100' }),
        node("active_users", "view", { preview: 'SELECT * FROM "public"."active_users" LIMIT 100' }),
      ],
    });
  }
  return undefined;
};

async function panel(f: Fake, list: ConnectionList = LIST, clients: Clients | null = CLIENTS): Promise<Mounted> {
  mounted = await mount(createElement(DbPanel, { ctx: f.ctx, list, clients, refresh: () => {} }));
  return mounted;
}

const button = (root: Element, label: string) =>
  [...root.querySelectorAll("button")].find((each) => each.getAttribute("aria-label") === label || text(each) === label);

const row = (root: Element, name: string) =>
  [...root.querySelectorAll(".db-row")].find((each) => each.querySelector(".db-name")?.textContent === name);

async function expand(root: Element, name: string) {
  await click(row(root, name)?.querySelector(".db-toggle"));
}

test("clicking a table opens a table pane keyed by connection, db, schema, and table", async () => {
  const f = fake(PG_TREE);
  const { container } = await panel(f);
  await expand(container, "local-pg");
  await expand(container, "app");
  await expand(container, "public");
  assert.match(text(row(container, "orders")!), /48k/);
  await click(row(container, "orders")?.querySelector(".db-open"));
  const data: TablePaneData = {
    connectionId: "0123456789abcdef",
    db: "app",
    schema: "public",
    table: "orders",
    sql: 'SELECT * FROM "public"."orders" LIMIT 100',
    run: true,
    estimate: 48213,
  };
  assert.deepEqual(f.panes, [{ view: "table", title: "orders", data, reuseKey: "0123456789abcdef/app/public/orders" }]);
});

test("the filter keeps matching tables", async () => {
  const f = fake(PG_TREE);
  const { container } = await panel(f);
  await expand(container, "local-pg");
  await expand(container, "app");
  await expand(container, "public");
  await type(container.querySelector(".db-filter input"), "ACTIVE");
  assert.equal(row(container, "orders"), undefined);
  assert.ok(row(container, "active_users"));
});

test("a connection that needs a password asks for it in its row", async () => {
  let connected = false;
  const f = fake((method, path, body) => {
    if (path === "/connections/0123456789abcdef/connect") {
      connected = (body as { password?: string }).password === "pw";
      return json(204);
    }
    if (path === "/connections/0123456789abcdef/tree") return connected ? json(200, { nodes: [node("app", "database")] }) : json(409, { needPassword: true });
    return undefined;
  });
  const { container } = await panel(f);
  await expand(container, "local-pg");
  const field = container.querySelector<HTMLInputElement>(".db-password input");
  assert.ok(field, "the row asks for a password");
  await type(field, "pw");
  await click(button(container, "Connect"));
  assert.ok(connected);
  assert.ok(row(container, "app"), "the tree loads after connecting");
});

test("a failed level shows the error and a Retry", async () => {
  let tries = 0;
  const f = fake((_method, path) => {
    if (path !== "/connections/0123456789abcdef/tree") return undefined;
    tries += 1;
    return tries === 1 ? json(502, { error: "connection refused", kind: "db" }) : json(200, { nodes: [node("app", "database")] });
  });
  const { container } = await panel(f);
  await expand(container, "local-pg");
  assert.match(text(container), /connection refused/);
  await click(button(container, "Retry"));
  assert.ok(row(container, "app"));
});

test("the console button opens a console with the server's token", async () => {
  const f = fake((method, path) => {
    if (method === "POST" && path === "/connections/0123456789abcdef/console") return json(200, { token: "a".repeat(32), name: "app console" });
    return PG_TREE(method, path, null);
  });
  const { container } = await panel(f);
  await expand(container, "local-pg");
  await click(row(container, "app")?.querySelector(".db-console"));
  assert.deepEqual(f.calls.at(-1), { method: "POST", path: "/connections/0123456789abcdef/console", body: { db: "app" } });
  assert.deepEqual(f.shells, [{ action: "console", target: "a".repeat(32), cwd: null, name: "app console", reuse: false }]);
});

test("the console button is disabled with a hint when psql is missing", async () => {
  const f = fake(PG_TREE);
  const { container } = await panel(f, LIST, { ...CLIENTS, psql: false });
  const consoleButton = row(container, "local-pg")?.querySelector<HTMLButtonElement>(".db-console");
  assert.ok(consoleButton?.disabled);
  assert.match(consoleButton?.title ?? "", /psql not found/);
});

test("no connections show the empty state with an Add button", async () => {
  const f = fake(() => undefined);
  const { container } = await panel(f, { connections: [], keychain: true, open: 0 });
  assert.match(text(container), /No connections yet/);
  assert.ok(button(container, "Add connection"));
});

test("a red connection shows its color and a failed one its error", async () => {
  const f = fake(() => undefined);
  const red: Connection = { ...PG, color: "red", status: "failed", error: "password authentication failed" };
  const { container } = await panel(f, { ...LIST, connections: [red] });
  assert.ok(row(container, "local-pg")?.querySelector(".db-color-red"));
  assert.match(text(container), /password authentication failed/);
});

test("delete asks first and then deletes", async () => {
  const f = fake((method) => (method === "DELETE" ? json(204) : undefined));
  const { container } = await panel(f);
  await click(row(container, "local-pg")?.querySelector(".db-more"));
  await click([...document.querySelectorAll("[role=menuitem]")].find((item) => text(item) === "Delete…"));
  assert.match(text(container), /Delete local-pg\? Its saved password is removed too\./);
  await click(button(container, "Delete"));
  assert.deepEqual(f.calls.at(-1), { method: "DELETE", path: "/connections/0123456789abcdef", body: null });
});

// ---- Found connections ----

const FOUND = {
  source: "env",
  label: ".env DATABASE_URL · ~/blog",
  input: { ...INPUT, name: "blog@localhost", database: "blog" },
  password: "pw",
};

test("found entries open a prefilled dialog instead of saving", async () => {
  const f = fake((_method, path) => (path === "/detect?folder=%2Fhome%2Fme%2Fblog" ? json(200, { found: [FOUND] }) : undefined));
  f.ctx.folders = () => ["/home/me/blog"];
  const { container } = await panel(f, { connections: [], keychain: true, open: 0 });
  const box = container.querySelector(".db-found");
  assert.ok(box, "the found box shows");
  assert.match(text(box!), /Found in your folders/);
  assert.match(text(box!), /\.env DATABASE_URL · ~\/blog/);
  await click(button(box!, "Add"));
  assert.equal(document.querySelector<HTMLInputElement>('[name="database"]')?.value, "blog");
  assert.equal(document.querySelector<HTMLInputElement>('[name="password"]')?.value, "pw");
  assert.ok(document.querySelector<HTMLInputElement>('[name="readOnly"]')?.checked);
  assert.equal(f.calls.filter((call) => call.method === "POST").length, 0, "nothing is saved yet");
});

test("the found box hides when nothing is found", async () => {
  const f = fake((_method, path) => (path.startsWith("/detect") ? json(200, { found: [] }) : undefined));
  const { container } = await panel(f);
  assert.equal(container.querySelector(".db-found"), null);
  assert.ok(f.calls.some((call) => call.path === "/detect"), "the panel looks when it opens");
});

// ---- Connection dialog ----

async function dialog(f: Fake, initial: Partial<ConnectionInput> = {}, extra: { id?: string; passwordSaved?: boolean; keychain?: boolean } = {}) {
  const done: boolean[] = [];
  mounted = await mount(
    createElement(ConnectionDialog, {
      ctx: f.ctx,
      id: extra.id ?? null,
      initial: { ...EMPTY, ...initial },
      password: null,
      passwordSaved: extra.passwordSaved ?? false,
      keychain: extra.keychain ?? true,
      sshHosts: ["bastion"],
      onDone: (saved: boolean) => done.push(saved),
    }),
  );
  return { container: mounted.container, done };
}

const EMPTY: ConnectionInput = { ...INPUT, name: "", host: "", port: null, user: "", database: "" };
const field = (root: Element, name: string) => root.querySelector<HTMLInputElement>(`[name="${name}"]`);

test("pasting a URL fills the dialog fields and moves the password", async () => {
  const f = fake(() => undefined);
  const { container } = await dialog(f);
  await type(field(container, "url"), "postgres://dev:s3cret@db.local:6543/app?sslmode=require");
  assert.equal(field(container, "host")?.value, "db.local");
  assert.equal(field(container, "port")?.value, "6543");
  assert.equal(field(container, "user")?.value, "dev");
  assert.equal(field(container, "database")?.value, "app");
  assert.equal(field(container, "password")?.value, "s3cret");
  assert.equal(field(container, "name")?.value, "app@db.local");
  assert.equal(container.querySelector<HTMLSelectElement>('[name="ssl"]')?.value, "require");
  assert.equal(field(container, "url")?.value, "", "the URL with its password does not stay in the field");
});

test("a new connection is read-only by default and saves with its password", async () => {
  const f = fake((method, path) => (method === "POST" && path === "/connections" ? json(201, { ...PG }) : undefined));
  const { container, done } = await dialog(f);
  assert.ok(field(container, "readOnly")?.checked);
  await type(field(container, "url"), "postgres://dev:pw@localhost/app");
  await click(button(container, "Save"));
  const body = f.calls.at(-1)?.body as Record<string, unknown>;
  assert.equal(body.readOnly, true);
  assert.equal(body.password, "pw");
  assert.equal(body.name, "app@localhost");
  assert.deepEqual(done, [true]);
});

test("a password the keychain did not take is reported after saving", async () => {
  const f = fake((method, path) =>
    method === "POST" && path === "/connections" ? json(201, { ...PG, keychainError: "The keychain is locked" }) : path.startsWith("/detect") ? json(200, { found: [] }) : undefined,
  );
  const { container } = await panel(f, { connections: [], keychain: true, open: 0 });
  await click(button(container, "Add connection"));
  await type(document.querySelector('[name="url"]'), "postgres://dev:pw@localhost/app");
  await click([...document.querySelectorAll("button")].find((each) => text(each) === "Save"));
  assert.equal(document.querySelector(".db-dialog"), null, "the connection is saved");
  assert.match(text(container), /The keychain did not take the password \(The keychain is locked\)\. It is kept until westerm stops\./);
});

test("a used name shows Name already used and keeps the dialog open", async () => {
  const f = fake(() => json(400, { error: "nameUsed" }));
  const { container, done } = await dialog(f, { name: "local-pg", host: "localhost" });
  await click(button(container, "Save"));
  assert.match(text(container), /Name already used/);
  assert.deepEqual(done, []);
});

test("Test connection shows the server's version or its error", async () => {
  let ok = true;
  const f = fake((_method, path) =>
    path === "/connections/test" ? json(200, ok ? { ok: true, ms: 12, version: "PostgreSQL 16.4" } : { ok: false, error: "no route to host" }) : undefined,
  );
  const { container } = await dialog(f, { name: "x", host: "localhost" });
  await click(button(container, "Test connection"));
  assert.match(text(container), /Connected · 12 ms · PostgreSQL 16\.4/);
  ok = false;
  await click(button(container, "Test connection"));
  assert.match(text(container), /no route to host/);
});

test("editing keeps a saved password unless it is changed or cleared", async () => {
  const f = fake((method) => (method === "PUT" ? json(200, PG) : undefined));
  const { container } = await dialog(f, INPUT, { id: PG.id, passwordSaved: true });
  assert.equal(field(container, "password")?.placeholder, "Saved");
  await click(button(container, "Save"));
  assert.equal((f.calls.at(-1)?.body as Record<string, unknown>).password, undefined);
  await click(button(container, "Clear"));
  await click(button(container, "Save"));
  assert.equal((f.calls.at(-1)?.body as Record<string, unknown>).clearPassword, true);
});

test("a computer without a keychain says where the password stays", async () => {
  const f = fake(() => undefined);
  const { container } = await dialog(f, {}, { keychain: false });
  assert.match(text(container), /This computer has no keychain\. The password is kept until westerm stops\./);
});

test("a SQLite connection asks only for a file", async () => {
  const f = fake(() => undefined);
  const { container } = await dialog(f, { engine: "sqlite" });
  assert.ok(field(container, "path"));
  assert.equal(field(container, "host"), null);
  assert.equal(container.querySelector('[name="ssh"]'), null);
});

// ---- Plugin ----

test("the plugin bar shows the db plugin after docker", () => {
  assert.deepEqual(BUILTIN_PLUGINS.map((plugin) => plugin.id), ["git", "docker", "db"]);
  assert.equal(dbPlugin.version, "0.1.0");
  assert.equal(dbPlugin.text.en.name, "Databases");
  assert.equal(dbPlugin.text.en.permissions.length, 4);
});

test("a build without the db plugin hides it from the manager", () => {
  assert.deepEqual(availableBuiltins(BUILTIN_PLUGINS, ["git", "docker"]).map((plugin) => plugin.id), ["git", "docker"]);
  assert.deepEqual(availableBuiltins(BUILTIN_PLUGINS, null).map((plugin) => plugin.id), ["git", "docker", "db"], "an older server lists none: all show");
});

test("the badge counts open connections and hides at zero", async () => {
  let open = 2;
  const f = fake((_method, path) => {
    if (path === "/connections") return json(200, { connections: [PG], keychain: true, open });
    if (path === "/clients") return json(200, CLIENTS);
    return undefined;
  });
  const instance = dbPlugin.start(f.ctx);
  try {
    await settle(8);
    assert.deepEqual(instance.status(), { badge: 2, alert: false });
    open = 0;
    instance.setPanelOpen(true);
    await settle(8);
    assert.deepEqual(instance.status(), { badge: null, alert: false });
  } finally {
    instance.dispose();
  }
});
