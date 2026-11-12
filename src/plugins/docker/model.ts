// The Docker plugin's data and the rules that read it. Pure functions only.

export type EngineStatus = "ok" | "no-cli" | "down";
export type ContainerStateName = "running" | "exited" | "paused" | "restarting" | "created" | "dead";
export type ContainerHealth = "healthy" | "unhealthy" | "starting";

export interface DockerEngine {
  status: EngineStatus;
  version: string | null;
  error: string | null;
}

export interface DockerContainer {
  id: string;
  name: string;
  /** Compose service name. */
  service: string | null;
  /** Compose project name. */
  project: string | null;
  workingDir: string | null;
  image: string;
  state: ContainerStateName;
  health: ContainerHealth | null;
  exitCode: number | null;
  finishedAt: number | null;
  /** Host TCP ports. */
  ports: number[];
}

export interface DockerState {
  engine: DockerEngine;
  containers: DockerContainer[];
}

const ENGINE_STATUSES: readonly string[] = ["ok", "no-cli", "down"];
const STATES: readonly string[] = ["running", "exited", "paused", "restarting", "created", "dead"];
const HEALTHS: readonly string[] = ["healthy", "unhealthy", "starting"];
const CONTAINER_ID = /^[0-9a-f]{12,64}$/;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isStringOrNull(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isNumberOrNull(value: unknown): value is number | null {
  return value === null || (typeof value === "number" && Number.isFinite(value));
}

function parseEngine(value: unknown): DockerEngine | null {
  if (!isRecord(value)) return null;
  const { status, version, error } = value;
  if (typeof status !== "string" || !ENGINE_STATUSES.includes(status)) return null;
  if (!isStringOrNull(version) || !isStringOrNull(error)) return null;
  return { status: status as EngineStatus, version, error };
}

function parseContainer(value: unknown): DockerContainer | null {
  if (!isRecord(value)) return null;
  const { id, name, service, project, workingDir, image, state, health, exitCode, finishedAt, ports } = value;
  if (typeof id !== "string" || !CONTAINER_ID.test(id)) return null;
  if (typeof name !== "string" || typeof image !== "string") return null;
  if (!isStringOrNull(service) || !isStringOrNull(project) || !isStringOrNull(workingDir)) return null;
  if (typeof state !== "string" || !STATES.includes(state)) return null;
  if (health !== null && (typeof health !== "string" || !HEALTHS.includes(health))) return null;
  if (!isNumberOrNull(exitCode) || !isNumberOrNull(finishedAt)) return null;
  if (!Array.isArray(ports)) return null;
  return {
    id,
    name,
    service,
    project,
    workingDir,
    image,
    state: state as ContainerStateName,
    health: health as ContainerHealth | null,
    exitCode,
    finishedAt,
    ports: ports.filter((p): p is number => Number.isInteger(p) && p >= 1 && p <= 65535),
  };
}

/** Reads the server's state JSON. A bad engine gives null; a bad container is dropped. */
export function parseDockerState(json: unknown): DockerState | null {
  if (!isRecord(json) || !Array.isArray(json.containers)) return null;
  const engine = parseEngine(json.engine);
  if (!engine) return null;
  const containers: DockerContainer[] = [];
  for (const raw of json.containers) {
    const container = parseContainer(raw);
    if (container) containers.push(container);
  }
  return { engine, containers };
}

export function displayName(c: DockerContainer): string {
  return c.service ?? c.name;
}

export type Dot = "running" | "warn" | "failed" | "stopped";

export function containerDot(c: DockerContainer): Dot {
  switch (c.state) {
    case "running":
      return c.health === "unhealthy" ? "warn" : "running";
    case "paused":
    case "restarting":
      return "warn";
    case "exited":
    case "dead":
      return c.exitCode !== null && c.exitCode !== 0 ? "failed" : "stopped";
    default:
      return "stopped";
  }
}

export interface ContainerGroup {
  /** The compose project name; "" for containers outside compose. */
  key: string;
  project: string | null;
  workingDir: string | null;
  containers: DockerContainer[];
  running: number;
  failed: boolean;
}

function matchesFilter(c: DockerContainer, needle: string): boolean {
  return [c.name, c.service, c.image, c.project].some((text) => text?.toLowerCase().includes(needle));
}

/** Groups by compose project (others last), sorted by name. `filter` is a case-insensitive substring. */
export function groupContainers(list: DockerContainer[], filter: string): ContainerGroup[] {
  const needle = filter.trim().toLowerCase();
  const byKey = new Map<string, ContainerGroup>();
  for (const c of list) {
    if (needle && !matchesFilter(c, needle)) continue;
    const key = c.project ?? "";
    let group = byKey.get(key);
    if (!group) {
      group = { key, project: c.project, workingDir: null, containers: [], running: 0, failed: false };
      byKey.set(key, group);
    }
    group.workingDir ??= c.workingDir;
    group.containers.push(c);
    if (c.state === "running") group.running += 1;
    if (containerDot(c) === "failed") group.failed = true;
  }
  const groups = [...byKey.values()];
  // Scaled services share a display name; name, then id, keep the order fixed.
  const order = (a: DockerContainer, b: DockerContainer) =>
    displayName(a).localeCompare(displayName(b)) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id);
  for (const g of groups) g.containers.sort(order);
  return groups.sort((a, b) => {
    if (a.key === "" || b.key === "") return a.key === b.key ? 0 : a.key === "" ? 1 : -1;
    return a.key.localeCompare(b.key);
  });
}

export function runningCount(list: DockerContainer[]): number {
  return list.filter((c) => c.state === "running").length;
}

/** True when a container that ran before has failed or turned unhealthy now. Old problems do not count. */
export function newFailures(prev: DockerContainer[] | null, next: DockerContainer[]): boolean {
  if (prev === null) return false;
  const before = new Map(prev.map((c) => [c.id, c]));
  return next.some((c) => {
    const old = before.get(c.id);
    if (old?.state !== "running") return false;
    if (containerDot(c) === "failed") return true;
    return c.state === "running" && c.health === "unhealthy" && old.health !== "unhealthy";
  });
}

/** The install command for a platform. Linux has none; see https://docs.docker.com/engine/install/. */
export function dockerInstallCommand(platform: "mac" | "windows" | "linux"): string | null {
  switch (platform) {
    case "mac":
      return "brew install --cask docker";
    case "windows":
      return "winget install Docker.DockerDesktop";
    case "linux":
      return null;
  }
}
