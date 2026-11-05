# Sessions

| What happens | What you get |
|---|---|
| Reload, or close and reopen the tab | The same shell process, with its screen replayed ("reconnected"). |
| Restart the dashboard | The same shell process. |
| Reboot the computer | A new shell in the same folder, with the previous screen restored ("restored"). Programs that were running cannot come back. |

## The PTY host

- `westerm host` is the background process that holds the shells.
- The dashboard starts it from the same executable the first time it runs. It keeps running when the dashboard, or the terminal that started the dashboard, closes.
  - macOS and Linux: in its own session (`setsid`).
  - Windows: without a console, in its own process group.
- It talks only through a private channel in the state folder:

| System | State folder | Channel |
|---|---|---|
| macOS, Linux | `~/.local/state/westerm` (under `$XDG_STATE_HOME` when set; mode 0700) | `host.sock`, a Unix socket (0600) |
| Windows | `%LOCALAPPDATA%\westerm` | A named pipe with a random name. Its name and the host's process id are in `host.pipe`. |

- One host per state folder: a second one exits at once (a lock on `host.lock`).
- `westerm stop` makes the host save every session and exit.

## Saving and restoring screens

- Shells and SSH sessions write their output to a screen copy inside the host (`vt100`). On reconnect, that screen comes back: scrollback, colors, cursor, and input modes.
- The screen (up to 1,000 lines) and the working folder are saved:
  - 3 seconds after output stops;
  - on exit, logout, and reboot (SIGTERM). A Windows host has no console, so it may not hear about logout or shutdown; the 3-second saves keep the loss small.
- Windows has no public way to read another process's folder, so a new terminal-code and a restored shell may start in the home folder.
- terminal-browser and terminal-code draw with Kitty images, which cannot be saved. On reconnect they start again and restore their own tabs and folders.

## Layout and saved files

- The layout (workspaces, tabs, pane order, names, workspace icons and colors, proportions) is saved in the state folder's `workspace.json`.
- Closing a pane, a tab, or a workspace ends its processes and deletes their saved screens.
- Saved files unused for 30 days are deleted.

## Reconnecting

- When the connection drops, a pane reconnects up to 8 times, waiting from 0.5 to 8 seconds between tries.

## Several windows

The macOS app and browser tabs use the same sessions. One window at a time has them all.

- When a window opens a session, every other window lets go of every session, and shows "In use in another window".
- Switching to that window or clicking it takes the sessions back. It first loads the layout the other window saved, so workspaces, tabs, and panes added or closed there show here too.
- Shells and SSH sessions keep running while they move between windows. terminal-browser and terminal-code start again in the window that takes them, and restore their own tabs and folders.
- A window that does not have the sessions does not reconnect and does not save its layout.

## Remote sessions

A remote westerm pane's shell lives in the PTY host on the server, not on this computer.

| What happens | What you get |
|---|---|
| Wi‑Fi changes, or a short drop | QUIC moves the connection to the new network, or the pane reconnects to the same shell. |
| Sleep, a long drop, a dashboard restart | The pane reconnects to the same shell, its screen replayed ("reconnected"). |
| The server reboots | The next connection starts westerm there again. Shells come back from their saved screens ("restored"). |
| westerm on this computer is updated | The server gets the same version. If its running host is older and has open shells, the pane asks before replacing it, because replacing ends their programs. |

- Remote panes keep trying to reconnect for as long as they are open (local panes stop after 8 tries).
- Closing a remote pane ends its shell on the server. When the server is out of reach at that moment, the shell ends at the next connection.
- A server that ends a user's processes at logout (systemd-logind `KillUserProcesses=yes`, without lingering) ends these shells too; the pane says so.

## Host replacement

- When the host code (`HOST_SOURCES` in `native/build.rs`) changes, the dashboard replaces the running host. `westerm --version` shows the host build id.
- Shells then restart once as "restored". Changes to the UI or the dashboard server alone leave shells running.
- The state folder from before the rename (`wterm-dashboard`) moves to `~/.local/state/westerm` on first start. A host still running there saves and exits, and its shells restart once as "restored".
