// The dialog that adds or edits a connection: a URL to paste, the fields,
// a connection test, and Save. New connections are read-only.

import { useEffect, useRef, useState, type ChangeEvent, type KeyboardEvent } from "react";
import { Check, Database, X } from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import type { PluginContext } from "../types.ts";
import { createConnection, testConnection, updateConnection } from "./api.ts";
import { DB_TEXT, type DbMessages } from "./i18n.ts";
import { defaultName, parseDbUrl } from "./model.ts";
import { CONN_COLORS, ENGINES, SSL_MODES, type ConnectionInput, type Engine } from "./types.ts";

export interface ConnectionDialogProps {
  ctx: PluginContext;
  /** The connection being edited; null adds one. */
  id: string | null;
  initial: ConnectionInput;
  /** A password to start with (from a found connection). */
  password: string | null;
  passwordSaved: boolean;
  /** The keychain worked the last time the server used it. */
  keychain: boolean;
  /** Hosts from ~/.ssh/config; null while loading. */
  sshHosts: string[] | null;
  /** Closes the dialog; true after a save, with a notice for the panel (the keychain did not take the password). */
  onDone: (saved: boolean, notice?: string | null) => void;
}

type TestState = { kind: "idle" } | { kind: "testing" } | { kind: "ok"; text: string } | { kind: "failed"; text: string };

const DEFAULT_PORT: Record<Engine, number | null> = { postgres: 5432, mysql: 3306, sqlite: null };

/** The dialog's label of a field the server named in `invalid:<field>`. */
function fieldLabel(field: string, t: DbMessages["dialog"]): string {
  const labels: Record<string, string> = {
    name: t.name,
    host: t.host,
    port: t.port,
    user: t.user,
    database: t.database,
    path: t.path,
    ssh: t.ssh,
  };
  return labels[field] ?? field;
}

export function ConnectionDialog({ ctx, id, initial, password: initialPassword, passwordSaved, keychain, sshHosts, onDone }: ConnectionDialogProps) {
  const { locale } = useI18n();
  const all = DB_TEXT[locale];
  const t = all.dialog;
  const [input, setInput] = useState<ConnectionInput>(initial);
  const [password, setPassword] = useState(initialPassword ?? "");
  const [clear, setClear] = useState(false);
  const [url, setUrl] = useState("");
  // A name the user typed (or an edited connection's) is not replaced by the default.
  const named = useRef(initial.name !== "");
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const first = useRef<HTMLInputElement>(null);

  useEffect(() => first.current?.focus(), []);

  const update = (change: Partial<ConnectionInput>) =>
    setInput((previous) => {
      const next = { ...previous, ...change };
      return named.current ? next : { ...next, name: defaultName(next) };
    });

  /** Fills the fields from a pasted URL; the URL (and its password) leaves the field. */
  const applyUrl = (text: string): boolean => {
    const parts = parseDbUrl(text);
    if (!parts) return false;
    const { password: urlPassword, ...fields } = parts;
    const engine = fields.engine ?? input.engine;
    update({
      host: "",
      port: null,
      user: "",
      database: "",
      path: null,
      ...(engine === "sqlite" ? { ssh: null } : {}),
      ...fields,
    });
    if (urlPassword !== undefined) {
      setPassword(urlPassword);
      setClear(false);
    }
    setUrl("");
    return true;
  };

  const onUrl = (event: ChangeEvent<HTMLInputElement>) => {
    const text = event.target.value;
    // A paste (more than one character at once) fills in at once; typing waits for Enter or leaving the field.
    if (text.length - url.length > 1 && applyUrl(text)) return;
    setUrl(text);
  };

  const onUrlKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      applyUrl(url);
    }
  };

  const typed = password !== "" ? password : null;

  const runTest = async () => {
    setTest({ kind: "testing" });
    const answer = await testConnection(ctx, input, typed);
    if (!answer.ok) {
      setTest({ kind: "failed", text: answer.failure.type === "error" ? answer.failure.error : all.requestFailed });
    } else if ("error" in answer.value) {
      setTest({ kind: "failed", text: answer.value.error });
    } else {
      setTest({ kind: "ok", text: t.connected(answer.value.ms, answer.value.version) });
    }
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    const answer = id === null ? await createConnection(ctx, input, typed) : await updateConnection(ctx, id, input, typed, clear && typed === null);
    setBusy(false);
    if (answer.ok) {
      const keychainError = answer.value.keychainError;
      onDone(true, keychainError ? t.keychainError(keychainError.split("\n")[0]) : null);
      return;
    }
    const failure = answer.failure;
    if (failure.type !== "error") {
      setError(all.requestFailed);
    } else if (failure.error === "nameUsed") {
      setError(t.nameUsed);
    } else if (failure.error.startsWith("invalid:")) {
      setError(t.invalid(fieldLabel(failure.error.slice("invalid:".length), t)));
    } else {
      setError(failure.error);
    }
  };

  const sqlite = input.engine === "sqlite";
  const hosts = sshHosts ?? [];
  const sshChoices = input.ssh && !hosts.includes(input.ssh) ? [input.ssh, ...hosts] : hosts;
  const showSaved = passwordSaved && !clear && password === "";

  return (
    <div className="db-dialog-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onDone(false)}>
      <div
        className="db-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={id === null ? t.addTitle : t.editTitle}
        onKeyDown={(event) => event.key === "Escape" && onDone(false)}
      >
        <header className="db-dialog-header">
          <Database size={15} aria-hidden="true" />
          <span>{id === null ? t.addTitle : t.editTitle}</span>
          <button type="button" className="icon-button" aria-label={all.cancel} onClick={() => onDone(false)}>
            <X size={15} />
          </button>
        </header>
        <form
          className="db-dialog-body"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <div className="db-engines" role="radiogroup" aria-label={t.engine}>
            {ENGINES.map((engine) => (
              <button
                key={engine}
                type="button"
                role="radio"
                aria-checked={input.engine === engine}
                className={input.engine === engine ? "db-engine on" : "db-engine"}
                onClick={() => update({ engine, ...(engine === "sqlite" ? { ssh: null } : {}) })}
              >
                <span className={`db-engine-mark db-engine-${engine}`} aria-hidden="true" />
                {all.engines[engine]}
              </button>
            ))}
          </div>
          <label className="db-field">
            <span>{t.url}</span>
            <input
              ref={first}
              name="url"
              value={url}
              placeholder={t.urlPlaceholder}
              spellCheck={false}
              autoComplete="off"
              onChange={onUrl}
              onKeyDown={onUrlKey}
              onBlur={() => url && applyUrl(url)}
            />
          </label>
          <div className="db-or">{t.orManual}</div>
          <label className="db-field">
            <span>{t.name}</span>
            <input
              name="name"
              value={input.name}
              spellCheck={false}
              onChange={(event) => {
                named.current = event.target.value !== "";
                setInput({ ...input, name: event.target.value });
              }}
            />
          </label>
          {sqlite ? (
            <label className="db-field">
              <span>{t.path}</span>
              <input
                name="path"
                value={input.path ?? ""}
                placeholder={t.pathPlaceholder}
                spellCheck={false}
                onChange={(event) => update({ path: event.target.value || null })}
              />
            </label>
          ) : (
            <>
              <div className="db-field db-host">
                <label>
                  <span>{t.host}</span>
                  <input name="host" value={input.host} spellCheck={false} onChange={(event) => update({ host: event.target.value })} />
                </label>
                <label>
                  <span>{t.port}</span>
                  <input
                    name="port"
                    inputMode="numeric"
                    value={input.port ?? ""}
                    placeholder={String(DEFAULT_PORT[input.engine] ?? "")}
                    onChange={(event) => {
                      const port = Number.parseInt(event.target.value, 10);
                      update({ port: Number.isFinite(port) && port > 0 && port < 65536 ? port : null });
                    }}
                  />
                </label>
              </div>
              <label className="db-field">
                <span>{t.user}</span>
                <input name="user" value={input.user} spellCheck={false} autoComplete="off" onChange={(event) => update({ user: event.target.value })} />
              </label>
              <div className="db-field">
                <label htmlFor="db-password">{t.password}</label>
                <div className="db-password-row">
                  <input
                    id="db-password"
                    name="password"
                    type="password"
                    autoComplete="new-password"
                    value={password}
                    placeholder={showSaved ? t.saved : ""}
                    onChange={(event) => setPassword(event.target.value)}
                  />
                  {showSaved && (
                    <button type="button" className="button" onClick={() => setClear(true)}>
                      {t.clear}
                    </button>
                  )}
                </div>
                {!keychain && <p className="db-note">{t.noKeychain}</p>}
              </div>
              <label className="db-field">
                <span>{t.database}</span>
                <input name="database" value={input.database} spellCheck={false} onChange={(event) => update({ database: event.target.value })} />
              </label>
              <label className="db-field">
                <span>{t.ssl}</span>
                <select name="ssl" value={input.ssl} onChange={(event) => update({ ssl: event.target.value as ConnectionInput["ssl"] })}>
                  {SSL_MODES.map((mode) => (
                    <option key={mode} value={mode}>
                      {mode}
                    </option>
                  ))}
                </select>
              </label>
              <label className="db-field">
                <span>{t.ssh}</span>
                <select name="ssh" value={input.ssh ?? ""} onChange={(event) => update({ ssh: event.target.value || null })}>
                  <option value="">{t.sshNone}</option>
                  {sshChoices.map((host) => (
                    <option key={host} value={host}>
                      {host}
                    </option>
                  ))}
                </select>
              </label>
            </>
          )}
          <div className="db-field">
            <span>{t.color}</span>
            <div className="db-colors" role="radiogroup" aria-label={t.color}>
              {CONN_COLORS.map((color) => (
                <button
                  key={color}
                  type="button"
                  role="radio"
                  aria-checked={input.color === color}
                  aria-label={all.colors[color]}
                  title={all.colors[color]}
                  className={`db-swatch db-color-${color}${input.color === color ? " on" : ""}`}
                  onClick={() => setInput({ ...input, color })}
                />
              ))}
            </div>
          </div>
          <label className="db-check">
            <input name="readOnly" type="checkbox" checked={input.readOnly} onChange={(event) => setInput({ ...input, readOnly: event.target.checked })} />
            <span>{t.readOnly}</span>
          </label>
          <p className="db-note">{t.readOnlyHelp}</p>
          {error && (
            <p className="db-error" role="alert">
              {error}
            </p>
          )}
          <footer className="db-dialog-footer">
            <button type="button" className="button" disabled={test.kind === "testing"} onClick={() => void runTest()}>
              {test.kind === "testing" ? t.testing : t.test}
            </button>
            {test.kind === "ok" && (
              <span className="db-test ok" role="status">
                <Check size={13} aria-hidden="true" /> {test.text}
              </span>
            )}
            {test.kind === "failed" && (
              <span className="db-test failed" role="status">
                {test.text}
              </span>
            )}
            <span className="db-spacer" />
            <button type="button" className="button" onClick={() => onDone(false)}>
              {all.cancel}
            </button>
            <button type="button" className="button primary" disabled={busy} onClick={() => void save()}>
              {t.save}
            </button>
          </footer>
        </form>
      </div>
    </div>
  );
}
