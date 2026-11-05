# Usage

## The screen

| Area | What it does |
|---|---|
| Activity bar | New shell, terminal-browser, terminal-code, SSH, and remote window at the top; a badge shows how many are open. The command palette, Settings, and (in the sidebar layout) the sidebar switch at the bottom. |
| Layout | Settings ▸ General ▸ Workspaces and tabs picks one. **Top** (the default): the workspace menu and the tab bar across the top, and no sidebar. **Sidebar**: the workspace menu and the tabs in the sidebar, and no tab bar. |
| Tab bar (top layout) | The workspace menu at the left, then the tabs. `+` or a double-click on empty space opens a tab. A middle click closes one. Drag to reorder. |
| Sidebar (sidebar layout) | The workspace menu, then the tabs of the workspace in view, with their sessions as a tree. Collapse, rename, and drag to reorder or move. Drag its right edge to resize it; drag it narrow and it shrinks to icons: the workspace icon, tab numbers, and the session icons of the tab in view. |
| Panes | Drag a pane's header onto another pane's edge to split there, onto its center to swap, or onto a tab or `+` to move it. Drag a divider to resize. |
| Update button | At the top right when a new release exists. |

## Renaming

Double-click a tab or a session, or press F2. In the workspace menu, use the pencil or F2. Clear the name to go back to the automatic one: the title the shell sends, or "Workspace 1", "Workspace 2", and so on.

## Workspaces

A workspace is a set of tabs, for example one per project.

- Only the tabs of the workspace in view show.
- The sessions of the other workspaces keep running. Switching back shows the same tab and pane as before.
- The workspace menu is at the left of the tab bar (top layout) or at the top of the sidebar (sidebar layout). Its button shows the workspace in view by its icon and color, and in the sidebar also by its name.
  - Click a workspace to show it. The arrow keys move in the menu, Enter picks, and Escape closes it.
  - "New workspace" opens one with one shell.
  - The pencil renames a workspace, and `×` closes it.
  - Click a workspace's icon to pick its color (8 colors) and its icon (12 icons). Picking a color keeps the choices open; picking an icon closes them. Both are saved with the layout.
  - A new workspace takes the first color that no other workspace has.
  - The tabs show the color of the workspace in view: the line under the tab in view and its number, in the tab bar and the sidebar, and the selected session in the sidebar.
- Closing a workspace ends all its sessions. Closing the last one leaves an empty workspace.
- In the command palette: new workspace, rename, close, next and previous workspace, move the tab to another workspace, and go to a workspace. Going to a session in another workspace shows that workspace.
- Tab keys (Ctrl+Shift+1–9, ⌘1–9 in the macOS app) count only the tabs of the workspace in view.
- A layout saved before workspaces opens as one workspace with all its tabs.

## Command palette

- Open it with ⌘K on macOS, or Ctrl+Shift+K on Linux and Windows.
- It launches sessions, switches workspaces, tabs, and sessions, renames, maximizes, moves panes and tabs, changes the font size and language, opens settings, and checks for updates.
- Type a URL to open it in terminal-browser.
- English words find commands in any language, for example `rename`, `font`, or `language`.

## Panes and layout

Each tab arranges its panes as a **split tree**, the way tiling window managers and VS Code editor groups do.

- **Docking**: drag a pane's header over another pane to see where it would land.
  - An edge (left, right, top, bottom) splits that pane in half and puts the dragged pane on that side.
  - The center swaps the two panes.
- **New panes** split the focused pane along its longer side.
- **Resizing**: dragging a divider changes only the two panes on either side of it. Arrow keys nudge it; a double-click or Enter makes that split even again.
- Layouts and proportions are saved per tab.

## Switching terminals from the keyboard

The same keys work in the browser and in the macOS app. Browsers keep ⌘1–9 and similar keys for their own tabs, so the browser keys use Ctrl+Shift. The macOS app also takes the terminal-app keys used by iTerm and Ghostty.

| Action | Browser and app | macOS app also |
|---|---|---|
| Tab 1–8 | Ctrl+Shift+1 … 8 | ⌘1 … ⌘8 |
| Last tab | Ctrl+Shift+9 | ⌘9 |
| Next / previous tab | Ctrl+Shift+] / [ | ⌘⇧] / ⌘⇧[ |
| Next / previous pane | Ctrl+Shift+. / , | ⌘] / ⌘[ |
| Pane to the left, right, above, below | — | ⌥⌘ + arrow keys |

- The keys work while a terminal has the keyboard. In the palette, a rename field, or Settings, they type as usual.
- Ctrl+[ without Shift stays Escape for programs in the terminal.
- In the macOS app the Window menu lists the same commands.

## Settings

Open Settings from the gear at the bottom of the activity bar, from the palette, or with ⌘, in the macOS app. Pick a page from the menu on the left: General, Appearance, Terminal, or Updates. Changes apply at once and are saved in this browser (or the app).

| Page | Setting | Effect |
|---|---|---|
| General | Language | Same as the browser, or English, Korean, Japanese, or Simplified Chinese. |
| General | Workspaces and tabs | Top: the workspace menu and the tab bar, with no sidebar. Sidebar: the workspace menu and the tabs in the sidebar, with no tab bar. |
| Appearance | Mode | System (follows the OS, live), Light, or Dark. Terminal colors follow it, and programs that ask for the background color (OSC 11) pick a matching theme. |
| Appearance | Color theme | Default, One, GitHub, Solarized, Gruvbox, Catppuccin, Tokyo Night, Rosé Pine, Everforest, or Kanagawa. Colors the window and the terminals; each theme has a light and a dark set, which follow Mode. Default keeps the window's own colors. Programs that ask for the background color (OSC 11) get the theme's. |
| Terminal | Font size | Overrides the Ghostty `font-size`. "Use the Ghostty size" goes back. |
| Terminal | Font | Overrides the Ghostty `font-family`; separate fallbacks with commas. Leave it empty for the Ghostty font. Applies on Enter or when the field loses focus. |
| Updates | Check for updates automatically | Asks GitHub for the newest release when the dashboard opens and every 6 hours. With it off, westerm contacts GitHub only when you press Check now. A remote window has no Updates page. |

## Updates

- **Dashboard**: when a newer release exists, an update button appears in the header. It shows the version and links to the release notes.
  - An installed westerm downloads the new release, checks its SHA-256 sum and version, replaces itself, and restarts. The page reloads when the new version answers.
  - The new version replaces the PTY host, so open shells come back once as "restored".
  - A build from source shows how to update instead: `git pull` and `npm run build`.
- **Command line**: `westerm update` installs the newest release; `westerm update --check` only reports it.
- **macOS app**: westerm ▸ Check for Updates downloads `westerm-macos-app.zip`, checks it, replaces the app, and opens the new copy. The update button in the app window does the same.
- `WESTERM_NO_UPDATE_CHECK=1` turns update checks off on that machine.

## Languages

- The UI is in English, Korean, Japanese, and Simplified Chinese.
- It starts in the browser's language, or English when that language is not one of these.
- Change it in the header, in Settings, or with the palette's `Language:` commands.
- Text the server writes into terminals follows it too: the restore notice and session errors. It uses the language the pane had when it connected.
- Han characters use the chosen language's fonts first, so Japanese shows Japanese glyphs and Chinese shows Simplified glyphs.

## SSH

- The SSH launcher and the palette's SSH command list the hosts in `~/.ssh/config` (with Include). You can also type `user@host`.
- **Remote shell**: runs `ssh <host>` in a pane. Like a local shell, it survives reloads and dashboard restarts.
- **Remote browser**: opens `terminal-browser --ssh <host>`. Pages load through the remote server, so `localhost:3000` is that server's dev server.
- Authentication uses your SSH keys and agent. Type passwords into the terminal when a server asks; the dashboard never stores or sends them.

## Remote windows

A window for a Linux server: its tabs, workspaces, and shells live on the server and keep running when your network does not.

- Open it from the activity bar (the server icon) or the command palette: pick a host from `~/.ssh/config` under "Open in a new window", or type `user@host`.
- In a browser, it opens a new browser window. The browser may block it as a popup: allow popups for `127.0.0.1`. The window's address is `/?remote=<target>` on the dashboard, for example `http://127.0.0.1:4317/?remote=prod`. In westerm.app, it opens an app window; the window of each server remembers its size and position. Opening the same server again brings its window to the front.
- The window title is `westerm — <target>`. The header shows the server name and how the window is connected: `QUIC`, or `SSH tunnel` when UDP does not get through.
- The first time, a connecting screen shows the stages: checking the server, installing westerm, starting, connecting. westerm copies the same version of itself to `~/.local/bin/westerm` on the server over SSH and starts it there. The server needs no internet access.
- Tabs, workspaces, and layout are saved on the server. Shells, terminal-browser, and terminal-code run on the server, started by your login shell there. Programs are found on the PATH of your interactive login shell on the server, so folders that `~/.zshrc` or a similar file adds (such as `~/.local/bin`) count. This applies once westerm next starts the server's host, for example after `westerm stop` there. Plain SSH panes and remote browsers opened inside a remote window run from the server too.
- Closing the window or losing the network does not end anything. Open the server again to see the same tabs and shells, also from another computer. See [Remote sessions](sessions.md#remote-sessions).
- A window that comes back (after sleep or a lost network) reads the server's layout first. If another computer changed the tabs meanwhile, the window takes the server's layout, and its own changes from while it was away are lost.
- Known limit: a window that comes back also takes the sessions from the window in use, which then shows "In use in another window". Click that window to take them back.
- When the link drops, a "Reconnecting…" banner shows. The panes reconnect once the link is back and the server's layout is read. If the failure cannot fix itself (for example the SSH key is refused), the window shows the reason and a **Try again** button.
- Mode, color theme, language, and font size are shared with the local window. Fonts follow the Ghostty config of this computer.
- A remote window cannot open another remote window.
- The plain SSH pane stays as it is, for servers where you do not want to install anything.

| Needs | |
|---|---|
| Server | Linux on x86_64 or aarch64 |
| Login | An SSH key or ssh-agent. Password and one-time-code logins are not supported. |
| Host key | Already in `~/.ssh/known_hosts`. For a new server, connect once with a plain SSH pane and accept its key. |
| Firewall | UDP 4317 open on the server (4318–4326 when another user's westerm holds 4317). Without it, westerm falls back to an SSH tunnel. |
| macOS, server on your LAN | Local Network access for the app that runs westerm: your terminal app, or westerm.app (System Settings ▸ Privacy & Security ▸ Local Network). Without it, macOS blocks UDP to local addresses and westerm uses the SSH tunnel. Servers on the internet do not need it. |

### Updating a server

- A remote window has no update button, no update commands, and no update section in Settings. Update westerm on this computer. The server follows at the next connection: westerm installs the new version there, then asks about the host if shells run. When westerm on this computer updates, open remote windows reload.
- When the server's westerm host is older than this one and has open shells, the connecting screen asks before replacing it: replacing ends the programs in those shells, which then come back from their saved screens. **Replace now** does it. **Keep the old host** keeps them running; the window's command palette has **Update this server's westerm host** to replace it later.
- westerm never replaces a newer westerm on the server. The window shows "The server has a newer westerm. Update westerm here first".
- Two computers with different westerm versions: the newer one wins. On the older computer, update westerm first.

### On the server

- `westerm stop` stops the gate, the remote dashboard (the part that serves the layout and sessions to windows), and then the host. Shells end after their screens are saved. The next connection from a window starts westerm there again, and the shells come back as "restored".
- To stop a computer from connecting, delete its line from `~/.local/state/westerm/remote/authorized_clients`.
- If the server ends your processes at logout (systemd-logind `KillUserProcesses=yes`), run `loginctl enable-linger` once, or the shells end with your SSH login.

| Variable | Where | Effect |
|---|---|---|
| `WESTERM_REMOTE_TRANSPORT` | this computer | `quic` or `tunnel`: use only that way. |
| `WESTERM_REMOTE_BINARY_DIR` | this computer | A folder with `westerm-<target>` Linux builds to install, for source builds. |
| `WESTERM_RELEASES_URL` | this computer | Another copy of the GitHub releases, as for `install.sh`. |
| `WESTERM_GATE_PORT` | the server | The gate's UDP port; `0` picks any free port. |

### From v0.2.0

The remote westerm pane of v0.2.0 is gone. A saved layout drops those panes when it loads; a layout with only remote panes opens with one shell. Their shells keep running on the server until you run `westerm stop` there.

## Fonts and themes

- The server reads `font-family` and `font-size` from your Ghostty config (`/api/terminal-config`).
- Ghostty has the Nerd Font icons built in; browsers do not. So the font stack adds the Nerd Fonts you have installed after your font.
- Light and dark follow the OS unless Settings picks one. Terminal colors switch with it, in the color theme picked in Settings (`src/terminalThemes.ts`, `WTerm.setThemeColors`).
