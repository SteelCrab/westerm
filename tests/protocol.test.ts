import assert from "node:assert/strict";
import { test } from "node:test";
import { externalLaunch, isId, isLaunchArgv, isSessionKind, normalizeBrowserUrl, parsePlace } from "../shared/protocol.ts";
import { sessionQuery, type ConnectionOptions } from "../src/terminal/connection.ts";

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

test("place messages", () => {
  assert.deepEqual(parsePlace({ cwd: "/w", branch: "main" }), { cwd: "/w", branch: "main" });
  assert.deepEqual(parsePlace({ cwd: "/w" }), { cwd: "/w", branch: null });
  assert.equal(parsePlace({ cwd: "" }), null);
  assert.equal(parsePlace({ cwd: 3 }), null);
  assert.equal(parsePlace(null), null);
  assert.deepEqual(parsePlace({ cwd: "/w", branch: "b".repeat(4097) }), { cwd: "/w", branch: null });
});

const BASE: Pick<
  ConnectionOptions,
  "session" | "kind" | "url" | "cwdFrom" | "cwd" | "launch" | "plugin" | "target" | "lang" | "resumeAgents" | "size"
> = {
  session: "s1",
  kind: "shell",
  url: null,
  cwdFrom: null,
  cwd: null,
  launch: null,
  plugin: null,
  target: null,
  lang: "en",
  resumeAgents: true,
  size: { cols: 80, rows: 24, width: 800, height: 600 },
};

test("session queries carry a plugin launch", () => {
  const query = sessionQuery({ ...BASE, plugin: { plugin: "docker", action: "logs", target: "0123456789ab" } }, "wtest");
  assert.equal(query.get("plugin"), "docker");
  assert.equal(query.get("action"), "logs");
  assert.equal(query.get("target"), "0123456789ab");
  assert.equal(query.get("agent"), null);
  assert.equal(query.getAll("target").length, 1);
  const plain = sessionQuery(BASE, "wtest");
  assert.equal(plain.get("plugin"), null);
  assert.equal(plain.get("action"), null);
  assert.equal(plain.get("target"), null);
});

test("external launches carry the argv as JSON", () => {
  const launch = externalLaunch("k8s-tools.kubernetes", ["kubectl", "get", "pods"]);
  assert.deepEqual(launch, { plugin: "k8s-tools.kubernetes", action: "run", target: '["kubectl","get","pods"]' });
  assert.equal(sessionQuery({ ...BASE, plugin: launch }, "wtest").get("target"), '["kubectl","get","pods"]');
  assert.equal(isLaunchArgv(["kubectl", "a\u001b"]), false);
  assert.equal(isLaunchArgv(["kubectl", "é".repeat(512)]), true, "1024 bytes");
  assert.equal(isLaunchArgv(["kubectl", "é".repeat(513)]), false);
});

test("launch argv follows the host's rules", () => {
  // The same rules as parse_argv in native/src/plugins/external.rs.
  assert.equal(isLaunchArgv(Array(32).fill("x")), true);
  assert.equal(isLaunchArgv(["kubectl", "pod-\u{1F600}"]), true, "a surrogate pair is one character");
  // serde refuses a lone surrogate in the JSON target, so the page does too.
  assert.equal(isLaunchArgv(["kubectl", "a\ud800"]), false, "lone high surrogate");
  assert.equal(isLaunchArgv(["kubectl", "\udc00b"]), false, "lone low surrogate");
  for (const bad of [[], Array(33).fill("x"), ["/bin/kubectl"], ["-x"], [1], ["kubectl", "a\u0000b"], ["kubectl", "a\u0085"], "kubectl", null]) {
    assert.equal(isLaunchArgv(bad), false, JSON.stringify(bad));
  }
});
