import { useEffect, useRef, useState } from "react";
import { Puzzle, type LucideIcon } from "lucide-react";
import { useI18n } from "../i18n/index.tsx";
import type { Messages } from "../i18n/messages.ts";
import { uninstall, update, type UpdateConfirm } from "./external/api.ts";
import type { ExternalPermissions } from "./external/info.ts";
import { permissionRows, type Badge, type Entry, type Risk } from "./managerModel.ts";

const BADGE_TEXT: Record<Badge, (t: Messages) => string> = {
  "built-in": (t) => t.plugins.badgeBuiltIn,
  verified: (t) => t.plugins.badgeVerified,
  unverified: (t) => t.plugins.badgeUnverified,
  local: (t) => t.plugins.badgeLocal,
};

const RISK_TEXT: Record<Risk, (t: Messages) => string> = {
  high: (t) => t.plugins.riskHigh,
  medium: (t) => t.plugins.riskMedium,
  low: (t) => t.plugins.riskLow,
};

export function BadgeChip({ badge }: { badge: Badge }) {
  const { t } = useI18n();
  return <span className={`plugin-badge ${badge}`}>{BADGE_TEXT[badge](t)}</span>;
}

/** A plugin's SVG as an <img>, a built-in's own icon, or the puzzle. */
export function EntryIcon({ iconUrl, icon: Icon = Puzzle, large = false }: { iconUrl: string | null; icon?: LucideIcon; large?: boolean }) {
  const size = large ? 22 : 16;
  return (
    <span className={large ? "plugin-icon large" : "plugin-icon"} aria-hidden="true">
      {iconUrl ? <img src={iconUrl} alt="" width={size} height={size} /> : <Icon size={size} strokeWidth={1.75} />}
    </span>
  );
}

/** What an external plugin may do, one line each with its risk. */
export function PermissionRows({ permissions }: { permissions: ExternalPermissions }) {
  const { t } = useI18n();
  const rows = permissionRows(permissions, t);
  return (
    <ul className="plugin-permissions risks">
      {rows.length === 0 && <li>{t.plugins.permNone}</li>}
      {rows.map((row) => (
        <li key={row.text}>
          <span>{row.text}</span>
          <span className={`plugin-risk ${row.risk}`}>{RISK_TEXT[row.risk](t)}</span>
        </li>
      ))}
    </ul>
  );
}

/** A failure line: the server's reason, or the general one when it gave none. */
export function FailedLine({ error }: { error: string | null }) {
  const { t } = useI18n();
  if (error === null) return null;
  return (
    <p className="plugin-failed" role="alert">
      {error || t.plugins.failed}
    </p>
  );
}

export type DetailState =
  | { kind: "idle" }
  | { kind: "confirmUninstall" }
  | { kind: "confirmUpdate" }
  /** The new version (or the reloaded folder) asks for more: show only what it adds. */
  | { kind: "grow"; confirm: UpdateConfirm };

export type DetailAction = "install" | "uninstall" | "update" | "reload" | "reinstall" | "confirm" | "cancel";

export interface PluginDetailViewProps {
  entry: Entry;
  /** A built-in plugin's own icon. */
  icon?: LucideIcon;
  /** A built-in plugin's own permission lines; external ones come from `entry.permissions`. */
  lines: string[] | null;
  state: DetailState;
  busy: boolean;
  error: string | null;
  onAction: (action: DetailAction) => void;
}

/**
 * The picked plugin: name, version, author, source, description, what it
 * may do and how risky that is, and its buttons. Uninstall and Update ask
 * on a line inside the panel first; an update that asks for more shows only
 * what it adds. What appears takes the focus, so it stays inside the dialog.
 */
export function PluginDetailView({ entry, icon, lines, state, busy, error, onAction }: PluginDetailViewProps) {
  const { t } = useI18n();
  const builtin = entry.badge === "built-in";
  const reload = entry.updateMode === "reload";
  const confirmText =
    state.kind === "confirmUninstall"
      ? [t.plugins.uninstallConfirm(entry.name), entry.badge === "local" ? t.plugins.folderStays : ""].filter(Boolean).join(" ")
      : state.kind === "confirmUpdate" && entry.update !== null
        ? t.plugins.updateConfirm(entry.name, entry.update)
        : null;
  const asking = confirmText !== null;
  const growHeading = useRef<HTMLHeadingElement>(null);
  const confirmFirst = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (state.kind === "grow") growHeading.current?.focus();
  }, [state.kind]);
  useEffect(() => {
    if (asking) confirmFirst.current?.focus();
  }, [asking]);

  if (state.kind === "grow") {
    return (
      <>
        <div className="plugin-detail-head">
          <EntryIcon iconUrl={entry.iconUrl} icon={icon} large />
          <div className="plugin-detail-name">
            <h3 ref={growHeading} tabIndex={-1}>
              {reload ? t.plugins.reloadConfirm(entry.name) : t.plugins.updateConfirm(entry.name, state.confirm.version)}
            </h3>
          </div>
        </div>
        <div>
          <h4 className="plugin-can">{t.plugins.adds}</h4>
          <PermissionRows permissions={state.confirm.added} />
        </div>
        <p className="plugin-note">{t.plugins.networkNote}</p>
        <FailedLine error={error} />
        <div className="plugin-actions">
          <button type="button" className="button" disabled={busy} onClick={() => onAction("cancel")}>
            {t.plugins.cancel}
          </button>
          <button type="button" className="button primary" disabled={busy} onClick={() => onAction("confirm")}>
            {reload ? t.plugins.reload : t.plugins.update}
          </button>
        </div>
      </>
    );
  }

  const idle = state.kind === "idle";

  return (
    <>
      <div className="plugin-detail-head">
        <EntryIcon iconUrl={entry.iconUrl} icon={icon} large />
        <div className="plugin-detail-name">
          <h3>
            {entry.name} <BadgeChip badge={entry.badge} />
          </h3>
          <span className="settings-version">
            v{entry.version}
            {entry.author !== null && ` · ${t.plugins.by(entry.author)}`}
          </span>
        </div>
        {entry.installed && entry.updateMode === "check" && entry.update !== null && (
          <button type="button" className="button primary" disabled={busy || !idle} onClick={() => onAction("update")}>
            {t.plugins.updateTo(entry.update)}
          </button>
        )}
        {entry.installed && entry.updateMode === "reinstall" && (
          <button type="button" className="button" disabled={busy || !idle} onClick={() => onAction("reinstall")}>
            {t.plugins.reinstallToUpdate}
          </button>
        )}
        {entry.installed && entry.updateMode === "reload" && (
          <button type="button" className="button" disabled={busy || !idle} onClick={() => onAction("reload")}>
            {t.plugins.reload}
          </button>
        )}
        <button
          type="button"
          className={entry.installed ? "button" : "button primary"}
          disabled={busy || !idle}
          onClick={() => onAction(entry.installed ? "uninstall" : "install")}
        >
          {entry.installed ? t.plugins.uninstall : t.plugins.install}
        </button>
      </div>
      {confirmText !== null && (
        <div className="plugin-confirm" role="group">
          <p>{confirmText}</p>
          <button ref={confirmFirst} type="button" className="button" disabled={busy} onClick={() => onAction("cancel")}>
            {t.plugins.cancel}
          </button>
          <button type="button" className="button primary" disabled={busy} onClick={() => onAction("confirm")}>
            {state.kind === "confirmUninstall" ? t.plugins.uninstall : t.plugins.update}
          </button>
        </div>
      )}
      <FailedLine error={error} />
      {entry.source !== null && (
        <p className="plugin-source">
          <span>{t.plugins.source}</span> <span className="plugin-source-value">{entry.source}</span>
        </p>
      )}
      <p className="plugin-description">{entry.description}</p>
      <div>
        <h4 className="plugin-can">{t.plugins.canDo}</h4>
        {entry.permissions !== null ? (
          <PermissionRows permissions={entry.permissions} />
        ) : (
          <ul className="plugin-permissions">
            {(lines ?? []).map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        )}
      </div>
      {!builtin && <p className="plugin-note">{t.plugins.networkNote}</p>}
    </>
  );
}

interface PluginDetailProps {
  entry: Entry;
  icon?: LucideIcon;
  lines: string[] | null;
  /** Install or uninstall a built-in plugin; false when the server did not take it. */
  onSetInstalled: (id: string, on: boolean) => Promise<boolean>;
  onExternalChanged: (id: string, installed: boolean) => Promise<void>;
  /** Install from the plugin list, or reinstall from the same address: the install screen. */
  onInstallFlow: (entry: Entry) => void;
}

/** The details with their requests: built-in on/off, update, reload, and uninstall. Keyed by plugin id. */
export function PluginDetail({ entry, icon, lines, onSetInstalled, onExternalChanged, onInstallFlow }: PluginDetailProps) {
  const [state, setState] = useState<DetailState>({ kind: "idle" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const builtin = entry.badge === "built-in";

  const run = async (work: () => Promise<string | null>) => {
    setBusy(true);
    setError(null);
    const failed = await work();
    setBusy(false);
    setError(failed);
  };

  const setBuiltin = (on: boolean) => run(async () => ((await onSetInstalled(entry.id, on)) ? null : ""));

  const runUpdate = (body?: { token: string; granted: ExternalPermissions }) =>
    run(async () => {
      const answer = await update(entry.id, body);
      if ("error" in answer) return answer.error;
      if ("added" in answer) {
        setState({ kind: "grow", confirm: answer });
        return null;
      }
      await onExternalChanged(entry.id, true);
      setState({ kind: "idle" });
      return null;
    });

  const remove = () =>
    run(async () => {
      if (!(await uninstall(entry.id))) return "";
      setState({ kind: "idle" });
      await onExternalChanged(entry.id, false);
      return null;
    });

  const onAction = (action: DetailAction) => {
    switch (action) {
      case "cancel":
        setState({ kind: "idle" });
        setError(null);
        return;
      case "install":
        if (builtin) void setBuiltin(true);
        else onInstallFlow(entry);
        return;
      case "uninstall":
        if (builtin) void setBuiltin(false);
        else setState({ kind: "confirmUninstall" });
        return;
      case "update":
        setState({ kind: "confirmUpdate" });
        return;
      case "reload":
        void runUpdate();
        return;
      case "reinstall":
        onInstallFlow(entry);
        return;
      case "confirm":
        if (state.kind === "confirmUninstall") void remove();
        else if (state.kind === "confirmUpdate") void runUpdate();
        else if (state.kind === "grow") void runUpdate({ token: state.confirm.token, granted: state.confirm.permissions });
    }
  };

  return <PluginDetailView entry={entry} icon={icon} lines={lines} state={state} busy={busy} error={error} onAction={onAction} />;
}
