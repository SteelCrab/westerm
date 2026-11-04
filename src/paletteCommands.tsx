import {
  ArrowLeftRight,
  ArrowRightToLine,
  MoveLeft,
  MoveRight,
  Globe,
  Maximize2,
  Minus,
  PanelLeft,
  Pencil,
  Plus,
  RotateCcw,
  SquarePlus,
  X,
} from "lucide-react";
import type { SessionKind } from "../shared/protocol.ts";
import type { RenameTarget } from "./components/InlineRename.tsx";
import type { PaletteCommand } from "./components/CommandPalette.tsx";
import { KindIcon, KINDS, LAUNCHERS } from "./kinds.tsx";
import { normalizeBrowserUrl, normalizeSshTarget } from "../shared/protocol.ts";
import type { SshHost } from "./useSshHosts.ts";
import { activeTab, sessionLabel, sessionTab, tabLabel, type Workspace, type WorkspaceAction } from "./workspace.ts";

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
  state: Workspace;
  dispatch: (action: WorkspaceAction) => void;
  open: (kind: SessionKind, url?: string | null, target?: string | null) => void;
  newTab: () => void;
  newTabId: () => string;
  movePaneToNewTab: (paneId: string) => void;
  rename: (target: RenameTarget) => void;
  openSshPicker: () => void;
  toggleSidebar: () => void;
  changeFontSize: (delta: number) => void;
  resetFontSize: () => void;
}

export function buildCommands(ctx: PaletteContext): PaletteCommand[] {
  const { state, dispatch } = ctx;
  const tab = activeTab(state);
  const focused = state.sessions.find((session) => session.id === tab.focusedId) ?? null;
  const tabIndex = state.tabs.findIndex((item) => item.id === tab.id);
  const step = (offset: number) => state.tabs[(tabIndex + offset + state.tabs.length) % state.tabs.length];
  const commands: PaletteCommand[] = [];

  for (const kind of LAUNCHERS) {
    commands.push({
      id: `open-${kind}`,
      title: kind === "ssh" ? "SSH 연결…" : `새 ${kind === "shell" ? "셸" : KINDS[kind].label}`,
      group: "실행",
      icon: <KindIcon kind={kind} size={14} />,
      keywords: `new open launch ${kind} ${KINDS[kind].label} remote 원격`,
      run: kind === "ssh" ? ctx.openSshPicker : () => ctx.open(kind),
    });
  }
  commands.push({
    id: "new-tab",
    title: "새 탭",
    group: "탭",
    icon: <SquarePlus size={14} />,
    keywords: "new tab",
    run: ctx.newTab,
  });
  commands.push({
    id: "rename-tab",
    title: "현재 탭 이름 변경",
    group: "탭",
    icon: <Pencil size={14} />,
    keywords: "rename tab name",
    run: () => ctx.rename({ kind: "tab", id: tab.id, where: "tabbar" }),
  });
  if (state.tabs.length > 1) {
    commands.push(
      {
        id: "next-tab",
        title: "다음 탭",
        group: "탭",
        icon: <ArrowLeftRight size={14} />,
        keywords: "next tab",
        run: () => dispatch({ type: "selectTab", id: step(1).id }),
      },
      {
        id: "previous-tab",
        title: "이전 탭",
        group: "탭",
        icon: <ArrowLeftRight size={14} />,
        keywords: "previous prev tab",
        run: () => dispatch({ type: "selectTab", id: step(-1).id }),
      },
    );
  }
  if (state.tabs.length > 1) {
    if (tabIndex > 0)
      commands.push({
        id: "move-tab-left",
        title: "현재 탭을 왼쪽으로 이동",
        group: "탭",
        icon: <MoveLeft size={14} />,
        keywords: "move tab left reorder",
        run: () => dispatch({ type: "moveTab", id: tab.id, toIndex: tabIndex - 1 }),
      });
    if (tabIndex < state.tabs.length - 1)
      commands.push({
        id: "move-tab-right",
        title: "현재 탭을 오른쪽으로 이동",
        group: "탭",
        icon: <MoveRight size={14} />,
        keywords: "move tab right reorder",
        run: () => dispatch({ type: "moveTab", id: tab.id, toIndex: tabIndex + 1 }),
      });
  }
  commands.push({
    id: "close-tab",
    title: "현재 탭 닫기",
    group: "탭",
    icon: <X size={14} />,
    keywords: "close tab",
    run: () => dispatch({ type: "closeTab", id: tab.id, fallbackId: ctx.newTabId() }),
  });

  if (focused) {
    commands.push(
      {
        id: "rename-session",
        title: "현재 세션 이름 변경",
        group: "세션",
        icon: <Pencil size={14} />,
        keywords: "rename session pane name",
        run: () => ctx.rename({ kind: "session", id: focused.id, where: "pane" }),
      },
      {
        id: "toggle-maximize",
        title: tab.maximizedId === focused.id ? "패널 그리드로 돌아가기" : "현재 패널 최대화",
        group: "세션",
        icon: <Maximize2 size={14} />,
        keywords: "maximize zoom restore pane",
        run: () => dispatch({ type: "toggleMaximize", id: focused.id }),
      },
      {
        id: "move-pane-new-tab",
        title: "현재 패널을 새 탭으로 이동",
        group: "세션",
        icon: <ArrowRightToLine size={14} />,
        keywords: "move pane new tab",
        run: () => ctx.movePaneToNewTab(focused.id),
      },
      ...state.tabs
        .filter((item) => item.id !== tab.id)
        .map((item) => ({
          id: `move-pane-${item.id}`,
          title: `현재 패널을 '${tabLabel(state, item)}' 탭으로 이동`,
          group: "세션",
          icon: <ArrowRightToLine size={14} />,
          keywords: "move pane to tab",
          run: () => dispatch({ type: "movePane", id: focused.id, toTabId: item.id }),
        })),
      {
        id: "close-session",
        title: "현재 세션 닫기",
        group: "세션",
        icon: <X size={14} />,
        keywords: "close session pane kill",
        run: () => dispatch({ type: "close", id: focused.id }),
      },
    );
  }

  commands.push(
    {
      id: "toggle-sidebar",
      title: "사이드바 보이기/숨기기",
      group: "보기",
      icon: <PanelLeft size={14} />,
      keywords: "toggle sidebar panel",
      run: ctx.toggleSidebar,
    },
    {
      id: "font-bigger",
      title: "글자 크게",
      group: "보기",
      icon: <Plus size={14} />,
      keywords: "font size increase zoom in bigger",
      run: () => ctx.changeFontSize(1),
    },
    {
      id: "font-smaller",
      title: "글자 작게",
      group: "보기",
      icon: <Minus size={14} />,
      keywords: "font size decrease zoom out smaller",
      run: () => ctx.changeFontSize(-1),
    },
    {
      id: "font-reset",
      title: "글자 크기를 Ghostty 설정으로",
      group: "보기",
      icon: <RotateCcw size={14} />,
      keywords: "font size reset default",
      run: ctx.resetFontSize,
    },
  );

  state.tabs.forEach((item, index) => {
    commands.push({
      id: `go-tab-${item.id}`,
      title: tabLabel(state, item),
      hint: `탭 ${index + 1}`,
      group: "탭으로 이동",
      icon: <span className="tab-index">{index + 1}</span>,
      keywords: "go switch tab",
      run: () => dispatch({ type: "selectTab", id: item.id }),
    });
  });
  for (const session of state.sessions) {
    const owner = state.tabs.findIndex((item) => item.id === sessionTab(state, session.id)?.id);
    commands.push({
      id: `go-session-${session.id}`,
      title: sessionLabel(session),
      hint: `탭 ${owner + 1}`,
      group: "세션으로 이동",
      icon: <KindIcon kind={session.kind} size={14} />,
      keywords: `go switch focus session ${KINDS[session.kind].label}`,
      run: () => dispatch({ type: "focus", id: session.id }),
    });
  }
  return commands;
}

/** Typing something URL-like offers to open it in terminal-browser. */
export function urlCommands(query: string, open: PaletteContext["open"]): PaletteCommand[] {
  const text = query.trim();
  const looksLikeUrl = /^https?:\/\//i.test(text) || /^localhost(:\d+)?(\/|$)/i.test(text) || /^[\w-]+(\.[\w-]+)+(:\d+)?(\/\S*)?$/.test(text);
  const url = looksLikeUrl ? normalizeBrowserUrl(text) : null;
  if (!url) return [];
  return [
    {
      id: "open-url",
      title: `terminal-browser로 열기: ${url}`,
      group: "실행",
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
): PaletteCommand[] {
  const entries: Array<{ target: string; hint?: string }> = [];
  if (query !== null) {
    const typed = normalizeSshTarget(query);
    if (typed && !hosts.some((host) => host.alias === typed)) entries.push({ target: typed, hint: "입력한 주소" });
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
      group: "원격 셸",
      icon: <KindIcon kind="ssh" size={14} />,
      keywords: `ssh remote shell ${target}`,
      run: () => open("ssh", null, target),
    },
    {
      id: `ssh-browser-${target}`,
      title: `원격 브라우저 · ${target}`,
      hint: "localhost가 원격 서버를 가리킵니다",
      group: "원격 브라우저",
      icon: <Globe size={14} />,
      keywords: `ssh remote browser terminal-browser ${target}`,
      run: () => open("browser", null, target),
    },
  ]);
}
