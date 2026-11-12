// An external plugin as a WestermPlugin: its text from the server's info, and
// an instance that runs the plugin's panel page in a sandboxed iframe behind
// the message bridge. The panel component draws only a slot; the iframe lives
// in the frame layer and is laid over it (frame.ts).

import { useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { Ellipsis, Puzzle } from "lucide-react";
import { useI18n } from "../../i18n/index.tsx";
import { HTML_LANG, LOCALES, MESSAGES, type Locale } from "../../i18n/messages.ts";
import type { PluginContext, PluginInstance, PluginStatus, PluginText, WestermPlugin } from "../types.ts";
import { createBridge, type Bridge } from "./bridge.ts";
import { createFrame, type Frame } from "./frame.ts";
import { assetUrl, localText, permissionLines, type ExternalInfo } from "./info.ts";

/** The CSS custom properties a plugin's page gets, so it can look like westerm. */
const THEME_KEYS = [
  "--bg",
  "--surface",
  "--surface-2",
  "--border",
  "--text",
  "--text-muted",
  "--text-faint",
  "--accent",
  "--ok",
  "--warn",
  "--danger",
] as const;

export interface Theme {
  vars: Record<string, string>;
  font: string;
}

/** What a running plugin needs from the page around it. */
export interface FrameEnvironment {
  /** The frame layer every plugin iframe lives in. */
  host(): HTMLElement;
  locale(): Locale;
  theme(): Theme;
  /** Called when the locale or the theme may have changed. */
  subscribe(listener: () => void): () => void;
}

/** The environment of this page: a frame layer under <body>, and <html>'s language and colors. */
export function documentEnvironment(): FrameEnvironment {
  return {
    host() {
      let layer = document.querySelector<HTMLElement>("body > .plugin-frames");
      if (layer === null) {
        layer = document.createElement("div");
        layer.className = "plugin-frames";
        document.body.appendChild(layer);
      }
      return layer;
    },
    locale() {
      const lang = document.documentElement.lang;
      return LOCALES.find((locale) => HTML_LANG[locale] === lang) ?? "en";
    },
    theme() {
      const root = getComputedStyle(document.documentElement);
      return {
        vars: Object.fromEntries(THEME_KEYS.map((key) => [key, root.getPropertyValue(key).trim()])),
        font: getComputedStyle(document.body).fontFamily,
      };
    },
    subscribe(listener) {
      // The language and the color theme are set on <html>; the "system"
      // mode follows the OS without touching it.
      const observer = new MutationObserver(() => listener());
      observer.observe(document.documentElement, {
        attributes: true,
        attributeFilter: ["lang", "style", "class", "data-theme"],
      });
      const media = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: light)") : null;
      media?.addEventListener("change", listener);
      return () => {
        observer.disconnect();
        media?.removeEventListener("change", listener);
      };
    },
  };
}

const QUIET: PluginStatus = { badge: null, alert: false };

/**
 * A plugin's own status changes reach the plugin bar at most this often. Each
 * message is its own task, so a microtask would not gather a flood of them.
 */
const STATUS_DELAY = 100;

const sameStatus = (a: PluginStatus, b: PluginStatus) => a.badge === b.badge && a.alert === b.alert && a.note === b.note;

/** The iframe while it runs, with what it was last sent. */
interface Live {
  frame: Frame;
  bridge: Bridge;
  stopListening(): void;
  /** "ready" was sent; theme, locale, and panel events follow it. */
  loaded: boolean;
  theme: string;
  locale: Locale;
}

function start(info: ExternalInfo, env: FrameEnvironment, ctx: PluginContext, shellPanes: () => boolean): PluginInstance {
  const listeners = new Set<() => void>();
  let status = QUIET;
  let open = false;
  // The panel's slot while the panel component is mounted.
  let slot: HTMLElement | null = null;
  let live: Live | null = null;

  const emit = () => {
    for (const listener of [...listeners]) listener();
  };
  // What the listeners last heard, and the timer of a status change from the plugin.
  let shown = status;
  let statusTimer: ReturnType<typeof setTimeout> | null = null;
  const publish = () => {
    if (statusTimer !== null) clearTimeout(statusTimer);
    statusTimer = null;
    if (sameStatus(status, shown)) {
      // Back where it was: keep the object the listeners have.
      status = shown;
      return;
    }
    shown = status;
    emit();
  };
  /** westerm's own changes show at once; the plugin's (`soon`) are gathered. */
  const setStatus = (next: PluginStatus, soon = false) => {
    if (sameStatus(next, status)) return;
    status = next;
    if (!soon) publish();
    else statusTimer ??= setTimeout(publish, STATUS_DELAY);
  };
  const stoppedNote = () => MESSAGES[env.locale()].plugins.stopped;

  /**
   * Ends the iframe and the bridge. A page that navigated away is removed
   * first, so the bridge's last answers never reach it.
   */
  const halt = (navigated: boolean) => {
    if (live === null) return;
    const { frame, bridge, stopListening } = live;
    live = null;
    stopListening();
    if (navigated) {
      frame.remove();
      bridge.stop();
    } else {
      bridge.stop();
      frame.remove();
    }
  };

  const launch = () => {
    if (live !== null) return;
    const frame = createFrame({
      src: assetUrl(info, info.panel),
      title: localText(info, env.locale()).name,
      host: env.host(),
      onFirstLoad: () => {
        const theme = env.theme();
        const locale = env.locale();
        Object.assign(current, { loaded: true, theme: JSON.stringify(theme), locale });
        bridge.send("ready", { id: info.id, version: info.version, locale, theme, permissions: info.granted, panelOpen: open });
      },
      onNavigate: () => {
        halt(true);
        setStatus({ ...QUIET, note: stoppedNote() });
      },
    });
    const bridge = createBridge({
      id: info.id,
      granted: info.granted,
      ctx,
      isSource: (source) => source != null && source === frame.element.contentWindow,
      post: (text) => frame.element.contentWindow?.postMessage(text, "*"),
      setStatus: ({ badge, alert }) => setStatus({ badge, alert }, true),
      gone: () => {
        // Removed in another window: stop quietly and read the installed list again.
        halt(false);
        ctx.refreshInstalled();
      },
      shellPanes,
    });
    const onMessage = (event: MessageEvent) => bridge.receive(event.source, event.data);
    window.addEventListener("message", onMessage);
    const current: Live = {
      frame,
      bridge,
      stopListening: () => window.removeEventListener("message", onMessage),
      loaded: false,
      theme: "",
      locale: env.locale(),
    };
    live = current;
    frame.place(open ? slot : null);
  };

  const sendPanel = () => {
    if (live?.loaded) live.bridge.send("panel", { open });
  };

  // Theme and locale changes go to a loaded page; a stop note follows the language.
  const stopWatching = env.subscribe(() => {
    if (status.note !== undefined) setStatus({ ...status, note: stoppedNote() });
    if (!live?.loaded) return;
    const theme = env.theme();
    const themeText = JSON.stringify(theme);
    if (themeText !== live.theme) {
      live.theme = themeText;
      live.bridge.send("theme", theme);
    }
    const locale = env.locale();
    if (locale !== live.locale) {
      live.locale = locale;
      live.bridge.send("locale", { locale });
    }
  });

  const subscribe = (listener: () => void) => {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  };
  const snapshot = () => status;
  const attach = (next: HTMLElement | null) => {
    slot = next;
    live?.frame.place(open ? slot : null);
  };

  // A background plugin runs from the start; the others only while the panel is open.
  if (info.granted.background) launch();

  function Panel() {
    const { t, locale } = useI18n();
    const current = useSyncExternalStore(subscribe, snapshot, snapshot);
    const slotRef = useRef<HTMLDivElement>(null);
    // Mounting moves only the slot; the iframe stays where it is.
    useLayoutEffect(() => {
      attach(slotRef.current);
      return () => attach(null);
    }, []);
    const { name } = localText(info, locale);
    const iconUrl = info.icon === null ? null : assetUrl(info, info.icon);
    return (
      <aside className="session-panel plugin-panel" aria-label={name}>
        <header className="plugin-panel-header">
          {iconUrl === null ? (
            <Puzzle size={15} className="plugin-panel-icon" aria-hidden="true" />
          ) : (
            <img className="plugin-panel-icon" src={iconUrl} alt="" width="15" height="15" />
          )}
          <span className="plugin-panel-title">{name}</span>
          <span className="plugin-external-tag">{t.plugins.external}</span>
          <span className="plugin-panel-version">{`v${info.version}`}</span>
          <button
            type="button"
            className="icon-button plugin-panel-more"
            aria-label={t.plugins.manage}
            title={t.plugins.manage}
            onClick={() => ctx.openManager()}
          >
            <Ellipsis size={15} />
          </button>
        </header>
        <div className="plugin-frame-slot" ref={slotRef}>
          {current.note !== undefined && <p className="plugin-stopped">{current.note}</p>}
        </div>
      </aside>
    );
  }

  return {
    subscribe,
    status: snapshot,
    setPanelOpen(next) {
      if (next === open) return;
      open = next;
      if (open) {
        // A stopped plugin starts afresh when its panel opens again.
        if (status.note !== undefined) setStatus(QUIET);
        if (live === null) launch();
        else live.frame.place(slot);
        sendPanel();
      } else if (info.granted.background) {
        live?.frame.place(null);
        sendPanel();
      } else {
        halt(false);
      }
    },
    dispose() {
      halt(false);
      stopWatching();
      if (statusTimer !== null) clearTimeout(statusTimer);
      listeners.clear();
    },
    Panel,
  };
}

/**
 * The WestermPlugin for an installed external plugin. `shellPanes` tells
 * whether the server can open plugin panes now (see BridgeOptions); without
 * it, `openShell` fails.
 */
export function externalPlugin(
  info: ExternalInfo,
  env: FrameEnvironment = documentEnvironment(),
  shellPanes: () => boolean = () => false,
): WestermPlugin {
  const text = Object.fromEntries(
    LOCALES.map((locale): [Locale, PluginText] => [
      locale,
      { ...localText(info, locale), permissions: permissionLines(info.granted, MESSAGES[locale]) },
    ]),
  ) as Record<Locale, PluginText>;
  return {
    id: info.id,
    version: info.version,
    icon: Puzzle,
    ...(info.icon === null ? {} : { iconUrl: assetUrl(info, info.icon) }),
    text,
    external: { source: info.source, granted: info.granted, installedAt: info.installedAt },
    start: (ctx) => start(info, env, ctx, shellPanes),
  };
}
