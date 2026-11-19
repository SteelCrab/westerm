// What the plugin manager lists, as pure functions: one entry per plugin
// from the built-ins, the verified plugin list, and the installed external
// plugins, with its badge, update, and permissions.

import type { Locale, Messages } from "../i18n/messages.ts";
import type { Catalog, UpdateMode } from "./external/api.ts";
import { assetUrl, localText, permissionLines, type ExternalInfo, type ExternalPermissions, type ExternalSource } from "./external/info.ts";
import type { WestermPlugin } from "./types.ts";

export type Badge = "built-in" | "verified" | "unverified" | "local";
export type Risk = "high" | "medium" | "low";
export type Tab = "installed" | "browse";

/** One row of the manager's list and what its details show. */
export interface Entry {
  id: string;
  name: string;
  description: string;
  /** The part of the id before the dot; null for a built-in plugin. */
  author: string | null;
  badge: Badge;
  installed: boolean;
  /** A newer version the server found, if any. */
  update: string | null;
  /** How an installed external plugin gets a new version; null otherwise. */
  updateMode: UpdateMode | null;
  /** What an external plugin may do (granted, or asked for by the list); null for a built-in. */
  permissions: ExternalPermissions | null;
  /** The repository address, the package address, or the folder; null for a built-in. */
  source: string | null;
  version: string;
  /** The plugin's SVG; null draws its own (built-in) or the puzzle icon. */
  iconUrl: string | null;
}

/** Who made an external plugin: the part of its id before the dot. */
export function authorOf(id: string): string | null {
  const dot = id.indexOf(".");
  return dot < 0 ? null : id.slice(0, dot);
}

export function badgeOf(source: ExternalSource): Badge {
  const badges: Record<ExternalSource["kind"], Badge> = { index: "verified", url: "unverified", folder: "local" };
  return badges[source.kind];
}

export function riskOf(kind: "run" | "panes" | "background"): Risk {
  const risks = { run: "high", panes: "medium", background: "low" } as const;
  return risks[kind];
}

/** The permission lines (`permissionLines`) with their risk; none for no permissions. */
export function permissionRows(p: ExternalPermissions, t: Messages): { text: string; risk: Risk }[] {
  const kinds = [
    ...p.run.map(() => "run" as const),
    ...(p.panes ? (["panes"] as const) : []),
    ...(p.background ? (["background"] as const) : []),
  ];
  const lines = permissionLines(p, t);
  return kinds.map((kind, index) => ({ text: lines[index], risk: riskOf(kind) }));
}

/** Whether `query` is in the entry's name, description, or author, ignoring case. */
export function matches(entry: Pick<Entry, "name" | "description" | "author">, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (needle === "") return true;
  return [entry.name, entry.description, entry.author ?? ""].some((text) => text.toLowerCase().includes(needle));
}

function builtinEntry(plugin: WestermPlugin, installed: readonly string[], locale: Locale): Entry {
  const text = plugin.text[locale];
  return {
    id: plugin.id,
    name: text.name,
    description: text.description,
    author: null,
    badge: "built-in",
    installed: installed.includes(plugin.id),
    update: null,
    updateMode: null,
    permissions: null,
    source: null,
    version: plugin.version,
    // A built-in plugin with its own image (a brand logo) shows it here too.
    iconUrl: plugin.iconUrl ?? null,
  };
}

function externalEntry(info: ExternalInfo, catalog: Catalog | null, locale: Locale): Entry {
  const text = localText(info, locale);
  const known = catalog?.installed.find((plugin) => plugin.id === info.id);
  const listed = catalog?.index?.find((entry) => entry.id === info.id);
  // Folder sources reload without the catalog; the others need it to know.
  const updateMode = known?.updateMode ?? (info.source.kind === "folder" ? "reload" : null);
  // An update the installed version already has (it just ran) is no update.
  const update = known?.update && known.update !== info.version ? known.update : null;
  const repo = info.source.kind === "index" && listed?.repo ? listed.repo : null;
  return {
    id: info.id,
    name: text.name,
    description: text.description,
    author: authorOf(info.id),
    badge: badgeOf(info.source),
    installed: true,
    update,
    updateMode,
    permissions: info.granted,
    source: repo ?? info.source.url ?? info.source.path ?? null,
    version: info.version,
    iconUrl: info.icon === null ? null : assetUrl(info, info.icon),
  };
}

/**
 * The entries of a tab. Installed: the installed built-ins, then the
 * installed external plugins. Browse: every built-in, the plugin list (an
 * installed one as installed), then installed plugins not on the list.
 */
export function entries(
  tab: Tab,
  builtins: readonly WestermPlugin[],
  catalog: Catalog | null,
  external: readonly ExternalInfo[],
  installed: readonly string[],
  locale: Locale,
): Entry[] {
  const builtin = builtins.map((plugin) => builtinEntry(plugin, installed, locale));
  const mine = external.map((info) => externalEntry(info, catalog, locale));
  if (tab === "installed") return [...builtin.filter((entry) => entry.installed), ...mine];
  const listed = (catalog?.index ?? []).map(
    (entry): Entry =>
      mine.find((each) => each.id === entry.id) ?? {
        id: entry.id,
        name: entry.name,
        description: entry.description,
        author: authorOf(entry.id),
        badge: "verified",
        installed: false,
        update: null,
        updateMode: null,
        permissions: entry.permissions,
        source: entry.repo || null,
        version: entry.version,
        iconUrl: null,
      },
  );
  const unlisted = mine.filter((each) => !listed.some((entry) => entry.id === each.id));
  return [...builtin, ...listed, ...unlisted];
}
