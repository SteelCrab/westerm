import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { prepareCargoPackage } from "../scripts/prepare-cargo.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "westerm cargo package "));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const files = {
    "dist/index.html": "<!doctype html><title>westerm</title>",
    "dist/assets/app.js": "console.log('web UI');",
    "dist/assets/app.css": "body { margin: 0; }",
    "dist/assets/ghostty.wasm": Buffer.from([0, 97, 115, 109]),
    "dist/assets/pixel-worker-test.js": "self.onmessage = () => {};",
    "LICENSE": "MIT test license",
    "native/web-dist/old.js": "old build",
  };
  for (const [name, content] of Object.entries(files)) {
    const path = join(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  return root;
}

test("stages the complete web build and license and removes old assets", (t) => {
  const root = fixture(t);
  prepareCargoPackage(root);
  for (const name of ["index.html", "assets/app.js", "assets/app.css", "assets/ghostty.wasm", "assets/pixel-worker-test.js"]) {
    assert.deepEqual(readFileSync(join(root, "native/web-dist", name)), readFileSync(join(root, "dist", name)));
  }
  assert.deepEqual(readFileSync(join(root, "native/LICENSE")), readFileSync(join(root, "LICENSE")));
  assert.equal(existsSync(join(root, "native/web-dist/old.js")), false);
});

test("rejects an incomplete web build and removes the previous staged package", (t) => {
  const root = fixture(t);
  rmSync(join(root, "dist/assets/ghostty.wasm"));
  assert.throws(() => prepareCargoPackage(root), /missing WASM/);
  assert.equal(existsSync(join(root, "native/web-dist")), false);
});

test("requires a web build and a license before packaging", (t) => {
  const root = fixture(t);
  rmSync(join(root, "LICENSE"));
  assert.throws(() => prepareCargoPackage(root), /LICENSE file is missing/);
  rmSync(join(root, "dist/index.html"));
  assert.throws(() => prepareCargoPackage(root), /run npx vite build first/);
});
