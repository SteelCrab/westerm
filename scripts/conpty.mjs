// Puts Microsoft's ConPTY (conpty.dll and OpenConsole.exe) in a folder, for
// Windows builds of westerm.
//
//   node scripts/conpty.mjs <folder> [--arch x64|arm64]
//
// The ConPTY built into Windows drops the Kitty graphics that terminal-browser
// and terminal-code draw with. portable-pty loads a conpty.dll found next to
// westerm.exe instead of the built-in one, and that conpty.dll starts the
// OpenConsole.exe next to it; without OpenConsole.exe it quietly uses the
// built-in console host again. The package is MIT licensed; its license goes
// along as conpty-LICENSE.txt.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const PACKAGE = "microsoft.windows.console.conpty";
const VERSION = "1.25.260930003";
const SHA256 = "02b07b349af66d801159bdf9e440d4a1ce78bb951f37fc8609731665afdae7ee";
const ARCHES = ["x64", "arm64"];

const [folder, ...args] = process.argv.slice(2);
const at = args.indexOf("--arch");
const arch = at >= 0 ? args[at + 1] : "x64";
if (!folder || !ARCHES.includes(arch)) {
  console.error(`usage: node scripts/conpty.mjs <folder> [--arch ${ARCHES.join("|")}]`);
  process.exit(2);
}

const url = `https://api.nuget.org/v3-flatcontainer/${PACKAGE}/${VERSION}/${PACKAGE}.${VERSION}.nupkg`;
const response = await fetch(url);
if (!response.ok) {
  console.error(`conpty: ${url}: HTTP ${response.status}`);
  process.exit(1);
}
const archive = Buffer.from(await response.arrayBuffer());
const sum = createHash("sha256").update(archive).digest("hex");
if (sum !== SHA256) {
  console.error(`conpty: ${PACKAGE} ${VERSION} does not match its SHA-256 sum (${sum})`);
  process.exit(1);
}

// A .nupkg is a zip. bsdtar reads zips (Windows and macOS).
const work = mkdtempSync(join(tmpdir(), "westerm-conpty-"));
try {
  const zip = join(work, "conpty.zip");
  writeFileSync(zip, archive);
  const files = {
    "conpty.dll": `runtimes/win-${arch}/native/conpty.dll`,
    "OpenConsole.exe": `build/native/runtimes/${arch}/OpenConsole.exe`,
  };
  // Windows' own tar is bsdtar; a GNU tar from Git may come first on PATH.
  const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
  const unpacked = spawnSync(tar, ["-xf", zip, "-C", work, ...Object.values(files)], { stdio: "inherit" });
  if (unpacked.status !== 0) {
    console.error("conpty: cannot unpack the package with tar");
    process.exit(1);
  }
  mkdirSync(folder, { recursive: true });
  for (const [name, path] of Object.entries(files)) {
    const from = join(work, path);
    if (!existsSync(from)) {
      console.error(`conpty: the package has no ${path}`);
      process.exit(1);
    }
    copyFileSync(from, join(folder, name));
  }
  copyFileSync(fileURLToPath(new URL("conpty-LICENSE.txt", import.meta.url)), join(folder, "conpty-LICENSE.txt"));
  console.log(`> conpty ${VERSION} (${arch}): ${Object.keys(files).join(", ")} in ${folder}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
