import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n/index.tsx";
import type { Locale } from "../i18n/messages.ts";
import * as serverApi from "./external/api.ts";
import type { IndexEntry, InstallBody, Preview } from "./external/api.ts";
import type { ExternalPermissions } from "./external/info.ts";
import { badgeOf, type Badge } from "./managerModel.ts";
import { BadgeChip, EntryIcon, FailedLine, PermissionRows } from "./PluginDetail.tsx";

/** What the confirm step shows about the package. */
export interface ConfirmPreview {
  name: string;
  version: string;
  /** Bytes; null for a folder or a plugin-list entry. */
  size: number | null;
  badge: Badge;
  permissions: ExternalPermissions;
  /** The address or folder the user is asked to trust. */
  source: string;
  /** Installing replaces the installed plugin of the same id (a reinstall). */
  replaces: boolean;
}

export type InstallState =
  /** A: an address or a folder to fetch. */
  | { step: "address"; kind: "url" | "folder"; value: string }
  /** B: what the plugin may do, and the trust checkbox for unverified and local plugins. */
  | { step: "confirm"; preview: ConfirmPreview; trusted: boolean };

export type InstallAction =
  | { type: "kind"; kind: "url" | "folder" }
  | { type: "value"; value: string }
  | { type: "next" }
  | { type: "back" }
  | { type: "trust"; on: boolean }
  | { type: "install" }
  | { type: "cancel" };

interface InstallFlowViewProps {
  state: InstallState;
  busy: boolean;
  error: string | null;
  onAction: (action: InstallAction) => void;
}

/** Whether installing waits for "I trust …": a plugin that is not on the verified list. */
const needsTrust = (badge: Badge) => badge === "unverified" || badge === "local";

/**
 * The install screens: A (address) and B (permissions, warning, trust). A
 * new step takes the focus (A its input, B its heading), so it stays inside
 * the dialog when the pressed button goes away.
 */
export function InstallFlowView({ state, busy, error, onAction }: InstallFlowViewProps) {
  const { t } = useI18n();
  const addressInput = useRef<HTMLInputElement>(null);
  const confirmHeading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    (state.step === "address" ? addressInput.current : confirmHeading.current)?.focus();
  }, [state.step]);

  if (state.step === "address") {
    const kinds = [
      ["url", t.plugins.url],
      ["folder", t.plugins.folder],
    ] as const;
    return (
      <form
        className="plugin-install"
        onSubmit={(event) => {
          event.preventDefault();
          onAction({ type: "next" });
        }}
      >
        <h3>{t.plugins.installFrom}</h3>
        <div className="plugin-segmented" role="group" aria-label={t.plugins.installFrom}>
          {kinds.map(([kind, label]) => (
            <button key={kind} type="button" aria-pressed={state.kind === kind} onClick={() => onAction({ type: "kind", kind })}>
              {label}
            </button>
          ))}
        </div>
        <input
          ref={addressInput}
          className="plugin-address"
          type="text"
          spellCheck={false}
          autoComplete="off"
          aria-label={state.kind === "url" ? t.plugins.url : t.plugins.folder}
          placeholder={state.kind === "url" ? t.plugins.urlPlaceholder : t.plugins.folderPlaceholder}
          value={state.value}
          onChange={(event) => onAction({ type: "value", value: event.target.value })}
        />
        <FailedLine error={error} />
        <div className="plugin-actions">
          <button type="button" className="button" onClick={() => onAction({ type: "cancel" })}>
            {t.plugins.cancel}
          </button>
          <button type="submit" className="button primary" disabled={busy || state.value.trim() === ""}>
            {t.plugins.next}
          </button>
        </div>
      </form>
    );
  }

  const { preview, trusted } = state;
  const trust = needsTrust(preview.badge);
  const facts = [`v${preview.version}`, ...(preview.size === null ? [] : [t.plugins.size(Math.max(1, Math.round(preview.size / 1024)))])];
  return (
    <div className="plugin-install">
      <div className="plugin-detail-head">
        <EntryIcon iconUrl={null} large />
        <div className="plugin-detail-name">
          <h3 ref={confirmHeading} tabIndex={-1}>
            {t.plugins.installTitle(preview.name)} <BadgeChip badge={preview.badge} />
          </h3>
          <span className="settings-version">{facts.join(" · ")}</span>
        </div>
      </div>
      {trust && (
        <p className="plugin-warning" role="note">
          {t.plugins.warning}
        </p>
      )}
      <div>
        <h4 className="plugin-can">{t.plugins.canDo}</h4>
        <PermissionRows permissions={preview.permissions} />
      </div>
      <p className="plugin-note">{t.plugins.networkNote}</p>
      {preview.replaces && <p className="plugin-note">{t.plugins.reinstallNote}</p>}
      {trust && (
        <label className="plugin-trust">
          <input type="checkbox" checked={trusted} onChange={(event) => onAction({ type: "trust", on: event.target.checked })} />
          <span>{t.plugins.trust(preview.source)}</span>
        </label>
      )}
      <FailedLine error={error} />
      <div className="plugin-actions">
        <button type="button" className="button" disabled={busy} onClick={() => onAction({ type: "back" })}>
          {t.plugins.back}
        </button>
        <button type="button" className="button primary" disabled={busy || (trust && !trusted)} onClick={() => onAction({ type: "install" })}>
          {preview.replaces ? t.plugins.reinstall : t.plugins.install}
        </button>
      </div>
    </div>
  );
}

/** Where the install screens start. */
export type InstallStart =
  /** A, empty, or with an address to reinstall the installed plugin `replaces` from. */
  | { kind: "address"; url?: string; replaces?: string }
  /** B for a plugin of the verified list: nothing to fetch, no warning. */
  | { kind: "index"; entry: IndexEntry };

/** The requests the install screens make; tests pass fakes. */
export interface InstallApi {
  fetchPreview: typeof serverApi.fetchPreview;
  install: typeof serverApi.install;
}

interface InstallFlowProps {
  start: InstallStart;
  api?: InstallApi;
  /** Installed (or reinstalled) `id`. */
  onInstalled: (id: string) => Promise<void>;
  /** Leave the install screens. */
  onCancel: () => void;
}

/** What B needs from the fetched package, and how to install it. */
type Pending = { from: "fetch"; preview: Preview } | { from: "index"; entry: IndexEntry };

function confirmState(pending: Pending, locale: Locale, replaces: string | undefined): InstallState {
  if (pending.from === "index") {
    const { entry } = pending;
    return {
      step: "confirm",
      preview: { name: entry.name, version: entry.version, size: null, badge: "verified", permissions: entry.permissions, source: entry.repo || entry.url, replaces: false },
      trusted: false,
    };
  }
  const { manifest, size, source } = pending.preview;
  return {
    step: "confirm",
    preview: {
      name: manifest.i18n[locale]?.name ?? manifest.name,
      version: manifest.version,
      size,
      badge: badgeOf(source),
      permissions: manifest.permissions,
      source: source.url ?? source.path ?? "",
      replaces: replaces === manifest.id,
    },
    trusted: false,
  };
}

/**
 * The install screens with their requests: fetch a package to preview,
 * then install it (a reinstall asks the server to replace the installed
 * plugin in one step, keeping its storage), or install a plugin of the
 * verified list. The server's reason for a failure shows as is.
 */
export function InstallFlow({ start, api = serverApi, onInstalled, onCancel }: InstallFlowProps) {
  const { locale } = useI18n();
  // The installed plugin a reinstall replaces.
  const replaces = start.kind === "address" ? start.replaces : undefined;
  const firstPending: Pending | null = start.kind === "index" ? { from: "index", entry: start.entry } : null;
  const [pending, setPending] = useState<Pending | null>(firstPending);
  const [state, setState] = useState<InstallState>(() =>
    firstPending !== null ? confirmState(firstPending, locale, undefined) : { step: "address", kind: "url", value: start.kind === "address" ? (start.url ?? "") : "" },
  );
  // The address step as it was, for Back.
  const [address, setAddress] = useState<InstallState>(state);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const next = async () => {
    if (state.step !== "address") return;
    setBusy(true);
    setError(null);
    const value = state.value.trim();
    const answer = await api.fetchPreview(state.kind === "url" ? { url: value } : { path: value });
    setBusy(false);
    if ("error" in answer) {
      setError(answer.error);
      return;
    }
    const fetched: Pending = { from: "fetch", preview: answer };
    setAddress(state);
    setPending(fetched);
    setState(confirmState(fetched, locale, replaces));
  };

  const confirm = async () => {
    if (state.step !== "confirm" || pending === null) return;
    setBusy(true);
    setError(null);
    let body: InstallBody;
    if (pending.from === "index") {
      body = { index: pending.entry.id, granted: pending.entry.permissions };
    } else {
      body = { token: pending.preview.token, granted: pending.preview.manifest.permissions, trusted: true };
      // The server swaps the package in one step; a failure leaves the installed one.
      if (state.preview.replaces) body.replace = true;
    }
    const answer = await api.install(body);
    if ("error" in answer) {
      setBusy(false);
      setError(answer.error);
      return;
    }
    await onInstalled(answer.id);
    setBusy(false);
  };

  const onAction = (action: InstallAction) => {
    switch (action.type) {
      case "kind":
        if (state.step === "address") setState({ ...state, kind: action.kind });
        setError(null);
        return;
      case "value":
        if (state.step === "address") setState({ ...state, value: action.value });
        return;
      case "next":
        void next();
        return;
      case "back":
        setError(null);
        if (pending?.from === "index") onCancel();
        else setState(address);
        return;
      case "trust":
        if (state.step === "confirm") setState({ ...state, trusted: action.on });
        return;
      case "install":
        void confirm();
        return;
      case "cancel":
        onCancel();
    }
  };

  return <InstallFlowView state={state} busy={busy} error={error} onAction={onAction} />;
}
