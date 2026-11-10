// Stage built web files inside the crate so cargo install needs no Node.js.
import { copyFileSync, cpSync, existsSync, readdirSync, rmSync, statSync } from "node:fs";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";

function filesIn(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesIn(path) : [path];
  });
}

export function prepareCargoPackage(root) {
  const source = join(root, "dist");
  const destination = join(root, "native", "web-dist");
  // A failed preparation must not leave an older web build ready to publish.
  rmSync(destination, { recursive: true, force: true });
  if (!existsSync(join(source, "index.html"))) throw new Error("dist/index.html is missing; run npx vite build first");
  if (!existsSync(join(root, "LICENSE"))) throw new Error("the repository LICENSE file is missing");

  const files = filesIn(source);
  const required = [
    ["JavaScript", files.some((path) => path.endsWith(".js"))],
    ["CSS", files.some((path) => path.endsWith(".css"))],
    ["WASM", files.some((path) => path.endsWith(".wasm"))],
    ["pixel worker", files.some((path) => /^pixel-worker.*\.js$/.test(basename(path)))],
  ];
  for (const [label, present] of required) {
    if (!present) throw new Error(`the web build is missing ${label}`);
  }
  cpSync(source, destination, { recursive: true });
  copyFileSync(join(root, "LICENSE"), join(root, "native", "LICENSE"));
  return { files: files.length, bytes: files.reduce((total, path) => total + statSync(path).size, 0) };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    const result = prepareCargoPackage(fileURLToPath(new URL("../", import.meta.url)));
    console.log(`Prepared ${result.files} web files (${result.bytes} bytes) in native/web-dist, and native/LICENSE`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
