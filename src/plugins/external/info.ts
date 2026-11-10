// What the server says about the installed external plugins (`GET
// /api/plugins`, `external`), read defensively: an entry of the wrong shape
// is dropped, not trusted.

import { LOCALES, type Locale, type Messages } from "../../i18n/messages.ts";

/** What the user granted at install; the server checks every call against it. */
export interface ExternalPermissions {
  /** Program names the plugin may run, with any arguments. */
  run: string[];
  /** Open terminal and browser panes. */
  panes: boolean;
  /** Keep running while the panel is closed. */
  background: boolean;
}

export interface ExternalSource {
  kind: "index" | "url" | "folder";
  url?: string;
  path?: string;
}

export interface LocalText {
  name?: string;
  description?: string;
}

/** One installed external plugin, as the server sends it. */
export interface ExternalInfo {
  id: string;
  version: string;
  name: string;
  description: string;
  i18n: Partial<Record<Locale, LocalText>>;
  /** An SVG in the package; null shows the puzzle icon. */
  icon: string | null;
  /** The panel's HTML page in the package. */
  panel: string;
  source: ExternalSource;
  granted: ExternalPermissions;
  /** When it was installed, updated, or reloaded; a change restarts its instance. */
  installedAt: number;
}

const EXTERNAL_ID = /^[a-z0-9-]{1,32}\.[a-z0-9-]{1,32}$/;
const VERSION = /^\d+\.\d+\.\d+$/;
const SOURCE_KINDS: readonly string[] = ["index", "url", "folder"];

type Loose = Record<string, unknown>;

const isObject = (value: unknown): value is Loose => typeof value === "object" && value !== null && !Array.isArray(value);
const isStrings = (value: unknown): value is string[] => Array.isArray(value) && value.every((each) => typeof each === "string");

/** A file path inside the package: plain segments only, so its URL stays under /plugin-assets/<id>/. */
function isPackagePath(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.split("/").every((segment) => segment !== "" && segment !== "." && segment !== ".." && !/[\\?#%]/.test(segment))
  );
}

export function readPermissions(value: unknown): ExternalPermissions | null {
  if (!isObject(value) || !isStrings(value.run) || typeof value.panes !== "boolean" || typeof value.background !== "boolean") {
    return null;
  }
  return { run: [...value.run], panes: value.panes, background: value.background };
}

export function readSource(value: unknown): ExternalSource | null {
  if (!isObject(value) || typeof value.kind !== "string" || !SOURCE_KINDS.includes(value.kind)) return null;
  const source: ExternalSource = { kind: value.kind as ExternalSource["kind"] };
  if (typeof value.url === "string") source.url = value.url;
  if (typeof value.path === "string") source.path = value.path;
  return source;
}

/** The translations the page knows; the server writes a missing one as null. */
export function readI18n(value: unknown): ExternalInfo["i18n"] {
  const i18n: ExternalInfo["i18n"] = {};
  if (!isObject(value)) return i18n;
  for (const locale of LOCALES) {
    const entry = value[locale];
    if (!isObject(entry)) continue;
    const text: LocalText = {};
    if (typeof entry.name === "string") text.name = entry.name;
    if (typeof entry.description === "string") text.description = entry.description;
    i18n[locale] = text;
  }
  return i18n;
}

export function readInfo(value: unknown): ExternalInfo | null {
  if (!isObject(value)) return null;
  const { id, version, name, description, icon, panel, installedAt } = value;
  const granted = readPermissions(value.granted);
  const source = readSource(value.source);
  const valid =
    typeof id === "string" &&
    EXTERNAL_ID.test(id) &&
    typeof version === "string" &&
    VERSION.test(version) &&
    typeof name === "string" &&
    typeof description === "string" &&
    (icon === null || isPackagePath(icon)) &&
    isPackagePath(panel) &&
    typeof installedAt === "number" &&
    Number.isFinite(installedAt) &&
    granted !== null &&
    source !== null;
  if (!valid) return null;
  return { id, version, name, description, i18n: readI18n(value.i18n), icon, panel, source, granted, installedAt };
}

/** `GET /api/plugins`: every installed id, and the external plugins of the right shape. */
export function parsePluginList(json: unknown): { installed: string[]; external: ExternalInfo[]; shellPanes: boolean } {
  if (!isObject(json)) return { installed: [], external: [], shellPanes: false };
  const installed = Array.isArray(json.installed) ? json.installed.filter((id): id is string => typeof id === "string") : [];
  const external = Array.isArray(json.external)
    ? json.external.map(readInfo).filter((info): info is ExternalInfo => info !== null)
    : [];
  // Whether the server's login shell can take plugin panes (`openShell`).
  return { installed, external, shellPanes: json.shellPanes === true };
}

/** The plugin's name and description in `locale`, or its own default text. */
export function localText(info: ExternalInfo, locale: Locale): { name: string; description: string } {
  const text = info.i18n[locale];
  return { name: text?.name ?? info.name, description: text?.description ?? info.description };
}

/** What the granted permissions let the plugin do, one line each. */
export function permissionLines(p: ExternalPermissions, t: Messages): string[] {
  const lines = [
    ...p.run.map((program) => t.plugins.permRun(program)),
    ...(p.panes ? [t.plugins.permPanes] : []),
    ...(p.background ? [t.plugins.permBackground] : []),
  ];
  return lines.length > 0 ? lines : [t.plugins.permNone];
}

/** Where the page loads a file of the package from. */
export function assetUrl(info: Pick<ExternalInfo, "id">, path: string): string {
  return `/plugin-assets/${info.id}/${path}`;
}
