/*
 * westerm plugin SDK, v1. Served at /plugin-sdk/v1.js; a panel loads it with
 * <script src="/plugin-sdk/v1.js"></script> and then uses `window.westerm`.
 *
 * The panel runs in <iframe sandbox="allow-scripts">, an opaque origin. It
 * talks to the westerm page only through postMessage. The SDK is a thin
 * wrapper over the messages below; a panel may also send them itself.
 *
 * Every message is a JSON *string* (not an object) with "westerm": 1.
 *
 * Request, panel -> westerm (sent with window.parent.postMessage(text, "*")):
 *   {"westerm":1,"id":<integer>,"call":<name>,"args":{...}}
 *   `id` is unique per frame (the SDK counts from 1). Calls and their args:
 *     "run"          {program: string, args: string[], cwd: string|null}
 *                    -> {code: number, stdout: string, stderr: string}
 *     "openShell"    {program: string, args: string[], cwd: string|null,
 *                     name: string|null, reuse: boolean}        -> null
 *     "openBrowser"  {url: string}                              -> null
 *     "storage.get"  {}                                         -> any JSON (null before the first set)
 *     "storage.set"  {value: any JSON}                          -> null
 *     "setStatus"    {badge: number|null, alert: boolean}       -> null
 *   Calls may be sent before "ready".
 *
 * Reply, westerm -> panel, one per request, with the same id:
 *   {"westerm":1,"id":<integer>,"ok":true,"result":<value>}
 *   {"westerm":1,"id":<integer>,"ok":false,"error":{"code":<code>,"message":string}}
 *   Codes: "permission" (not granted), "invalid" (bad call or args),
 *   "failed" (the work failed), "gone" (the plugin was removed or stopped).
 *   The SDK rejects with an Error whose message is `message` and whose
 *   `.code` is `code` ("failed" when the reply has no usable error).
 *
 * Event, westerm -> panel:
 *   {"westerm":1,"event":<name>,"data":<value>}
 *     "ready"   {id, version, locale, theme: {vars, font}, permissions: {run, panes, background}, panelOpen}
 *               Sent once, after the frame loads.
 *     "theme"   {vars: Record<string, string>, font: string}
 *               `vars` are CSS custom properties ("--bg", "--text", ...).
 *     "locale"  {locale: string}
 *     "panel"   {open: boolean}
 *
 * The SDK reads a message only when `event.source === window.parent` and
 * `event.data` is a string; anything else is ignored. On "ready" and "theme"
 * it sets each `--*` variable on <html> and `--font` to the font; on "ready"
 * and "locale" it sets <html lang>. It adds its base styles once, as
 * <style data-westerm> first in <head>, so the panel's own styles win.
 *
 * API:
 *   westerm.run(program, args, { cwd } = {})
 *   westerm.openShell({ program, args, cwd, name, reuse })
 *   westerm.openBrowser(url)
 *   westerm.storage.get(), westerm.storage.set(value)
 *   westerm.setStatus({ badge, alert })
 *   westerm.on(event, listener) -> unsubscribe function
 *   westerm.ready -> Promise of the "ready" data
 */
(() => {
  "use strict";
  // Loaded already: only the SDK's own property counts, the fixed one it
  // defines at the end. DOM named access makes <div id="westerm"> show up as
  // `window.westerm` too (in browsers on the prototype chain, in some DOMs as
  // a plain own property), and that must not stop the SDK.
  const own = Object.getOwnPropertyDescriptor(window, "westerm");
  if (own && !own.writable && !own.configurable) return;

  const VERSION = 1;
  const host = window.parent;
  const pending = new Map();
  const listeners = new Map();
  let nextId = 1;
  let markReady;
  const ready = new Promise((resolve) => {
    markReady = resolve;
  });

  const BASE_CSS = `
body { margin: 0; padding: 8px; background: var(--surface, #14171c); color: var(--text, #d7dce3);
  font: 13px/1.45 var(--font, system-ui, sans-serif); }
.w-row { display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 8px;
  border-bottom: 1px solid var(--border, #252a32); }
.w-button { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border: 1px solid var(--border, #252a32);
  border-radius: 6px; background: var(--surface-2, #1a1e24); color: var(--text, #d7dce3); font: inherit; cursor: pointer; }
.w-button:hover { border-color: var(--accent, #6aa9ff); }
.w-button:focus-visible { outline: 2px solid var(--accent, #6aa9ff); outline-offset: 1px; }
.w-button:disabled { opacity: 0.5; cursor: default; }
.w-dot { display: inline-block; flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--text-faint, #5d6672); }
.w-dot.ok { background: var(--ok, #4fc27a); }
.w-dot.warn { background: var(--warn, #e5b454); }
.w-dot.danger { background: var(--danger, #ef6b6b); }
.w-empty { padding: 24px 12px; text-align: center; color: var(--text-muted, #8b94a1); }
`;

  function addBaseStyles() {
    if (document.querySelector("style[data-westerm]")) return;
    const style = document.createElement("style");
    style.setAttribute("data-westerm", "");
    style.textContent = BASE_CSS;
    const head = document.head || document.documentElement;
    head.insertBefore(style, head.firstChild);
  }

  function applyTheme(theme) {
    if (!theme || typeof theme !== "object") return;
    const style = document.documentElement.style;
    const vars = theme.vars && typeof theme.vars === "object" ? theme.vars : {};
    for (const [name, value] of Object.entries(vars)) {
      if (name.startsWith("--") && typeof value === "string") style.setProperty(name, value);
    }
    if (typeof theme.font === "string") style.setProperty("--font", theme.font);
  }

  function applyLocale(locale) {
    if (typeof locale === "string") document.documentElement.lang = locale;
  }

  function emit(name, data) {
    for (const listener of listeners.get(name) || []) {
      try {
        listener(data);
      } catch (error) {
        // One broken listener must not stop the others; report it later.
        setTimeout(() => {
          throw error;
        });
      }
    }
  }

  function onEvent(name, data) {
    if (name === "ready" && data && typeof data === "object") {
      applyTheme(data.theme);
      applyLocale(data.locale);
      markReady(data);
    } else if (name === "theme") {
      applyTheme(data);
    } else if (name === "locale" && data && typeof data === "object") {
      applyLocale(data.locale);
    }
    emit(name, data);
  }

  function onReply(message) {
    const waiting = pending.get(message.id);
    if (!waiting) return;
    pending.delete(message.id);
    if (message.ok === true) {
      waiting.resolve(message.result === undefined ? null : message.result);
      return;
    }
    const failure = message.error && typeof message.error === "object" ? message.error : {};
    const error = new Error(typeof failure.message === "string" ? failure.message : "The call failed");
    error.code = typeof failure.code === "string" ? failure.code : "failed";
    waiting.reject(error);
  }

  window.addEventListener("message", (event) => {
    if (event.source !== host || typeof event.data !== "string") return;
    let message;
    try {
      message = JSON.parse(event.data);
    } catch {
      return;
    }
    if (!message || typeof message !== "object" || message.westerm !== VERSION) return;
    if (typeof message.event === "string") onEvent(message.event, message.data);
    else if (Number.isInteger(message.id)) onReply(message);
  });

  function call(name, args) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      host.postMessage(JSON.stringify({ westerm: VERSION, id, call: name, args }), "*");
    });
  }

  const orNull = (value) => (value === undefined ? null : value);

  const westerm = Object.freeze({
    run: (program, args = [], { cwd } = {}) => call("run", { program, args, cwd: orNull(cwd) }),
    openShell: ({ program, args = [], cwd, name, reuse = false } = {}) =>
      call("openShell", { program, args, cwd: orNull(cwd), name: orNull(name), reuse: Boolean(reuse) }),
    openBrowser: (url) => call("openBrowser", { url }),
    storage: Object.freeze({
      get: () => call("storage.get", {}),
      set: (value) => call("storage.set", { value: orNull(value) }),
    }),
    setStatus: ({ badge = null, alert = false } = {}) => call("setStatus", { badge: orNull(badge), alert: Boolean(alert) }),
    on(name, listener) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(listener);
      return () => {
        listeners.get(name).delete(listener);
      };
    },
    ready,
  });

  addBaseStyles();
  Object.defineProperty(window, "westerm", { value: westerm, enumerable: true });
})();
