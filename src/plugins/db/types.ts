// The Databases plugin's data, as the server routes send it
// (native/src/server/plugins/db/).

export type Engine = "postgres" | "mysql" | "sqlite";
export type SslMode = "prefer" | "disable" | "require" | "verify-full";
export type ConnColor = "gray" | "green" | "yellow" | "red" | "blue";

export const ENGINES: readonly Engine[] = ["postgres", "mysql", "sqlite"];
export const SSL_MODES: readonly SslMode[] = ["prefer", "disable", "require", "verify-full"];
export const CONN_COLORS: readonly ConnColor[] = ["gray", "green", "yellow", "red", "blue"];

/** A connection as the dialog edits it. */
export interface ConnectionInput {
  name: string;
  engine: Engine;
  host: string;
  port: number | null;
  user: string;
  database: string;
  ssl: SslMode;
  /** A SQLite file: absolute, or `~/…`. */
  path: string | null;
  /** A `~/.ssh/config` host to tunnel through. */
  ssh: string | null;
  color: ConnColor;
  readOnly: boolean;
}

export type ConnectionStatus = "idle" | "connecting" | "open" | "failed";

/** A saved connection as the list shows it. Never a password. */
export interface Connection extends ConnectionInput {
  id: string;
  passwordSaved: boolean;
  status: ConnectionStatus;
  error: string | null;
}

export interface ConnectionList {
  connections: Connection[];
  /** The keychain worked the last time it was used. */
  keychain: boolean;
  /** Connections with a pool open: the plugin button's badge. */
  open: number;
}

export type NodeKind = "database" | "schema" | "table" | "view";

export interface TreeNode {
  name: string;
  kind: NodeKind;
  /** The database's estimate. */
  rows: number | null;
  /** The first SQL a table pane runs. */
  preview: string | null;
}

export interface Column {
  name: string;
  type: string;
}

export interface QueryResult {
  columns: Column[];
  rows: (string | null)[][];
  /** Cells cut at the cell limit: [row, column]. */
  cut: [number, number][];
  /** More rows were there than shown. */
  more: boolean;
  /** Rows a statement without columns changed. */
  changed: number | null;
  ms: number;
}

export interface Clients {
  psql: boolean;
  mysql: "mysql" | "mariadb" | null;
  sqlite3: boolean;
  /** A console pane can open at all. */
  consoles: boolean;
}

/** A connection found in a session folder or in Docker. */
export interface Found {
  source: "env" | "file" | "docker";
  label: string;
  input: ConnectionInput;
  password: string | null;
}

/** What a table pane keeps in the saved layout. */
export interface TablePaneData {
  connectionId: string;
  db: string;
  schema: string | null;
  table: string | null;
  sql: string;
  /** Run once when the pane opens (from the tree); a restored pane waits for Run. */
  run: boolean;
  /** The table's estimated row count. */
  estimate: number | null;
}
