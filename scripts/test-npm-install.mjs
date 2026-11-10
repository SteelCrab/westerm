// Install the exact tarball in an isolated prefix, with lifecycle scripts disabled.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { filesFor, targets } from "../npm/bin/platform.mjs";

const { version: sourceVersion } = JSON.parse(readFileSync(fileURLToPath(new URL("../package.json", import.meta.url)), "utf8"));
const version = process.env.WESTERM_NPM_VERSION ?? sourceVersion;
const { name } = JSON.parse(readFileSync(fileURLToPath(new URL("../npm/package.json", import.meta.url)), "utf8"));
const tarballName = name.replace(/^@/, "").replace("/", "-");
const tarball = resolve(process.argv[2] ?? `${tarballName}-${version}.tgz`);
const npm = process.env.npm_execpath;
assert.ok(npm, "run this check with npm run test:npm-install");
const prefix = mkdtempSync(join(tmpdir(), "westerm npm install "));
try {
  execFileSync(process.execPath, [npm, "install", "--global", "--prefix", prefix, "--ignore-scripts", "--no-audit", "--no-fund", tarball], { stdio: "inherit" });
  const installed = join(prefix, process.platform === "win32" ? "node_modules" : "lib/node_modules", name);
  const manifest = JSON.parse(readFileSync(join(installed, "package.json"), "utf8"));
  assert.equal(manifest.version, version);
  assert.equal(manifest.name, name);
  assert.equal(manifest.scripts, undefined);
  assert.equal(manifest.dependencies, undefined);
  for (const target of targets) {
    for (const file of filesFor(target)) assert.ok(existsSync(join(installed, "vendor", target, file)), `${target}/${file} is missing`);
  }
  // Check npm's actual command entrypoint, including its Windows .cmd shim.
  const launcher = process.platform === "win32" ? join(prefix, "westerm.cmd") : join(prefix, "bin/westerm");
  const result = process.platform === "win32"
    ? execFileSync(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", `""${launcher}" --version"`], { encoding: "utf8", windowsVerbatimArguments: true })
    : execFileSync(launcher, ["--version"], { encoding: "utf8" });
  assert.ok(result.startsWith(`westerm ${version} `), `unexpected version: ${result}`);
  console.log(`Installed package verified: ${result.trim()}`);
} finally {
  rmSync(prefix, { recursive: true, force: true });
}
