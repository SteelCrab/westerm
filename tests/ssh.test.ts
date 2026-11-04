import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizeSshTarget } from "../shared/protocol.ts";
import { parseSavedWorkspace } from "../shared/workspace-file.ts";

test("ssh destinations reject options and shell syntax", () => {
  for (const ok of ["prod", "deploy@10.0.0.5", "me@build-box.local", "dev_1", "u@[::1]"]) {
    assert.equal(normalizeSshTarget(ok), ok, ok);
  }
  for (const bad of ["-oProxyCommand=evil", "a b", "host;rm", "$(id)", "me@-x", "", "x".repeat(300), null]) {
    assert.equal(normalizeSshTarget(bad), null, String(bad));
  }
});

test("saved ssh sessions need a valid destination", () => {
  const layout = (session: Record<string, unknown>) =>
    parseSavedWorkspace({
      version: 1,
      activeTabId: "t1",
      tabs: [{ id: "t1", name: null, paneIds: ["sa"], focusedId: "sa", maximizedId: null }],
      sessions: [{ id: "sa", kind: "ssh", title: "ssh prod", name: null, url: null, ...session }],
    });
  assert.equal(layout({ target: "prod" })?.sessions[0].target, "prod");
  assert.equal(layout({ target: "-oProxyCommand=x" }), null);
  assert.equal(layout({}), null, "an ssh session without a destination is rejected");
});
