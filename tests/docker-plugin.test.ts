import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { Container } from "lucide-react";
import { I18nProvider, LOCALES } from "../src/i18n/index.tsx";
import { containerAction, fetchDockerState } from "../src/plugins/docker/api.ts";
import { DockerPanelView, type PanelHandlers, type PanelUi } from "../src/plugins/docker/DockerPanel.tsx";
import type { Platform } from "../src/plugins/docker/EmptyStates.tsx";
import { dockerPlugin } from "../src/plugins/docker/index.tsx";
import {
  containerDot,
  displayName,
  dockerInstallCommand,
  groupContainers,
  newFailures,
  parseDockerState,
  runningCount,
  type DockerContainer,
  type DockerState,
} from "../src/plugins/docker/model.ts";
import { createPoller } from "../src/plugins/docker/poller.ts";
import { BUILTIN_PLUGINS } from "../src/plugins/registry.ts";
import type { PluginContext } from "../src/plugins/types.ts";

let counter = 0;
function c(over: Partial<DockerContainer> = {}): DockerContainer {
  counter += 1;
  const id = counter.toString(16).padStart(12, "0");
  return {
    id,
    name: `name-${id}`,
    service: null,
    project: null,
    workingDir: null,
    image: "alpine:3",
    state: "running",
    health: null,
    exitCode: null,
    finishedAt: null,
    ports: [],
    ...over,
  };
}
const running = (id: string, health: DockerContainer["health"] = null) =>
  c({ id: id.padEnd(12, "0"), state: "running", health });
const failed = (id: string) => c({ id: id.padEnd(12, "0"), state: "exited", exitCode: 1 });

test("containers group by compose project, others last, sorted by name", () => {
  const groups = groupContainers(
    [
      c({ name: "z", project: null }),
      c({ service: "web", project: "site" }),
      c({ service: "api", project: "site" }),
      c({ service: "app", project: "blog" }),
    ],
    "",
  );
  assert.deepEqual(groups.map((g) => g.key), ["blog", "site", ""]);
  assert.deepEqual(groups[1].containers.map(displayName), ["api", "web"]);
  assert.equal(groups[2].project, null);
});

test("containers with the same display name sort by name, then id, not by input order", () => {
  // Scaled compose services share the display name "web".
  const [site] = groupContainers(
    [
      c({ id: "f2".padEnd(12, "0"), name: "site-web-2", service: "web", project: "site" }),
      c({ id: "f1".padEnd(12, "0"), name: "site-web-1", service: "web", project: "site" }),
    ],
    "",
  );
  assert.deepEqual(site.containers.map((each) => each.name), ["site-web-1", "site-web-2"]);
  const [same] = groupContainers(
    [c({ id: "e2".padEnd(12, "0"), name: "same" }), c({ id: "e1".padEnd(12, "0"), name: "same" })],
    "",
  );
  assert.deepEqual(same.containers.map((each) => each.id.slice(0, 2)), ["e1", "e2"]);
});

test("a group counts running containers and flags failures", () => {
  const [g] = groupContainers(
    [
      c({ project: "p", state: "running" }),
      c({ project: "p", state: "paused" }),
      c({ project: "p", state: "exited", exitCode: 137 }),
    ],
    "",
  );
  assert.equal(g.running, 1);
  assert.equal(g.failed, true);
  const [ok] = groupContainers([c({ project: "p", state: "exited", exitCode: 0 })], "");
  assert.equal(ok.failed, false);
});

test("the group keeps the working directory of its containers", () => {
  const [g] = groupContainers([c({ project: "p", workingDir: "/home/me/p" })], "");
  assert.equal(g.workingDir, "/home/me/p");
});

test("the filter matches name, service, image, and project", () => {
  const list = [
    c({ name: "db-1", image: "postgres:16" }),
    c({ name: "x", service: "Cache", image: "redis:7" }),
    c({ name: "y", image: "node:22" }),
    c({ name: "w", project: "Shop", image: "nginx" }),
  ];
  const names = (f: string) => groupContainers(list, f).flatMap((g) => g.containers.map(displayName)).sort();
  assert.deepEqual(names("NODE"), ["y"]);
  assert.deepEqual(names("cache"), ["Cache"]);
  assert.deepEqual(names("db-"), ["db-1"]);
  assert.deepEqual(names("shop"), ["w"]);
  assert.deepEqual(names("nothing"), []);
  assert.equal(names("").length, 4);
});

test("dots follow the state rules", () => {
  assert.equal(containerDot(c({ state: "running", health: "healthy" })), "running");
  assert.equal(containerDot(c({ state: "running", health: "starting" })), "running");
  assert.equal(containerDot(c({ state: "running", health: "unhealthy" })), "warn");
  assert.equal(containerDot(c({ state: "paused" })), "warn");
  assert.equal(containerDot(c({ state: "restarting" })), "warn");
  assert.equal(containerDot(c({ state: "exited", exitCode: 1 })), "failed");
  assert.equal(containerDot(c({ state: "dead", exitCode: 137 })), "failed");
  assert.equal(containerDot(c({ state: "exited", exitCode: 0 })), "stopped");
  assert.equal(containerDot(c({ state: "created" })), "stopped");
});

test("runningCount counts running containers only", () => {
  assert.equal(runningCount([c(), c({ state: "paused" }), c({ state: "exited", exitCode: 0 }), c()]), 2);
});

test("the red dot needs a running container that failed", () => {
  assert.equal(newFailures(null, [failed("a")]), false);
  assert.equal(newFailures([running("a")], [failed("a")]), true);
  assert.equal(newFailures([failed("a")], [failed("a")]), false, "an old failure does not count");
  assert.equal(newFailures([running("a")], [running("a", "unhealthy")]), true);
  assert.equal(newFailures([running("a", "unhealthy")], [running("a", "unhealthy")]), false);
  assert.equal(newFailures([running("a")], [running("a")]), false);
  assert.equal(newFailures([running("a")], []), false, "a removed container is not a failure");
  assert.equal(newFailures([running("a")], [c({ id: "b".padEnd(12, "0"), state: "exited", exitCode: 1 })]), false);
});

test("install commands per platform", () => {
  assert.equal(dockerInstallCommand("mac"), "brew install --cask docker");
  assert.equal(dockerInstallCommand("windows"), "winget install Docker.DockerDesktop");
  assert.equal(dockerInstallCommand("linux"), null);
});

const RAW_CONTAINER = {
  id: "0123456789ab",
  name: "web",
  service: "web",
  project: "site",
  workingDir: "/srv/site",
  image: "node:22",
  state: "running",
  health: "healthy",
  exitCode: null,
  finishedAt: null,
  ports: [8080, 443],
};

test("a good state is read as it is", () => {
  const state = parseDockerState({
    engine: { status: "ok", version: "27.1.1", error: null },
    containers: [RAW_CONTAINER],
  });
  assert.deepEqual(state, {
    engine: { status: "ok", version: "27.1.1", error: null },
    containers: [RAW_CONTAINER],
  });
});

test("bad ids and ports are dropped when reading the state", () => {
  const state = parseDockerState({
    engine: { status: "ok", version: null, error: null },
    containers: [
      { ...RAW_CONTAINER, id: "ABC" },
      { ...RAW_CONTAINER, id: "0123456789AB" },
      { ...RAW_CONTAINER, id: "abc" },
      { ...RAW_CONTAINER, id: "a".repeat(65) },
      { ...RAW_CONTAINER, id: "a".repeat(64), ports: [70000, 0, -1, 1.5, "80", 22, 65535, 1] },
    ],
  });
  assert.ok(state);
  assert.deepEqual(state.containers.map((x) => x.id), ["a".repeat(64)]);
  assert.deepEqual(state.containers[0].ports, [22, 65535, 1]);
});

test("a bad container is dropped, not the whole state", () => {
  const engine = { status: "ok", version: null, error: null };
  const state = parseDockerState({
    engine,
    containers: [
      RAW_CONTAINER,
      null,
      "text",
      { ...RAW_CONTAINER, id: "111111111111", name: 5 },
      { ...RAW_CONTAINER, id: "222222222222", state: "flying" },
      { ...RAW_CONTAINER, id: "333333333333", health: "great" },
      { ...RAW_CONTAINER, id: "444444444444", exitCode: "1" },
      { ...RAW_CONTAINER, id: "555555555555", finishedAt: "now" },
      { ...RAW_CONTAINER, id: "666666666666", ports: "80" },
      { ...RAW_CONTAINER, id: "777777777777", image: undefined },
    ],
  });
  assert.ok(state);
  assert.deepEqual(state.containers.map((x) => x.id), ["0123456789ab"]);
});

test("a bad engine makes the whole state unreadable", () => {
  assert.equal(parseDockerState(null), null);
  assert.equal(parseDockerState("x"), null);
  assert.equal(parseDockerState({ containers: [] }), null);
  assert.equal(parseDockerState({ engine: { status: "weird", version: null, error: null }, containers: [] }), null);
  assert.equal(parseDockerState({ engine: { status: "ok", version: 1, error: null }, containers: [] }), null);
  assert.equal(parseDockerState({ engine: { status: "ok", version: null, error: null } }), null);
});

interface FakeCtx {
  ctx: PluginContext;
  calls: { path: string; init?: RequestInit }[];
}
function fakeCtx(respond: (path: string) => Response | Promise<Response>): FakeCtx {
  const calls: FakeCtx["calls"] = [];
  const ctx: PluginContext = {
    openShell() {},
    openBrowser() {},
    fetch: async (path, init) => {
      calls.push({ path, init });
      return respond(path);
    },
    refreshInstalled() {},
    openManager() {},
  };
  return { ctx, calls };
}

test("a 404 reports gone", async () => {
  const { ctx } = fakeCtx(() => new Response("{}", { status: 404 }));
  assert.equal(await fetchDockerState(ctx, false), "gone");
});

test("the state request asks for a retry only when told", async () => {
  const body = JSON.stringify({ engine: { status: "no-cli", version: null, error: "x" }, containers: [] });
  const { ctx, calls } = fakeCtx(() => new Response(body, { status: 200 }));
  const state = await fetchDockerState(ctx, false);
  assert.deepEqual(state, { engine: { status: "no-cli", version: null, error: "x" }, containers: [] });
  await fetchDockerState(ctx, true);
  assert.deepEqual(calls.map((x) => x.path), ["/state", "/state?retry=1"]);
});

test("other failures read as null", async () => {
  assert.equal(await fetchDockerState(fakeCtx(() => new Response("", { status: 500 })).ctx, false), null);
  assert.equal(await fetchDockerState(fakeCtx(() => new Response("not json", { status: 200 })).ctx, false), null);
  assert.equal(await fetchDockerState(fakeCtx(() => new Response("{}", { status: 200 })).ctx, false), null);
  const throwing = fakeCtx(() => {
    throw new TypeError("offline");
  });
  assert.equal(await fetchDockerState(throwing.ctx, false), null);
});

test("an action posts to the container and returns null on success", async () => {
  const { ctx, calls } = fakeCtx(() => new Response(null, { status: 204 }));
  assert.equal(await containerAction(ctx, "0123456789ab", "restart"), null);
  assert.equal(calls[0].path, "/containers/0123456789ab/restart");
  assert.equal(calls[0].init?.method, "POST");
});

test("a failed action returns the docker error", async () => {
  const { ctx } = fakeCtx(
    () => new Response(JSON.stringify({ error: "No such container" }), { status: 502 }),
  );
  assert.equal(await containerAction(ctx, "0123456789ab", "stop"), "No such container");
});

test("other action failures return a fixed phrase", async () => {
  const bad = (status: number, body = "") => fakeCtx(() => new Response(body, { status })).ctx;
  assert.equal(await containerAction(bad(400, '{"error":"bad id"}'), "0123456789ab", "start"), "Request failed");
  assert.equal(await containerAction(bad(404), "0123456789ab", "start"), "Request failed");
  assert.equal(await containerAction(bad(502, "garbage"), "0123456789ab", "start"), "Request failed");
  assert.equal(await containerAction(bad(502, '{"error":""}'), "0123456789ab", "start"), "Request failed");
  const throwing = fakeCtx(() => {
    throw new TypeError("offline");
  });
  assert.equal(await containerAction(throwing.ctx, "0123456789ab", "remove"), "Request failed");
});

/** Let pending promise callbacks run. */
const settle = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

test("the poller waits while hidden and runs once when poked", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let hidden = true;
  let ticks = 0;
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
    },
    interval: () => 2000,
    hidden: () => hidden,
  });
  poller.start();
  await settle();
  assert.equal(ticks, 0, "hidden: no request");
  t.mock.timers.tick(60_000);
  await settle();
  assert.equal(ticks, 0, "hidden: no timer");
  poller.poke();
  await settle();
  assert.equal(ticks, 1, "a poke runs once");
  t.mock.timers.tick(60_000);
  await settle();
  assert.equal(ticks, 1, "still hidden after the poke: no next timer");
  hidden = false;
  poller.poke();
  await settle();
  assert.equal(ticks, 2);
  t.mock.timers.tick(1999);
  await settle();
  assert.equal(ticks, 2);
  t.mock.timers.tick(1);
  await settle();
  assert.equal(ticks, 3);
  poller.stop();
});

test("the poller runs at once when visible, then every interval", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ticks = 0;
  let interval = 2000;
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
    },
    interval: () => interval,
    hidden: () => false,
  });
  poller.start();
  await settle();
  assert.equal(ticks, 1);
  t.mock.timers.tick(2000);
  await settle();
  assert.equal(ticks, 2);
  interval = 15_000;
  t.mock.timers.tick(2000);
  await settle();
  assert.equal(ticks, 3);
  t.mock.timers.tick(14_999);
  await settle();
  assert.equal(ticks, 3, "the new interval applies from the next wait");
  t.mock.timers.tick(1);
  await settle();
  assert.equal(ticks, 4);
  poller.stop();
});

test("the poller does not overlap ticks; a poke during a tick runs once more after it", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ticks = 0;
  let active = 0;
  let maxActive = 0;
  const releases: (() => void)[] = [];
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
      active += 1;
      maxActive = Math.max(maxActive, active);
      await new Promise<void>((resolve) => releases.push(resolve));
      active -= 1;
    },
    interval: () => 2000,
    hidden: () => false,
  });
  poller.start();
  await settle();
  assert.equal(ticks, 1);
  poller.poke();
  poller.poke();
  t.mock.timers.tick(60_000);
  await settle();
  assert.equal(ticks, 1, "the running tick blocks others, and no timer is set meanwhile");
  releases.shift()?.();
  await settle();
  assert.equal(ticks, 2, "the pokes run exactly one more tick");
  releases.shift()?.();
  await settle();
  assert.equal(ticks, 2);
  assert.equal(maxActive, 1);
  t.mock.timers.tick(2000);
  await settle();
  assert.equal(ticks, 3);
  poller.stop();
});

test("stop cancels the timer and later ticks", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ticks = 0;
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
    },
    interval: () => 2000,
    hidden: () => false,
  });
  poller.start();
  await settle();
  poller.stop();
  t.mock.timers.tick(60_000);
  await settle();
  poller.poke();
  await settle();
  assert.equal(ticks, 1);
});

test("stop during a tick prevents the next one", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ticks = 0;
  let release: () => void = () => {};
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
      await new Promise<void>((resolve) => (release = resolve));
    },
    interval: () => 2000,
    hidden: () => false,
  });
  poller.start();
  await settle();
  poller.poke();
  poller.stop();
  release();
  await settle();
  t.mock.timers.tick(60_000);
  await settle();
  assert.equal(ticks, 1);
});

test("a failing tick does not stop the poller", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let ticks = 0;
  const poller = createPoller({
    tick: async () => {
      ticks += 1;
      throw new Error("boom");
    },
    interval: () => 2000,
    hidden: () => false,
  });
  poller.start();
  await settle();
  t.mock.timers.tick(2000);
  await settle();
  assert.equal(ticks, 2);
  poller.stop();
});


// ---- The panel and the plugin (DockerPanel.tsx, index.tsx) -----------------

// I18nProvider follows the browser's language, which Node takes from LANG.
// Pin it so the English text below matches on every machine.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

const noop = () => {};
const HANDLERS: PanelHandlers = {
  filter: noop,
  toggleGroup: noop,
  logs: noop,
  shell: noop,
  openPort: noop,
  act: noop,
  actAll: noop,
  confirm: noop,
  menu: noop,
  copy: noop,
  refresh: noop,
  retry: noop,
  uninstall: noop,
};
const NO_UI: PanelUi = {
  filter: "",
  collapsed: new Set(),
  busy: new Set(),
  errors: new Map(),
  confirming: null,
  menu: null,
};
const NOW = 1_794_140_000_000;

function panel(state: DockerState | null, ui: Partial<PanelUi> = {}, platform: Platform = "mac") {
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(DockerPanelView, { state, ui: { ...NO_UI, ...ui }, now: NOW, platform, copied: null, on: HANDLERS }),
    ),
  );
}

const ok = (containers: DockerContainer[]): DockerState => ({
  engine: { status: "ok", version: "27.3.1", error: null },
  containers,
});
const web = c({ id: "a1".padEnd(12, "0"), service: "web", project: "westerm-site", workingDir: "/Users/me/westerm-site", image: "nginx:1.27", ports: [8080] });
const api = c({ id: "a2".padEnd(12, "0"), service: "api", project: "westerm-site", image: "node:22-slim", ports: [3000] });
const db = c({ id: "a3".padEnd(12, "0"), service: "db", project: "westerm-site", image: "postgres:16", health: "healthy" });
const worker = c({
  id: "b1".padEnd(12, "0"),
  service: "worker",
  project: "blog",
  image: "blog-worker",
  state: "exited",
  exitCode: 1,
  finishedAt: NOW - 3 * 60_000,
});
const loose = c({ id: "c1".padEnd(12, "0"), name: "scratch", image: "alpine:3", state: "exited", exitCode: 0 });
const SITE = ok([web, api, db, worker, loose]);

test("before the first answer the panel shows one muted loading line", () => {
  const html = panel(null);
  assert.match(html, /<p class="docker-none">Loading…<\/p>/);
  assert.doesNotMatch(panel(SITE), /Loading…/);
  assert.doesNotMatch(panel({ engine: { status: "no-cli", version: null, error: null }, containers: [] }), /Loading…/);
});

test("the panel lists groups, ports, and a failed container", () => {
  const html = panel(SITE);
  assert.match(html, /westerm-site/);
  assert.match(html, /~\/westerm-site/);
  assert.match(html, /:3000/);
  assert.match(html, /:8080/);
  assert.match(html, /Exited \(1\) · 3 min ago/);
  assert.match(html, /healthy/);
  assert.match(html, /Other containers/);
  assert.match(html, /placeholder="Filter containers"/);
  assert.match(html, /3 \/ 5 running/);
  assert.match(html, /27\.3\.1/);
  assert.match(html, />local</);
  // Groups: blog, westerm-site, then the others; the failed group's badge is red.
  assert.ok(html.indexOf(">blog<") < html.indexOf(">westerm-site<"));
  assert.ok(html.indexOf(">westerm-site<") < html.indexOf("Other containers"));
  assert.match(html, /docker-count failed[^>]*>0\/1</);
  assert.match(html, /docker-dot failed/);
  assert.match(html, /docker-dot stopped/);
});

test("a collapsed group hides its containers; the filter narrows the list", () => {
  const collapsed = panel(SITE, { collapsed: new Set(["westerm-site"]) });
  assert.match(collapsed, /aria-expanded="false"/);
  assert.doesNotMatch(collapsed, /nginx:1\.27/);
  const filtered = panel(SITE, { filter: "postgres" });
  assert.match(filtered, /postgres:16/);
  assert.doesNotMatch(filtered, /nginx:1\.27/);
  assert.doesNotMatch(filtered, /blog-worker/);
  assert.match(filtered, /3 \/ 5 running/, "the counts stay for all containers");
});

test("empty states: no CLI, engine down, no containers", () => {
  const noCli: DockerState = { engine: { status: "no-cli", version: null, error: "not found" }, containers: [] };
  const mac = panel(noCli, {}, "mac");
  assert.match(mac, /Docker CLI not found/);
  assert.match(mac, /brew install --cask docker/);
  assert.match(mac, />Copy</);
  assert.doesNotMatch(mac, /Filter containers/);
  assert.match(panel(noCli, {}, "windows"), /winget install Docker\.DockerDesktop/);
  const linux = panel(noCli, {}, "linux");
  assert.match(linux, /href="https:\/\/docs\.docker\.com\/engine\/install\/"/);
  assert.doesNotMatch(linux, />Copy</);

  const down: DockerState = {
    engine: { status: "down", version: null, error: "Cannot connect to the Docker daemon\nIs it running?" },
    containers: [],
  };
  const downHtml = panel(down);
  assert.match(downHtml, /Docker is not running/);
  assert.match(downHtml, /Cannot connect to the Docker daemon/);
  assert.doesNotMatch(downHtml, /Is it running\?</);
  assert.match(downHtml, />Retry</);

  const none = panel(ok([]));
  assert.match(none, /No containers/);
  assert.doesNotMatch(none, /Other containers/);
});

test("a failed action shows its error under the row", () => {
  const html = panel(SITE, { errors: new Map([[api.id, "No such container: a2\nsecond line"]]) });
  assert.match(html, /title="No such container: a2\nsecond line"[^>]*>No such container: a2</);
  assert.doesNotMatch(html, />second line/);
});

test("a row's hover buttons come before its port chips, Stop farthest from them", () => {
  const html = panel(SITE);
  // The first running row is api (westerm-site sorts api, db, web).
  const row = html.slice(html.indexOf('aria-label="Running"'));
  const tail = row.slice(row.indexOf('class="docker-tail"'), row.indexOf("</li>"));
  assert.ok(tail.length > 0, "the row has a docker-tail");
  const labels = [...tail.matchAll(/aria-label="([^"]+)"/g)].map((m) => m[1]);
  assert.deepEqual(labels, ["Stop api", "Restart api", "Shell api", "Logs api"]);
  assert.ok(tail.indexOf("docker-actions") < tail.indexOf("docker-ports"));
  assert.match(tail, /class="docker-ports"[^]*:3000/);
});

test("a busy row spins and locks its action buttons", () => {
  const html = panel(SITE, { busy: new Set([api.id]) });
  assert.match(html, /docker-dot busy/);
  assert.match(html, /<button[^>]*disabled=""[^>]*aria-label="Restart api"/);
  assert.doesNotMatch(html, /<button[^>]*disabled=""[^>]*aria-label="Restart web"/);
});

test("the context menu offers Remove only for a stopped container", () => {
  const running = panel(SITE, { menu: { kind: "container", id: api.id, x: 10, y: 10 } });
  assert.match(running, /role="menu"/);
  for (const item of ["Logs", "Shell", "Open :3000 in browser", "Stop", "Restart", "Copy container ID"]) {
    assert.match(running, new RegExp(`role="menuitem"[^>]*>(<svg.*?</svg>)?${item}<`), item);
  }
  assert.doesNotMatch(running, /Remove…/);
  assert.doesNotMatch(running, />Start</);

  const stopped = panel(SITE, { menu: { kind: "container", id: worker.id, x: 10, y: 10 } });
  assert.match(stopped, /Remove…/);
  assert.match(stopped, />Start</);

  const header = panel(SITE, { menu: { kind: "header", x: 10, y: 10 } });
  assert.match(header, /role="menuitem"[^>]*>Uninstall…</);
});

test("Remove asks under the row first", () => {
  const html = panel(SITE, { confirming: worker.id });
  assert.match(html, /Remove worker\? This deletes the container\. Its image and volumes stay\./);
  assert.match(html, />Remove</);
  assert.match(html, />Cancel</);
});

test("the plugin describes itself in every language", () => {
  assert.equal(dockerPlugin.id, "docker");
  assert.equal(dockerPlugin.version, "0.1.0");
  assert.equal(dockerPlugin.icon, Container);
  assert.deepEqual(dockerPlugin.text.en, {
    name: "Docker",
    description:
      "See your containers by compose project. Open logs and shells as westerm panes. Click a port to open it in a browser pane.",
    permissions: [
      "Run docker commands on this computer",
      "Open terminal and browser panes",
      "Check containers in the background",
    ],
  });
  for (const locale of LOCALES) {
    assert.equal(dockerPlugin.text[locale].permissions.length, 3, locale);
    assert.ok(dockerPlugin.text[locale].description, locale);
  }
  assert.ok(BUILTIN_PLUGINS.includes(dockerPlugin));
});

/** A stand-in for the page's document: visibility and its change event. */
const page = Object.assign(new EventTarget(), { visibilityState: "visible" as DocumentVisibilityState });
Object.defineProperty(globalThis, "document", { value: page, configurable: true });

/** Let pending requests and their callbacks run. */
const flush = async () => {
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setImmediate(resolve));
};

interface Plugged {
  ctx: PluginContext;
  fetches: string[];
  refreshes: number;
}
function plugged(answer: () => Response): Plugged {
  const p: Plugged = {
    fetches: [],
    refreshes: 0,
    ctx: {
      openShell: noop,
      openBrowser: noop,
      fetch: async (path) => {
        p.fetches.push(path);
        return answer();
      },
      refreshInstalled: () => {
        p.refreshes += 1;
      },
      openManager: noop,
    },
  };
  return p;
}
const json = (state: DockerState) => new Response(JSON.stringify(state), { status: 200 });

test("the badge counts running containers and hides when the engine is down", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let state: DockerState = ok([running("d1"), running("d2"), failed("d3")]);
  const p = plugged(() => json(state));
  const instance = dockerPlugin.start(p.ctx);
  let changes = 0;
  instance.subscribe(() => (changes += 1));
  await flush();
  assert.deepEqual(instance.status(), { badge: 2, alert: false });
  assert.ok(changes > 0);
  const same = instance.status();
  t.mock.timers.tick(15_000);
  await flush();
  assert.equal(instance.status(), same, "the same object while nothing changes");

  state = ok([failed("d1"), running("d2"), failed("d3")]);
  t.mock.timers.tick(15_000);
  await flush();
  assert.deepEqual(instance.status(), { badge: 1, alert: true }, "a closed panel shows the new failure");
  instance.setPanelOpen(true);
  assert.deepEqual(instance.status(), { badge: 1, alert: false }, "opening the panel clears it");
  await flush();

  state = ok([failed("d1"), running("d2"), failed("d3")]);
  state.engine = { status: "down", version: null, error: "x" };
  t.mock.timers.tick(2000);
  await flush();
  assert.deepEqual(instance.status(), { badge: null, alert: false });
  instance.dispose();
});

test("the panel open polls every 2 s, closed every 15 s, and a visible page asks at once", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const p = plugged(() => json(ok([])));
  const instance = dockerPlugin.start(p.ctx);
  await flush();
  assert.equal(p.fetches.length, 1);
  t.mock.timers.tick(2000);
  await flush();
  assert.equal(p.fetches.length, 1, "closed: not after 2 s");
  instance.setPanelOpen(true);
  await flush();
  assert.equal(p.fetches.length, 2, "opening asks at once");
  instance.setPanelOpen(true);
  await flush();
  assert.equal(p.fetches.length, 2, "the same view again asks nothing");
  t.mock.timers.tick(2000);
  await flush();
  assert.equal(p.fetches.length, 3);

  page.visibilityState = "hidden";
  t.mock.timers.tick(60_000);
  await flush();
  assert.equal(p.fetches.length, 3, "hidden: nothing asked");
  page.visibilityState = "visible";
  page.dispatchEvent(new Event("visibilitychange"));
  await flush();
  assert.equal(p.fetches.length, 4, "visible again: asks at once");

  instance.dispose();
  page.dispatchEvent(new Event("visibilitychange"));
  t.mock.timers.tick(60_000);
  await flush();
  assert.equal(p.fetches.length, 4, "disposed: no more requests");
});

test("gone stops polling and asks for the installed list", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const p = plugged(() => new Response("{}", { status: 404 }));
  const instance = dockerPlugin.start(p.ctx);
  await flush();
  assert.equal(p.refreshes, 1);
  t.mock.timers.tick(60_000);
  instance.setPanelOpen(true);
  page.dispatchEvent(new Event("visibilitychange"));
  await flush();
  assert.equal(p.fetches.length, 1, "no further fetch");
  assert.equal(p.refreshes, 1);
  assert.deepEqual(instance.status(), { badge: null, alert: false });
  const html = renderToStaticMarkup(createElement(I18nProvider, null, createElement(instance.Panel)));
  assert.match(html, /Docker/);
  assert.doesNotMatch(html, /not running|not found|failed/i);
  instance.dispose();
});
