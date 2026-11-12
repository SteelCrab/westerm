// The Databases panel in the sidebar slot: saved connections as a tree
// (connection → database → schema → tables and views), a filter, and the
// connection dialog. A table opens a table pane; the console button opens
// psql, mysql, or sqlite3 as a shell session.

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  ChevronRight,
  Copy,
  Database,
  Ellipsis,
  Eye,
  Folder,
  Pencil,
  Plug,
  Plus,
  RefreshCw,
  Search,
  SquareTerminal,
  Table2,
  Trash2,
  Unplug,
} from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import { useSshHosts } from "../../useSshHosts.ts";
import { PopupMenu, type MenuItem } from "../PopupMenu.tsx";
import type { PluginContext } from "../types.ts";
import { connect, deleteConnection, detect, disconnect, forgetAll, openConsole, readTree } from "./api.ts";
import { ConnectionDialog } from "./ConnectionDialog.tsx";
import { DB_TEXT, type DbMessages } from "./i18n.ts";
import { filterNodes, shortCount } from "./model.ts";
import type { Clients, Connection, ConnectionInput, ConnectionList, Engine, Found, TablePaneData, TreeNode } from "./types.ts";

export interface DbPanelProps {
  ctx: PluginContext;
  /** The last list the server sent; null until the first one. */
  list: ConnectionList | null;
  clients: Clients | null;
  /** Reads the list again. */
  refresh: () => void;
}

/** A level of the tree as the panel has it. */
type Level =
  | { state: "loading" }
  | { state: "ok"; nodes: TreeNode[] }
  | { state: "password" }
  | { state: "error"; error: string };

/** Where a level hangs: the connection, a database, a schema. */
interface Place {
  id: string;
  db: string | null;
  schema: string | null;
}

const keyOf = ({ id, db, schema }: Place) => `${id}\u0000${db ?? ""}\u0000${schema ?? ""}`;

type Menu = { x: number; y: number; items: MenuItem[]; label: string };
type Confirm = { kind: "delete"; connection: Connection } | { kind: "forget" };
type Dialog = { id: string | null; input: ConnectionInput; password: string | null; passwordSaved: boolean };

const BLANK: ConnectionInput = {
  name: "",
  engine: "postgres",
  host: "",
  port: null,
  user: "",
  database: "",
  ssl: "prefer",
  path: null,
  ssh: null,
  color: "gray",
  readOnly: true,
};

/** The console program of an engine, and whether it was found. */
function consoleProgram(engine: Engine, clients: Clients | null): { program: string; found: boolean } {
  switch (engine) {
    case "postgres":
      return { program: "psql", found: clients?.psql === true };
    case "mysql":
      return { program: clients?.mysql ?? "mysql", found: clients?.mysql !== null && clients?.mysql !== undefined };
    case "sqlite":
      return { program: "sqlite3", found: clients?.sqlite3 === true };
  }
}

/** How to install a console program on this computer. */
function installCommand(program: string): string {
  const mac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform ?? "");
  const brew: Record<string, string> = { psql: "brew install libpq", mysql: "brew install mysql-client", sqlite3: "brew install sqlite" };
  const apt: Record<string, string> = { psql: "sudo apt install postgresql-client", mysql: "sudo apt install mysql-client", sqlite3: "sudo apt install sqlite3" };
  return (mac ? brew : apt)[program] ?? program;
}

/** A connection's URL without its password. */
function connectionUrl(connection: Connection): string {
  if (connection.engine === "sqlite") return `sqlite:${connection.path ?? ""}`;
  const scheme = connection.engine === "postgres" ? "postgres" : "mysql";
  const user = connection.user ? `${encodeURIComponent(connection.user)}@` : "";
  const port = connection.port ? `:${connection.port}` : "";
  return `${scheme}://${user}${connection.host}${port}/${encodeURIComponent(connection.database)}`;
}

const below = (element: Element) => {
  const rect = element.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + 2 };
};

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // The page may not have clipboard access; nothing else to do.
  }
}

export function DbPanel({ ctx, list, clients, refresh }: DbPanelProps) {
  const { locale } = useI18n();
  const t = DB_TEXT[locale];
  const [levels, setLevels] = useState<Map<string, Level>>(new Map());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [filter, setFilter] = useState("");
  const [menu, setMenu] = useState<Menu | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [passwords, setPasswords] = useState<Map<string, string>>(new Map());
  const [notice, setNotice] = useState<string | null>(null);
  const [found, setFound] = useState<Found[]>([]);
  const sshHosts = useSshHosts(dialog !== null);
  const mounted = useRef(true);
  useEffect(
    () => () => {
      mounted.current = false;
    },
    [],
  );

  const setLevel = useCallback((key: string, level: Level) => {
    if (!mounted.current) return;
    setLevels((previous) => new Map(previous).set(key, level));
  }, []);

  const load = useCallback(
    async (place: Place) => {
      const key = keyOf(place);
      setLevel(key, { state: "loading" });
      const answer = await readTree(ctx, place.id, place.db, place.schema);
      if (answer.ok) setLevel(key, { state: "ok", nodes: answer.value });
      else if (answer.failure.type === "needPassword") setLevel(key, { state: "password" });
      else if (answer.failure.type === "gone") ctx.refreshInstalled();
      else setLevel(key, { state: "error", error: answer.failure.type === "error" ? answer.failure.error : t.requestFailed });
      refresh();
    },
    [ctx, refresh, setLevel, t.requestFailed],
  );

  /** Looks for connections in the session folders and Docker: when the panel opens, and on Refresh. */
  const look = useCallback(async () => {
    const answer = await detect(ctx, ctx.folders());
    if (mounted.current && answer.ok) setFound(answer.value);
  }, [ctx]);

  useEffect(() => {
    void look();
  }, [look]);

  const toggle = (place: Place) => {
    const key = keyOf(place);
    const next = new Set(open);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
      if (!levels.has(key) || levels.get(key)?.state !== "ok") void load(place);
    }
    setOpen(next);
  };

  const reloadAll = () => {
    setLevels(new Map());
    for (const key of open) {
      const [id, db, schema] = key.split("\u0000");
      void load({ id, db: db || null, schema: schema || null });
    }
    void look();
    refresh();
  };

  const submitPassword = async (connection: Connection, place: Place) => {
    const password = passwords.get(connection.id) ?? "";
    const answer = await connect(ctx, connection.id, password);
    if (answer.ok) {
      setPasswords((previous) => {
        const next = new Map(previous);
        next.delete(connection.id);
        return next;
      });
      await load(place);
    } else if (answer.failure.type === "error") {
      setLevel(keyOf(place), { state: "error", error: answer.failure.error });
    }
  };

  const openTable = (connection: Connection, db: string, schema: string | null, node: TreeNode) => {
    const data: TablePaneData = {
      connectionId: connection.id,
      db,
      schema,
      table: node.name,
      sql: node.preview ?? "",
      run: true,
      estimate: node.rows,
    };
    ctx.openPane({ view: "table", title: node.name, data, reuseKey: `${connection.id}/${db}/${schema ?? ""}/${node.name}` });
  };

  const openConsoleFor = async (connection: Connection, db: string) => {
    const answer = await openConsole(ctx, connection.id, db);
    if (answer.ok) {
      setNotice(null);
      ctx.openShell({ action: "console", target: answer.value.token, cwd: null, name: answer.value.name, reuse: false });
    } else {
      setNotice(answer.failure.type === "error" ? answer.failure.error : t.requestFailed);
    }
  };

  const consoleButton = (connection: Connection, db: string) => {
    const { program, found } = consoleProgram(connection.engine, clients);
    const usable = clients?.consoles === true && found;
    const title = clients && !clients.consoles ? t.noConsole : found ? t.openConsole : `${t.notFound(program)} · ${t.installWith(installCommand(program))}`;
    return (
      <button
        type="button"
        className="icon-button db-console"
        aria-label={t.openConsole}
        title={title}
        disabled={!usable}
        onClick={() => void openConsoleFor(connection, db)}
      >
        <SquareTerminal size={13} />
      </button>
    );
  };

  const connectionMenu = (connection: Connection, element: Element) => {
    const linked = connection.status === "open";
    const items: MenuItem[] = [
      linked
        ? { label: t.disconnect, icon: Unplug, run: () => void disconnect(ctx, connection.id).then(reloadAfterChange) }
        : { label: t.connect, icon: Plug, run: () => void connect(ctx, connection.id).then(reloadAfterChange) },
      { label: t.openConsole, icon: SquareTerminal, run: () => void openConsoleFor(connection, connection.database) },
      { label: t.edit, icon: Pencil, separated: true, run: () => setDialog({ id: connection.id, input: inputOf(connection), password: null, passwordSaved: connection.passwordSaved }) },
      {
        label: t.duplicate,
        icon: Copy,
        run: () => setDialog({ id: null, input: { ...inputOf(connection), name: `${connection.name} 2` }, password: null, passwordSaved: false }),
      },
      { label: t.copyUrl, icon: Copy, run: () => void copyText(connectionUrl(connection)) },
      { label: t.delete, icon: Trash2, danger: true, separated: true, run: () => setConfirm({ kind: "delete", connection }) },
    ];
    setMenu({ ...below(element), items, label: connection.name });
  };

  const tableMenu = (connection: Connection, db: string, schema: string | null, node: TreeNode, element: Element) => {
    const items: MenuItem[] = [
      { label: t.preview, icon: Table2, run: () => openTable(connection, db, schema, node) },
      { label: t.openInConsole, icon: SquareTerminal, run: () => void openConsoleFor(connection, db) },
      { label: t.copyName, icon: Copy, separated: true, run: () => void copyText(node.name) },
      { label: t.copySelect, icon: Copy, run: () => void copyText(node.preview ?? "") },
    ];
    setMenu({ ...below(element), items, label: node.name });
  };

  const reloadAfterChange = () => {
    refresh();
  };

  const confirmed = async () => {
    if (!confirm) return;
    if (confirm.kind === "delete") await deleteConnection(ctx, confirm.connection.id);
    else await forgetAll(ctx);
    setConfirm(null);
    setLevels(new Map());
    setOpen(new Set());
    refresh();
  };

  const connections = list?.connections ?? [];

  /** The rows of one level, or why there are none. */
  const levelRows = (connection: Connection, place: Place, depth: number): ReactNode => {
    const level = levels.get(keyOf(place));
    if (!level || level.state === "loading") return <p className="db-quiet" style={{ paddingLeft: depth * 12 }}>{t.loading}</p>;
    if (level.state === "password") {
      return (
        <form
          className="db-password"
          style={{ paddingLeft: depth * 12 }}
          onSubmit={(event) => {
            event.preventDefault();
            void submitPassword(connection, place);
          }}
        >
          <input
            type="password"
            aria-label={t.passwordFor(connection.name)}
            placeholder={t.passwordFor(connection.name)}
            value={passwords.get(connection.id) ?? ""}
            onChange={(event) => setPasswords(new Map(passwords).set(connection.id, event.target.value))}
          />
          <button type="button" className="button" onClick={() => void submitPassword(connection, place)}>
            {t.connect}
          </button>
        </form>
      );
    }
    if (level.state === "error") {
      return (
        <div className="db-failure" style={{ paddingLeft: depth * 12 }}>
          <p className="db-error">{level.error}</p>
          <button type="button" className="button" onClick={() => void load(place)}>
            {t.retry}
          </button>
        </div>
      );
    }
    const nodes = filterNodes(level.nodes, filter);
    if (nodes.length === 0) return <p className="db-quiet" style={{ paddingLeft: depth * 12 }}>{t.nothing}</p>;
    return (
      <ul role="group">
        {nodes.map((node) => renderNode(connection, place, node, depth))}
      </ul>
    );
  };

  /** A database, schema, table, or view row (and the level under it). */
  function renderNode(connection: Connection, place: Place, node: TreeNode, depth: number): ReactNode {
    const indent = { paddingLeft: depth * 12 };
    const key = `${node.kind}:${node.name}`;
    if (node.kind === "table" || node.kind === "view") {
      const db = place.db ?? "";
      return (
        <li key={key} role="treeitem" className="db-item">
          <div className="db-row db-leaf" style={indent}>
            <button type="button" className="db-open" title={node.preview ?? undefined} onClick={() => openTable(connection, db, place.schema, node)}>
              {node.kind === "view" ? <Eye size={13} className="db-icon db-view" aria-hidden="true" /> : <Table2 size={13} className="db-icon" aria-hidden="true" />}
              <span className="db-name">{node.name}</span>
              {node.kind === "view" ? <span className="db-quiet-tag">{t.view}</span> : <span className="db-count">{shortCount(node.rows)}</span>}
            </button>
            <span className="db-actions">
              <button type="button" className="icon-button db-preview" aria-label={t.preview} title={t.preview} onClick={() => openTable(connection, db, place.schema, node)}>
                <Table2 size={13} />
              </button>
              {consoleButton(connection, db)}
              <button type="button" className="icon-button db-more" aria-label={t.more} onClick={(event) => tableMenu(connection, db, place.schema, node, event.currentTarget)}>
                <Ellipsis size={13} />
              </button>
            </span>
          </div>
        </li>
      );
    }
    const child: Place =
      node.kind === "database" ? { id: connection.id, db: node.name, schema: null } : { id: connection.id, db: place.db, schema: node.name };
    const expanded = open.has(keyOf(child));
    return (
      <li key={key} role="treeitem" aria-expanded={expanded} className="db-item">
        <div className="db-row" style={indent}>
          <button type="button" className="db-toggle" aria-expanded={expanded} onClick={() => toggle(child)}>
            <ChevronRight size={12} className={expanded ? "db-chevron open" : "db-chevron"} aria-hidden="true" />
            {node.kind === "database" ? <Database size={13} className="db-icon" aria-hidden="true" /> : <Folder size={13} className="db-icon" aria-hidden="true" />}
            <span className="db-name">{node.name}</span>
          </button>
          {node.kind === "database" && <span className="db-actions">{consoleButton(connection, node.name)}</span>}
        </div>
        {expanded && levelRows(connection, child, depth + 1)}
      </li>
    );
  }

  const confirmText = confirm?.kind === "delete" ? t.deleteConfirm(confirm.connection.name) : t.forgetConfirm(connections.length);

  return (
    <aside className="session-panel db-panel" aria-label={t.name}>
      <header className="db-header">
        <span className="db-logo" aria-hidden="true">
          <Database size={13} />
        </span>
        <span className="db-title">{t.name}</span>
        {list && <span className="db-sub">{t.connections(connections.length)}</span>}
        <button type="button" className="icon-button" aria-label={t.add} title={t.add} onClick={() => setDialog({ id: null, input: BLANK, password: null, passwordSaved: false })}>
          <Plus size={15} />
        </button>
        <button type="button" className="icon-button" aria-label={t.refresh} title={t.refresh} onClick={reloadAll}>
          <RefreshCw size={14} />
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label={t.more}
          title={t.more}
          aria-haspopup="menu"
          onClick={(event) =>
            setMenu({
              ...below(event.currentTarget),
              label: t.more,
              items: [
                { label: t.forgetAll, icon: Trash2, danger: true, disabled: connections.length === 0, run: () => setConfirm({ kind: "forget" }) },
                { label: t.uninstall, run: () => ctx.openManager(), separated: true },
              ],
            })
          }
        >
          <Ellipsis size={15} />
        </button>
      </header>
      {connections.length > 0 && (
        <label className="db-filter">
          <Search size={13} aria-hidden="true" />
          <input type="search" value={filter} placeholder={t.filter} aria-label={t.filter} spellCheck={false} onChange={(event) => setFilter(event.target.value)} />
        </label>
      )}
      {confirm && (
        <div className="db-confirm" role="alertdialog" aria-label={confirmText}>
          <p>{confirmText}</p>
          <div className="db-confirm-actions">
            <button type="button" className="button danger" onClick={() => void confirmed()}>
              {confirm.kind === "delete" ? t.deleteButton : t.forgetButton}
            </button>
            <button type="button" className="button" onClick={() => setConfirm(null)}>
              {t.cancel}
            </button>
          </div>
        </div>
      )}
      {notice && <p className="db-error db-notice">{notice}</p>}
      {list === null ? (
        <p className="db-quiet">{t.loading}</p>
      ) : connections.length === 0 ? (
        <div className="db-empty">
          <p>{t.empty}</p>
          <button type="button" className="button primary" onClick={() => setDialog({ id: null, input: BLANK, password: null, passwordSaved: false })}>
            {t.add}
          </button>
        </div>
      ) : (
        <ul className="tree db-tree" role="tree" aria-label={t.tree}>
          {connections.map((connection) => {
            const place: Place = { id: connection.id, db: null, schema: null };
            const expanded = open.has(keyOf(place));
            const level = levels.get(keyOf(place));
            return (
              <li key={connection.id} role="treeitem" aria-expanded={expanded} className="db-item">
                <div className="db-row db-connection">
                  <button type="button" className="db-toggle" aria-expanded={expanded} title={subtitle(connection)} onClick={() => toggle(place)}>
                    <ChevronRight size={12} className={expanded ? "db-chevron open" : "db-chevron"} aria-hidden="true" />
                    <span className={`db-engine-mark db-engine-${connection.engine}`} aria-hidden="true" />
                    {connection.color !== "gray" && <span className={`db-color db-color-${connection.color}`} aria-hidden="true" />}
                    <span className="db-name">{connection.name}</span>
                    <span className="db-sub">{subtitle(connection)}</span>
                    {connection.ssh && <span className="db-tag db-tag-ssh">ssh {connection.ssh}</span>}
                    {connection.readOnly && <span className="db-tag db-tag-ro" title={t.readOnly}>RO</span>}
                    <span className={`db-dot db-dot-${connection.status}`} role="img" aria-label={t.status[connection.status]} />
                  </button>
                  <span className="db-actions">
                    {consoleButton(connection, connection.database)}
                    <button type="button" className="icon-button db-more" aria-label={t.more} onClick={(event) => connectionMenu(connection, event.currentTarget)}>
                      <Ellipsis size={13} />
                    </button>
                  </span>
                </div>
                {connection.status === "failed" && connection.error && level?.state !== "error" && (
                  <p className="db-error db-row-error">{connection.error}</p>
                )}
                {expanded && levelRows(connection, place, 1)}
              </li>
            );
          })}
        </ul>
      )}
      {found.length > 0 && (
        <section className="db-found" aria-label={t.found}>
          <h3>{t.found}</h3>
          <ul>
            {found.map((item, index) => (
              <li key={`${item.source}:${item.label}:${index}`} className="db-found-row">
                <span className={`db-engine-mark db-engine-${item.input.engine}`} aria-hidden="true" />
                <span className="db-name" title={item.label}>
                  {item.label}
                </span>
                <button
                  type="button"
                  className="button db-found-add"
                  onClick={() => setDialog({ id: null, input: item.input, password: item.password, passwordSaved: false })}
                >
                  {t.foundAdd}
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {menu && <PopupMenu label={menu.label} x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {dialog && (
        <ConnectionDialog
          ctx={ctx}
          id={dialog.id}
          initial={dialog.input}
          password={dialog.password}
          passwordSaved={dialog.passwordSaved}
          keychain={list?.keychain !== false}
          sshHosts={sshHosts?.map((host) => host.alias) ?? null}
          onDone={(saved, saveNotice) => {
            setDialog(null);
            setNotice(saveNotice ?? null);
            if (saved) {
              setLevels(new Map());
              void look();
              refresh();
            }
          }}
        />
      )}
    </aside>
  );
}

/** The connection's fields, as the dialog edits them. */
function inputOf(connection: Connection): ConnectionInput {
  const { name, engine, host, port, user, database, ssl, path, ssh, color, readOnly } = connection;
  return { name, engine, host, port, user, database, ssl, path, ssh, color, readOnly };
}

/** `host:port`, or the folder of a SQLite file (home as ~). */
function subtitle(connection: Connection): string {
  if (connection.engine === "sqlite") {
    const path = (connection.path ?? "").replace(/^\/(?:Users|home)\/[^/]+(?=\/|$)/, "~");
    const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    return cut > 0 ? path.slice(0, cut) : path;
  }
  return connection.port ? `${connection.host}:${connection.port}` : connection.host;
}

export type { DbMessages };
