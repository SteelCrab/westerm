# Usage

## Git sidebar

The folder-shaped Git button below Agents opens the repository of the focused terminal. It follows that terminal's folder. Choose another workspace folder from the list, or enter an absolute folder path to pin a repository. Choose **Follow focused terminal** to follow again.

- The panel shows the current branch, changed files, the upstream's last known ahead/behind counts, and recent commits.
- Select a recent commit to read its full message, author, timestamp, and changed files. Select a committed file to view its patch. Merge commits compare with their first parent; the comparison is labeled.
- Select a file to read its staged or unstaged diff. A partially staged file appears in both groups.
- Use `+` and `−` to stage or unstage a file. The section buttons apply to the listed files. Unstaging keeps working files intact.
- Enter a message and choose **Commit** to commit the staged changes. Git hooks and Git's configured author identity still apply.
- Select a local branch to switch branches. Git refuses a switch that would overwrite changes or use a branch checked out in another worktree.
- **Pull** uses fast-forward only. **Push** uses the configured upstream. Both require an upstream branch. Neither runs automatically.
- The refresh button reloads the repository. While open, the panel also refreshes every five seconds and when the window regains focus.
- Git errors, such as missing identity, conflicts, authentication failure, and large diffs, appear in the panel. Interactive authentication is unavailable; configure credentials in the terminal first.
- Linked Git worktrees are supported. In a remote window, the panel runs Git on that server. The panel uses the current theme and opens as a drawer on narrow windows.

## The screen

| Area | What it does |
|---|---|
| Activity bar | New shell, terminal-browser, terminal-code, SSH, docs, and remote window at the top, then the Agents, Git, and Tunnel buttons; a badge shows how many panes are open. The plugin bar sits between them and the bottom group. The command palette, Settings, and (in the sidebar layout) the sidebar switch at the bottom. See [Agents sidebar](#agents-sidebar) and [Plugins](#plugins). |
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
- It launches sessions, switches workspaces, tabs, and sessions, renames, maximizes, moves panes and tabs, changes the font size and language, opens settings and the plugin manager (**Plugins…**), and checks for updates.
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

## Small screens

In a window 640px wide or less, such as a phone:

- The activity bar is at the bottom, so the terminals get the full width.
- In the sidebar layout, the sidebar opens as a drawer over the panes, from the button at the right end of the bottom bar. It starts closed. A pick in it, a tap outside it, or Escape closes it.
- The panes of a tab stack top to bottom, each the same height. The split layout is kept: a wider window shows it again.
- Pane headers show the name and the status dot only.

## Switching terminals from the keyboard

The same keys work in the browser and in the desktop app. Browsers keep ⌘1–9 and similar keys for their own tabs, so the browser keys use Ctrl+Shift. The macOS app also takes the terminal-app keys used by iTerm and Ghostty. On Windows and Linux the app uses the Ctrl+Shift keys, since ⌘ there is the Windows or Super key.

| Action | Browser and app | macOS app also |
|---|---|---|
| Tab 1–8 | Ctrl+Shift+1 … 8 | ⌘1 … ⌘8 |
| Last tab | Ctrl+Shift+9 | ⌘9 |
| Next / previous tab | Ctrl+Shift+] / [ | ⌘⇧] / ⌘⇧[ |
| Next / previous pane | Ctrl+Shift+. / , | ⌘] / ⌘[ |
| Pane to the left, right, above, below | — | ⌥⌘ + arrow keys |

- The keys work while a terminal has the keyboard. In the palette, a rename field, or Settings, they type as usual.
- Ctrl+[ without Shift stays Escape for programs in the terminal.
- In the macOS app the Window menu lists the same commands. The app has a menu bar on macOS only.

## Settings

Open Settings from the gear at the bottom of the activity bar, from the palette, or with ⌘, in the macOS app. Pick a page from the menu on the left: General, Appearance, Terminal, or Updates. Changes apply at once and are saved in this browser (or the desktop app).

| Page | Setting | Effect |
|---|---|---|
| General | Language | Same as the browser, or English, Korean, Japanese, or Simplified Chinese. |
| General | Workspaces and tabs | Top: the workspace menu and the tab bar, with no sidebar. Sidebar: the workspace menu and the tabs in the sidebar, with no tab bar. |
| Appearance | Mode | System (follows the OS, live), Light, or Dark. Terminal colors follow it, and programs that ask for the background color (OSC 11) pick a matching theme. |
| Appearance | Color theme | Default, One, GitHub, Solarized, Gruvbox, Catppuccin, Tokyo Night, Rosé Pine, Everforest, or Kanagawa. Colors the window and the terminals; each theme has a light and a dark set, which follow Mode. Default keeps the window's own colors. Programs that ask for the background color (OSC 11) get the theme's. |
| Terminal | Font size | Overrides the Ghostty `font-size`. "Use the Ghostty size" goes back. |
| Terminal | Font | Overrides the Ghostty `font-family` with a list of up to 8 fonts, the first one first; glyphs a font lacks come from the next. **Add font** searches the fonts installed on this computer (monospaced only, unless unticked) and tags those with your language's script or Nerd Font icons; a name that is not installed can be typed in. Drag a font, or press Alt+↑/↓, to reorder; Delete or × removes it. Each change applies at once. "Use the Ghostty setting" empties the list. |
| Terminal | Resume AI agents | On (the default): a shell that comes back as "restored" runs the Claude Code, Codex, or Gemini CLI conversation it had again. See [Sessions ▸ AI agents](sessions.md#ai-agents). |
| Updates | Check for updates automatically | Asks GitHub for the newest release when the dashboard opens and every 6 hours. With it off, westerm contacts GitHub only when you press Check now. A remote window has no Updates page. |

## Updates

- **Dashboard**: when a newer release exists, an update button appears in the header. It shows the version and links to the release notes.
  - An installed westerm downloads the new release, checks its SHA-256 sum and version, replaces itself, and restarts. The page reloads when the new version answers.
  - The new version replaces the PTY host, so open shells come back once as "restored".
  - A build from source shows how to update instead: `git pull` and `npm run build`.
- **Command line**: `westerm update` installs the newest release; `westerm update --check` only reports it.
- **Desktop app**: the update button in the app window updates the whole app. On macOS, westerm ▸ Check for Updates opens the same panel. The panel is about the app's own version, also when the app shows a westerm started in a terminal; update that one with `westerm update`.
  - macOS: downloads `westerm-macos-app.zip`, checks its SHA-256 sum and version, replaces the app, and opens the new copy.
  - Windows: downloads `westerm-windows-app-setup.exe` and checks its SHA-256 sum. The app stops its dashboard, the PTY host saves every session and stops, the app quits, and the installer runs. Open the app again when it is done.
  - Linux: the app opens the release page in your browser. Install the new `.deb` or `.rpm` from there.
  - A browser on a westerm that the app started shows "Update it from the app" instead.
- `WESTERM_NO_UPDATE_CHECK=1` turns update checks off on that machine.

## Languages

- The UI is in English, Korean, Japanese, and Simplified Chinese.
- It starts in the browser's language, or English when that language is not one of these.
- Change it in the header, in Settings, or with the palette's `Language:` commands.
- Text the server writes into terminals follows it too: the restore notice and session errors. It uses the language the pane had when it connected.
- Han characters use the chosen language's fonts first, so Japanese shows Japanese glyphs and Chinese shows Simplified glyphs.

## AI agents

While Claude Code, Codex, or Gemini CLI runs in a shell (macOS and Linux):

- The pane header shows an AI mark in place of the shell icon: the agent's logo (orange for Claude Code, the OpenAI logo for Codex, colored for Gemini CLI). The sidebar's session list shows it too. Point at it for the agent, its state, and its title.
- The ring around the mark shows the state:

| Ring | State |
|---|---|
| Blue, turning | Working |
| Yellow, pulsing | Needs your input: a permission, approval, or a question with choices |
| Green | Done: it finished while you were not looking at the pane. It turns gray when you look. |
| Gray | Idle |

- A tab whose agent works shows a small turning ring; one that asks, a yellow dot; one that finished out of view, a green dot.
- With reduced motion turned on in the OS, the rings do not move.
- Press the mark in the pane header (or move to it with Tab and press Enter) for the agent's card. Press the mark again, press outside, or press Esc to close it. The card shows:
  - The agent and its version (Claude Code only), its state, and for how long.
  - The conversation id with **Copy**. Codex and Gemini CLI show "The folder's latest conversation" unless the command line names the id (`gemini --resume <id>`), since westerm cannot tell it otherwise.
  - The folder, the model when the command line names one, and when westerm first saw the agent.
  - The command a reboot runs, with **Copy**.
  - **Resume in this pane**: clear it to keep this pane's agent from resuming. The layout keeps the choice. When Settings ▸ Terminal ▸ Resume AI agents is off, the box is off for every pane.
- After a reboot the agent resumes its conversation; see [Sessions ▸ AI agents](sessions.md#ai-agents).

## Agents sidebar

A list of the AI agents in the workspace in view, grouped by the folder they run in. It shows Claude Code, Codex, and Gemini CLI.

### Open it

- Press **Agents** at the top of the activity bar, below the launchers. Point at it for "AI agents in this workspace, by folder". A yellow dot on the button means an agent in any workspace needs your input.
- Top layout: the button opens and closes the panel.
- Sidebar layout: the button switches the sidebar between the sessions list and the agents panel. The sidebar switch still opens and closes the sidebar.
- Windows 640px wide or less: the panel opens as a drawer, like the sidebar. A pick in it, a tap outside it, or Escape closes it.
- This browser remembers whether the panel is open.
- At the top is the workspace menu. The workspace you pick is the one the panel shows. "Elsewhere N" next to it counts the agents in other workspaces that need your input.
- Dragged narrow, the panel shows icons only. Point at a folder for its full path, and at an agent for the agent, its state, and the session name.

### Folders

- Only shells are listed. SSH, terminal-browser, terminal-code, and docs panes are not.
- A shell belongs to the folder its agent runs in. With no agent, it belongs to the folder of the shell itself.
- A shell whose folder is not known yet goes under "Unknown folder". On Windows every shell does, and the panel says "Agents show on macOS and Linux".
- Folders keep the order in which their panes first show: tab order, then pane order. A state change does not move a row.
- A folder row shows:
  - the folder name in bold, with its parent in gray: `~/` when the parent is your home folder, else `<parent name>/`; your home folder is `~`;
  - the git branch, or the first 7 characters of the commit on a detached HEAD;
  - the number of shells and of agents, and a yellow dot when an agent needs your input.
- Point at a folder row for its full path. Click it to fold or unfold the folder. The folded state is not saved.

### Agents in a folder

Each agent row shows the agent's logo, its name (as in the session list), how long it has been in this state, and a sign:

| Sign | State |
|---|---|
| Yellow dot, pulsing | Needs your input. The row adds "Needs your input", and the time is yellow. |
| Blue ring, turning | Working |
| Green check | Done: it finished while you were not looking at the pane. The time says how long ago. |
| Gray dot | Idle |

- Within a folder the order is: needs your input, done, working, idle. Equal states keep pane order.
- Click a row to go to its pane. The row of the focused pane has the workspace color.
- With reduced motion turned on in the OS, the signs do not move.

### Start an agent

- A folder with no agents shows "No agents" and "Start here" with one button for each agent: **Start Claude Code here**, **Start Codex here**, and **Start Gemini CLI here**. A folder with agents shows the same buttons when you point at its row.
- A button opens a new shell pane in that folder. When its prompt is up, westerm types `claude`, `codex`, or `gemini`.
- You must have the agent installed. westerm does not install it.

### Past conversations

- Under each folder: "Past conversations N", open at first. It lists up to 5 conversations that ended in that folder, newest first, from Claude Code, Codex, and Gemini CLI. Each line has the agent's logo, the title, when it ended, and **Resume**. A conversation without a title is "Untitled conversation".
- Conversations that an agent in this dashboard runs now are not listed. Codex and Gemini CLI do not tell which conversation they run, and neither does a Claude Code that just started. While such an agent runs in the folder, its kind's conversations written since it started are not listed either.
- **Resume** opens a new shell pane in that folder and types `claude --resume <id>`, `codex resume <id>`, or `gemini --resume <id>`.
- The list is read when the panel opens, and again when a conversation running in that folder changes. A list read less than 30 seconds ago shows again without a new read, for example when you unfold a folder or switch workspaces. westerm reads only ids and titles from the agents' own files (see [Sessions ▸ AI agents](sessions.md#ai-agents)), and sends no conversation text.
- Where westerm looks:

| Agent | Folder | Title from |
|---|---|---|
| Claude Code | `~/.claude/projects` (`$CLAUDE_CONFIG_DIR` when set) | Its custom or AI title, else its summary, else your last prompt |
| Codex | `~/.codex/sessions` (`$CODEX_HOME` when set) | Its `session_index.jsonl` |
| Gemini CLI | `~/.gemini/tmp` (`$GEMINI_CLI_HOME/.gemini` when set) | Its summary, else your first message |

- Limits: a file is read up to 16 MiB; at most the 500 newest Codex files are checked; Codex's `session_index.jsonl` is read from its last 4 MiB; Gemini folders are matched by name, or by up to 256 `.project_root` files. Titles are cut to 120 characters.
- In a remote window, the panel shows the server's shells, folders, and past conversations.

## Plugins

A plugin adds a panel to westerm. Docker and Databases are built in: installing one turns it on, and nothing is downloaded. Other plugins come from outside (the plugin list, a GitHub repository, a `.tar.gz` link, or a folder on your computer) and run in an isolated frame with only the permissions you confirm. See [External plugins](#external-plugins). To write one, see [Writing plugins](plugins.md).

### The plugin bar

- The plugin bar is the group of buttons in the activity bar between the launchers at the top and the bottom group (Agents, palette, Settings). Lines above and below set it apart.
- Each installed plugin has a button. The last button is the puzzle piece. It opens the plugin manager. With nothing installed, you see only the puzzle piece.
- A button of an external plugin has a small yellow mark, and its panel header says `external`.
- A plugin button can show a number badge and a red dot. The plugin decides what they mean. For Docker, see [Docker](#docker).
- A plugin button opens the plugin's panel in the sidebar's place, the same way the Agents button does:
  - Top layout: the button opens and closes the panel.
  - Sidebar layout: the button switches the sidebar between the sessions list and the panel. Press it again to go back to the sessions list.
  - Windows 640px wide or less: the panel opens as a drawer.
- This browser remembers which panel is open. A panel whose plugin is not installed is closed.
- A remote window and a page opened through a [tunnel](#tunnel) have no plugin bar: neither has the plugin routes. A pane that a plugin started, such as Docker logs, still opens there.

### The plugin manager

- Open it with the puzzle piece, or with **Plugins…** in the command palette.
- It is a dialog like Settings. At the top left, **Installed** and **Browse** switch the list, and a search box filters it by name, description, and author (the part of the id before the dot). **Installed** lists what you installed, built-in plugins included. **Browse** lists the plugin list and what you installed. Below the list is **Install from URL or folder…**. The plugin list is a GitHub repository, `SteelCrab/westerm-plugins`, that does not exist yet. Until it does, **Browse** shows "Could not load the plugin list" and **Retry**, and installs from a URL or a folder work as usual.
- Each plugin has a badge:

| Badge | Meaning |
|---|---|
| `built-in` | Part of westerm (Docker, Databases). |
| `verified` | Installed from the plugin list. |
| `unverified` | Installed from a URL. |
| `local` | Installed from a folder, for developing a plugin. |

- The right side shows the plugin you picked: its name, version, author, source (a repository address or a folder), description, what **This plugin can** do, and its buttons. Each permission has a risk: `run` is high, `panes` is medium, `background` is low.
- A built-in plugin's "can do" list is a description. westerm does not enforce it, because a built-in plugin is westerm's own code. For an external plugin the list is enforced (see [External plugins](#external-plugins)).
- **Install** of a built-in plugin asks for no confirmation. The button appears in the plugin bar and its panel opens. No restart is needed.
- **Uninstall** removes the button, stops the plugin's background checks, and makes its server routes answer 404. If its panel was open, the sidebar goes back to the sessions list (the top layout closes it). Panes the plugin opened stay, because they are ordinary shells.
- With nothing installed (the default), the page makes no plugin request except `GET /api/plugins`, and westerm runs no `docker` process. westerm downloads the plugin list only when you open the plugin manager. The server also looks for updates once a day, but only when an installed plugin came from the plugin list or a GitHub repository.
- If the plugin list cannot be loaded (for now it always cannot, because the repository does not exist yet), **Browse** shows "Could not load the plugin list" and **Retry**. Installing from a URL or a folder still works.

### External plugins

An external plugin is a folder of files that someone wrote. westerm shows its panel in a sandboxed frame. The frame cannot read your sessions, terminal output, layout, or other plugins, and cannot make network requests itself. It reaches westerm only through messages, and the server checks each one against the permissions you confirmed.

#### Install

1. Open the plugin manager.
2. From the plugin list (not available until the plugin list repository exists): pick the plugin in **Browse** and press **Install**. From elsewhere: press **Install from URL or folder…**, choose **URL** or **Folder**, enter the address, and press **Next**.
   - **URL**: `https://github.com/<owner>/<repo>` (westerm takes `westerm-plugin.tar.gz` from the latest release), or an `https://` link to a `.tar.gz` file.
   - **Folder**: an absolute path on this computer. westerm uses the folder in place and copies nothing.
3. westerm downloads and checks the package, then shows its name, version, size, badge, and what it can do. A `run` permission reads "Can run kubectl with any arguments, as you". That means the plugin is as trusted as you are with that program: westerm does not limit the arguments.
4. For `unverified` and `local` plugins, read the warning and tick **I trust \<source\>**. **Install** stays locked until you do. A `verified` plugin shows the permissions and **Install**.
5. After **Install**, the plugin's button appears in the plugin bar and its panel opens. The manager closes. No restart is needed.

Every package is checked before it is unpacked: `.tar.gz` only, at most 5 MiB packed and 10 MiB unpacked, at most 300 files and folders, regular files and folders only, and no path that leaves the package. A plugin that needs a newer westerm (`plugin.json` `westerm`) is refused. Downloads use `https://` only. A package from the plugin list must also match the SHA-256 sum in the list.

A note on trust: westerm blocks the plugin's direct network requests, but a bad plugin can still send out the data it receives, for example by WebRTC. Install a plugin only if you trust where it comes from.

#### Update

- westerm looks for new versions when you open the plugin manager, and once a day. A plugin with a new version shows **Update** in the list and **Update to vX.Y.Z** in its details. westerm never updates by itself.
  - From the plugin list: the list's version is compared with yours.
  - From a GitHub repository: the latest release tag is compared (a tag must be a plain version such as `v1.2.3`; a tag with a suffix such as `-beta` is refused).
  - From a `.tar.gz` link: there is no check. The plugin shows **Reinstall to update**. Reinstalling replaces the installed version in one step and keeps the plugin's saved data. If it fails, the installed version stays.
  - From a folder: there is no check. Press **Reload** after you change a file. It reads the files again and restarts the panel.
- An update that asks for the same permissions or fewer needs one confirmation. An update that asks for more (a new program, a new permission) shows what it adds and asks again. westerm applies only the permissions you confirmed.
- When the update is done, the old version's files are deleted. An open panel restarts with the new version.

#### Uninstall

**Uninstall** asks once, then deletes the plugin's files and its saved data. For a folder plugin, the folder itself stays ("The folder itself stays."). Panes the plugin opened stay, because they are ordinary shells. A window that still shows the plugin finds out when it reads the installed list again, and closes the panel without an error.

#### Things to know

- External plugins have no on/off switch. Installing turns one on, and uninstalling deletes it.
- A plugin keeps one small JSON value (up to 64 KiB) on the server, in `plugins/<id>/data.json` in the state folder.
- A plugin's shell pane types its command into your new shell. This works on macOS and Linux, in the sh family (`sh`, `bash`, `zsh`, `dash`, `ksh`) and `fish`. With another shell (`nu`, `pwsh`, `xonsh`, `elvish`, and also `ash`, `mksh`, `yash`, for example Alpine's default), and on Windows, the plugin's call fails with "Your shell is not supported for plugin panes" and no pane opens. See [Writing plugins](plugins.md#openshell).
- A plugin that tries to leave its page is stopped: the bar shows "Plugin stopped: it tried to leave its page", and opening the panel again starts it fresh.
- A plugin cannot open westerm's own address in a browser pane. Other local addresses, such as a dev server, open normally.
- A remote window and a tunnel page show no plugins.
- A plugin's programs run with the server's environment plus the whole environment of your login shell, so tools find `PATH`, `KUBECONFIG`, and similar settings as in your shells.

### Where the list is kept

- The installed list is saved on the server, in `plugins.json` in the state folder. It is the same for every browser and window of this westerm, and the server uses it to refuse routes of plugins that are off. For an external plugin it also holds where the plugin came from and the permissions you confirmed. The server checks requests against those, not against the plugin's own `plugin.json`. Package files of a plugin from the list, a repository, or a link are in `plugins/<id>/<version>/`. A folder plugin has no such folder: westerm uses your folder in place.
- A page reads the list when it opens, and again when it becomes visible or gets focus. So a window shows an install or an uninstall made in another window when you switch to it. A plugin route that answers 404 (uninstalled elsewhere) also makes the page read the list again. Its panel closes without an error.

### Docker

Install **Docker** to see this computer's containers in the sidebar.

- **Header**: the Docker mark, "Docker", the engine name `local`, and the Docker server's version. The refresh button reads the list now. The ⋯ menu has **Uninstall…**.
- **Filter**: type to filter by container name, image, or compose project. It filters on the page only. At the right: "running N / total M".
- **Groups**: containers are grouped by compose project (the `com.docker.compose.project` label). A group row shows the project name, its working folder (`~` for your home), and a "running / total" badge. The badge turns red when a container in the group exited with a code other than 0. Click a group row to fold or unfold it (not saved). Containers outside compose are under **Other containers**, at the end. Groups sort by project name, containers by name, so a row does not move when its state changes.
- **Container row**: a status dot, the compose service name (else the container name), a chip for each published TCP port (`:8080`), and the image below.

| Dot | State |
|---|---|
| Green | Running ("healthy" next to the name when the container says so) |
| Yellow | Unhealthy, paused, or restarting |
| Red, with "Exited (code) · when" | Exited with a code other than 0 |
| Empty gray circle | Exited with code 0, or created |
| Turning circle | An action is running on it |

- **Click a row** to open its logs in a new shell pane (`docker logs -f --tail 200 <id>`), split next to the focused pane. If this workspace already has that container's logs pane, you go to it instead.
- **Click a port chip** to open `http://localhost:<port>` in a terminal-browser pane.
- **Point at a row** for buttons: Logs, Shell, Restart, and Stop (**Start** for a stopped container). **Shell** opens a new shell pane that runs `docker exec -it <id>` with bash if the container has it, else sh.
- **Right-click** (or the menu key, or Shift+F10) for the menu: Logs, Shell, Open :port in browser (one per port), Start or Stop, Restart, Remove…, Copy container ID. Tab moves between rows, and Enter opens the logs.
- **Remove…** exists for stopped containers only, and asks once: "Remove {name}? This deletes the container. Its image and volumes stay." It never uses `-f`.
- **Start all** and **Stop all** on a group row run `docker start` or `docker stop` for that group's containers. westerm does not use `docker compose`, so it works without the compose plugin.
- While an action runs, the row's dot turns and its buttons lock. When it ends, the list is read again. If it fails, Docker's first error line shows under the row.
- A log or shell pane is an ordinary shell session. It stays when you close the window or restart westerm. After a restart or reboot it comes back as a plain shell, and the command is not typed again. When the container stops, the shell stays open.
- The shell opens in the compose working folder when it exists, else in the default folder.

#### Badge and red dot

- The page asks the server for the containers every 2 seconds while the Docker panel is open, and every 15 seconds while it is closed. A hidden page asks nothing. Each window asks on its own.
- The badge on the Docker button is the number of running containers. It is hidden at 0.
- The red dot turns on when a container that was running at the last check stops with a code other than 0, or becomes unhealthy. It turns off when you open the Docker panel. A new page starts with it off, and a container that stopped long ago never sets it.
- Badge and dot are hidden while Docker is not available.

#### When Docker is missing or off

| Panel shows | Meaning |
|---|---|
| "Docker CLI not found" | `docker` is not on the PATH. On macOS and Windows the panel shows the install command (`brew install --cask docker`, `winget install Docker.DockerDesktop`) with **Copy**; on Linux a link to Docker's install guide. westerm never types commands from the browser: copy the command and run it in a shell. |
| "Docker is not running", and Docker's first error line | The engine does not answer, or you may lack permission (on Linux, the `docker` group). Checks go on every 15 seconds, and the list appears by itself when the engine is up. **Retry** checks now and reads your login shell's environment again. |
| "No containers" | Docker works, and there are no containers. |

- **Which `docker`**: a desktop app started from Finder gets a short PATH, without `/opt/homebrew/bin` or `DOCKER_HOST` from your `.zshrc`. So the server runs your login shell once the first time it checks Docker and keeps only `PATH`, `DOCKER_HOST`, `DOCKER_CONTEXT`, `DOCKER_CONFIG`, `DOCKER_CERT_PATH`, and `DOCKER_TLS_VERIFY`. It runs `docker` with them, so the list and the log and shell panes see the same Docker. If the shell takes more than 5 seconds, the server's own environment is used. Install and **Retry** read it again. On Windows the server's environment is used.
- A `docker` call that takes more than 5 seconds counts as "Docker is not running" ("timed out"). Start, stop, restart, and remove wait up to 30 seconds, because Docker waits up to 10 seconds for a container to stop.
- Not in this version: images, volumes, networks, `docker context` choice, remote engines, CPU and memory, compose up and down, and Docker in remote windows.

### Databases

Install **Databases** to browse PostgreSQL, MySQL/MariaDB, and SQLite databases from the sidebar.

- **Header**: "Databases" and the number of saved connections. **＋** adds a connection, the refresh button reads the open levels and looks for connections again, and the ⋯ menu has **Forget all connections…** and **Uninstall…**.
- **Tree**: connection → database → schema → tables and views (MySQL has no schema level; SQLite lists its tables at once). A level is read when you open it, and the first open connects. System databases and schemas are hidden. A table shows the database's own row estimate (none for SQLite). The filter keeps the tables and views whose name matches, among the levels already open.
- **A connection row** shows its engine, name, `host:port` (or the SQLite file's folder), `ssh <host>` when it uses a tunnel, `RO` when it is read-only, its color, and a dot: green connected, yellow connecting, red failed (the error shows under the row), hollow not connected. Its menu has Connect or Disconnect, Open console, Edit…, Duplicate, Copy URL (without the password), and Delete… (asks once; the saved password goes too).
- **A table or view**: click it to open a table pane. Its buttons open the table pane or a console, and its menu also copies its name or its `SELECT`.
- **A password**: when the database asks for one and none is saved, the row asks for it. It is saved in the keychain (see below).

#### Adding a connection

- Paste a URL (`postgres://`, `postgresql://`, `mysql://`, `mariadb://`, `sqlite:`, or a file path) to fill in the fields; the password moves to its own field. Or fill in the fields: name, host, port, user, password, database, SSL mode, an SSH tunnel through a host of `~/.ssh/config`, a color, and **Read-only**. A SQLite connection has a file path only (`~/` works; the file is on the computer that runs westerm).
- **New connections are read-only.** The database itself refuses writes: PostgreSQL runs with `default_transaction_read_only`, MySQL with `SET SESSION TRANSACTION READ ONLY`, and SQLite opens the file read-only. This stops mistakes, not people: SQL can turn it off. Turn **Read-only** off to write.
- A red connection draws a red line at the top of its table panes, so a production database stands out.
- **Test connection** connects without saving and shows the server version, or the error.
- Passwords are kept in the keychain (macOS Keychain, Windows Credential Manager, the Secret Service on Linux), never in westerm's files. When the keychain cannot be used (a locked keychain, a server without a desktop session, Linux without a Secret Service), the password is kept in memory until westerm stops, and the dialog says so.
- **SSH tunnels** use your `ssh` with keys or an agent only. A host that asks for a password or a second factor fails with "SSH needs a key or an agent for <host>".

#### Found in your folders

When the panel opens (and on refresh), westerm looks for connections in the folders of your shells: `DATABASE_URL` in `.env` and `.env.local`, SQLite files (`*.db`, `*.sqlite`, `*.sqlite3`) at the top of the folder, and running PostgreSQL, MySQL, and MariaDB containers in Docker (with the user, password, and database from the container's environment). **Add** opens the dialog filled in; nothing is saved until you press **Save**. Connections you already saved are not listed again.

#### Table panes

- A table pane shows an SQL box over a result grid. A pane opened from the tree runs `SELECT * FROM <table> LIMIT 100` once. Press **Run** (or ⌘↵ / Ctrl+Enter) to run the SQL again; **Cancel** stops it.
- One statement per Run. Each Run starts a fresh database session, so a `BEGIN`, a `SET`, or a `USE` does not carry over to the next Run. Use a console for work that spans several statements.
- At most 1000 rows and 8 MiB show; longer cells are cut at 64 KB and marked "(truncated)". A query stops after 30 seconds. Double-click a cell to see all of it, click a cell and press ⌘C / Ctrl+C to copy it, or **Copy as CSV** for the whole result.
- The pane's SQL is saved with the layout, but not its result. **A restored pane does not run by itself**: it shows "Press Run to load", so opening the page never sends a query to a database.
- When the plugin is not installed, the pane says "This pane needs the Databases plugin". When the connection was deleted, it says so and offers Close.

#### Consoles

The console button opens `psql`, `mysql` (or `mariadb`), or `sqlite3` as a shell session beside the focused pane, connected and read-only like the connection. It is a normal westerm shell: it stays when you close the window. The password never appears on the typed line or in your shell history; it waits in a private file in the state folder while westerm runs. The button is disabled, with the install command in its tooltip, when the program is missing. Consoles need macOS or Linux with a shell of the sh family or fish.

- When westerm restarts (an update, for example), an open console stays, but a console through an SSH tunnel loses its connection, and the others cannot reconnect (`\c`). Open a new console.

#### Badge

The page reads the connection list every 10 seconds while the panel is open and every minute while it is closed (a hidden page reads nothing). The badge on the Databases button is the number of connections with a pool open; it is hidden at 0. An unused pool closes after 10 minutes.

#### Uninstalling

Uninstalling closes the pools and tunnels and removes the console password files. The saved connections and keychain entries stay, so installing again brings them back. **Forget all connections…** removes them all.

## terminal-browser and terminal-code

- When one of these programs is not installed, its pane shows the official install command with a **Copy** button. Run the command, then press **Restart**. westerm never installs anything itself.
- When the program is installed but cannot start, the pane asks you to read the program's own message in the terminal.
- Only the changed parts of the picture are sent, which matters most in a remote window. Scrolling and video still send whole pictures. To send whole pictures every time, run `terminal-browser config set render.presenter full`.

## Docs

A docs pane lists the Markdown files of a folder and shows, edits, and saves them. It has no process: it sits in tabs and splits like any other pane.

- **Open**: the book icon in the activity bar, or "New docs pane" in the palette. The folder is the one of the focused shell, else your home folder.
- **Folder**: the chip at the top shows it. Click it to type another absolute folder.
- **Follow shell**: while on, the folder follows the shell it opened beside. `cd` in that shell, and the pane lists the new folder within a few seconds. Picking a folder by hand turns it off; the switch turns it on again for the first running shell of the tab. Windows has no follow.
- **Tree**: `.md` and `.markdown` files only, by folder. It follows `.gitignore`, skips `.git`, `node_modules`, and `target`, and lists at most 2,000 files, from folders up to 7 levels below the root. The search box filters by path. The file icon at the top makes a new document; type `notes.md` or `docs/notes.md` in a folder that exists.
- **Modes**: View (the rendered document), Edit (the text), and Split (both, side by side). On a phone-wide window, Split shows as Edit and the list button opens the tree.
- **Keys** while the pane has the keyboard: ⌘S (Ctrl+S) saves, ⌘E (Ctrl+E) switches between View and Split.
- **Links**: a link to another Markdown file opens it in the pane, `#heading` links scroll, and web links open in a terminal-browser pane. Links that leave the folder do nothing.
- **Unsaved changes** are kept per file in this browser (or the app), also across reloads and when the pane closes. Every docs pane on the same file shows the same changes, also in other windows. When the browser's storage is full, the status bar says that the changes are lost when the window closes; save them to keep them. The tree marks such files with a dot; **Discard** drops the changes. Unsaved files in other folders are listed under the tree; click one to open it.
- **Folder field**: it suggests the folders this window's computer (or server) showed lately.
- **Changes on disk**: a file without unsaved changes reloads by itself. When the file changed on disk after you started editing, saving asks first: load the disk version, or save anyway.
- Files keep their line ends (`\r\n` stays `\r\n`). Files larger than 2 MiB and files that are not UTF-8 text do not open.
- Images show their description text instead of the picture, and HTML in a document shows as text.
- In a remote window, the pane shows the server's files.

## SSH

- The SSH launcher and the palette's SSH command list the hosts in `~/.ssh/config` (with Include). You can also type `user@host`.
- **Remote shell**: runs `ssh <host>` in a pane. Like a local shell, it survives reloads and dashboard restarts.
- **Remote browser**: opens `terminal-browser --ssh <host>`. Pages load through the remote server, so `localhost:3000` is that server's dev server.
- Authentication uses your SSH keys and agent. Type passwords into the terminal when a server asks; the dashboard never stores or sends them.

## Tunnel

Use this westerm from another device's browser, such as your phone, through a temporary Cloudflare address. Only you use it: the address and a 6-digit access code are for one person.

- Open it from the activity bar (the cloud icon, under the remote window button) or the command palette ("Start tunnel"). The panel takes the sidebar's place, like the agents panel; on a phone-wide window it opens as a drawer.
- westerm runs [cloudflared](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/downloads/) as a Quick Tunnel. It needs no Cloudflare account, no domain, and no open port. If cloudflared is missing, the panel shows the install command (macOS: `brew install cloudflared`, Windows: `winget install --id Cloudflare.cloudflared`, Linux: Cloudflare's downloads page). westerm does not install it. Press **Check again** after you install it.
- **Start tunnel** gives an address like `https://<name>.trycloudflare.com`, its QR code, and an access code. The address changes every time you start a tunnel. The QR code holds the address only; type the code on the other device.
- On the other device, open the address and enter the code. Then you see the same workspaces, tabs, and shells. One window at a time owns the sessions, as always: when the phone opens a session, the computer's window says "In use in another window"; click it to take them back.
- Five wrong codes in a row lock logins for 30 seconds, then for twice as long after each wrong code, up to 15 minutes. Twenty wrong codes stop the tunnel.
- The panel shows the devices that signed in and when each was last seen. **Stop tunnel** ends every device's access: their cookies stop working, and the page there says "The tunnel has ended". Shells keep running.
- While the tunnel is on, the computer does not go to idle sleep. Closing the lid, quitting westerm, or quitting the desktop app ends the tunnel. On Linux, westerm uses `systemd-inhibit`; where that does not work, the panel says the computer cannot be kept awake.
- With no device connected for 12 hours, the tunnel stops by itself.
- If cloudflared loses Cloudflare (no network, the computer slept), the panel shows **Reconnecting** and the address works again once it reconnects. If cloudflared ends, the panel shows its last output and **Start again**; a new tunnel has a new address and code.
- Settings kept in the browser (theme, font size, sidebar width, docs drafts) start over on the other device with each tunnel, because the address changes. The layout and the shells are on the computer and stay.
- A tunnel page has no tunnel, remote window, or update controls.
- Cloudflare ends TLS at its edge, so Cloudflare can see what passes through the tunnel, terminal output included.

## Remote windows

A window for a Linux server: its tabs, workspaces, and shells live on the server and keep running when your network does not.

- Open it from the activity bar (the server icon) or the command palette: pick a host from `~/.ssh/config` under "Open in a new window", or type `user@host`.
- In a browser, it opens a new browser window. The browser may block it as a popup: allow popups for `127.0.0.1`. The window's address is `/?remote=<target>` on the dashboard, for example `http://127.0.0.1:4317/?remote=prod`. In the desktop app, it opens an app window; the window of each server remembers its size and position. Opening the same server again brings its window to the front.
- The window title is `westerm — <target>`. The header shows the server name and how the window is connected: `QUIC`, or `SSH tunnel` when UDP does not get through.
- The first time, a connecting screen shows the stages: checking the server, installing westerm, starting, connecting. westerm copies the same version of itself to `~/.local/bin/westerm` on the server over SSH and starts it there. The server needs no internet access.
- Tabs, workspaces, and layout are saved on the server. Shells, terminal-browser, and terminal-code run on the server, started by your login shell there. Programs are found on the PATH of your interactive login shell on the server, so folders that `~/.zshrc` or a similar file adds (such as `~/.local/bin`) count. This applies once westerm next starts the server's host, for example after `westerm stop` there. Plain SSH panes and remote browsers opened inside a remote window run from the server too.
- If terminal-browser or terminal-code is not installed on the server, its pane says so and shows the install command. Run the command on the server (for example in a shell pane of this window), then press **Restart**. westerm does not install it.
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
| macOS, server on your LAN | Local Network access for the app that runs westerm: your terminal app, or the westerm app (System Settings ▸ Privacy & Security ▸ Local Network). Without it, macOS blocks UDP to local addresses and westerm uses the SSH tunnel. Servers on the internet do not need it. |

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
- Settings lists the installed font families from `/api/system-fonts`: on macOS the families CoreText offers (as Font Book shows them), elsewhere the system font folders and fontconfig. The browser draws the fonts, so the list is right when the browser runs on the same computer as the server.
- Ghostty has the Nerd Font icons built in; browsers do not. So the font stack adds the Nerd Fonts you have installed after your font.
- Then come monospace defaults (Menlo, Cascadia Mono, Consolas, …), and after them fonts for Korean, Japanese, and Chinese. Those fonts are proportional for Latin, so they only fill in the glyphs the fonts before them lack.
- Light and dark follow the OS unless Settings picks one. Terminal colors switch with it, in the color theme picked in Settings (`src/terminalThemes.ts`, `WTerm.setThemeColors`).
