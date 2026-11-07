import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { TriangleAlert } from "lucide-react";
import type { PaneKind } from "../shared/protocol.ts";
import type { WorkspaceColor, WorkspaceIcon } from "../shared/workspace-file.ts";
import { ActivityBar } from "./components/ActivityBar.tsx";
import { CommandPalette, type PaletteCommand } from "./components/CommandPalette.tsx";
import { placeRename, type RenameTarget } from "./components/InlineRename.tsx";
import { PaneLayer } from "./components/PaneLayer.tsx";
import { RemoteBanner, RemoteConnect, stageText } from "./components/RemoteConnect.tsx";
import { SessionList } from "./components/SessionList.tsx";
import { isCompact, SidebarResizer } from "./components/SidebarResizer.tsx";
import { TabBar } from "./components/TabBar.tsx";
import { tabColors, WorkspaceMenu } from "./components/WorkspaceMenu.tsx";
import { SettingsDialog } from "./components/SettingsDialog.tsx";
import { UpdateButton, updateShown } from "./components/UpdateButton.tsx";
import { useI18n } from "./i18n/index.tsx";
import { RemoteWindowIcon } from "./kinds.tsx";
import {
  buildCommands,
  IS_MAC,
  isPaletteShortcut,
  PALETTE_SHORTCUT,
  remoteWindowCommands,
  sshCommands,
  urlCommands,
} from "./paletteCommands.tsx";
import { openRemoteWindow, REMOTE_TARGET } from "./remoteWindow.ts";
import { useRemoteLink } from "./useRemoteLink.ts";
import { useSshHosts } from "./useSshHosts.ts";
import { terminalColors } from "./colorThemes.ts";
import { useColorScheme, useWindowColors } from "./theme.ts";
import { useNarrow } from "./useNarrow.ts";
import { useSidebarWidth } from "./useSidebarWidth.ts";
import { useTerminalFont } from "./useTerminalFont.ts";
import { useUpdate } from "./useUpdate.ts";
import { parseFamilies, useSettings } from "./settings.ts";
import { switchCommand, typingElsewhere, type SwitchCommand } from "./shortcuts.ts";
import { appBridge, appCommandKeys } from "./appBridge.ts";
import { markEnding } from "./terminal/ending.ts";
import { loadSaved, mayReattach, reclaimAction, useWorkspacePersistence } from "./useWorkspacePersistence.ts";
import {
  activeTab,
  activeWorkspace,
  createDashboard,
  focusedShellId,
  paneToward,
  stepPane,
  stepTab,
  tabByNumber,
  takenOver,
  dashboardReducer,
  workspaceTabs,
  type DashboardAction,
} from "./workspace.ts";

/** Shape of the pane area, to split new panes along their longer side. */
function layerAspect(): number {
  const layer = document.querySelector(".pane-layer");
  const rect = layer?.getBoundingClientRect();
  return rect && rect.height > 0 ? rect.width / rect.height : window.innerWidth / Math.max(1, window.innerHeight);
}

let nextId = 0;
const newId = (prefix: "s" | "t" | "w") => `${prefix}${Date.now().toString(36)}${(nextId++).toString(36)}`;
const createFresh = () => createDashboard(newId("w"), newId("t"));

/** The host picker of the activity bar's remote window button lists only the window entries. */
const pickCommands = (commands: PaletteCommand[], windowsOnly: boolean) =>
  windowsOnly ? commands.filter((command) => command.id.startsWith("window-")) : commands;

/** A local window's layout needs no link. */
const LOCAL_LINK = { ready: true, reconnects: 0 };

export function App() {
  const [state, rawDispatch] = useReducer(dashboardReducer, null, createFresh);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  // On a phone the sidebar is a drawer over the panes. It starts closed and
  // keeps its own state, so the wide window's choice stays as it was.
  const narrow = useNarrow();
  const narrowRef = useRef(narrow);
  narrowRef.current = narrow;
  const [drawerOpen, setDrawerOpen] = useState(false);
  const sidebarVisible = narrow ? drawerOpen : sidebarOpen;
  /** Opens, closes, or flips the sidebar: the drawer on a phone, else the column. */
  const setSidebar = useCallback((open: boolean | ((value: boolean) => boolean)) => {
    (narrowRef.current ? setDrawerOpen : setSidebarOpen)(open);
  }, []);
  /** A pick in the drawer shows its pane, so the drawer gets out of the way. */
  const closeDrawer = useCallback(() => {
    if (narrowRef.current) setDrawerOpen(false);
  }, []);
  useEffect(() => {
    if (!narrow || !drawerOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setDrawerOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [narrow, drawerOpen]);
  const [sidebarWidth, setSidebarWidth] = useSidebarWidth();
  const [renaming, setRenaming] = useState<RenameTarget | null>(null);
  // "commands": the command palette. "ssh": the same palette listing ssh hosts.
  // "window": the host list again, with only the "open in a new window" entries.
  const [palette, setPalette] = useState<"commands" | "ssh" | "window" | null>(null);
  const sshHosts = useSshHosts(palette === "ssh" || palette === "window");
  const [dragging, setDragging] = useState<string | null>(null);
  // Bumped when a dashboard control gives keyboard focus back to the terminal.
  const [focusNonce, setFocusNonce] = useState(0);
  const { t, locale, setLocale } = useI18n();
  const [settings, changeSettings] = useSettings();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const fontOverride = useMemo(() => parseFamilies(settings.fontFamily), [settings.fontFamily]);
  const { font, sizeOverridden, ghosttyFont, changeSize, resetSize } = useTerminalFont(locale, fontOverride);
  // A remote window does not update westerm: the server follows this one.
  const update = useUpdate(settings.autoUpdate && REMOTE_TARGET === null);
  const scheme = useColorScheme(settings.theme);
  const colors = terminalColors(settings.colorTheme, scheme);
  useWindowColors(settings.colorTheme, scheme);

  // A remote window's link to its server. A new local westerm (an update)
  // brings new page files, so the window loads again.
  const link = useRemoteLink(REMOTE_TARGET);
  // The desktop app uses the page title as the window title.
  useEffect(() => {
    if (REMOTE_TARGET !== null) document.title = `westerm — ${REMOTE_TARGET}`;
  }, []);
  useEffect(() => {
    if (link.reload) location.reload();
  }, [link.reload]);
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
  // The macOS app's Check for Updates menu sends this event. Remote windows
  // have no updates of their own.
  useEffect(() => {
    if (REMOTE_TARGET !== null) return;
    window.addEventListener("westerm:update", checkUpdates);
    return () => window.removeEventListener("westerm:update", checkUpdates);
  }, [checkUpdates]);

  // Keep the latest state at hand for callbacks that must not change identity.
  const stateRef = useRef(state);
  stateRef.current = state;

  // Closing a pane, tab, or workspace ends its processes; anything else only detaches.
  const dispatch = useCallback((action: DashboardAction) => {
    if (action.type === "close") markEnding([action.id]);
    if (action.type === "closeTab") {
      markEnding(stateRef.current.tabs.find((tab) => tab.id === action.id)?.paneIds ?? []);
    }
    if (action.type === "closeWorkspace") {
      markEnding(workspaceTabs(stateRef.current, action.id).flatMap((tab) => tab.paneIds));
    }
    rawDispatch(action);
  }, []);

  // A new editor opens the focused shell's directory, and a new docs pane
  // lists it and follows that shell. SSH asks for a host first.
  const open = useCallback(
    (kind: PaneKind, url: string | null = null, target: string | null = null) => {
      if (kind === "ssh" && !target) {
        setPalette("ssh");
        return;
      }
      const shellId = focusedShellId(stateRef.current);
      const cwdFrom = kind === "editor" ? shellId : null;
      const docs = kind === "docs" ? { root: null, followFrom: shellId, file: null, mode: "view" as const } : undefined;
      dispatch({ type: "open", id: newId("s"), kind, url, cwdFrom, target, aspect: layerAspect(), docs });
    },
    [dispatch],
  );
  const openUrl = useCallback((url: string) => open("browser", url), [open]);


  // A new tab starts with a shell, like a terminal app's new tab.
  const newTab = useCallback(() => {
    dispatch({ type: "newTab", id: newId("t") });
    dispatch({ type: "open", id: newId("s"), kind: "shell", aspect: layerAspect() });
  }, [dispatch]);

  // A new workspace starts like a new tab: one shell.
  const newWorkspace = useCallback(() => {
    dispatch({ type: "newWorkspace", id: newId("w"), tabId: newId("t") });
    dispatch({ type: "open", id: newId("s"), kind: "shell", aspect: layerAspect() });
  }, [dispatch]);

  const closeWorkspace = useCallback(
    (id: string) => dispatch({ type: "closeWorkspace", id, fallbackId: newId("w"), fallbackTabId: newId("t") }),
    [dispatch],
  );

  const moveTabToWorkspace = useCallback(
    (id: string, workspaceId: string) => dispatch({ type: "moveTabToWorkspace", id, workspaceId, fallbackId: newId("t") }),
    [dispatch],
  );

  // A rename field opens where the layout shows the item. In the sidebar
  // layout, the sidebar must be open for it.
  const layout = settings.layout;
  const rename = useCallback(
    (target: RenameTarget | null) => {
      if (target === null) {
        setRenaming(null);
        setFocusNonce((value) => value + 1);
        return;
      }
      const placed = placeRename(target, layout);
      if (placed.where === "sidebar") setSidebar(true);
      setRenaming(placed);
    },
    [layout, setSidebar],
  );

  const renameWorkspace = useCallback(
    () => rename({ kind: "workspace", id: activeWorkspace(stateRef.current).id, where: "tabbar" }),
    [rename],
  );

  const setWorkspaceIcon = useCallback(
    (id: string, icon: WorkspaceIcon) => dispatch({ type: "setWorkspaceIcon", id, icon }),
    [dispatch],
  );

  const setWorkspaceColor = useCallback(
    (id: string, color: WorkspaceColor) => dispatch({ type: "setWorkspaceColor", id, color }),
    [dispatch],
  );

  const movePaneToNewTab = useCallback(
    (paneId: string) => {
      const id = newId("t");
      dispatch({ type: "newTab", id });
      dispatch({ type: "movePane", id: paneId, toTabId: id, aspect: layerAspect() });
    },
    [dispatch],
  );

  const closePalette = useCallback(() => {
    setPalette(null);
    setFocusNonce((value) => value + 1);
  }, []);

  // Restore the saved layout; panes then reattach to their running shells.
  // With nothing saved, start with one shell. A remote window reads its
  // server's layout once the link is up, and again after every reconnect;
  // its disconnected panes wait for that read.
  const away = takenOver(state);
  const persistenceLink =
    REMOTE_TARGET === null ? LOCAL_LINK : { ready: link.connected, reconnects: link.reconnects };
  const { loaded, adopted, checked } = useWorkspacePersistence(
    state,
    rawDispatch,
    () => open("shell"),
    away,
    persistenceLink,
  );
  const reattach = mayReattach(REMOTE_TARGET !== null, link.connected, link.reconnects, checked);

  // One window at a time has the sessions. Using this window again, by
  // switching to it or clicking it, takes them back with the latest layout.
  const reclaim = useCallback(() => {
    void loadSaved().then((result) => {
      const next = reclaimAction(REMOTE_TARGET !== null, result);
      if (next === "stay") return;
      adopted(next.saved);
      rawDispatch({ type: "reclaim", saved: next.saved });
    });
  }, [adopted]);
  useEffect(() => {
    if (!away) return;
    window.addEventListener("focus", reclaim);
    return () => window.removeEventListener("focus", reclaim);
  }, [away, reclaim]);

  // Moving between tabs and panes from the keyboard; the keys are in shortcuts.ts.
  const runSwitch = useCallback(
    (command: SwitchCommand) => {
      const current = stateRef.current;
      if (command.type === "tab" || command.type === "tabStep") {
        const tab = command.type === "tab" ? tabByNumber(current, command.number) : stepTab(current, command.delta);
        if (tab && tab.id !== activeTab(current).id) dispatch({ type: "selectTab", id: tab.id });
      } else {
        const id = command.type === "paneStep" ? stepPane(current, command.delta) : paneToward(current, command.direction);
        if (id) dispatch({ type: "focus", id });
      }
      setFocusNonce((value) => value + 1);
    },
    [dispatch],
  );

  useEffect(() => {
    const appKeys = appCommandKeys(appBridge(), IS_MAC);
    const dialogOpen = () => document.querySelector('[aria-modal="true"]') !== null;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing || typingElsewhere(event.target) || dialogOpen()) return;
      const command = switchCommand(event, appKeys);
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
    () => [
      ...buildCommands({
        state,
        dispatch,
        open,
        newTab,
        newTabId: () => newId("t"),
        movePaneToNewTab,
        rename,
        newWorkspace,
        renameWorkspace,
        closeWorkspace,
        moveTabToWorkspace,
        openSshPicker: () => setPalette("ssh"),
        toggleSidebar: layout === "top" ? null : () => setSidebar((value) => !value),
        changeFontSize: changeSize,
        resetFontSize: resetSize,
        t,
        locale,
        setLocale,
        checkUpdates: REMOTE_TARGET === null ? checkUpdates : null,
        newVersion: REMOTE_TARGET === null && update.status?.newer ? (update.status.latest ?? null) : null,
        showUpdate: () => setUpdateOpen(true),
        openSettings: () => setSettingsOpen(true),
      }),
      ...(REMOTE_TARGET === null ? [] : remoteWindowCommands(REMOTE_TARGET, t)),
    ],
    [
      state,
      dispatch,
      open,
      newTab,
      movePaneToNewTab,
      rename,
      newWorkspace,
      renameWorkspace,
      closeWorkspace,
      moveTabToWorkspace,
      changeSize,
      resetSize,
      t,
      locale,
      setLocale,
      checkUpdates,
      update.status,
      layout,
    ],
  );
  const dynamicCommands = useCallback((query: string) => urlCommands(query, open, t), [open, t]);
  // A remote window opens no windows: they do not nest.
  const openWindow = REMOTE_TARGET === null ? openRemoteWindow : null;
  const windowsOnly = palette === "window";
  const sshPaletteCommands = useMemo(
    () => pickCommands(sshCommands(sshHosts ?? [], null, open, openWindow, t), windowsOnly),
    [sshHosts, open, openWindow, t, windowsOnly],
  );
  const sshDynamicCommands = useCallback(
    (query: string) => pickCommands(sshCommands([], query, open, openWindow, t), windowsOnly),
    [open, openWindow, t, windowsOnly],
  );

  const counts = { shell: 0, browser: 0, editor: 0, ssh: 0, docs: 0 } satisfies Record<PaneKind, number>;
  for (const session of state.sessions) counts[session.kind] += 1;

  // Top: the workspace menu and the tab bar, no sidebar. Sidebar: both in the
  // sidebar, no tab bar; the header stays only for the update button.
  const onTop = layout === "top";
  const sidebarShown = !onTop && sidebarVisible;
  // A remote window always shows its server in the header.
  const headerShown = onTop || REMOTE_TARGET !== null || updateShown(update.status, updateOpen);

  return (
    <div
      className={`app${sidebarShown ? "" : " sidebar-collapsed"}`}
      // The tabs on screen show the color of the workspace in view.
      style={
        { "--sidebar-width": `${sidebarWidth}px`, ...tabColors(activeWorkspace(state).color) } as React.CSSProperties
      }
    >
      <ActivityBar
        sidebarOpen={sidebarVisible}
        counts={counts}
        onLaunch={open}
        onToggleSidebar={onTop ? null : () => setSidebar((value) => !value)}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenPalette={() => setPalette("commands")}
        onOpenRemoteWindow={openWindow && (() => setPalette("window"))}
        paletteShortcut={PALETTE_SHORTCUT}
      />
      {sidebarShown && (
        <div className="sidebar-slot">
          <SessionList
            state={state}
            renaming={renaming}
            onSelectTab={(id) => {
              dispatch({ type: "selectTab", id });
              closeDrawer();
            }}
            onCloseTab={(id) => dispatch({ type: "closeTab", id, fallbackId: newId("t") })}
            onNewTab={newTab}
            onFocus={(id) => {
              dispatch({ type: "focus", id });
              closeDrawer();
            }}
            onClose={(id) => dispatch({ type: "close", id })}
            onRename={rename}
            onRenameTab={(id, name) => dispatch({ type: "renameTab", id, name })}
            onRenameSession={(id, name) => dispatch({ type: "renameSession", id, name })}
            onMoveTab={(id, toIndex) => dispatch({ type: "moveTab", id, toIndex })}
            onMovePane={(id, toTabId) => dispatch({ type: "movePane", id, toTabId, aspect: layerAspect() })}
            onSwapPanes={(id, withId) => dispatch({ type: "swapPanes", id, withId })}
            onDragPane={setDragging}
            onNewWorkspace={newWorkspace}
            onSelectWorkspace={(id) => {
              dispatch({ type: "selectWorkspace", id });
              closeDrawer();
            }}
            onCloseWorkspace={closeWorkspace}
            onRenameWorkspace={(id, name) => dispatch({ type: "renameWorkspace", id, name })}
            onSetWorkspaceIcon={setWorkspaceIcon}
            onSetWorkspaceColor={setWorkspaceColor}
            compact={!narrow && isCompact(sidebarWidth)}
          />
          <SidebarResizer width={sidebarWidth} onResize={setSidebarWidth} />
        </div>
      )}
      {sidebarShown && narrow && <div className="drawer-backdrop" aria-hidden="true" onClick={() => setDrawerOpen(false)} />}
      <main className="workspace">
        {headerShown && (
          <header className="workspace-header">
            {onTop && (
              <>
                <WorkspaceMenu
                  where="tabbar"
                  labelled={false}
                  state={state}
                  renaming={renaming}
                  onRename={rename}
                  onSelect={(id) => dispatch({ type: "selectWorkspace", id })}
                  onNew={newWorkspace}
                  onClose={closeWorkspace}
                  onRenameWorkspace={(id, name) => dispatch({ type: "renameWorkspace", id, name })}
                  onSetIcon={setWorkspaceIcon}
                  onSetColor={setWorkspaceColor}
                />
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
              </>
            )}
            <div className="workspace-tools">
              {REMOTE_TARGET === null ? (
                <UpdateButton update={update} open={updateOpen} onOpenChange={setUpdateOpen} />
              ) : (
                <span className="remote-badge" title={stageText(link.stage, t)}>
                  <RemoteWindowIcon size={13} />
                  <span className="remote-badge-target">{REMOTE_TARGET}</span>
                  {link.stage?.stage === "connected" && (
                    <>
                      <span className="remote-badge-via">{t.remote.via[link.stage.via]}</span>
                      {link.stage.killOnLogout && (
                        <TriangleAlert size={13} className="remote-badge-warn" role="img" aria-label={t.remote.killOnLogout} />
                      )}
                    </>
                  )}
                </span>
              )}
            </div>
          </header>
        )}
        {REMOTE_TARGET !== null && link.connectedOnce && !link.connected && (
          <RemoteBanner target={REMOTE_TARGET} stage={link.stage} />
        )}
        {font && loaded ? (
          <PaneLayer
            state={state}
            font={font}
            colors={colors}
            focusNonce={focusNonce}
            renaming={renaming}
            dragging={dragging}
            onDragPane={setDragging}
            onRename={rename}
            onLaunch={open}
            onOpenUrl={openUrl}
            mayReattach={reattach}
            narrow={narrow}
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
          scheme={scheme}
          onChange={changeSettings}
          font={font}
          ghosttyFont={ghosttyFont}
          sizeOverridden={sizeOverridden}
          changeSize={changeSize}
          resetSize={resetSize}
          update={update}
          updates={REMOTE_TARGET === null}
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
      {(palette === "ssh" || palette === "window") && (
        <CommandPalette
          placeholder={windowsOnly ? t.palette.remoteWindowPlaceholder : t.palette.sshPlaceholder}
          commands={sshPaletteCommands}
          dynamic={sshDynamicCommands}
          empty={sshHosts === null ? t.palette.sshLoading : t.palette.sshEmpty}
          onClose={closePalette}
        />
      )}
      {REMOTE_TARGET !== null && !link.connectedOnce && <RemoteConnect target={REMOTE_TARGET} stage={link.stage} />}
      {away && (
        <div
          className="window-away"
          role="dialog"
          aria-modal="true"
          aria-labelledby="window-away-title"
          onMouseDown={reclaim}
        >
          <p id="window-away-title" className="window-away-title">
            {t.away.title}
          </p>
          <p className="window-away-hint">{t.away.hint}</p>
          <div>
            <button type="button" className="button primary" onClick={reclaim}>
              {t.away.useHere}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
