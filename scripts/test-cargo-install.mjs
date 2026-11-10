// Install the packaged crate outside the repository and verify the embedded UI over HTTP.
import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--manifest-path", join(root, "native/Cargo.toml"), "--locked", "--no-deps", "--format-version", "1",
], { encoding: "utf8" }));
const { version } = metadata.packages.find((pkg) => pkg.name === "westerm");
assert.equal(version, JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version);
const archive = join(metadata.target_directory, "package", `westerm-${version}.crate`);
assert.ok(existsSync(archive), "run npm run pack:cargo first");
const directory = mkdtempSync(join(tmpdir(), "westerm cargo install "));

function filesIn(path) {
  return readdirSync(path, { withFileTypes: true }).flatMap((entry) => {
    const file = join(path, entry.name);
    return entry.isDirectory() ? filesIn(file) : [file];
  });
}

async function verifyDashboard(binary, assets) {
  const env = { ...process.env, WESTERM_STATE_DIR: join(directory, "state") };
  const server = spawn(binary, ["--port", "0"], { cwd: directory, env, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  let failure;
  server.stdout.on("data", (chunk) => { output += chunk; });
  server.stderr.on("data", (chunk) => { output += chunk; });
  server.on("error", (error) => { failure = error; });
  const exited = new Promise((resolve) => server.once("exit", resolve));
  try {
    const deadline = Date.now() + 20_000;
    let base;
    while (Date.now() < deadline) {
      if (failure) throw failure;
      assert.equal(server.exitCode, null, output);
      const address = output.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (address) {
        base = address[0];
        break;
      }
      await delay(100);
    }
    assert.ok(base, `dashboard did not start: ${output}`);
    for (const [name, expected] of assets) {
      const response = await fetch(new URL(name, `${base}/`), { signal: AbortSignal.timeout(10_000) });
      assert.equal(response.status, 200, name);
      assert.deepEqual(Buffer.from(await response.arrayBuffer()), expected, `embedded asset differs: ${name}`);
    }
    const response = await fetch(`${base}/`, { signal: AbortSignal.timeout(10_000) });
    assert.equal(response.status, 200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()), assets.get("index.html"));
    console.log(`Verified ${assets.size} embedded web files over HTTP`);
  } finally {
    server.kill();
    await Promise.race([exited, delay(5_000)]);
    if (server.exitCode === null && server.signalCode === null && !failure) {
      server.kill("SIGKILL");
      await exited;
    }
    execFileSync(binary, ["stop"], { cwd: directory, env, stdio: "inherit", timeout: 15_000 });
  }
}

try {
  const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
  execFileSync(tar, ["-xzf", archive, "-C", directory]);
  const crate = join(directory, `westerm-${version}`);
  const web = join(crate, "web-dist");
  assert.ok(existsSync(join(web, "index.html")), "packaged web UI is missing");
  assert.ok(existsSync(join(crate, "LICENSE")), "packaged license is missing");
  const assets = new Map(filesIn(web).map((file) => [relative(web, file).split(sep).join("/"), readFileSync(file)]));
  execFileSync("cargo", [
    "install", "--path", crate, "--locked", "--root", join(directory, "install"),
    "--target-dir", join(metadata.target_directory, "cargo-install-check"),
  ], { cwd: directory, stdio: "inherit" });
  const binary = join(directory, "install", "bin", process.platform === "win32" ? "westerm.exe" : "westerm");
  const actual = execFileSync(binary, ["--version"], { encoding: "utf8" });
  assert.ok(actual.startsWith(`westerm ${version} `), `unexpected version: ${actual}`);
  // Remove the source assets to prove that the installed release embeds them.
  rmSync(web, { recursive: true, force: true });
  await verifyDashboard(binary, assets);
  console.log(`Installed crate verified: ${actual.trim()}`);
} finally {
  rmSync(directory, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
}
