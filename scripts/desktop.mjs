// Builds and runs the desktop app (desktop/, Tauri) on macOS, Windows, and Linux.
//
//   node scripts/desktop.mjs dev                # npm run app:dev: debug westerm + `tauri dev`
//   node scripts/desktop.mjs build              # npm run build:app: this computer's target
//   node scripts/desktop.mjs build --target universal-apple-darwin
//   node scripts/desktop.mjs sidecar [--release] [--target T]   # only desktop/binaries/
//
// The app ships the westerm executable next to its own (Tauri externalBin):
// desktop/binaries/westerm-<target triple>[.exe]. `build` puts the release
// files in dist-app/ under the names the release and the app's updater use:
//   westerm-macos-app.zip, westerm-macos-app.dmg     (macOS, Apple silicon and Intel)
//   westerm-windows-app-setup.exe                    (Windows x64)
//   westerm-linux-app-<arch>.deb, westerm-linux-app-<arch>.rpm
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, chmodSync, existsSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const desktop = join(root, "desktop");
const windows = process.platform === "win32";
const exe = windows ? ".exe" : "";

function say(text) {
  console.log(`> ${text}`);
}

function attempt(command, args, options = {}) {
  return spawnSync(command, args, { cwd: root, stdio: "inherit", shell: windows, ...options }).status === 0;
}

function run(command, args, options = {}) {
  if (!attempt(command, args, options)) {
    console.error(`${command} ${args.join(" ")} failed`);
    process.exit(1);
  }
}

function hostTarget() {
  const info = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  return /^host: (.+)$/m.exec(info)[1].trim();
}

function option(args, name) {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
}

/** Builds westerm (with the web UI embedded) and puts it where Tauri looks. */
function sidecar(target, release) {
  say("web UI");
  run("npx", ["vite", "build"]);
  const targets = target === "universal-apple-darwin" ? ["aarch64-apple-darwin", "x86_64-apple-darwin"] : [target];
  const built = targets.map((triple) => {
    say(`westerm for ${triple}`);
    const args = ["build", "--locked", "--manifest-path", "native/Cargo.toml", "--target", triple];
    if (release) args.push("--release");
    run("cargo", args);
    return join(root, "native", "target", triple, release ? "release" : "debug", `westerm${exe}`);
  });
  const binaries = join(desktop, "binaries");
  mkdirSync(binaries, { recursive: true });
  const place = (from, triple) => {
    const to = join(binaries, `westerm-${triple}${exe}`);
    copyFileSync(from, to);
    if (!windows) chmodSync(to, 0o755);
    return to;
  };
  // A universal build compiles the app once per CPU, and each of those checks
  // for its own westerm-<triple>; the bundle takes the universal one.
  targets.forEach((triple, at) => place(built[at], triple));
  if (built.length === 1) return join(binaries, `westerm-${target}${exe}`);
  const out = join(binaries, `westerm-${target}${exe}`);
  run("lipo", ["-create", ...built, "-output", out]);
  return out;
}

/** The files a bundle folder holds with `extension`. */
function bundled(folder, extension) {
  if (!existsSync(folder)) return [];
  return readdirSync(folder)
    .filter((name) => name.endsWith(extension))
    .map((name) => join(folder, name));
}

function only(files, what) {
  if (files.length !== 1) {
    console.error(`expected one ${what}, found ${files.length}: ${files.join(", ")}`);
    process.exit(1);
  }
  return files[0];
}

/** Debian and RPM names for the CPU of `target`. */
function linuxArch(target) {
  if (target.startsWith("x86_64")) return { deb: "amd64", rpm: "x86_64" };
  if (target.startsWith("aarch64")) return { deb: "arm64", rpm: "aarch64" };
  console.error(`no Linux app for ${target}`);
  process.exit(1);
}

function build(target) {
  sidecar(target, true);
  const bundles = target.endsWith("-apple-darwin") ? "app,dmg" : windows ? "nsis" : "deb,rpm";
  const bundle = join(desktop, "target", target, "release", "bundle");
  // Files of earlier builds would stay next to the new ones.
  rmSync(bundle, { recursive: true, force: true });
  say(`app for ${target} (${bundles})`);
  const tauri = ["tauri", "build", "--target", target, "--bundles", bundles];
  if (target.endsWith("-apple-darwin")) {
    // Finder arranges the disk image window (the background, the app, and
    // the Applications link), which needs a session that may control Finder.
    // Tauri skips that when CI=true unless told otherwise. Without Finder the
    // image still works, unarranged.
    const arranged = { ...process.env, TAURI_BUNDLER_DMG_IGNORE_CI: "true" };
    if (!attempt("npx", tauri, { cwd: desktop, env: arranged })) {
      console.warn("warning: Finder could not arrange the disk image window; building it unarranged");
      run("npx", tauri, { cwd: desktop, env: { ...process.env, CI: "true", TAURI_BUNDLER_DMG_IGNORE_CI: "false" } });
    }
  } else {
    run("npx", tauri, { cwd: desktop });
  }

  const out = join(root, "dist-app");
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  if (target.endsWith("-apple-darwin")) {
    // The zip is for the app's updater; the disk image is for people, who
    // install by dragging the app to Applications.
    const app = join(bundle, "macos", "westerm.app");
    run("ditto", ["-c", "-k", "--keepParent", app, join(out, "westerm-macos-app.zip")]);
    copyFileSync(only(bundled(join(bundle, "dmg"), ".dmg"), "disk image"), join(out, "westerm-macos-app.dmg"));
  } else if (windows) {
    copyFileSync(only(bundled(join(bundle, "nsis"), "-setup.exe"), "installer"), join(out, "westerm-windows-app-setup.exe"));
  } else {
    const arch = linuxArch(target);
    copyFileSync(only(bundled(join(bundle, "deb"), ".deb"), ".deb"), join(out, `westerm-linux-app-${arch.deb}.deb`));
    copyFileSync(only(bundled(join(bundle, "rpm"), ".rpm"), ".rpm"), join(out, `westerm-linux-app-${arch.rpm}.rpm`));
  }
  for (const name of readdirSync(out)) say(`done: dist-app/${name}`);
}

const [command, ...args] = process.argv.slice(2);
const target = option(args, "--target") ?? hostTarget();
switch (command) {
  case "dev":
    sidecar(target, false);
    run("npx", ["tauri", "dev"], { cwd: desktop });
    break;
  case "build":
    build(target);
    break;
  case "sidecar":
    say(`sidecar: ${sidecar(target, args.includes("--release"))}`);
    break;
  default:
    console.error("usage: node scripts/desktop.mjs dev | build [--target T] | sidecar [--release] [--target T]");
    process.exit(2);
}
