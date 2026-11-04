import assert from "node:assert/strict";
import { test } from "node:test";
import { isId, isSessionKind, normalizeBrowserUrl } from "../shared/protocol.ts";

// The server side of these rules lives in native/src/protocol.rs (cargo test).

test("only known session kinds and safe ids are accepted", () => {
  for (const kind of ["shell", "browser", "editor", "ssh"]) assert.ok(isSessionKind(kind));
  assert.equal(isSessionKind("rm"), false);
  assert.ok(isId("smw26lu0mc"));
  assert.equal(isId("../x"), false);
});

test("browser URLs allow web addresses only", () => {
  assert.equal(normalizeBrowserUrl("https://example.com"), "https://example.com");
  assert.equal(normalizeBrowserUrl("  example.com "), "example.com");
  assert.equal(normalizeBrowserUrl("localhost:3000"), "localhost:3000");
  assert.equal(normalizeBrowserUrl("example.com:8080/x"), "example.com:8080/x");
  assert.equal(normalizeBrowserUrl("javascript:alert(1)"), null);
  assert.equal(normalizeBrowserUrl("file:///etc/passwd"), null);
  assert.equal(normalizeBrowserUrl("--split"), null);
  assert.equal(normalizeBrowserUrl("a\nb"), null);
  assert.equal(normalizeBrowserUrl(null), null);
});
