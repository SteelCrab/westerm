// Prepare dist-npm from the GitHub release matching package.json.
// Run `npm run pack:npm`, then publish the resulting .tgz after testing it.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { filesFor, targets } from "../npm/bin/platform.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));

export function verifyChecksum(archive, checksum, name) {
  const expected = checksum.trim().split(/\s+/)[0].toLowerCase();
  const actual = createHash("sha256").update(archive).digest("hex");
  if (!/^[a-f0-9]{64}$/.test(expected) || actual !== expected) {
    throw new Error(`SHA-256 mismatch for ${name}`);
  }
  return actual;
}

async function download(url) {
  const response = await fetch(url, { signal: AbortSignal.timeout(120_000) });
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

async function prepare() {
  const { version: sourceVersion } = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  const version = process.env.WESTERM_NPM_VERSION ?? sourceVersion;
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version)) throw new Error(`invalid version: ${version}`);
  const manifest = JSON.parse(readFileSync(join(root, "npm/package.json"), "utf8"));
  const work = mkdtempSync(join(tmpdir(), "westerm-npm-"));
  const stage = join(work, "package");
  const output = join(root, "dist-npm");
  // A failed preparation must not leave an older package ready to publish.
  rmSync(output, { recursive: true, force: true });
  try {
    mkdirSync(stage);
    cpSync(join(root, "npm/bin"), join(stage, "bin"), { recursive: true });
    chmodSync(join(stage, "bin/westerm.mjs"), 0o755);
    copyFileSync(join(root, "npm/README.md"), join(stage, "README.md"));
    copyFileSync(join(root, "LICENSE"), join(stage, "LICENSE"));
    const release = { version, url: `https://github.com/SteelCrab/westerm/releases/tag/v${version}`, assets: [] };
    for (const target of targets) {
      const extension = target.endsWith("-windows-msvc") ? "zip" : "tar.gz";
      const name = `westerm-${target}.${extension}`;
      const url = `https://github.com/SteelCrab/westerm/releases/download/v${version}/${name}`;
      console.log(`Preparing ${name}`);
      const [archive, checksum] = await Promise.all([download(url), download(`${url}.sha256`)]);
      const sha256 = verifyChecksum(archive, checksum.toString("utf8"), name);
      const path = join(work, name);
      writeFileSync(path, archive);
      const files = filesFor(target);
      const members = files.map((file) => `westerm-${target}/${file}`);
      // GNU tar cannot read zip. macOS and Windows use bsdtar.
      if (extension === "zip" && process.platform === "linux") {
        execFileSync("unzip", ["-q", path, ...members, "-d", work]);
      } else {
        const tar = process.platform === "win32"
          ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
        execFileSync(tar, ["-xf", path, "-C", work, ...members]);
      }
      const destination = join(stage, "vendor", target);
      mkdirSync(destination, { recursive: true });
      for (const file of files) {
        copyFileSync(join(work, `westerm-${target}`, file), join(destination, file));
        chmodSync(join(destination, file), file.endsWith(".txt") || file.endsWith(".dll") ? 0o644 : 0o755);
      }
      release.assets.push({ name, url, sha256 });
    }
    writeFileSync(join(stage, "release.json"), JSON.stringify(release, null, 2) + "\n");
    writeFileSync(join(stage, "package.json"), JSON.stringify({ ...manifest, version }, null, 2) + "\n");
    // Copy across filesystems, then rename only when the package is complete.
    const pending = `${output}.tmp`;
    rmSync(pending, { recursive: true, force: true });
    cpSync(stage, pending, { recursive: true });
    renameSync(pending, output);
    console.log(`Prepared ${manifest.name}@${version} in ${output}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await prepare();
}
