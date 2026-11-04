# Usage

## The screen

| Area | What it does |
|---|---|
| Activity bar | New shell, terminal-browser, terminal-code, and SSH at the top; a badge shows how many are open. The command palette, Settings, and the sidebar switch at the bottom. |
| Sidebar | Tabs with their sessions as a tree. Collapse, rename, and drag to reorder or move. Drag its right edge to resize it; drag it narrow and it shrinks to icons: tab numbers, plus the session icons of the tab in view. |
| Tab bar | `+` or a double-click on empty space opens a tab. A middle click closes one. Drag to reorder. |
| Panes | Drag a pane's header onto another pane's edge to split there, onto its center to swap, or onto a tab or `+` to move it. Drag a divider to resize. |
| Header | The tabs, and an update button when a new release exists. Language, theme, and fonts are in Settings. |

## Renaming

Double-click a tab or a session, or press F2. Clear the name to go back to the automatic one (the title the shell sends).

## Command palette

- Open it with ⌘K on macOS, or Ctrl+Shift+K on Linux and Windows.
- It launches sessions, switches tabs and sessions, renames, maximizes, moves panes, changes the font size and language, opens settings, and checks for updates.
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

Open Settings from the gear at the bottom of the activity bar, from the palette, or with ⌘, in the macOS app. Changes apply at once and are saved in this browser (or the app).

| Setting | Effect |
|---|---|
| Language | Same as the browser, or English, Korean, Japanese, or Simplified Chinese. |
| Theme | System (follows the OS, live), Light, or Dark. Terminal colors follow it, and programs that ask for the background color (OSC 11) pick a matching theme. |
| Font size | Overrides the Ghostty `font-size`. "Use the Ghostty size" goes back. |
| Font | Overrides the Ghostty `font-family`; separate fallbacks with commas. Leave it empty for the Ghostty font. Applies on Enter or when the field loses focus. |
| Check for updates automatically | Asks GitHub for the newest release when the dashboard opens and every 6 hours. With it off, westerm contacts GitHub only when you press Check now. |

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

## Fonts and themes

- The server reads `font-family` and `font-size` from your Ghostty config (`/api/terminal-config`).
- Ghostty has the Nerd Font icons built in; browsers do not. So the font stack adds the Nerd Fonts you have installed after your font.
- Light and dark follow the OS unless Settings picks one. Terminal colors switch with it (`WTerm.setThemeColors`).
