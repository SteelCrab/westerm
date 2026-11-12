// The message bridge between an external plugin's iframe and westerm. It
// speaks the protocol in the header of native/src/server/plugins/external/
// sdk_v1.js: JSON strings, one reply per call, events from westerm. It checks
// every call against the granted permissions before it reaches the server,
// which checks them again.

import { externalLaunch, isLaunchArgv } from "../../../shared/protocol.ts";
import type { PluginContext, PluginStatus } from "../types.ts";
import type { ExternalPermissions } from "./info.ts";

/** A message from the plugin larger than this (UTF-8) is dropped. Replies may be larger. */
export const MAX_MESSAGE_BYTES = 262_144;

const VERSION = 1;
/** Longest pane name a plugin may give, in characters. */
const MAX_NAME = 80;
const MAX_BADGE = 999;
/**
 * Server requests one frame may have in flight. More would queue behind the
 * browser's few connections per host and delay westerm's own requests.
 */
const MAX_IN_FLIGHT = 8;

type Code = "permission" | "invalid" | "failed" | "gone";

/** A call's answer: a result, or an error the SDK turns into a rejected promise. */
type Outcome = { ok: true; result: unknown } | { ok: false; code: Code; message: string };

const done = (result: unknown = null): Outcome => ({ ok: true, result });
const fail = (code: Code, message: string): Outcome => ({ ok: false, code, message });

export interface BridgeOptions {
  /** The plugin's id, for the session launch of `openShell`. */
  id: string;
  granted: ExternalPermissions;
  ctx: PluginContext;
  /** Whether a message's `event.source` is this plugin's iframe window. */
  isSource(source: unknown): boolean;
  /** Sends a JSON string to the iframe. */
  post(text: string): void;
  setStatus(status: PluginStatus): void;
  /** The server no longer knows the plugin (404): stop it and read the installed list again. */
  gone(): void;
  /**
   * Whether the server can open plugin panes: macOS or Linux with a login
   * shell westerm can quote for (`shellPanes` of `GET /api/plugins`).
   */
  shellPanes(): boolean;
}

export interface Bridge {
  /** One `message` event: its source and data. */
  receive(source: unknown, data: unknown): void;
  send(event: string, data: unknown): void;
  /** Answers every pending call with "gone", then sends nothing more. */
  stop(): void;
}

type Args = Record<string, unknown>;

const isArgs = (value: unknown): value is Args => typeof value === "object" && value !== null && !Array.isArray(value);
const isStrings = (value: unknown): value is string[] => Array.isArray(value) && value.every((each) => typeof each === "string");
const isCwd = (value: unknown): value is string | null | undefined => value == null || typeof value === "string";

const encoder = new TextEncoder();

/** Whether `text` fits in MAX_MESSAGE_BYTES of UTF-8, without encoding it when the length decides. */
function fits(text: string): boolean {
  if (text.length > MAX_MESSAGE_BYTES) return false;
  if (text.length * 3 <= MAX_MESSAGE_BYTES) return true;
  return encoder.encode(text).length <= MAX_MESSAGE_BYTES;
}

/** The server's one-line reason (`{"error": …}`), or the status. */
async function reason(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body?.error === "string") return body.error;
  } catch {
    // No JSON body: fall back to the status.
  }
  return `The request failed (${response.status})`;
}

export function createBridge(o: BridgeOptions): Bridge {
  const { granted, ctx } = o;
  // Calls without an answer yet; each object is one call, even if ids repeat.
  const pending = new Set<{ id: number }>();
  let stopped = false;
  let inFlight = 0;

  const post = (message: object) => o.post(JSON.stringify({ westerm: VERSION, ...message }));

  const answer = (call: { id: number }, outcome: Outcome) => {
    if (stopped || !pending.delete(call)) return;
    post(
      outcome.ok
        ? { id: call.id, ok: true, result: outcome.result }
        : { id: call.id, ok: false, error: { code: outcome.code, message: outcome.message } },
    );
  };

  /** A server answer that is not the success status: 404 means the plugin is gone. */
  const refused = async (response: Response): Promise<Outcome> => {
    if (response.status === 404) return fail("gone", "The plugin was removed");
    return fail("failed", await reason(response));
  };

  /**
   * Runs `work`, which asks the server, in one of the frame's MAX_IN_FLIGHT
   * slots; with none free the call fails at once.
   */
  const viaServer = (work: () => Promise<Outcome>): Outcome | Promise<Outcome> => {
    if (inFlight >= MAX_IN_FLIGHT) return fail("failed", "Too many calls at once");
    inFlight += 1;
    let promise: Promise<Outcome>;
    try {
      promise = work();
    } catch (error) {
      inFlight -= 1;
      throw error;
    }
    return promise.finally(() => {
      inFlight -= 1;
    });
  };

  const send = (path: string, method: string, body: string) =>
    ctx.fetch(path, { method, headers: { "Content-Type": "application/json" }, body });

  const notPermitted = (program: string) => fail("permission", `${program} is not permitted`);

  // Checks answer at once; only the work that asks the server is async.
  function run(args: Args): Outcome | Promise<Outcome> {
    const { program, cwd } = args;
    if (typeof program !== "string" || !isStrings(args.args) || !isCwd(cwd)) return fail("invalid", "run takes program, args, and cwd");
    if (!granted.run.includes(program)) return notPermitted(program);
    return viaServer(() =>
      send("/run", "POST", JSON.stringify({ program, args: args.args, cwd: cwd ?? null })).then(async (response) =>
        response.ok ? done(await response.json()) : refused(response),
      ),
    );
  }

  function openShell(args: Args): Outcome {
    const { program, cwd, name, reuse } = args;
    if (typeof program !== "string" || !isStrings(args.args)) return fail("invalid", "openShell takes program and args");
    if (!granted.panes) return fail("permission", "opening panes is not permitted");
    if (!granted.run.includes(program)) return notPermitted(program);
    const argv = [program, ...args.args];
    if (!isLaunchArgv(argv)) return fail("invalid", "the program and arguments cannot be typed into a shell");
    if (!isCwd(cwd)) return fail("invalid", "cwd must be a string or null");
    if (name != null && (typeof name !== "string" || name.length < 1 || [...name].length > MAX_NAME)) {
      return fail("invalid", `name must be 1 to ${MAX_NAME} characters`);
    }
    if (reuse !== undefined && typeof reuse !== "boolean") return fail("invalid", "reuse must be true or false");
    // The host would refuse the session and leave an empty pane behind.
    if (!o.shellPanes()) return fail("failed", "Your shell is not supported for plugin panes");
    const { action, target } = externalLaunch(o.id, argv);
    ctx.openShell({ action, target, cwd: cwd ?? null, name: name ?? program, reuse: reuse ?? false });
    return done();
  }

  function openBrowser(args: Args): Outcome | Promise<Outcome> {
    const { url } = args;
    if (typeof url !== "string") return fail("invalid", "openBrowser takes a url");
    if (!granted.panes) return fail("permission", "opening panes is not permitted");
    if (!/^https?:\/\//i.test(url)) return fail("invalid", "only http and https addresses open");
    return viaServer(() =>
      send("/browser", "POST", JSON.stringify({ url })).then((response) => {
        // Exactly the address the server checked opens, and only on its 204.
        if (response.status === 204) {
          ctx.openBrowser(url);
          return done();
        }
        return response.ok ? fail("failed", "The address was not approved") : refused(response);
      }),
    );
  }

  function storageGet(): Outcome | Promise<Outcome> {
    return viaServer(async () => {
      const response = await ctx.fetch("/storage");
      return response.ok ? done(await response.json()) : refused(response);
    });
  }

  function storageSet(args: Args): Outcome | Promise<Outcome> {
    return viaServer(async () => {
      const response = await send("/storage", "PUT", JSON.stringify(args.value ?? null));
      return response.ok ? done() : refused(response);
    });
  }

  function setStatus(args: Args): Outcome {
    const { badge, alert } = args;
    const badgeOk = badge === null || (Number.isInteger(badge) && (badge as number) >= 0 && (badge as number) <= MAX_BADGE);
    if (!badgeOk || typeof alert !== "boolean") return fail("invalid", `badge is a whole number from 0 to ${MAX_BADGE} or null`);
    o.setStatus({ badge: badge as number | null, alert });
    return done();
  }

  function handle(call: string, args: Args): Outcome | Promise<Outcome> {
    switch (call) {
      case "run":
        return run(args);
      case "openShell":
        return openShell(args);
      case "openBrowser":
        return openBrowser(args);
      case "storage.get":
        return storageGet();
      case "storage.set":
        return storageSet(args);
      case "setStatus":
        return setStatus(args);
      default:
        return fail("invalid", "unknown call");
    }
  }

  return {
    receive(source, data) {
      if (stopped || typeof data !== "string" || !o.isSource(source) || !fits(data)) return;
      let message: unknown;
      try {
        message = JSON.parse(data);
      } catch {
        return;
      }
      if (!isArgs(message) || message.westerm !== VERSION || !Number.isInteger(message.id)) return;
      const call = { id: message.id as number };
      pending.add(call);
      if (typeof message.call !== "string" || !isArgs(message.args)) {
        answer(call, fail("invalid", "a call needs a name and args"));
        return;
      }
      let outcome: Outcome | Promise<Outcome>;
      try {
        outcome = handle(message.call, message.args);
      } catch (error) {
        // Every call gets an answer, even when westerm itself throws.
        outcome = fail("failed", error instanceof Error ? error.message : "The call failed");
      }
      if (!(outcome instanceof Promise)) {
        answer(call, outcome);
        return;
      }
      outcome
        .catch((error: unknown) => fail("failed", error instanceof Error ? error.message : "The call failed"))
        .then((result) => {
          // A bridge stopped meanwhile has answered "gone" already and is told nothing more.
          const live = !stopped;
          answer(call, result);
          if (live && !result.ok && result.code === "gone") o.gone();
        });
    },
    send(event, data) {
      if (!stopped) post({ event, data });
    },
    stop() {
      if (stopped) return;
      for (const call of [...pending]) answer(call, fail("gone", "The plugin stopped"));
      stopped = true;
    },
  };
}
