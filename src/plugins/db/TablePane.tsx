// The table pane: an SQL box over a result grid. A pane opened from the tree
// runs its preview once; a restored pane waits for Run, so reloading the page
// never sends a query on its own.

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Play, Square, Table2, X } from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import { useCopy } from "../../useCopy.ts";
import { PLUGIN_PANE_DATA_MAX } from "../../../shared/workspace-file.ts";
import type { PluginContext, PluginPaneProps } from "../types.ts";
import { cancelQuery, connect, runQuery } from "./api.ts";
import { DB_TEXT, type DbMessages } from "./i18n.ts";
import { isNumericType, newQueryId, shortCount, toCsv } from "./model.ts";
import type { ConnectionList, QueryResult, TablePaneData } from "./types.ts";

export interface TablePaneProps extends PluginPaneProps {
  ctx: PluginContext;
  list: ConnectionList | null;
}

type Outcome =
  | { kind: "waiting" }
  | { kind: "running"; queryId: string }
  | { kind: "result"; result: QueryResult }
  | { kind: "error"; error: string }
  | { kind: "password" }
  | { kind: "removed" };

const SAVE_DELAY = 400;
/** A cell shows this many characters; the popover and copy have all of it. */
const CELL_PREVIEW = 500;

/** The pane's data, or null when it is not a table pane's. */
function readData(data: unknown): TablePaneData | null {
  if (typeof data !== "object" || data === null) return null;
  const raw = data as Record<string, unknown>;
  if (typeof raw.connectionId !== "string" || typeof raw.sql !== "string") return null;
  return {
    connectionId: raw.connectionId,
    db: typeof raw.db === "string" ? raw.db : "",
    schema: typeof raw.schema === "string" ? raw.schema : null,
    table: typeof raw.table === "string" ? raw.table : null,
    sql: raw.sql,
    run: raw.run === true,
    estimate: typeof raw.estimate === "number" ? raw.estimate : null,
  };
}

const RUN_KEYS = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform ?? "") ? "⌘↵" : "Ctrl+Enter";

export function TablePane(props: TablePaneProps) {
  const { locale } = useI18n();
  const all = DB_TEXT[locale];
  const t = all.table;
  const data = readData(props.data);
  if (!data) return <p className="db-quiet">{all.requestFailed}</p>;
  return <TableView {...props} data={data} all={all} t={t} />;
}

function TableView({ ctx, list, data, setData, close, all, t }: TablePaneProps & { data: TablePaneData; all: DbMessages; t: DbMessages["table"] }) {
  const [sql, setSql] = useState(data.sql);
  const [outcome, setOutcome] = useState<Outcome>({ kind: "waiting" });
  const [selected, setSelected] = useState<[number, number] | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [tooLong, setTooLong] = useState(false);
  const [csvCopy, copyCsv] = useCopy();
  const [, copyCell] = useCopy();
  const dataRef = useRef(data);
  dataRef.current = data;
  const sqlRef = useRef(sql);
  sqlRef.current = sql;
  const started = useRef(false);
  const alive = useRef(true);
  useEffect(
    () => () => {
      alive.current = false;
    },
    [],
  );

  const connection = list?.connections.find((each) => each.id === data.connectionId) ?? null;

  const run = async () => {
    const queryId = newQueryId();
    setOutcome({ kind: "running", queryId });
    setSelected(null);
    const current = dataRef.current;
    const answer = await runQuery(ctx, current.connectionId, { db: current.db, schema: current.schema, sql: sqlRef.current, queryId });
    if (!alive.current) return;
    if (answer.ok) {
      setOutcome({ kind: "result", result: answer.value });
      return;
    }
    const failure = answer.failure;
    if (failure.type === "removed") setOutcome({ kind: "removed" });
    else if (failure.type === "needPassword") setOutcome({ kind: "password" });
    else if (failure.type === "gone") setOutcome({ kind: "error", error: all.requestFailed });
    else if (failure.kind === "cancelled") setOutcome({ kind: "error", error: t.cancelled });
    else if (failure.kind === "timeout") setOutcome({ kind: "error", error: t.timedOut });
    else setOutcome({ kind: "error", error: failure.error });
  };

  // A pane opened from the tree runs once, and remembers that it did.
  useEffect(() => {
    if (started.current || !data.run) return;
    started.current = true;
    setData({ ...data, run: false });
    void run();
  }, []);

  // The SQL is saved with the layout a moment after the last key, when the
  // saved file can hold it.
  useEffect(() => {
    if (sql === dataRef.current.sql) return;
    const timer = setTimeout(() => {
      const next = { ...dataRef.current, run: false, sql };
      const fits = JSON.stringify(next).length <= PLUGIN_PANE_DATA_MAX;
      setTooLong(!fits);
      if (fits) setData(next);
    }, SAVE_DELAY);
    return () => clearTimeout(timer);
  }, [sql, setData]);

  const cancel = () => {
    if (outcome.kind === "running") void cancelQuery(ctx, outcome.queryId);
  };

  const onSqlKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      if (outcome.kind !== "running") void run();
    }
  };

  const submitPassword = async () => {
    const answer = await connect(ctx, data.connectionId, password);
    if (!alive.current) return;
    if (answer.ok) {
      setPassword("");
      await run();
    } else if (answer.failure.type === "error") {
      setOutcome({ kind: "error", error: answer.failure.error });
    }
  };

  const result = outcome.kind === "result" ? outcome.result : null;
  const cut = new Set(result?.cut.map(([row, column]) => `${row}:${column}`));

  const onGridKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!result || !selected || event.key.toLowerCase() !== "c" || !(event.metaKey || event.ctrlKey)) return;
    event.preventDefault();
    copyCell(result.rows[selected[0]]?.[selected[1]] ?? "");
  };

  const place = [connection?.name, data.db, data.schema].filter((part) => part).join(" / ");
  const danger = connection?.color === "red";
  const estimate = shortCount(data.estimate);

  return (
    <div className={danger ? "db-table-pane db-danger" : "db-table-pane"}>
      <header className="db-pane-header">
        <Table2 size={13} className="db-icon" aria-hidden="true" />
        <span className="db-name">{data.table ?? t.sql}</span>
        {place && <span className="db-sub">{place}</span>}
        {connection?.readOnly && <span className="db-tag db-tag-ro">{all.readOnly}</span>}
      </header>
      <div className="db-sql">
        <textarea
          aria-label={t.sql}
          value={sql}
          spellCheck={false}
          rows={3}
          onChange={(event) => setSql(event.target.value)}
          onKeyDown={onSqlKey}
        />
        {outcome.kind === "running" ? (
          <button type="button" className="button" aria-label={t.cancel} onClick={cancel}>
            <Square size={12} /> {t.cancel}
          </button>
        ) : (
          <button type="button" className="button primary" title={t.runHint(RUN_KEYS)} onClick={() => void run()}>
            <Play size={12} /> {t.run}
          </button>
        )}
      </div>
      {tooLong && <p className="db-note db-too-long">{t.tooLong}</p>}
      <div className="db-result">
        {outcome.kind === "waiting" && <p className="db-quiet">{t.waiting}</p>}
        {outcome.kind === "running" && <p className="db-quiet">{t.running}</p>}
        {outcome.kind === "error" && <ErrorText text={outcome.error} />}
        {outcome.kind === "removed" && (
          <div className="db-result-notice">
            <p>{t.removed}</p>
            <button type="button" className="button" onClick={close}>
              {t.close}
            </button>
          </div>
        )}
        {outcome.kind === "password" && (
          <form
            className="db-result-notice"
            onSubmit={(event) => {
              event.preventDefault();
              void submitPassword();
            }}
          >
            <p>{t.needPassword}</p>
            <div className="db-password">
              <input type="password" aria-label={all.dialog.password} value={password} onChange={(event) => setPassword(event.target.value)} />
              <button type="button" className="button" onClick={() => void submitPassword()}>
                {all.connect}
              </button>
            </div>
          </form>
        )}
        {result && result.columns.length === 0 && result.changed === null && <p className="db-quiet">{t.noResult}</p>}
        {result && result.columns.length > 0 && (
          <div className="db-grid" tabIndex={0} onKeyDown={onGridKey}>
            <table>
              <thead>
                <tr>
                  {result.columns.map((column, index) => (
                    <th key={index}>
                      {column.name} <span className="db-type">{column.type}</span>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {result.rows.map((row, rowIndex) => (
                  <tr key={rowIndex}>
                    {row.map((cell, columnIndex) => {
                      const numeric = isNumericType(result.columns[columnIndex]?.type ?? "");
                      const classes = [
                        numeric ? "db-num" : "",
                        cell === null ? "db-null" : "",
                        selected?.[0] === rowIndex && selected[1] === columnIndex ? "db-cell-on" : "",
                      ]
                        .filter(Boolean)
                        .join(" ");
                      return (
                        <td
                          key={columnIndex}
                          className={classes || undefined}
                          onClick={() => setSelected([rowIndex, columnIndex])}
                          onDoubleClick={() => setShown(cell ?? "NULL")}
                        >
                          {cell === null ? "NULL" : cell.length > CELL_PREVIEW ? `${cell.slice(0, CELL_PREVIEW)}…` : cell}
                          {cut.has(`${rowIndex}:${columnIndex}`) && <span className="db-cut"> {t.truncated}</span>}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {shown !== null && (
          <div className="db-cell-popover" role="dialog" aria-label={t.sql}>
            <button type="button" className="icon-button" aria-label={t.close} onClick={() => setShown(null)}>
              <X size={13} />
            </button>
            <pre>{shown}</pre>
          </div>
        )}
      </div>
      {result && (
        <footer className="db-pane-footer">
          <span>{result.changed !== null ? t.changed(result.changed, result.ms) : t.rows(result.rows.length, result.ms)}</span>
          {estimate && data.table && result.changed === null && <span>{t.estimate(estimate)}</span>}
          {result.more && <span className="db-more-note">{t.more(result.rows.length)}</span>}
          <span className="db-spacer" />
          {result.columns.length > 0 && (
            <button type="button" className="button" onClick={() => copyCsv(toCsv(result))}>
              {csvCopy === "copied" ? t.copied : t.copyCsv}
            </button>
          )}
        </footer>
      )}
    </div>
  );
}

/** A database error: its first line strong, the rest faint. */
function ErrorText({ text }: { text: string }) {
  const [first, ...rest] = text.split("\n");
  return (
    <div className="db-result-error" role="alert">
      <strong>{first}</strong>
      {rest.length > 0 && <pre>{rest.join("\n")}</pre>}
    </div>
  );
}
