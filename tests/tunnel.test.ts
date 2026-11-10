import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { ActivityBar } from "../src/components/ActivityBar.tsx";
import { TunnelEndScreen, TunnelLogin } from "../src/components/TunnelLogin.tsx";
import { TunnelPanel } from "../src/components/TunnelPanel.tsx";
import { I18nProvider } from "../src/i18n/index.tsx";
import { parseTunnelAuth, parseTunnelStatus, type TunnelStatus } from "../shared/protocol.ts";
import { MESSAGES } from "../src/i18n/messages.ts";
import { tunnelCommands } from "../src/paletteCommands.tsx";
import {
  codeDigits,
  elapsedMinutes,
  gateReducer,
  hoursLeft,
  installHint,
  pageControls,
  tunnelEnded,
  UNREACHABLE_AFTER_MS,
  type Gate,
} from "../src/tunnel.ts";

const t = MESSAGES.en;

// I18nProvider follows the browser's language; pin English for the text below.
Object.defineProperty(navigator, "languages", { value: ["en-US"], configurable: true });

export const ON: TunnelStatus = {
  phase: "on",
  os: "macos",
  url: "https://fake-tunnel.trycloudflare.com",
  code: "482913",
  startedAt: 1_000,
  devices: [{ label: "iPhone · Safari", lastSeen: 2_000, connected: true }],
  idleDeadline: null,
  keepAwake: true,
  reason: null,
  log: [],
  localPort: 50_000,
};

test("code digits", () => {
  assert.equal(codeDigits("482 913"), "482913");
  assert.equal(codeDigits("４８２９１３"), "482913");
  assert.equal(codeDigits("12a3456789"), "123456");
  assert.equal(codeDigits(""), "");
});

test("install hints", () => {
  assert.equal(installHint("macos").command, "brew install cloudflared");
  assert.equal(installHint("windows").command, "winget install --id Cloudflare.cloudflared");
  assert.equal(installHint("linux").command, null);
  for (const os of ["macos", "windows", "linux"] as const) {
    assert.match(installHint(os).link, /^https:\/\/developers\.cloudflare\.com\//);
  }
});

test("time helpers", () => {
  assert.equal(elapsedMinutes(0, 59_999), 0);
  assert.equal(elapsedMinutes(0, 12 * 60_000 + 1), 12);
  assert.equal(hoursLeft(11 * 3_600_000 + 1, 0), 12);
  assert.equal(hoursLeft(3_600_000, 0), 1);
  assert.equal(hoursLeft(0, 5), 0);
});

test("parse status", () => {
  assert.deepEqual(parseTunnelStatus(JSON.parse(JSON.stringify(ON))), ON);
  assert.equal(parseTunnelStatus({ ...ON, phase: "boom" }), null);
  assert.equal(parseTunnelStatus({ ...ON, os: "plan9" }), null);
  assert.equal(parseTunnelStatus(null), null);
  assert.deepEqual(parseTunnelStatus({ ...ON, devices: [{ label: 1 }] })?.devices, []);
  assert.deepEqual(parseTunnelStatus({ ...ON, reason: "tooManyCodes", phase: "failed" })?.reason, "tooManyCodes");
});

test("parse auth", () => {
  assert.deepEqual(parseTunnelAuth({ tunnel: false }), { tunnel: false, signedIn: false, lockedFor: null });
  assert.deepEqual(parseTunnelAuth({ tunnel: true, signedIn: true, lockedFor: null }), {
    tunnel: true,
    signedIn: true,
    lockedFor: null,
  });
  assert.deepEqual(parseTunnelAuth({ tunnel: true, signedIn: false, lockedFor: 30 })?.lockedFor, 30);
  assert.equal(parseTunnelAuth("<html>"), null);
  assert.equal(parseTunnelAuth({}), null);
});

const auth = (signedIn: boolean, lockedFor: number | null = null) =>
  ({ type: "auth", auth: { tunnel: true, signedIn, lockedFor } }) as const;

test("gate: local stays local", () => {
  let gate: Gate = gateReducer({ phase: "checking" }, { type: "auth", auth: { tunnel: false, signedIn: false, lockedFor: null } });
  assert.deepEqual(gate, { phase: "local" });
  gate = gateReducer(gate, { type: "ended" });
  gate = gateReducer(gate, { type: "failed", now: 0 });
  gate = gateReducer(gate, auth(false));
  assert.deepEqual(gate, { phase: "local" });
});

test("gate: login then in", () => {
  let gate: Gate = gateReducer({ phase: "checking" }, auth(false, 30));
  assert.deepEqual(gate, { phase: "login", lockedFor: 30 });
  gate = gateReducer(gate, { type: "failed", now: 0 });
  assert.equal(gate.phase, "login");
  gate = gateReducer(gate, auth(true));
  assert.deepEqual(gate, { phase: "in", failingSince: null });
});

test("gate: in becomes ended on signed out", () => {
  assert.deepEqual(gateReducer({ phase: "in", failingSince: null }, auth(false)), { phase: "ended" });
  assert.deepEqual(gateReducer({ phase: "in", failingSince: null }, { type: "ended" }), { phase: "ended" });
});

test("gate: unreachable after two minutes", () => {
  let gate: Gate = { phase: "in", failingSince: null };
  gate = gateReducer(gate, { type: "failed", now: 0 });
  assert.deepEqual(gate, { phase: "in", failingSince: 0 });
  gate = gateReducer(gate, { type: "failed", now: UNREACHABLE_AFTER_MS - 1 });
  assert.deepEqual(gate, { phase: "in", failingSince: 0 });
  assert.deepEqual(gateReducer(gate, auth(true)), { phase: "in", failingSince: null });
  gate = gateReducer(gate, { type: "failed", now: UNREACHABLE_AFTER_MS });
  assert.deepEqual(gate, { phase: "unreachable" });
  assert.deepEqual(gateReducer({ phase: "checking" }, { type: "failed", now: 0 }), { phase: "unreachable" });
});

test("gate: retry", () => {
  assert.deepEqual(gateReducer({ phase: "unreachable" }, { type: "retry" }), { phase: "checking" });
  assert.deepEqual(gateReducer({ phase: "ended" }, { type: "retry" }), { phase: "checking" });
  assert.deepEqual(gateReducer({ phase: "in", failingSince: null }, { type: "retry" }), { phase: "in", failingSince: null });
});

test("tunnel commands", () => {
  const actions = { start: () => {}, stop: () => {}, copy: () => {}, open: () => {} };
  const ids = (status: TunnelStatus | null) => tunnelCommands(status, actions, t).map((command) => command.id);
  assert.deepEqual(ids({ ...ON, phase: "off", url: null, code: null }), ["tunnel-start"]);
  assert.deepEqual(ids({ ...ON, phase: "failed", url: null, code: null }), ["tunnel-start"]);
  assert.deepEqual(ids(ON), ["tunnel-stop", "tunnel-copy"]);
  assert.deepEqual(ids({ ...ON, phase: "starting", url: null, code: null }), ["tunnel-stop"]);
  assert.deepEqual(ids({ ...ON, phase: "missing", url: null, code: null }), ["tunnel-open"]);
  assert.deepEqual(ids(null), []);
  for (const command of tunnelCommands(ON, actions, t)) {
    assert.match(command.keywords, /^[a-z ]+$/);
  }
  let copied = "";
  tunnelCommands(ON, { ...actions, copy: (url) => (copied = url) }, t)
    .find((command) => command.id === "tunnel-copy")!
    .run();
  assert.equal(copied, ON.url);
});

const NOW = 1_000 + 12 * 60_000;
/** The text as React writes it into markup. */
const shown = (text: string) => text.replace(/&/g, "&amp;").replace(/'/g, "&#x27;").replace(/"/g, "&quot;");
const has = (html: string, text: string) => assert.ok(html.includes(shown(text)), `${text} in ${html}`);
const noop = () => {};

function panel(status: TunnelStatus | null): string {
  return renderToStaticMarkup(
    createElement(I18nProvider, null, createElement(TunnelPanel, { status, now: NOW, onStart: noop, onStop: noop, onCheck: noop })),
  );
}

test("panel off shows start", () => {
  const html = panel({ ...ON, phase: "off", url: null, code: null, devices: [] });
  assert.match(html, />Start tunnel</);
  assert.doesNotMatch(html, /<svg[^>]*viewBox="0 0 \d+ \d+"><rect fill="white"/);
});

test("panel off after idle says why", () => {
  const html = panel({ ...ON, phase: "off", url: null, code: null, devices: [], reason: "idle" });
  has(html, t.tunnel.reasons.idle);
});

test("panel on shows url, qr, code, devices, warning", () => {
  const html = panel(ON);
  assert.match(html, /Connected · 12 min/);
  assert.match(html, /fake-tunnel\.trycloudflare\.com/);
  assert.match(html, /<svg/);
  assert.match(html, />482 913</);
  assert.match(html, /iPhone · Safari/);
  has(html, t.tunnel.warning);
  has(html, t.tunnel.keepAwake);
  assert.match(html, />Stop tunnel</);
  assert.doesNotMatch(html, /stops in/);
});

test("panel on without devices shows auto stop hours", () => {
  const html = panel({ ...ON, devices: [], idleDeadline: NOW + 11 * 3_600_000 - 5, keepAwake: false });
  has(html, t.tunnel.autoStopIn(11));
  has(html, t.tunnel.noKeepAwake);
});

test("panel reconnecting dims url and code", () => {
  const html = panel({ ...ON, phase: "reconnecting" });
  assert.match(html, />Reconnecting</);
  assert.match(html, /class="[^"]*\bdim\b[^"]*"[^>]*>[\s\S]*fake-tunnel/);
  assert.match(html, />Stop tunnel</);
});

test("panel missing shows the install command for the os", () => {
  const html = panel({ ...ON, phase: "missing", os: "windows", url: null, code: null, devices: [] });
  assert.match(html, /winget install --id Cloudflare\.cloudflared/);
  assert.match(html, />Check again</);
  const linux = panel({ ...ON, phase: "missing", os: "linux", url: null, code: null, devices: [] });
  assert.match(linux, /href="https:\/\/developers\.cloudflare\.com\//);
  assert.doesNotMatch(linux, /brew install/);
});

test("panel failed shows reason and log", () => {
  const html = panel({ ...ON, phase: "failed", url: null, code: null, devices: [], reason: "tooManyCodes", log: ["ERR boom"] });
  has(html, t.tunnel.reasons.tooManyCodes);
  assert.match(html, /ERR boom/);
  assert.match(html, />Start again</);
});

function bar(change: Record<string, unknown>): string {
  return renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(ActivityBar, {
        sidebarOpen: false,
        counts: { shell: 0, browser: 0, editor: 0, ssh: 0, docs: 0 },
        onLaunch: noop,
        onToggleSidebar: null,
        onOpenSettings: noop,
        onOpenPalette: noop,
        onOpenRemoteWindow: null,
        paletteShortcut: "⌘K",
        agentsOpen: false,
        agentsWaiting: false,
        onToggleAgents: noop,
        gitOpen: false,
        onToggleGit: noop,
        tunnelOpen: false,
        tunnelOn: false,
        onToggleTunnel: null,
        plugins: [],
        onTogglePlugin: noop,
        onOpenPlugins: null,
        ...change,
      }),
    ),
  );
}

test("the activity bar shows the tunnel button only when it can", () => {
  assert.doesNotMatch(bar({}), /aria-label="Tunnel/);
  const on = bar({ onToggleTunnel: noop, tunnelOn: true, tunnelOpen: true });
  assert.match(on, /aria-label="Tunnel/);
  assert.match(on, /activity-dot on/);
  assert.match(on, /aria-pressed="true"/);
  assert.doesNotMatch(bar({ onToggleTunnel: noop }), /activity-dot on/);
});

test("a panel clock from before the tunnel started still shows 12 hours", () => {
  // The panel opened (and took its clock) before the tunnel started.
  const startedAt = NOW + 20_000;
  const html = renderToStaticMarkup(
    createElement(
      I18nProvider,
      null,
      createElement(TunnelPanel, {
        status: { ...ON, startedAt, devices: [], idleDeadline: startedAt + 12 * 3_600_000 },
        now: NOW,
        onStart: noop,
        onStop: noop,
        onCheck: noop,
      }),
    ),
  );
  has(html, t.tunnel.autoStopIn(12));
  has(html, t.tunnel.connectedFor(0));
});

const render = (element: ReturnType<typeof createElement>) => renderToStaticMarkup(createElement(I18nProvider, null, element));

test("login renders a numeric one-time-code input", async () => {
  const html = render(createElement(TunnelLogin, { lockedFor: null, onSignedIn: noop }));
  assert.match(html, /inputMode="numeric"|inputmode="numeric"/);
  assert.match(html, /autoComplete="one-time-code"|autocomplete="one-time-code"/);
  has(html, t.tunnelPage.connectTo);
  const { hostname, userInfo } = await import("node:os");
  assert.ok(!html.includes(hostname()), "no computer name before the code");
  assert.ok(!html.includes(userInfo().username), "no user name before the code");
});

test("login shows the lock countdown", () => {
  const html = render(createElement(TunnelLogin, { lockedFor: 30, onSignedIn: noop }));
  has(html, t.tunnelPage.lockedFor(30));
  assert.match(html, /<input[^>]*disabled=""/);
});

test("ended and unreachable screens", () => {
  const ended = render(createElement(TunnelEndScreen, { kind: "ended", onRetry: noop }));
  has(ended, t.tunnelPage.ended);
  assert.doesNotMatch(ended, /<button/, "an ended tunnel does not reconnect");
  const unreachable = render(createElement(TunnelEndScreen, { kind: "unreachable", onRetry: noop }));
  has(unreachable, t.tunnelPage.unreachable);
  has(unreachable, t.tunnelPage.retry);
});

test("a tunnel-ended close code is told apart", () => {
  assert.equal(tunnelEnded(4010), true);
  for (const code of [1000, 1006, 1011, 4001]) assert.equal(tunnelEnded(code), false, String(code));
});

test("the tunnel page hides local-only controls", () => {
  assert.deepEqual(pageControls(false, false), { updates: true, remoteWindows: true, tunnel: true, plugins: true });
  assert.deepEqual(pageControls(true, false), { updates: false, remoteWindows: false, tunnel: false, plugins: false });
  assert.deepEqual(pageControls(false, true), { updates: false, remoteWindows: false, tunnel: false, plugins: false });
  const html = bar({ onOpenRemoteWindow: null, onToggleTunnel: null, onOpenPlugins: null });
  assert.doesNotMatch(html, /aria-label="(Tunnel|Remote window|Plugins)/);
});

test("a pasted code with a space is not cut short by the input", () => {
  // Browsers cut a paste to maxlength before onChange: "482 913" must fit whole.
  const html = render(createElement(TunnelLogin, { lockedFor: null, onSignedIn: noop }));
  const max = /maxLength="(\d+)"|maxlength="(\d+)"/.exec(html);
  const limit = max ? Number(max[1] ?? max[2]) : Infinity;
  assert.ok(limit >= "４８２ ９１３".length * 2, `maxlength ${limit} cuts a pasted "482 913"`);
});
