import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Puzzle, X } from "lucide-react";
import { stepIndex } from "../uiHelpers.ts";
import { useI18n } from "../i18n/index.tsx";
import { loadCatalog, type Catalog } from "./external/api.ts";
import type { ExternalInfo } from "./external/info.ts";
import { InstallFlow, type InstallStart } from "./InstallFlow.tsx";
import { entries, matches, type Entry, type Tab } from "./managerModel.ts";
import { BadgeChip, EntryIcon, PluginDetail } from "./PluginDetail.tsx";
import type { WestermPlugin } from "./types.ts";

interface PluginManagerProps {
  /** Every built-in plugin, installed or not. */
  builtins: readonly WestermPlugin[];
  /** The installed external plugins, as the server describes them. */
  external: readonly ExternalInfo[];
  installed: readonly string[];
  /** The plugin to show first; null for the first in the list. */
  selected: string | null;
  /** Installs or uninstalls a built-in plugin; resolves false when the server did not take it. */
  onSetInstalled: (id: string, on: boolean) => Promise<boolean>;
  /** An external plugin was installed, updated, reloaded, or removed; resolves when the list is read again. */
  onExternalChanged: (id: string, installed: boolean) => Promise<void>;
  onClose: () => void;
}

/** What Tab moves between inside the dialog. */
const FOCUSABLE = 'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * Where focus goes when the focused control is removed or disabled (a
 * confirm row that closes, a busy button, Retry): back into the part of the
 * dialog it was in. Focus left on <body> would let the next Tab leave the dialog.
 */
function focusFallback(root: HTMLElement, side: boolean): HTMLElement {
  const usable = (element: HTMLElement | null) => (element !== null && !(element as HTMLButtonElement).disabled ? element : null);
  const fromSide = side
    ? (usable(root.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')) ?? usable(root.querySelector<HTMLElement>(".plugin-search")))
    : null;
  return fromSide ?? root.querySelector<HTMLElement>(".settings-panel") ?? root;
}

/** Keeps focus inside `root` while it is open: see `focusFallback`. */
function keepFocusInside(root: HTMLElement): () => void {
  let side = false;
  const onFocusIn = (event: FocusEvent) => {
    side = root.querySelector(".plugin-side")?.contains(event.target as Node) ?? false;
  };
  const observer = new MutationObserver(() => {
    const active = document.activeElement as HTMLElement | null;
    const lost =
      active === null ||
      active === document.body ||
      !active.isConnected ||
      (root.contains(active) && (active as HTMLButtonElement).disabled === true);
    if (lost) focusFallback(root, side).focus();
  });
  root.addEventListener("focusin", onFocusIn);
  observer.observe(root, { subtree: true, childList: true, attributes: true, attributeFilter: ["disabled"] });
  return () => {
    root.removeEventListener("focusin", onFocusIn);
    observer.disconnect();
  };
}

/** The catalog request: no answer yet, its answer, or failed. */
type CatalogState = { status: "loading" } | { status: "ready"; catalog: Catalog } | { status: "failed" };

interface PluginListViewProps {
  tab: Tab;
  /** The tab's entries that match the search. */
  entries: Entry[];
  pickedId: string | null;
  query: string;
  /** The plugin list could not be loaded (shown on Browse only). */
  failed: boolean;
  builtins: readonly WestermPlugin[];
  onTab: (tab: Tab) => void;
  onQuery: (query: string) => void;
  onPick: (id: string) => void;
  onListKey: (event: React.KeyboardEvent) => void;
  onRetry: () => void;
  onInstallFrom: () => void;
}

/**
 * The left of the manager: Installed | Browse, the search, the plugins (one
 * tab stop, arrow keys between them), the plugin list's failure line, and
 * "Install from URL or folder…".
 */
export function PluginListView(props: PluginListViewProps) {
  const { tab, entries: shown, pickedId, query, failed, builtins } = props;
  const { t } = useI18n();
  const tabs = [
    ["installed", t.plugins.installedTab],
    ["browse", t.plugins.browseTab],
  ] as const;
  return (
    <div className="plugin-side">
      <div className="plugin-segmented" role="group" aria-label={t.plugins.title}>
        {tabs.map(([id, label]) => (
          <button key={id} type="button" aria-pressed={tab === id} onClick={() => props.onTab(id)}>
            {label}
          </button>
        ))}
      </div>
      <input
        className="plugin-search"
        type="search"
        placeholder={t.plugins.search}
        aria-label={t.plugins.search}
        value={query}
        onChange={(event) => props.onQuery(event.target.value)}
      />
      <nav className="plugin-list" role="tablist" aria-orientation="vertical" aria-label={t.plugins.title} onKeyDown={props.onListKey}>
        {shown.map((entry) => (
          <button
            key={entry.id}
            id={`plugin-tab-${entry.id}`}
            type="button"
            role="tab"
            aria-selected={entry.id === pickedId}
            aria-controls="plugin-detail"
            tabIndex={entry.id === pickedId ? 0 : -1}
            onClick={() => props.onPick(entry.id)}
          >
            <EntryIcon iconUrl={entry.iconUrl} icon={builtins.find((plugin) => plugin.id === entry.id)?.icon} />
            <span className="plugin-list-text">
              <span className="plugin-list-name">
                {entry.name} <BadgeChip badge={entry.badge} />
              </span>
              <span className="plugin-list-description">{entry.description}</span>
            </span>
            {entry.update !== null && <span className="plugin-update-tag">{t.plugins.update}</span>}
            {entry.installed && <Check size={14} className="plugin-installed" aria-hidden="true" />}
          </button>
        ))}
      </nav>
      {tab === "browse" && failed && (
        <p className="plugin-list-failed" role="status">
          <span>{t.plugins.listFailed}</span>
          <button type="button" className="button" onClick={props.onRetry}>
            {t.plugins.retry}
          </button>
        </p>
      )}
      <button type="button" className="plugin-install-from" onClick={props.onInstallFrom}>
        {t.plugins.installFrom}
      </button>
    </div>
  );
}

/**
 * The plugin manager: Installed | Browse with search on the left, the picked
 * plugin's details (or the install screens) on the right. Opening it reads
 * the catalog once: the plugin list, and the updates of the installed
 * plugins. A dialog like Settings: Escape or a click outside closes it, and
 * Tab stays inside.
 */
export function PluginManager({ builtins, external, installed, selected, onSetInstalled, onExternalChanged, onClose }: PluginManagerProps) {
  const { t, locale } = useI18n();
  const dialog = useRef<HTMLDivElement>(null);
  const [catalogState, setCatalogState] = useState<CatalogState>({ status: "loading" });
  const catalog = catalogState.status === "ready" ? catalogState.catalog : null;
  const listOf = (tab: Tab) => entries(tab, builtins, catalog, external, installed, locale);
  const [tab, setTab] = useState<Tab>(() => {
    const mine = listOf("installed");
    const wanted = selected === null ? mine.length > 0 : mine.some((entry) => entry.id === selected);
    return wanted ? "installed" : "browse";
  });
  const [query, setQuery] = useState("");
  const [pickedId, setPickedId] = useState<string | null>(selected);
  const [installing, setInstalling] = useState<InstallStart | null>(null);
  const shown = listOf(tab).filter((entry) => matches(entry, query));
  const picked = shown.find((entry) => entry.id === pickedId) ?? shown[0] ?? null;
  const pickedBuiltin = builtins.find((plugin) => plugin.id === picked?.id);
  // Read at Escape time: focus moves to the list only when the dialog opens,
  // not each time the caller renders a new onClose (e.g. after Install).
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  const load = useCallback(async () => {
    setCatalogState({ status: "loading" });
    const answer = await loadCatalog();
    setCatalogState(answer === null ? { status: "failed" } : { status: "ready", catalog: answer });
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => (dialog.current ? keepFocusInside(dialog.current) : undefined), []);

  useEffect(() => {
    const root = dialog.current;
    (root?.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]') ?? root?.querySelector<HTMLElement>(".plugin-search"))?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      closeRef.current();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  const pick = (id: string) => {
    setPickedId(id);
    setInstalling(null);
  };

  // The list is one tab stop; the arrow keys move between its plugins.
  const onListKey = (event: React.KeyboardEvent) => {
    const keys: Record<string, 1 | -1> = { ArrowDown: 1, ArrowUp: -1 };
    const delta = keys[event.key];
    if (!delta || shown.length === 0) return;
    event.preventDefault();
    const index = shown.findIndex((entry) => entry.id === picked?.id);
    const next = shown[stepIndex(index, delta, shown.length)].id;
    pick(next);
    // External ids have a dot, which a #selector would read as a class.
    dialog.current?.querySelector<HTMLElement>(`[id="plugin-tab-${next}"]`)?.focus();
  };

  // Tab from the last control goes to the first, and Shift+Tab back.
  const onDialogKey = (event: React.KeyboardEvent) => {
    event.stopPropagation();
    if (event.key !== "Tab" || !dialog.current) return;
    const stops = [...dialog.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
    if (stops.length === 0) return;
    // The dialog itself holds focus only as a last resort; Tab goes into it either way.
    if (document.activeElement === dialog.current) {
      event.preventDefault();
      (event.shiftKey ? stops[stops.length - 1] : stops[0]).focus();
      return;
    }
    const edge = event.shiftKey ? stops[0] : stops[stops.length - 1];
    if (document.activeElement !== edge) return;
    event.preventDefault();
    (event.shiftKey ? stops[stops.length - 1] : stops[0]).focus();
  };

  // A plugin-list entry installs from the list; an installed one reinstalls from its address.
  const startInstall = (entry: Entry) => {
    if (entry.installed) {
      setInstalling({ kind: "address", url: entry.source ?? "", replaces: entry.id });
      return;
    }
    const listed = catalog?.index?.find((each) => each.id === entry.id);
    if (listed) setInstalling({ kind: "index", entry: listed });
  };

  // Back to the details of what changed, when the manager stays open.
  const changed = async (id: string, on: boolean) => {
    await onExternalChanged(id, on);
    setInstalling(null);
    setPickedId(id);
  };

  return (
    <div className="settings-backdrop" onMouseDown={onClose}>
      <div
        className="settings plugin-manager"
        role="dialog"
        aria-modal="true"
        aria-labelledby="plugin-manager-title"
        ref={dialog}
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
        onKeyDown={onDialogKey}
      >
        <header className="settings-head">
          <h2 id="plugin-manager-title" className="plugin-manager-title">
            <Puzzle size={16} aria-hidden="true" />
            {t.plugins.title}
          </h2>
          <button type="button" className="icon-button" aria-label={t.settings.close} onClick={onClose}>
            <X size={15} />
          </button>
        </header>

        <div className="settings-body">
          <PluginListView
            tab={tab}
            entries={shown}
            pickedId={picked?.id ?? null}
            query={query}
            failed={catalogState.status === "failed" || (catalog !== null && catalog.index === null)}
            builtins={builtins}
            onTab={(next) => {
              setTab(next);
              setInstalling(null);
            }}
            onQuery={setQuery}
            onPick={pick}
            onListKey={onListKey}
            onRetry={() => void load()}
            onInstallFrom={() => setInstalling({ kind: "address" })}
          />

          {installing !== null ? (
            <section className="settings-panel" tabIndex={-1}>
              <InstallFlow
                key={installing.kind === "index" ? `index:${installing.entry.id}` : `address:${installing.replaces ?? ""}`}
                start={installing}
                onInstalled={(id) => changed(id, true)}
                onCancel={() => setInstalling(null)}
              />
            </section>
          ) : (
            picked && (
              <section id="plugin-detail" className="settings-panel" role="tabpanel" tabIndex={-1} aria-labelledby={`plugin-tab-${picked.id}`}>
                <PluginDetail
                  key={picked.id}
                  entry={picked}
                  icon={pickedBuiltin?.icon}
                  lines={pickedBuiltin?.text[locale].permissions ?? null}
                  onSetInstalled={onSetInstalled}
                  onExternalChanged={changed}
                  onInstallFlow={startInstall}
                />
              </section>
            )
          )}
        </div>
      </div>
    </div>
  );
}
