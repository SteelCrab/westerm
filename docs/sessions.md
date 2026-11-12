# Sessions

| What happens | What you get |
|---|---|
| Reload, or close and reopen the tab | The same shell process, with its screen replayed ("reconnected"). |
| Restart the dashboard | The same shell process. |
| Reboot the computer | A new shell in the same folder, with the previous screen restored ("restored"). Programs that were running cannot come back. An AI agent that ran in the shell resumes its conversation (see [AI agents](#ai-agents)). |

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
- On Windows, PowerShell does not move its process on `cd`. westerm reads the folder the shell started in, so a new terminal-code and a restored shell start there.
- terminal-browser and terminal-code draw with Kitty images, which cannot be saved. On reconnect they start again and restore their own tabs and folders.

## AI agents

A shell that runs Claude Code, Codex, or Gemini CLI shows it, and resumes it when the shell comes back as "restored": after a reboot, a logout, `westerm stop`, or a host update.

### What the host watches

- Every second, each shell looks at the program in the foreground of its terminal. ConPTY on Windows has no foreground process group, so there the shell looks at the processes below it, nearest first (up to 64), and takes the first that is an agent.
- It is an agent when its command is `claude`, `codex`, or `gemini` (`claude.exe` on Windows), also when run from a path or by node from the npm package.
- Gemini CLI is not an agent when it runs one task or prints something: `-p`/`--prompt`, `--acp`, `--list-sessions`, `--delete-session`, `--list-extensions`, `--version`, `--help`, and the subcommands `mcp`, `extensions`, `skills`, `hooks`, and `gemma`.
- Claude Code writes `~/.claude/sessions/<pid>.json` (`$CLAUDE_CONFIG_DIR` when set). westerm reads the conversation id and the status from it: busy (works), waiting (asks), or idle.
- Gemini CLI writes no file for its process. westerm learns its conversation id only when the command line names it: `-r`/`--resume <id>` or `--session-id <id>`. `latest` and a number are not ids.
- The last 16 rows with text tell the rest: "esc to interrupt" means it works; for Gemini CLI, "(esc to cancel," does. A question means it asks, and wins: "Do you want to", "Would you like to", Codex's questions with choices ("enter to submit answer"), Codex's "Implement this plan?", and Gemini CLI's "allow execution of" and "apply this change?". When the file and the screen differ, the more urgent one wins: asks, then works.
- Each shell also reports its own folder and git branch once a second. The host sends a `place` message when the folder or branch changes, and once when a window attaches. westerm reads the branch from the files in `.git`; it does not run `git`. The [agents sidebar](usage.md#agents-sidebar) groups shells by this folder.
- Not watched: SSH panes (the agent runs on the other end), terminal-browser and terminal-code, and programs inside WSL (Windows cannot see their command lines).

### What is saved

- The session's saved file (`sessions/<id>.json`) keeps the agent, its conversation id, and the flags that a resume keeps.
- Kept flags: those that set the model, permissions, directories, settings, MCP servers, and system prompts. For Gemini CLI: the model, approval mode, policies, allowed tools and MCP servers, extensions, extra directories, and `--yolo`, `--sandbox`, `--skip-trust`, and `--screen-reader`. A prompt on the command line, print mode, resume flags, and output flags are not kept. Gemini CLI's `--worktree` is not kept either: a resume must not make another worktree.
- When the agent ends, westerm forgets it after two checks in a row. On a reboot the agent may end a moment before the host saves, and one check must not erase it.

### Resuming

- A restored shell writes "Resuming Claude Code · 9f00f957" under the restore notice. When the shell's prompt is up (300 ms without output, at most 5 seconds), westerm types the command:

| Agent | Command |
|---|---|
| Claude Code | `claude --resume <id> <kept flags>`; without an id, `claude --continue` |
| Codex | `codex resume --last <kept flags>`: the newest conversation in that folder. Codex writes no per-process file, so westerm cannot tell its id; two Codex panes in one folder both resume the newest one. `codex resume <id>` keeps its id. |
| Gemini CLI | `gemini --resume <id> <kept flags>`; when the id is unknown, `gemini --resume latest <kept flags>`: the newest conversation in that folder, with the same limit as Codex. |

- An agent run by its path resumes by the same path.
- The agents sidebar can also start an agent in a new shell: **Start here** types `claude`, `codex`, or `gemini`, and **Resume** types `claude --resume <id>`, `codex resume <id>`, or `gemini --resume <id>`. The host builds the command from the agent kind and a checked conversation id. It does so only for a brand-new shell. A shell that reconnects or is restored ignores these start parameters. When the folder no longer exists, the shell opens in the home folder and westerm types nothing.
- The command runs in the shell, so its output and errors show there. After it ends, the shell stays.
- Settings ▸ Terminal ▸ Resume AI agents turns this off for the window. The agent's card in a pane turns it off for that pane only (saved with the layout). The shell then comes back without its agent, and the saved agent is forgotten.

## Layout and saved files

- The layout (workspaces, tabs, pane order, names, workspace icons and colors, proportions) is saved in the state folder's `workspace.json`.
- Closing a pane, a tab, or a workspace ends its processes and deletes their saved screens.
- Saved files unused for 30 days are deleted.

## Reconnecting

- When the connection drops, a pane reconnects up to 8 times, waiting from 0.5 to 8 seconds between tries.

## Several windows

The desktop app and browser tabs use the same sessions. One window at a time has them all.

- When a window opens a session, every other window lets go of every session, and shows "In use in another window".
- Switching to that window or clicking it takes the sessions back. It first loads the layout the other window saved, so workspaces, tabs, and panes added or closed there show here too.
- Shells and SSH sessions keep running while they move between windows. terminal-browser and terminal-code start again in the window that takes them, and restore their own tabs and folders.
- A window that does not have the sessions does not reconnect and does not save its layout.

## Remote sessions

A remote window shows the tabs and shells of a server. They live in the PTY host on the server, not on this computer. The layout (workspaces, tabs, panes) is saved on the server too, in its `workspace.json`.

| What happens | What you get |
|---|---|
| Close the window | Nothing ends on the server. Open the server again to see the same tabs and shells. |
| Wi‑Fi changes, or a short drop | QUIC moves the connection to the new network, or the panes reconnect to the same shells. |
| Sleep, a long drop | A "Reconnecting…" banner shows. The panes reconnect to the same shells, their screens replayed ("reconnected"). |
| Restart the local dashboard | The window reconnects and shows the same tabs and shells. |
| The server reboots | The next connection starts westerm there again. Shells come back from their saved screens ("restored"). |
| westerm on this computer is updated | Open remote windows reload. The server gets the same version at the next connection. If its running host is older and has open shells, the window asks before replacing it, because replacing ends their programs. |

- Panes keep trying to reconnect for as long as the window is open (local panes stop after 8 tries). Temporary failures retry with waits of up to 8 seconds. After a failed SSH check, the next one waits at least 30 seconds.
- A permanent failure stops the retries and shows the reason with a "Try again" button. Examples: the SSH key is refused, the host key is unknown, the server is not supported, or the server has a newer westerm.
- Another computer that opens the same server sees the same tabs and shells. As with local windows, one window at a time has the sessions. The other shows "In use in another window" and takes them back when you click it, once its link is up.
- A window that comes back after sleep or a lost network reads the server's layout before it saves its own. If another computer changed the layout meanwhile, the window takes the server's layout. Known limit: the window that comes back also takes the sessions from the window in use; click that window to take them back.
- Closing a pane, a tab, or a workspace ends its shells on the server. Panes closed while the link was down end at the next connection.
- A server that ends a user's processes at logout (systemd-logind `KillUserProcesses=yes`, without lingering) ends these shells too; the header badge then shows a warning icon, and its tooltip says so.
- Shells of a v0.2.0 remote westerm pane stay on the server after the update, but no window shows them. `westerm stop` on the server ends them.

## Host replacement

- When the host code (`HOST_SOURCES` in `native/build.rs`) changes, the dashboard replaces the running host. `westerm --version` shows the host build id.
- Shells then restart once as "restored". Changes to the UI or the dashboard server alone leave shells running.
- The state folder from before the rename (`wterm-dashboard`) moves to `~/.local/state/westerm` on first start. A host still running there saves and exits, and its shells restart once as "restored".
