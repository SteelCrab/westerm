import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import type { SessionKind } from "../shared/protocol.ts";
import { ActivityBar } from "./components/ActivityBar.tsx";
import { CommandPalette } from "./components/CommandPalette.tsx";
import type { RenameTarget } from "./components/InlineRename.tsx";
import { PaneLayer } from "./components/PaneLayer.tsx";
import { SessionList } from "./components/SessionList.tsx";
import { isCompact, SidebarResizer } from "./components/SidebarResizer.tsx";
import { TabBar } from "./components/TabBar.tsx";
import { SettingsDialog } from "./components/SettingsDialog.tsx";
import { UpdateButton } from "./components/UpdateButton.tsx";
import { useI18n } from "./i18n/index.tsx";
import { buildCommands, isPaletteShortcut, PALETTE_SHORTCUT, sshCommands, urlCommands } from "./paletteCommands.tsx";
import { useSshHosts } from "./useSshHosts.ts";
import { useColorScheme } from "./theme.ts";
import { useSidebarWidth } from "./useSidebarWidth.ts";
import { useTerminalFont } from "./useTerminalFont.ts";
import { useUpdate } from "./useUpdate.ts";
import { parseFamilies, useSettings } from "./settings.ts";
import { switchCommand, typingElsewhere, type SwitchCommand } from "./shortcuts.ts";
import { appBridge } from "./useUpdate.ts";
import { markEnding } from "./terminal/ending.ts";
import { useWorkspacePersistence } from "./useWorkspacePersistence.ts";
import {
  createWorkspace,
  focusedShellId,
  paneToward,
  stepPane,
  stepTab,
  tabByNumber,
  workspaceReducer,
  type WorkspaceAction,
} from "./workspace.ts";

/** Shape of the pane area, to split new panes along their longer side. */
function layerAspect(): number {
  const layer = document.querySelector(".pane-layer");
  const rect = layer?.getBoundingClientRect();
  return rect && rect.height > 0 ? rect.width / rect.height : window.innerWidth / Math.max(1, window.innerHeight);
}

let nextId = 0;
const newId = (prefix: "s" | "t") => `${prefix}${Date.now().toString(36)}${(nextId++).toString(36)}`;

export function App() {
  const [state, rawDispatch] = useReducer(workspaceReducer, newId("t"), createWorkspace);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sidebarWidth, setSidebarWidth] = useSidebarWidth();
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  // "commands": the command palette. "ssh": the same palette listing ssh hosts.
  const [palette, setPalette] = useState<"commands" | "ssh" | null>(null);
  const sshHosts = useSshHosts(palette === "ssh");
  const [dragging, setDragging] = useState<string | null>(null);
  // Bumped when a dashboard control gives keyboard focus back to the terminal.
  const [focusNonce, setFocusNonce] = useState(0);
  const { t, locale, setLocale } = useI18n();
  const [settings, changeSettings] = useSettings();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const fontOverride = useMemo(() => parseFamilies(settings.fontFamily), [settings.fontFamily]);
  const { font, sizeOverridden, ghosttyFont, changeSize, resetSize } = useTerminalFont(locale, fontOverride);
  const update = useUpdate(settings.autoUpdate);
  const scheme = useColorScheme(settings.theme);

  // The macOS app's Settings menu (⌘,) sends this event.
  useEffect(() => {
    const open = () => setSettingsOpen(true);
    window.addEventListener("westerm:settings", open);
    return () => window.removeEventListener("westerm:settings", open);
  }, []);
  const [updateOpen, setUpdateOpen] = useState(false);
  const checkUpdates = useCallback(() => {
    void update.check(true).then(() => setUpdateOpen(true));
  }, [update.check]);

  // Keep the latest state at hand for callbacks that must not change identity.
  const stateRef = useRef(state);
  stateRef.current = state;

  // Closing a pane or tab ends its processes; anything else only detaches.
  const dispatch = useCallback((action: WorkspaceAction) => {
    if (action.type === "close") markEnding([action.id]);
    if (action.type === "closeTab") {
      markEnding(stateRef.current.tabs.find((tab) => tab.id === action.id)?.paneIds ?? []);
    }
    rawDispatch(action);
  }, []);

  // A new editor opens the focused shell's directory. SSH and remote westerm
  // ask for a host first.
  const open = useCallback(
    (kind: SessionKind, url: string | null = null, target: string | null = null) => {
      if ((kind === "ssh" || kind === "remote") && !target) {
        setPalette("ssh");
        return;
      }
      const cwdFrom = kind === "editor" ? focusedShellId(stateRef.current) : null;
      dispatch({ type: "open", id: newId("s"), kind, url, cwdFrom, target, aspect: layerAspect() });
    },
    [dispatch],
  );


  // A new tab starts with a shell, like a terminal app's new tab.
  const newTab = useCallback(() => {
    dispatch({ type: "newTab", id: newId("t") });
    dispatch({ type: "open", id: newId("s"), kind: "shell", aspect: layerAspect() });
  }, [dispatch]);

  const movePaneToNewTab = useCallback(
    (paneId: string) => {
      const id = newId("t");
      dispatch({ type: "newTab", id });
      dispatch({ type: "movePane", id: paneId, toTabId: id, aspect: layerAspect() });
    },
    [dispatch],
  );

  const rename = useCallback((target: RenameTarget | null) => {
    setRenaming(target);
    if (target === null) setFocusNonce((value) => value + 1);
  }, []);

  const closePalette = useCallback(() => {
    setPalette(null);
    setFocusNonce((value) => value + 1);
  }, []);

  // Restore the saved layout; panes then reattach to their running shells.
  // With nothing saved, start with one shell.
  const loaded = useWorkspacePersistence(state, rawDispatch, () => open("shell"));

  // Moving between tabs and panes from the keyboard; the keys are in shortcuts.ts.
  const runSwitch = useCallback(
    (command: SwitchCommand) => {
      const current = stateRef.current;
      if (command.type === "tab" || command.type === "tabStep") {
        const tab = command.type === "tab" ? tabByNumber(current, command.number) : stepTab(current, command.delta);
        if (tab && tab.id !== current.activeTabId) dispatch({ type: "selectTab", id: tab.id });
      } else {
        const id = command.type === "paneStep" ? stepPane(current, command.delta) : paneToward(current, command.direction);
        if (id) dispatch({ type: "focus", id });
      }
      setFocusNonce((value) => value + 1);
    },
    [dispatch],
  );

  useEffect(() => {
    const inApp = appBridge() !== null;
    const dialogOpen = () => document.querySelector('[aria-modal="true"]') !== null;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing || typingElsewhere(event.target) || dialogOpen()) return;
      const command = switchCommand(event, inApp);
      if (!command) return;
      event.preventDefault();
      event.stopPropagation();
      runSwitch(command);
    };
    // The macOS app's Window menu sends the same commands.
    const onCommand = (event: Event) => {
      const command = (event as CustomEvent<SwitchCommand>).detail;
      if (command && !dialogOpen()) runSwitch(command);
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("westerm:command", onCommand);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("westerm:command", onCommand);
    };
  }, [runSwitch]);

  // The palette shortcut wins over terminals: capture runs before their input.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPaletteShortcut(event)) return;
      event.preventDefault();
      event.stopPropagation();
      setPalette((value) => (value ? null : "commands"));
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  const commands = useMemo(
    () =>
      buildCommands({
        state,
        dispatch,
        open,
        newTab,
        newTabId: () => newId("t"),
        movePaneToNewTab,
        rename,
        openSshPicker: () => setPalette("ssh"),
        toggleSidebar: () => setSidebarOpen((value) => !value),
        changeFontSize: changeSize,
        resetFontSize: resetSize,
        t,
        locale,
        setLocale,
        checkUpdates,
        newVersion: update.status?.newer ? (update.status.latest ?? null) : null,
        showUpdate: () => setUpdateOpen(true),
        openSettings: () => setSettingsOpen(true),
      }),
    [state, dispatch, open, newTab, movePaneToNewTab, rename, changeSize, resetSize, t, locale, setLocale, checkUpdates, update.status],
  );
  const dynamicCommands = useCallback((query: string) => urlCommands(query, open, t), [open, t]);
  const sshPaletteCommands = useMemo(() => sshCommands(sshHosts ?? [], null, open, t), [sshHosts, open, t]);
  const sshDynamicCommands = useCallback((query: string) => sshCommands([], query, open, t), [open, t]);

  const counts = { shell: 0, browser: 0, editor: 0, ssh: 0, remote: 0 } satisfies Record<SessionKind, number>;
  for (const session of state.sessions) counts[session.kind] += 1;

  return (
    <div
      className={`app${sidebarOpen ? "" : " sidebar-collapsed"}`}
      style={{ "--sidebar-width": `${sidebarWidth}px` } as React.CSSProperties}
    >
      <ActivityBar
        sidebarOpen={sidebarOpen}
        counts={counts}
        onLaunch={open}
        onToggleSidebar={() => setSidebarOpen((value) => !value)}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenPalette={() => setPalette("commands")}
        paletteShortcut={PALETTE_SHORTCUT}
      />
      {sidebarOpen && (
        <div className="sidebar-slot">
          <SessionList
            state={state}
            renaming={renaming}
            onSelectTab={(id) => dispatch({ type: "selectTab", id })}
            onCloseTab={(id) => dispatch({ type: "closeTab", id, fallbackId: newId("t") })}
            onNewTab={newTab}
            onFocus={(id) => dispatch({ type: "focus", id })}
            onClose={(id) => dispatch({ type: "close", id })}
            onRename={rename}
            onRenameTab={(id, name) => dispatch({ type: "renameTab", id, name })}
            onRenameSession={(id, name) => dispatch({ type: "renameSession", id, name })}
            onMoveTab={(id, toIndex) => dispatch({ type: "moveTab", id, toIndex })}
            onMovePane={(id, toTabId) => dispatch({ type: "movePane", id, toTabId, aspect: layerAspect() })}
            onSwapPanes={(id, withId) => dispatch({ type: "swapPanes", id, withId })}
            onDragPane={setDragging}
            compact={isCompact(sidebarWidth)}
          />
          <SidebarResizer width={sidebarWidth} onResize={setSidebarWidth} />
        </div>
      )}
      <main className="workspace">
        <header className="workspace-header">
          <TabBar
            state={state}
            renaming={renaming}
            onSelect={(id) => dispatch({ type: "selectTab", id })}
            onClose={(id) => dispatch({ type: "closeTab", id, fallbackId: newId("t") })}
            onNew={newTab}
            onRename={rename}
            onRenameTab={(id, name) => dispatch({ type: "renameTab", id, name })}
            onMoveTab={(id, toIndex) => dispatch({ type: "moveTab", id, toIndex })}
            onMovePane={(paneId, tabId) =>
              dispatch({ type: "movePane", id: paneId, toTabId: tabId, aspect: layerAspect() })
            }
            onMovePaneToNewTab={movePaneToNewTab}
            onDragEnd={() => setDragging(null)}
          />
          <div className="workspace-tools">
            <UpdateButton update={update} open={updateOpen} onOpenChange={setUpdateOpen} />
          </div>
        </header>
        {font && loaded ? (
          <PaneLayer
            state={state}
            font={font}
            scheme={scheme}
            focusNonce={focusNonce}
            renaming={renaming}
            dragging={dragging}
            onDragPane={setDragging}
            onRename={rename}
            onLaunch={open}
            dispatch={dispatch}
          />
        ) : (
          <div className="workspace-empty">
            <p>{t.app.loadingConfig}</p>
          </div>
        )}
      </main>
      {settingsOpen && (
        <SettingsDialog
          settings={settings}
          onChange={changeSettings}
          font={font}
          ghosttyFont={ghosttyFont}
          sizeOverridden={sizeOverridden}
          changeSize={changeSize}
          resetSize={resetSize}
          update={update}
          onShowUpdate={() => {
            setSettingsOpen(false);
            setUpdateOpen(true);
          }}
          onClose={() => {
            setSettingsOpen(false);
            setFocusNonce((value) => value + 1);
          }}
        />
      )}
      {palette === "commands" && (
        <CommandPalette commands={commands} dynamic={dynamicCommands} onClose={closePalette} />
      )}
      {palette === "ssh" && (
        <CommandPalette
          placeholder={t.palette.sshPlaceholder}
          commands={sshPaletteCommands}
          dynamic={sshDynamicCommands}
          empty={sshHosts === null ? t.palette.sshLoading : t.palette.sshEmpty}
          onClose={closePalette}
        />
      )}
    </div>
  );
}
