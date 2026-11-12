// The plugin manager's requests to the server's external plugin routes:
// the catalog, fetching a package to preview, install, update, and remove.
// Answers are read defensively; a failure carries the server's one-line
// reason, or "" when there is none (the view then says it failed).

import type { Locale } from "../../i18n/messages.ts";
import {
  readI18n,
  readInfo,
  readPermissions,
  readSource,
  type ExternalInfo,
  type ExternalPermissions,
  type ExternalSource,
  type LocalText,
} from "./info.ts";

export type UpdateMode = "check" | "reinstall" | "reload";

/** One plugin of the verified plugin list. */
export interface IndexEntry {
  id: string;
  name: string;
  description: string;
  /** The repository, for people to read; may be empty. */
  repo: string;
  version: string;
  url: string;
  sha256: string;
  permissions: ExternalPermissions;
}

/** An installed plugin and what its next update looks like. */
export type CatalogPlugin = ExternalInfo & { update: string | null; updateMode: UpdateMode };

/** `GET /api/plugins/catalog`. */
export interface Catalog {
  /** null when the server could not load the plugin list. */
  index: IndexEntry[] | null;
  installed: CatalogPlugin[];
}

/** A package's `plugin.json`, as the server checked it. */
export interface Manifest {
  id: string;
  name: string;
  version: string;
  description: string;
  icon: string | null;
  panel: string;
  westerm: string | null;
  permissions: ExternalPermissions;
  i18n: Partial<Record<Locale, LocalText>>;
}

/** `POST /api/plugins/fetch`: the checked package, kept on the server under `token`. */
export interface Preview {
  token: string;
  manifest: Manifest;
  /** Bytes of the package; null for a folder. */
  size: number | null;
  source: ExternalSource;
}

/** An update that asks for more than is granted: confirm with its token. */
export interface UpdateConfirm {
  token: string;
  version: string;
  /** Everything the new version asks for; the confirm call grants all of it. */
  permissions: ExternalPermissions;
  /** What it asks for beyond what is granted now. */
  added: ExternalPermissions;
}

/**
 * A fetched package (`replace`: a reinstall over the installed plugin of the
 * same id, which keeps its storage like an update), or a plugin-list entry.
 */
export type InstallBody =
  | { token: string; granted: ExternalPermissions; trusted: true; replace?: true }
  | { index: string; granted: ExternalPermissions };

export interface Failed {
  error: string;
}

type Loose = Record<string, unknown>;

const isObject = (value: unknown): value is Loose => typeof value === "object" && value !== null && !Array.isArray(value);
const UPDATE_MODES: readonly string[] = ["check", "reinstall", "reload"];

function readIndexEntry(value: unknown): IndexEntry | null {
  if (!isObject(value)) return null;
  const { id, name, description, repo, version, url, sha256 } = value;
  const permissions = readPermissions(value.permissions);
  const strings = [id, name, description, repo, version, url, sha256].every((each) => typeof each === "string");
  if (!strings || permissions === null) return null;
  return { id, name, description, repo, version, url, sha256, permissions } as IndexEntry;
}

function readCatalogPlugin(value: unknown): CatalogPlugin | null {
  const info = readInfo(value);
  if (info === null || !isObject(value)) return null;
  const update = typeof value.update === "string" ? value.update : null;
  const updateMode = typeof value.updateMode === "string" && UPDATE_MODES.includes(value.updateMode) ? (value.updateMode as UpdateMode) : null;
  return updateMode === null ? null : { ...info, update, updateMode };
}

function readManifest(value: unknown): Manifest | null {
  if (!isObject(value)) return null;
  const { id, name, version, description, icon, panel, westerm } = value;
  const permissions = readPermissions(value.permissions);
  const valid =
    [id, name, version, description, panel].every((each) => typeof each === "string") &&
    (icon === null || typeof icon === "string") &&
    (westerm === null || typeof westerm === "string") &&
    permissions !== null;
  if (!valid) return null;
  return { id, name, version, description, icon, panel, westerm, permissions, i18n: readI18n(value.i18n) } as Manifest;
}

function readPreview(value: unknown): Preview | null {
  if (!isObject(value) || typeof value.token !== "string") return null;
  const manifest = readManifest(value.manifest);
  const source = readSource(value.source);
  const size = typeof value.size === "number" && Number.isFinite(value.size) ? value.size : null;
  if (manifest === null || source === null) return null;
  return { token: value.token, manifest, size, source };
}

function readUpdateConfirm(value: unknown): UpdateConfirm | null {
  if (!isObject(value) || typeof value.token !== "string" || typeof value.version !== "string") return null;
  const permissions = readPermissions(value.permissions);
  const added = readPermissions(value.added);
  if (permissions === null || added === null) return null;
  return { token: value.token, version: value.version, permissions, added };
}

/** The JSON of an answer, or undefined when it has none. */
async function bodyOf(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    return undefined;
  }
}

const reasonOf = (json: unknown): Failed => ({ error: isObject(json) && typeof json.error === "string" ? json.error : "" });

/** POSTs `body` as JSON; null when the request did not reach the server. */
async function post(path: string, body: unknown): Promise<Response | null> {
  try {
    return await fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  } catch {
    return null;
  }
}

/** POSTs and reads a 200 with `read`; anything else is a failure with the server's reason. */
async function call<T>(path: string, body: unknown, read: (json: unknown) => T | null): Promise<T | Failed> {
  const response = await post(path, body);
  if (response === null) return { error: "" };
  const json = await bodyOf(response);
  if (!response.ok) return reasonOf(json);
  return read(json) ?? { error: "" };
}

/** The plugin list and the installed plugins with their updates; null when the request failed. */
export async function loadCatalog(): Promise<Catalog | null> {
  try {
    const response = await fetch("/api/plugins/catalog");
    if (!response.ok) return null;
    const json = await bodyOf(response);
    if (!isObject(json)) return null;
    const index = Array.isArray(json.index)
      ? json.index.map(readIndexEntry).filter((entry): entry is IndexEntry => entry !== null)
      : null;
    const installed = Array.isArray(json.installed)
      ? json.installed.map(readCatalogPlugin).filter((plugin): plugin is CatalogPlugin => plugin !== null)
      : [];
    return { index, installed };
  } catch {
    return null;
  }
}

/** Downloads (or reads) a package and checks it, for the install screen. */
export function fetchPreview(input: { url: string } | { path: string }): Promise<Preview | Failed> {
  return call("/api/plugins/fetch", input, readPreview);
}

export function install(body: InstallBody): Promise<ExternalInfo | Failed> {
  return call("/api/plugins/install", body, readInfo);
}

/**
 * Updates to the newest version, or reloads a folder. A version that asks
 * for more answers 409 with an `UpdateConfirm`; send its token and
 * permissions back to install it.
 */
export async function update(id: string, body?: { token: string; granted: ExternalPermissions }): Promise<ExternalInfo | UpdateConfirm | Failed> {
  const response = await post(`/api/plugins/${id}/update`, body ?? {});
  if (response === null) return { error: "" };
  const json = await bodyOf(response);
  if (response.status === 409) return readUpdateConfirm(json) ?? reasonOf(json);
  if (!response.ok) return reasonOf(json);
  return readInfo(json) ?? { error: "" };
}

/** Removes the package and its storage; false when the server did not. */
export async function uninstall(id: string): Promise<boolean> {
  try {
    const response = await fetch(`/api/plugins/${id}`, { method: "DELETE" });
    return response.ok;
  } catch {
    return false;
  }
}
