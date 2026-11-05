import {
  ArrowLeftRight,
  ArrowRightToLine,
  MoveLeft,
  MoveRight,
  CircleArrowUp,
  Globe,
  Languages,
  Layers,
  Maximize2,
  Minus,
  PanelLeft,
  Pencil,
  Plus,
  Settings,
  RotateCcw,
  SquarePlus,
  X,
} from "lucide-react";
import type { SessionKind } from "../shared/protocol.ts";
import type { RenameTarget } from "./components/InlineRename.tsx";
import type { PaletteCommand } from "./components/CommandPalette.tsx";
import { LOCALE_NAMES, LOCALES, type Locale, type Messages } from "./i18n/messages.ts";
import { KindIcon, LAUNCHERS } from "./kinds.tsx";
import { normalizeBrowserUrl, normalizeSshTarget, REMOTE_UPDATE_HOST_PATH } from "../shared/protocol.ts";
import { postRemote } from "./remoteApi.ts";
import type { SshHost } from "./useSshHosts.ts";
import { shortcutLabel, type SwitchCommand } from "./shortcuts.ts";
import { appBridge } from "./useUpdate.ts";
import {
  activeTab,
  activeWorkspace,
  sessionLabel,
  sessionTab,
  stepPane,
  tabLabel,
  visibleTabs,
  workspaceLabel,
  workspaceTabs,
  type Dashboard,
  type DashboardAction,
  type Tab,
  type Workspace,
} from "./workspace.ts";

export const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
export const PALETTE_SHORTCUT = IS_MAC ? "⌘K" : "Ctrl+Shift+K";

/** Cmd+K on macOS; Ctrl+Shift+K elsewhere, since Ctrl+K belongs to the shell. */
export function isPaletteShortcut(event: KeyboardEvent): boolean {
  if (event.code !== "KeyK" || event.altKey) return false;
  return IS_MAC
    ? event.metaKey && !event.ctrlKey && !event.shiftKey
    : event.ctrlKey && event.shiftKey && !event.metaKey;
}

export interface PaletteContext {
  state: Dashboard;
  dispatch: (action: DashboardAction) => void;
  open: (kind: SessionKind, url?: string | null, target?: string | null) => void;
  newTab: () => void;
  newTabId: () => string;
  movePaneToNewTab: (paneId: string) => void;
  rename: (target: RenameTarget) => void;
  newWorkspace: () => void;
  /** Opens the sidebar and edits the current workspace's name there. */
  renameWorkspace: () => void;
  closeWorkspace: (id: string) => void;
  moveTabToWorkspace: (tabId: string, workspaceId: string) => void;
  openSshPicker: () => void;
  toggleSidebar: () => void;
  changeFontSize: (delta: number) => void;
  resetFontSize: () => void;
  t: Messages;
  locale: Locale;
  setLocale: (locale: Locale) => void;
  /** Asks GitHub again and shows the result. */
  checkUpdates: () => void;
  /** A newer release, when there is one. */
  newVersion: string | null;
  showUpdate: () => void;
  openSettings: () => void;
}

/** Each language named in all four, so the switch is found from any UI language. */
const LANGUAGE_KEYWORDS: Record<Locale, string> = {
  ko: "korean 한국어 韓国語 韩语",
  en: "english 영어 英語 英语",
  ja: "japanese 일본어 日本語 日语",
  zh: "chinese 중국어 中国語 中文 汉语",
};

export function buildCommands(ctx: PaletteContext): PaletteCommand[] {
  const { state, dispatch, t } = ctx;
  const inApp = appBridge() !== null;
  const keys = (command: SwitchCommand) => shortcutLabel(command, inApp, IS_MAC);
  const g = t.palette.groups;
  const label = (tab: Tab) => tabLabel(state, tab, t.tabs.untitled);
  const workspaceName = (workspace: Workspace) => workspaceLabel(state, workspace, t.workspaces.untitled);
  // Tab commands work on the workspace in view.
  const tabs = visibleTabs(state);
  const tab = activeTab(state);
  const focused = state.sessions.find((session) => session.id === tab.focusedId) ?? null;
  const tabIndex = tabs.findIndex((item) => item.id === tab.id);
  const step = (offset: number) => tabs[(tabIndex + offset + tabs.length) % tabs.length];
  const workspace = activeWorkspace(state);
  const workspaceIndex = state.workspaces.findIndex((item) => item.id === workspace.id);
  const stepWorkspace = (offset: number) =>
    state.workspaces[(workspaceIndex + offset + state.workspaces.length) % state.workspaces.length];
  const commands: PaletteCommand[] = [];

  for (const kind of LAUNCHERS) {
    commands.push({
      id: `open-${kind}`,
      title: t.kinds[kind].paletteTitle,
      group: g.launch,
      icon: <KindIcon kind={kind} size={14} />,
      keywords: `new open launch ${kind} ${t.kinds[kind].name}${kind === "ssh" ? " remote connect" : ""}`,
      run: kind === "ssh" ? ctx.openSshPicker : () => ctx.open(kind),
    });
  }
  commands.push({
    id: "new-tab",
    title: t.palette.newTab,
    group: g.tab,
    icon: <SquarePlus size={14} />,
    keywords: "new tab",
    run: ctx.newTab,
  });
  commands.push({
    id: "rename-tab",
    title: t.palette.renameTab,
    group: g.tab,
    icon: <Pencil size={14} />,
    keywords: "rename tab name",
    run: () => ctx.rename({ kind: "tab", id: tab.id, where: "tabbar" }),
  });
  if (tabs.length > 1) {
    commands.push(
      {
        id: "next-tab",
        title: t.palette.nextTab,
        hint: keys({ type: "tabStep", delta: 1 }),
        group: g.tab,
        icon: <ArrowLeftRight size={14} />,
        keywords: "next tab",
        run: () => dispatch({ type: "selectTab", id: step(1).id }),
      },
      {
        id: "previous-tab",
        title: t.palette.previousTab,
        hint: keys({ type: "tabStep", delta: -1 }),
        group: g.tab,
        icon: <ArrowLeftRight size={14} />,
        keywords: "previous prev tab",
        run: () => dispatch({ type: "selectTab", id: step(-1).id }),
      },
    );
  }
  if (tabs.length > 1) {
    if (tabIndex > 0)
      commands.push({
        id: "move-tab-left",
        title: t.palette.moveTabLeft,
        group: g.tab,
        icon: <MoveLeft size={14} />,
        keywords: "move tab left reorder",
        run: () => dispatch({ type: "moveTab", id: tab.id, toIndex: tabIndex - 1 }),
      });
    if (tabIndex < tabs.length - 1)
      commands.push({
        id: "move-tab-right",
        title: t.palette.moveTabRight,
        group: g.tab,
        icon: <MoveRight size={14} />,
        keywords: "move tab right reorder",
        run: () => dispatch({ type: "moveTab", id: tab.id, toIndex: tabIndex + 1 }),
      });
  }
  commands.push({
    id: "close-tab",
    title: t.palette.closeTab,
    group: g.tab,
    icon: <X size={14} />,
    keywords: "close tab",
    run: () => dispatch({ type: "closeTab", id: tab.id, fallbackId: ctx.newTabId() }),
  });

  commands.push(
    {
      id: "new-workspace",
      title: t.palette.newWorkspace,
      group: g.workspace,
      icon: <Layers size={14} />,
      keywords: "new workspace project",
      run: ctx.newWorkspace,
    },
    {
      id: "rename-workspace",
      title: t.palette.renameWorkspace,
      group: g.workspace,
      icon: <Pencil size={14} />,
      keywords: "rename workspace project name",
      run: ctx.renameWorkspace,
    },
  );
  if (state.workspaces.length > 1) {
    commands.push(
      {
        id: "next-workspace",
        title: t.palette.nextWorkspace,
        group: g.workspace,
        icon: <ArrowLeftRight size={14} />,
        keywords: "next workspace project switch",
        run: () => dispatch({ type: "selectWorkspace", id: stepWorkspace(1).id }),
      },
      {
        id: "previous-workspace",
        title: t.palette.previousWorkspace,
        group: g.workspace,
        icon: <ArrowLeftRight size={14} />,
        keywords: "previous prev workspace project switch",
        run: () => dispatch({ type: "selectWorkspace", id: stepWorkspace(-1).id }),
      },
      ...state.workspaces
        .filter((item) => item.id !== workspace.id)
        .map((item) => ({
          id: `move-tab-workspace-${item.id}`,
          title: t.palette.moveTabToWorkspace(workspaceName(item)),
          group: g.workspace,
          icon: <ArrowRightToLine size={14} />,
          keywords: "move tab to workspace project",
          run: () => ctx.moveTabToWorkspace(tab.id, item.id),
        })),
    );
  }
  commands.push({
    id: "close-workspace",
    title: t.palette.closeWorkspace,
    group: g.workspace,
    icon: <X size={14} />,
    keywords: "close workspace project",
    run: () => ctx.closeWorkspace(workspace.id),
  });

  if (tab.paneIds.length > 1) {
    commands.push(
      {
        id: "next-pane",
        title: t.palette.nextPane,
        hint: keys({ type: "paneStep", delta: 1 }),
        group: g.session,
        icon: <ArrowLeftRight size={14} />,
        keywords: "next pane focus switch",
        run: () => {
          const id = stepPane(state, 1);
          if (id) dispatch({ type: "focus", id });
        },
      },
      {
        id: "previous-pane",
        title: t.palette.previousPane,
        hint: keys({ type: "paneStep", delta: -1 }),
        group: g.session,
        icon: <ArrowLeftRight size={14} />,
        keywords: "previous prev pane focus switch",
        run: () => {
          const id = stepPane(state, -1);
          if (id) dispatch({ type: "focus", id });
        },
      },
    );
  }
  if (focused) {
    commands.push(
      {
        id: "rename-session",
        title: t.palette.renameSession,
        group: g.session,
        icon: <Pencil size={14} />,
        keywords: "rename session pane name",
        run: () => ctx.rename({ kind: "session", id: focused.id, where: "pane" }),
      },
      {
        id: "toggle-maximize",
        title: tab.maximizedId === focused.id ? t.palette.restore : t.palette.maximize,
        group: g.session,
        icon: <Maximize2 size={14} />,
        keywords: "maximize zoom restore pane",
        run: () => dispatch({ type: "toggleMaximize", id: focused.id }),
      },
      {
        id: "move-pane-new-tab",
        title: t.palette.movePaneNewTab,
        group: g.session,
        icon: <ArrowRightToLine size={14} />,
        keywords: "move pane new tab",
        run: () => ctx.movePaneToNewTab(focused.id),
      },
      ...tabs
        .filter((item) => item.id !== tab.id)
        .map((item) => ({
          id: `move-pane-${item.id}`,
          title: t.palette.movePaneToTab(label(item)),
          group: g.session,
          icon: <ArrowRightToLine size={14} />,
          keywords: "move pane to tab",
          run: () => dispatch({ type: "movePane", id: focused.id, toTabId: item.id }),
        })),
      {
        id: "close-session",
        title: t.palette.closeSession,
        group: g.session,
        icon: <X size={14} />,
        keywords: "close session pane kill",
        run: () => dispatch({ type: "close", id: focused.id }),
      },
    );
  }

  commands.push(
    {
      id: "toggle-sidebar",
      title: t.palette.toggleSidebar,
      group: g.view,
      icon: <PanelLeft size={14} />,
      keywords: "toggle sidebar panel",
      run: ctx.toggleSidebar,
    },
    {
      id: "font-bigger",
      title: t.font.bigger,
      group: g.view,
      icon: <Plus size={14} />,
      keywords: "font size increase zoom in bigger",
      run: () => ctx.changeFontSize(1),
    },
    {
      id: "font-smaller",
      title: t.font.smaller,
      group: g.view,
      icon: <Minus size={14} />,
      keywords: "font size decrease zoom out smaller",
      run: () => ctx.changeFontSize(-1),
    },
    {
      id: "font-reset",
      title: t.palette.fontReset,
      group: g.view,
      icon: <RotateCcw size={14} />,
      keywords: "font size reset default",
      run: ctx.resetFontSize,
    },
    ...(ctx.newVersion
      ? [
          {
            id: "install-update",
            title: t.palette.installUpdate(ctx.newVersion),
            group: g.app,
            icon: <CircleArrowUp size={14} />,
            keywords: "update upgrade new version release",
            run: ctx.showUpdate,
          },
        ]
      : []),
    {
      id: "open-settings",
      title: t.palette.openSettings,
      group: g.app,
      icon: <Settings size={14} />,
      keywords: "settings preferences options config theme font",
      run: ctx.openSettings,
    },
    {
      id: "check-updates",
      title: t.palette.checkUpdates,
      group: g.app,
      icon: <CircleArrowUp size={14} />,
      keywords: "update upgrade version release check",
      run: ctx.checkUpdates,
    },
    ...LOCALES.filter((locale) => locale !== ctx.locale).map((locale) => ({
      id: `language-${locale}`,
      title: t.palette.setLanguage(LOCALE_NAMES[locale]),
      group: g.language,
      icon: <Languages size={14} />,
      keywords: `language locale lang 언어 言語 语言 ${locale} ${LANGUAGE_KEYWORDS[locale]}`,
      run: () => ctx.setLocale(locale),
    })),
  );

  state.workspaces.forEach((item, index) => {
    if (item.id === workspace.id) return;
    commands.push({
      id: `go-workspace-${item.id}`,
      title: workspaceName(item),
      group: g.goWorkspace,
      icon: <span className="workspace-index">{index + 1}</span>,
      keywords: "go switch workspace project",
      run: () => dispatch({ type: "selectWorkspace", id: item.id }),
    });
  });
  tabs.forEach((item, index) => {
    commands.push({
      id: `go-tab-${item.id}`,
      title: label(item),
      // The key that reaches it: 1–8 by position, 9 for the last tab.
      hint:
        index < 8
          ? keys({ type: "tab", number: index + 1 })
          : index === tabs.length - 1
            ? keys({ type: "tab", number: 9 })
            : t.palette.tabHint(index + 1),
      group: g.goTab,
      icon: <span className="tab-index">{index + 1}</span>,
      keywords: "go switch tab",
      run: () => dispatch({ type: "selectTab", id: item.id }),
    });
  });
  // Sessions of every workspace: going to one shows its workspace.
  for (const session of state.sessions) {
    const owner = sessionTab(state, session.id);
    if (!owner) continue;
    const siblings = workspaceTabs(state, owner.workspaceId);
    const tabHint = t.palette.tabHint(siblings.indexOf(owner) + 1);
    const home = state.workspaces.find((item) => item.id === owner.workspaceId);
    commands.push({
      id: `go-session-${session.id}`,
      title: sessionLabel(session),
      hint: home && home.id !== workspace.id ? `${workspaceName(home)} · ${tabHint}` : tabHint,
      group: g.goSession,
      icon: <KindIcon kind={session.kind} size={14} />,
      keywords: `go switch focus session ${session.kind} ${t.kinds[session.kind].name}`,
      run: () => dispatch({ type: "focus", id: session.id }),
    });
  }
  // A kept older host on a server can be replaced later, once per server.
  const remoteTargets = new Set(state.sessions.flatMap((session) => (session.kind === "remote" && session.target ? [session.target] : [])));
  for (const target of remoteTargets) {
    commands.push({
      id: `remote-update-${target}`,
      title: t.palette.updateRemoteHost(target),
      group: g.remoteShell,
      icon: <KindIcon kind="remote" size={14} />,
      keywords: `westerm remote host update replace ${target}`,
      run: () => void postRemote(REMOTE_UPDATE_HOST_PATH, { target }),
    });
  }
  return commands;
}

/** Typing something URL-like offers to open it in terminal-browser. */
export function urlCommands(query: string, open: PaletteContext["open"], t: Messages): PaletteCommand[] {
  const text = query.trim();
  const looksLikeUrl = /^https?:\/\//i.test(text) || /^localhost(:\d+)?(\/|$)/i.test(text) || /^[\w-]+(\.[\w-]+)+(:\d+)?(\/\S*)?$/.test(text);
  const url = looksLikeUrl ? normalizeBrowserUrl(text) : null;
  if (!url) return [];
  return [
    {
      id: "open-url",
      title: t.palette.openUrl(url),
      group: t.palette.groups.launch,
      icon: <Globe size={14} />,
      run: () => open("browser", url),
    },
  ];
}

/**
 * SSH picker entries: each host as a remote shell and as a remote browser
 * (terminal-browser fetching through that host), plus whatever destination
 * the user typed.
 */
export function sshCommands(
  hosts: SshHost[],
  query: string | null,
  open: PaletteContext["open"],
  t: Messages,
): PaletteCommand[] {
  const entries: Array<{ target: string; hint?: string }> = [];
  if (query !== null) {
    const typed = normalizeSshTarget(query);
    if (typed && !hosts.some((host) => host.alias === typed)) entries.push({ target: typed, hint: t.palette.typedAddress });
  } else {
    for (const host of hosts) {
      const where = host.hostname ? `${host.user ? `${host.user}@` : ""}${host.hostname}` : undefined;
      entries.push({ target: host.alias, hint: where });
    }
  }
  return entries.flatMap(({ target, hint }) => [
    {
      id: `ssh-${target}`,
      title: `ssh ${target}`,
      hint,
      group: t.palette.groups.remoteShell,
      icon: <KindIcon kind="ssh" size={14} />,
      keywords: `ssh remote shell ${target}`,
      run: () => open("ssh", null, target),
    },
    {
      id: `remote-${target}`,
      title: t.palette.remoteWesterm(target),
      hint: t.palette.remoteWestermHint,
      group: t.palette.groups.remoteShell,
      icon: <KindIcon kind="remote" size={14} />,
      keywords: `westerm remote quic persistent ${target}`,
      run: () => open("remote", null, target),
    },
    {
      id: `ssh-browser-${target}`,
      title: t.palette.remoteBrowser(target),
      hint: t.palette.remoteBrowserHint,
      group: t.palette.groups.remoteBrowser,
      icon: <Globe size={14} />,
      keywords: `ssh remote browser terminal-browser ${target}`,
      run: () => open("browser", null, target),
    },
  ]);
}
