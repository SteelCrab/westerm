import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { filesFor, targetFor } from "../npm/bin/platform.mjs";
import { verifyChecksum } from "../scripts/prepare-npm.mjs";

test("selects a release for every supported OS and CPU", () => {
  for (const [platform, arch, target] of [
    ["darwin", "arm64", "aarch64-apple-darwin"],
    ["darwin", "x64", "x86_64-apple-darwin"],
    ["linux", "arm64", "aarch64-unknown-linux-musl"],
    ["linux", "x64", "x86_64-unknown-linux-musl"],
    ["win32", "x64", "x86_64-pc-windows-msvc"],
    ["win32", "arm64", "x86_64-pc-windows-msvc"],
  ]) assert.equal(targetFor(platform, arch), target);
  for (const [platform, arch] of [["freebsd", "x64"], ["linux", "arm"], ["win32", "ia32"]]) {
    assert.throws(() => targetFor(platform, arch), /unsupported platform/);
  }
  assert.deepEqual(filesFor("x86_64-pc-windows-msvc"), ["westerm.exe", "conpty.dll", "OpenConsole.exe", "conpty-LICENSE.txt"]);
});

test("rejects changed archives and malformed checksums before extraction", () => {
  const archive = Buffer.from("release executable");
  const digest = createHash("sha256").update(archive).digest("hex");
  assert.equal(verifyChecksum(archive, `${digest}  westerm.tar.gz\n`, "westerm.tar.gz"), digest);
  assert.equal(verifyChecksum(archive, `${digest.toUpperCase()}  westerm.zip\r\n`, "westerm.zip"), digest);
  assert.throws(() => verifyChecksum(Buffer.from("changed"), digest, "westerm.tar.gz"), /SHA-256 mismatch/);
  for (const checksum of ["", "not a checksum", "0".repeat(64)]) {
    assert.throws(() => verifyChecksum(archive, checksum, "westerm.tar.gz"), /SHA-256 mismatch/);
  }
});

function fixture(t, source) {
  const directory = mkdtempSync(join(tmpdir(), "westerm launcher test "));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  cpSync(fileURLToPath(new URL("../npm/bin/", import.meta.url)), join(directory, "bin"), { recursive: true });
  if (source !== undefined) {
    const vendor = join(directory, "vendor", targetFor(process.platform, process.arch));
    mkdirSync(vendor, { recursive: true });
    writeFileSync(join(vendor, "westerm"), `#!/usr/bin/env node\n${source}`, { mode: 0o755 });
  }
  return { directory, launcher: join(directory, "bin/westerm.mjs") };
}

test("reports a missing binary with a reinstall instruction", (t) => {
  const { launcher } = fixture(t);
  const result = spawnSync(process.execPath, [launcher, "--version"], { encoding: "utf8" });
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Reinstall with "npm install -g westerm"/);
});

test("preserves arguments, environment, working directory, and exit status", { skip: process.platform === "win32" }, (t) => {
  const { directory, launcher } = fixture(t, `
    console.log(JSON.stringify({ args: process.argv.slice(2), cwd: process.cwd(), value: process.env.WESTERM_NPM_TEST }));
    console.error("native stderr");
    process.exit(7);
  `);
  const args = ["--port", "4318", "spaces and 'quotes'", "$(echo unexpected)"];
  const result = spawnSync(process.execPath, [launcher, ...args], {
    cwd: directory,
    env: { ...process.env, WESTERM_NPM_TEST: "preserved" },
    encoding: "utf8",
  });
  assert.equal(result.status, 7);
  const output = JSON.parse(result.stdout);
  assert.deepEqual(output.args, args);
  // macOS resolves /var to /private/var in the child's cwd.
  assert.ok(output.cwd.endsWith(directory.replace(/^\/private/, "")));
  assert.equal(output.value, "preserved");
  assert.equal(result.stderr, "native stderr\n");
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  test(`forwards ${signal} sent to the launcher`, { skip: process.platform === "win32", timeout: 10_000 }, async (t) => {
    const { launcher } = fixture(t, `
      process.on("${signal}", () => process.exit(23));
      console.log("ready");
      setInterval(() => {}, 1000);
    `);
    const child = spawn(process.execPath, [launcher], { stdio: ["ignore", "pipe", "pipe"] });
    t.after(() => child.kill("SIGKILL"));
    const exit = new Promise((resolve, reject) => {
      child.on("error", reject);
      child.on("exit", (code) => resolve(code));
    });
    child.stdout.once("data", () => child.kill(signal));
    assert.equal(await exit, 23);
  });
}
