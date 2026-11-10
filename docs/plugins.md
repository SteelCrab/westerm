# Writing plugins

An external plugin is a folder with a `plugin.json` and a panel page. westerm shows the panel in the sidebar's place, in a sandboxed frame. The panel talks to westerm only through messages, and can do only what `plugin.json` asks for and the user confirmed.

To install a plugin, see [Plugins in Usage](usage.md#plugins). This page is for people who write them.

## What a plugin can and cannot do

- It can run the programs it lists, with any arguments, as the user.
- It can open shell panes and browser panes, keep a badge and a red dot on its button, and keep one small JSON value.
- By itself it cannot read other sessions, terminal output, the layout, other plugins, or any file, unless it runs a program it is allowed to run (see `run` below). It cannot make network requests itself.
- It cannot run in the background unless it asks for `background`. Without it, westerm removes the frame when the panel closes.
- It does not work in a remote window. Remote windows show no plugins.

## Package layout

```
my-plugin/
  plugin.json     required
  panel.html      the page named by "panel"
  panel.js        any other files: scripts, styles, images
  icon.svg        optional
```

- Everything in the folder is public to the panel through `/plugin-assets/<id>/…`. Do not put secrets in it. A file or folder whose name starts with `.` (`.env`, `.git/`) is never served, and `plugin.json` paths must not have such a part.
- A package may hold only regular files and folders. Links, devices, and other entry types are refused. (A folder you install from disk is used in place, but a link must not lead outside it: westerm serves only files inside the folder.)
- A folder that has one folder at the top is treated as the package: the contents of that folder are used. So `tar -czf x.tar.gz my-plugin` works too.

### `plugin.json`

```json
{
  "id": "k8s-tools.kubernetes",
  "name": "Kubernetes",
  "version": "0.3.1",
  "description": "See pods by namespace. Open logs and exec shells as westerm panes.",
  "icon": "icon.svg",
  "panel": "panel.html",
  "westerm": ">=0.5.0",
  "permissions": { "run": ["kubectl"], "panes": true, "background": true },
  "i18n": { "ko": { "name": "쿠버네티스", "description": "…" } }
}
```

| Key | Required | Rule |
|---|---|---|
| `id` | yes | `<author>.<name>`. Each part is 1 to 32 characters of `a-z`, `0-9`, `-`. Built-in plugins have no dot, so an id never clashes with one. |
| `name` | yes | At most 40 characters (characters, not bytes). Not empty. |
| `version` | yes | `x.y.z`, digits only. No `v`, no suffix. |
| `panel` | yes | A relative path inside the package that ends in `.html`. |
| `description` | no | At most 200 characters. |
| `icon` | no | A relative path to an `.svg` in the package. Without it, the plugin gets a puzzle-piece icon. westerm shows it only through an `<img>`, so scripts inside the SVG never run. |
| `westerm` | no | `>=x.y.z` only. If this westerm is older, the install is refused. |
| `permissions` | no | See below. Without it the plugin has no permissions. |
| `i18n` | no | An object with the language keys `ko`, `ja`, `zh`, and `en`. Each has an optional `name` and `description`. westerm shows the entry for its language. |

- Unknown keys are ignored. A wrong value is refused, with a one-line reason such as `plugin.json: version must look like 1.2.3`.
- A relative path has no empty, `.`, or `..` part and no `\`, `:`, or NUL. The panel and the icon must be regular files in the package.

### Permissions

| Permission | Risk | What it allows |
|---|---|---|
| `run`: a list of program names | high | Run these programs. Up to 8 names. Each name matches `[A-Za-z0-9][A-Za-z0-9._-]*`, so no paths and no leading `-`. |
| `panes`: `true` | medium | Open shell panes and browser panes. |
| `background`: `true` | low | Keep running while the panel is closed (for a badge and a red dot). |

**`run` means "trust this plugin".** The user is told: "Can run kubectl with any arguments, as you". westerm runs only the listed program, but it does not limit the arguments. So the plugin can do anything that program can do as the user (`kubectl exec`, `git -c core.sshCommand=…`). List only the programs you need, and tell your users why.

**Refused names.** `run` may not list a shell, an interpreter, or a wrapper that runs other programs. Case does not matter, and a version does not help (`python3.12` counts as `python`, `ksh93` as `ksh`). The list:

`sh bash zsh fish dash ksh csh tcsh nu xonsh elvish pwsh powershell cmd wsl cscript wscript mshta env command builtin alias fc eval exec source sudo doas su runas xargs nohup nice time timeout watch script python python3 node deno bun ruby perl php lua osascript`

This check catches mistakes. It is not a boundary: a listed program can start other programs.

An update that asks for more (a new program, a new permission) shows what it adds and asks the user again. An update that asks for the same or less needs one confirmation.

## The panel page

```html
<!doctype html>
<meta charset="utf-8">
<script src="/plugin-sdk/v1.js"></script>
<script src="panel.js"></script>
```

- The SDK defines `window.westerm`. It also adds base styles and sets westerm's colors as CSS variables on `<html>`.
- **Add `<meta charset="utf-8">`.** westerm sends text files without a charset, so the browser guesses without it.
- **Inline scripts do not run** (the policy blocks them). Put code in a file and load it with `<script src>`. Inline styles and `<style>` work.
- Scripts, styles, and images load only from your plugin's folder and the SDK. Use relative paths (`panel.js`, `img/logo.png`). ES modules (`<script type="module">`) work too.
- The page runs in a frame with an opaque origin: no cookies, no `localStorage`, no link to the westerm page. Use `westerm.storage` to keep data.
- Make the page fit a narrow column. The frame has the size of the sidebar's panel.

### SDK styles and variables

- Classes: `.w-row` (a row with a bottom line), `.w-button`, `.w-dot` (with `.ok`, `.warn`, `.danger`), and `.w-empty` (an empty state). Your own styles win over them.
- Variables: `--bg --surface --surface-2 --border --text --text-muted --text-faint --accent --ok --warn --danger`, and `--font`. They update when the user changes the theme.

### API v1

| Call | Needs | What it does |
|---|---|---|
| `westerm.run(program, args, { cwd }?)` | `run` lists `program` | Runs the program with the argument array, with no shell. Resolves to `{ code, stdout, stderr }`. `code` is `null` when a signal ended the program. A non-zero exit is not an error. |
| `westerm.openShell({ program, args, cwd?, name?, reuse? })` | `panes`, and `run` lists `program` | Opens a new shell pane that runs `program args…`. It fails with `failed` (and opens nothing) on Windows or when the user's shell is not supported; see below. `name` is the pane's name (1 to 80 characters; default: the program). With `reuse: true`, a pane this plugin opened with the same program and arguments in this workspace is focused instead. |
| `westerm.openBrowser(url)` | `panes` | Opens `url` in a browser pane. Only `http` and `https`. westerm's own addresses are refused (see below). |
| `westerm.storage.get()` / `westerm.storage.set(value)` | none | One JSON value for the plugin, at most 64 KiB. `get` returns `null` before the first `set`. |
| `westerm.setStatus({ badge, alert })` | none | `badge` is a whole number from 0 to 999, or `null`. `alert` is `true` or `false` (the red dot). Changes reach the plugin bar in batches, at most every 100 ms. |
| `westerm.on(event, listener)` | none | Listens to an event. Returns a function that stops listening. |
| `westerm.ready` | none | A promise of the `ready` event's data. |

A failed call rejects with an `Error`. `error.message` is the reason, and `error.code` is one of:

| Code | Meaning |
|---|---|
| `permission` | The permission was not granted. |
| `invalid` | The call or its arguments are wrong. |
| `failed` | The work failed: a timeout, a program that is not installed, a size limit, a server error. |
| `gone` | The plugin was removed or stopped. |

Events:

| Event | Data |
|---|---|
| `ready` | `{ id, version, locale, theme: { vars, font }, permissions: { run, panes, background }, panelOpen }`. Sent once, after the frame loads. |
| `theme` | `{ vars, font }` |
| `locale` | `{ locale }` (`en`, `ko`, `ja`, or `zh`) |
| `panel` | `{ open }`. `true` when the panel is shown. With `background`, the frame lives on while the panel is closed. |

You can call before `ready`.

#### Limits

| Limit | Value |
|---|---|
| `run` time | 10 seconds. The program is killed after that. The answer comes when the program exits, even if a background job it started still holds its output. On macOS and Linux that job is killed with it (the program runs in a process group of its own). |
| `run` output | 1 MiB for stdout and 1 MiB for stderr. Output beyond that is dropped. |
| `run` at once | 4 for one plugin. More fail with `failed`. |
| `run` environment | The server's environment, with the whole environment of your login shell on top (the login value wins). So `KUBECONFIG`, `AWS_PROFILE`, and `PATH` from your shell startup files are there. The login shell's own `PWD`, `OLDPWD`, `SHLVL`, and `_` are left out. |
| `run` `cwd` | An absolute path to a folder. Without it, the home folder. |
| `openShell` | At most 32 words (program and arguments), each at most 1024 bytes, no control characters. |
| Message size | 256 KiB for a message from the panel. Bigger ones are dropped without an answer. |
| In flight | 8 calls that wait for the server (`run`, `openBrowser`, storage) per frame. More fail at once. |
| Storage | 64 KiB. |

#### `openShell`

- macOS and Linux only. On Windows the call fails with the code `failed` and the message "Your shell is not supported for plugin panes", and no pane opens. `run` works on Windows.
- westerm types the command into a new shell. Every word is single-quoted for that shell, and the line starts with `env --`, so a shell builtin or alias never takes the program's place:

  ```
  'env' '--' 'git' 'log' '--oneline'
  ```

- Only the sh family (`sh bash zsh dash ksh`) and `fish` are supported. With any other shell (`nu`, `pwsh`, `xonsh`, `elvish`, and also `ash`, `mksh`, `yash`, for example Alpine's default), the call fails with the code `failed` and the message "Your shell is not supported for plugin panes", and no pane opens.
- A program name or argument with a control character (including Esc, a newline, DEL, and the C1 range) is refused, because the terminal would act on it before the quotes.
- A shell pane is an ordinary session. It stays when the panel closes, and after a restart it comes back as a plain shell without the command.

#### `openBrowser`

A browser pane's requests come from the dashboard's own address, so opening the dashboard in it would give the plugin westerm's powers. westerm refuses:

- every spelling of the local computer (`127.0.0.1`, `localhost`, `[::1]`, any `127.x.x.x`, `0.0.0.0`, `[::ffff:127.0.0.1]`, `127.1`, `2130706433`, …) on the dashboard's port,
- any address whose origin is one of the dashboard's allowed origins (`--allow-origin` too),
- a host name with characters westerm does not convert to ASCII (international names, look-alike letters, a soft hyphen) on the dashboard's port, or on the scheme and port of any allowed origin, because a browser may turn it into one of those hosts.

Other local ports, such as a dev server on `http://localhost:3000`, open normally.

### Message format

`westerm.*` is a thin layer over `postMessage`. You may send the messages yourself. The full format is in the header of [`sdk_v1.js`](../native/src/server/plugins/external/sdk_v1.js).

- Every message is a JSON string with `"westerm": 1`. The panel sends with `window.parent.postMessage(text, "*")`.
- Request: `{"westerm":1,"id":1,"call":"run","args":{"program":"git","args":["--version"],"cwd":null}}`. `id` is an integer, unique in the frame.
- Reply, one per request: `{"westerm":1,"id":1,"ok":true,"result":…}` or `{"westerm":1,"id":1,"ok":false,"error":{"code":"failed","message":"…"}}`.
- Event: `{"westerm":1,"event":"theme","data":{…}}`.
- westerm reads a message only from the plugin's own frame. The SDK reads a message only when `event.source === window.parent`.

## Isolation

You do not need these details to write a plugin, but they explain what you will run into.

- The panel is `<iframe sandbox="allow-scripts">`. It never gets `allow-same-origin`, `allow-popups`, `allow-forms`, `allow-top-navigation`, or `allow-modals`.
- Every response under `/plugin-assets/` (also 403 and 404) carries `Content-Security-Policy: sandbox allow-scripts; …`, so a page opened outside its frame (a link, a browser pane) is still sandboxed. The policy also sets:
  - `default-src 'none'`, so anything not named below is blocked: web fonts (there is no `font-src`, so not even from the plugin's folder), media, workers, and so on;
  - scripts from `/plugin-assets/<id>/` and the SDK only (no inline scripts); styles from the plugin's folder and inline styles; images from the plugin's folder. `data:` and `blob:` images and styles are blocked, and so is anything from another site;
  - `connect-src 'none'`, `frame-src 'none'`, `form-action 'none'`, `base-uri 'none'`, `object-src 'none'`;
  - `frame-ancestors` set to the dashboard's own origins, so no other site can frame the page and answer its calls.
- Responses also have `X-Content-Type-Options: nosniff` and `Cache-Control: no-cache`. A served file has `Access-Control-Allow-Origin: *` only when the browser loads it as a script, style, image, or font (`Sec-Fetch-Dest`), because a module script in an opaque-origin frame is a cross-origin request. A `fetch()` from another website gets no such header, so it cannot read the plugin's files.
- The dashboard page itself sends `Content-Security-Policy: frame-src 'self'`.
- westerm counts the frame's `load` events. A second `load` means the page navigated away from itself (`location = …`). westerm then removes the frame, ignores its messages, and shows "Plugin stopped: it tried to leave its page". Opening the panel again starts it fresh.

### Known limits

- **`run` is trust.** A listed program can do anything it can do as the user, with any arguments.
- **Data can still leak once.** The policy blocks direct requests, and westerm stops a page that navigates away, but it cannot block the first request of a navigation or WebRTC. A bad plugin can send out data it was given: its own storage, the output of its `run` calls, the theme. This is why westerm asks the user to trust the source before it installs an unverified or local plugin.

## Build and release a package

A package is a `.tar.gz` of the folder's contents:

```bash
tar -czf westerm-plugin.tar.gz -C dist .
```

On macOS, build it with `COPYFILE_DISABLE=1 tar -czf westerm-plugin.tar.gz -C dist .`. That keeps `._*` files and extended attributes out of the archive.

Rules:

- `.tar.gz` only, at most 5 MiB packed, 10 MiB unpacked, and 300 entries (folders count).
- Regular files and folders only. No links, devices, `..`, or absolute paths.
- A single folder at the top is the package.

Ways to publish:

- **A GitHub release.** Attach `westerm-plugin.tar.gz` to a release and tell users to install `https://github.com/<owner>/<repo>`. westerm uses the latest release. The tag must be a plain version: `v1.2.3` or `1.2.3`. A tag such as `v1.2.3-beta` is refused. Use a tag that matches `version` in `plugin.json`. westerm checks the latest tag once a day and offers the update.
- **A direct link** to a `.tar.gz` (`https://` only). It has no update check: the plugin shows "Reinstall to update". A reinstall replaces the installed version in one step, like an update: the plugin's saved storage stays, and a failed reinstall leaves the installed version as it was.
- **The plugin list.** Once the `SteelCrab/westerm-plugins` repository exists, westerm will read its `index.json` for the Browse tab. The repository does not exist yet: until it does, Browse shows "Could not load the plugin list", and installs from a URL or a folder work regardless. The list is a top-level JSON array, and a plugin in it is shown as `verified`. Each entry has `id`, `name`, `description`, `repo`, `version`, `url` (the package), `sha256` (of the package), and `permissions` (`run`, `panes`, `background`, as in `plugin.json`). westerm refuses a package whose SHA-256 differs from the entry. The server keeps the list for one hour.

Downloads are `https://` only, with a time limit and a size limit.

## Develop with a folder

1. Write the folder, for example `/Users/me/dev/hello-git`.
2. In westerm, open the plugin manager, choose **Install from URL or folder…**, pick **Folder**, enter the absolute path, and press **Next**.
3. Confirm with **I trust …** and press **Install**. westerm uses the folder in place (it copies nothing). The badge is `local`.
4. After a change, open the plugin in the manager and press **Reload**. westerm reads the files again and restarts the panel. If `permissions` grew, it asks again.
5. **Uninstall** removes only the registration. The folder stays.

## Example: `hello.git`

A plugin that shows `git --version`, a badge, and two buttons.

`plugin.json`:

```json
{
  "id": "hello.git",
  "name": "Hello git",
  "version": "1.0.0",
  "description": "Shows the git version. Opens git log in a shell pane.",
  "panel": "panel.html",
  "permissions": { "run": ["git"], "panes": true }
}
```

`panel.html`:

```html
<!doctype html>
<meta charset="utf-8">
<div class="w-row" id="version">Loading…</div>
<div class="w-row">
  <button class="w-button" id="log">Log</button>
  <button class="w-button" id="docs">Docs</button>
</div>
<script src="/plugin-sdk/v1.js"></script>
<script src="panel.js"></script>
```

`panel.js`:

```js
const row = document.getElementById("version");

westerm.ready
  .then(() => westerm.run("git", ["--version"]))
  .then(({ code, stdout, stderr }) => {
    row.textContent = code === 0 ? stdout.trim() : stderr.trim() || `git exited with ${code}`;
    westerm.setStatus({ badge: 1, alert: false });
  })
  .catch((error) => {
    row.textContent = error.message;
  });

document.getElementById("log").addEventListener("click", () => {
  westerm
    .openShell({ program: "git", args: ["log", "--oneline"], name: "git log", reuse: true })
    .catch((error) => (row.textContent = error.message));
});

document.getElementById("docs").addEventListener("click", () => {
  westerm.openBrowser("https://git-scm.com/docs").catch((error) => (row.textContent = error.message));
});
```

Install it from its folder. The panel shows `git version …` and a badge of 1. **Log** opens `git log --oneline` in a new shell pane, and pressing it again focuses that pane. **Docs** opens git's documentation in a browser pane.
