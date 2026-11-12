import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { test } from "node:test";

// The core knows a built-in plugin only through the registry, and the plugin
// knows the core only through src/plugins/types.ts and shared helpers.

const SRC = resolve(import.meta.dirname, "../src");
const DOCKER = join(SRC, "plugins/docker");
const DB = join(SRC, "plugins/db");
const GIT = join(SRC, "plugins/git");
const EXTERNAL = join(SRC, "plugins/external");
const REGISTRY = join(SRC, "plugins/registry.ts");
const CORE_STATE = ["workspace.ts", "agents.ts", "App.tsx"].map((file) => join(SRC, file));

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    return /\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

/** Every relative module a file imports or re-exports, as an absolute path. */
function imports(file: string): string[] {
  const text = readFileSync(file, "utf8");
  const specs = [...text.matchAll(/\bfrom\s+["']([^"']+)["']|\bimport\s*\(?\s*["']([^"']+)["']/g)].map(
    (match) => match[1] ?? match[2],
  );
  return specs.filter((spec) => spec.startsWith(".")).map((spec) => resolve(dirname(file), spec));
}

const inside = (path: string, dir: string) => !relative(dir, path).startsWith("..");

test("core files reach the docker plugin only through the registry", () => {
  const core = sources(SRC).filter((file) => !inside(file, DOCKER) && file !== REGISTRY);
  assert.ok(core.length > 20, "found the core files");
  for (const file of core) {
    for (const target of imports(file)) {
      assert.ok(!inside(target, DOCKER), `${relative(SRC, file)} imports ${relative(SRC, target)}`);
    }
  }
  assert.ok(imports(REGISTRY).some((target) => inside(target, DOCKER)), "the registry lists the plugin");
});

test("core files reach the db plugin only through the registry", () => {
  const core = sources(SRC).filter((file) => !inside(file, DB) && file !== REGISTRY);
  for (const file of core) {
    for (const target of imports(file)) {
      assert.ok(!inside(target, DB), `${relative(SRC, file)} imports ${relative(SRC, target)}`);
    }
  }
  assert.ok(imports(REGISTRY).some((target) => inside(target, DB)), "the registry lists the plugin");
});

test("core files reach the git plugin only through the registry", () => {
  const core = sources(SRC).filter((file) => !inside(file, GIT) && file !== REGISTRY);
  for (const file of core) {
    for (const target of imports(file)) {
      assert.ok(!inside(target, GIT), `${relative(SRC, file)} imports ${relative(SRC, target)}`);
    }
  }
  assert.ok(imports(REGISTRY).some((target) => inside(target, GIT)), "the registry lists the plugin");
});

/** Fails when a file in `dir` reaches core state through any chain of relative imports. */
function assertNoCoreState(dir: string, atLeast: number) {
  const plugin = sources(dir);
  assert.ok(plugin.length >= atLeast, "found the plugin files");
  // Walk the relative-import graph (type-only imports count) and keep the
  // path that led to each file, for the failure message.
  const via = new Map<string, string[]>(plugin.map((file) => [file, [file]]));
  const queue = [...plugin];
  while (queue.length > 0) {
    const file = queue.shift()!;
    for (const target of imports(file)) {
      if (via.has(target)) continue;
      const path = [...via.get(file)!, target];
      assert.ok(!CORE_STATE.includes(target), path.map((each) => relative(SRC, each)).join(" -> "));
      via.set(target, path);
      queue.push(target);
    }
  }
}

test("the docker plugin does not reach core state, even through other files", () => {
  assertNoCoreState(DOCKER, 5);
});

test("the db plugin does not reach core state, even through other files", () => {
  assertNoCoreState(DB, 7);
});

test("the git plugin does not reach core state, even through other files", () => {
  // It learns the focused folder from its panel's props, not from the dashboard.
  assertNoCoreState(GIT, 5);
});

test("the external plugin runtime does not reach core state, even through other files", () => {
  assertNoCoreState(EXTERNAL, 4);
});

test("the dev server sends the dashboard's frame policy", async () => {
  // The same policy as PAGE_CSP in native/src/server/mod.rs: a plugin iframe
  // that navigates elsewhere is blocked, and its error page's load stops it.
  const { default: config } = await import("../vite.config.ts");
  const { server, preview } = config as { server?: { headers?: Record<string, string> }; preview?: { headers?: Record<string, string> } };
  assert.equal(server?.headers?.["Content-Security-Policy"], "frame-src 'self'");
  assert.equal(preview?.headers?.["Content-Security-Policy"], "frame-src 'self'");
});
