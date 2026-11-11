// The Databases plugin's rules, as pure functions.

import { SSL_MODES, type ConnectionInput, type QueryResult, type SslMode, type TreeNode } from "./types.ts";

export type UrlParts = Partial<ConnectionInput> & { password?: string };

/** A SQLite file path: absolute (Unix or Windows) or under the home folder. */
function isFilePath(text: string): boolean {
  return text.startsWith("/") || text.startsWith("~/") || /^[A-Za-z]:[\\/]/.test(text) || text.startsWith("\\\\");
}

const decode = (text: string) => {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
};

/**
 * The connection fields of a pasted URL: `postgres://`, `postgresql://`,
 * `mysql://`, `mariadb://` (as MySQL), `sqlite:`, or a file path. Only
 * `sslmode` is read from the query. Null for anything else.
 */
export function parseDbUrl(text: string): UrlParts | null {
  const value = text.trim();
  if (value === "") return null;
  if (isFilePath(value)) return { engine: "sqlite", path: value };
  const sqlite = /^sqlite:(?:\/\/)?(.*)$/i.exec(value);
  if (sqlite) {
    const path = decode(sqlite[1]);
    return isFilePath(path) ? { engine: "sqlite", path } : null;
  }
  const scheme = /^(postgres|postgresql|mysql|mariadb):\/\//i.exec(value);
  if (!scheme) return null;
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    return null;
  }
  if (url.hostname === "") return null;
  const engine = scheme[1].toLowerCase().startsWith("postgres") ? "postgres" : "mysql";
  const parts: UrlParts = { engine };
  if (url.username) parts.user = decode(url.username);
  if (url.password) parts.password = decode(url.password);
  parts.host = decode(url.hostname.replace(/^\[(.*)\]$/, "$1"));
  if (url.port) parts.port = Number(url.port);
  const database = decode(url.pathname.replace(/^\//, ""));
  if (database) parts.database = database;
  const ssl = url.searchParams.get("sslmode");
  if (ssl && (SSL_MODES as readonly string[]).includes(ssl)) parts.ssl = ssl as SslMode;
  return parts;
}

/** A row estimate in a few characters: 999, 1.2k, 48k, 1.2M, 12M. */
export function shortCount(count: number | null): string | null {
  if (count === null || count < 0 || !Number.isFinite(count)) return null;
  const short = (value: number, unit: string) =>
    value < 10 ? `${Math.floor(value * 10) / 10}${unit}` : `${Math.floor(value)}${unit}`;
  if (count < 1000) return String(Math.floor(count));
  if (count < 1_000_000) return short(count / 1000, "k");
  return short(count / 1_000_000, "M");
}

/** The name a new connection gets: the file for SQLite, else `<database>@<host>`. */
export function defaultName(input: Partial<ConnectionInput>): string {
  if (input.engine === "sqlite") {
    const path = input.path ?? "";
    return path.split(/[\\/]/).pop() ?? path;
  }
  const host = input.host ?? "";
  return input.database ? `${input.database}@${host}` : host;
}

/** Tables and views whose name holds `query` (any case); other nodes stay. */
export function filterNodes(nodes: TreeNode[], query: string): TreeNode[] {
  const wanted = query.trim().toLowerCase();
  if (wanted === "") return nodes;
  return nodes.filter((node) => (node.kind !== "table" && node.kind !== "view") || node.name.toLowerCase().includes(wanted));
}

/** One CSV field: quoted when it holds a comma, a quote, or a line break. */
function csvField(value: string | null): string {
  if (value === null) return "";
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** The result as CSV (RFC 4180): a header line, then the rows; NULL is empty. */
export function toCsv(result: QueryResult): string {
  const lines = [result.columns.map((column) => csvField(column.name)), ...result.rows.map((row) => row.map(csvField))];
  return lines.map((line) => `${line.join(",")}\r\n`).join("");
}

const NUMERIC =
  /^(?:INT|INT2|INT4|INT8|INTEGER|SMALLINT|MEDIUMINT|BIGINT|TINYINT|FLOAT|FLOAT4|FLOAT8|REAL|DOUBLE|NUMERIC|DECIMAL|OID|YEAR)(?: UNSIGNED)?$/;

/** Whether values of the type line up on the right. */
export function isNumericType(type: string): boolean {
  return NUMERIC.test(type.trim().toUpperCase());
}

/** A query id the cancel route takes: 16 lowercase hex digits. */
export function newQueryId(): string {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}
