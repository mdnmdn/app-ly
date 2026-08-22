# AGENTS.md

Generic Tauri desktop shell that loads app identity and UI from `app.toml`.

## What this project is

`app-ly` is a reusable container binary. Each deployment provides:

- `app.toml` — icon, title, HTML entrypoint, data directory
- `contents/` — static HTML/JS/CSS loaded into the window
- optional icon asset

The shell exposes `window.shell` to contents HTML for persistence, logging, notifications, CORS-free HTTP, and SQLite databases stored in `dataPath`, OS keychain secrets, HTTP/WebSocket servers, allowlisted subprocess execution, and an on-device LLM.

## Config

TOML schema:

```toml
icon = "icon.png"
name = "My App"
contents = "contents"   # UI directory (or an HTML file); relative to this app.toml
dataPath = "data"       # writable data dir; relative to this app.toml, independent of contents
showDevMenu = true      # optional; default true in debug, false in release
keychainPrefix = "app"  # optional; prefix for OS keychain keys, default "app-ly"

[[allowedCommands]]                            # optional, repeatable; absent = no process execution
name = "git"                                   # required; alias JS passes to shell.run/spawn
program = "git"                                # required; bare name (PATH) or absolute path
args = ["^(status|log|diff)$", "^--oneline$"]  # optional; positional regex allowlist, implicitly anchored
extraArgs = "^[\\w./-]+$"                      # optional; pattern for every arg beyond `args`
maxArgs = 8                                    # optional; hard cap on argument count
cwd = "repo"                                   # optional; relative to the app.toml directory
timeoutMs = 30000                              # optional; default timeout, absent = no timeout
env = { GIT_PAGER = "cat" }                    # optional; merged over the inherited environment

[ai]                                           # optional; absent = all defaults, feature on
enabled = true                                 # optional; default true. false => reason "disabled-by-config"
instructions = "Answer briefly."               # optional; default system prompt for every request
temperature = 0.7                              # optional; default sampling temperature
maxTokens = 512                                # optional; default cap on response length
toolTimeoutMs = 30000                          # optional; default 30000. Wait for a JS tool handler
```

Omitting both `args` and `extraArgs` leaves arguments unrestricted for that program. Commands never run through a shell, and `program`/`cwd`/`env` can only come from config, never from JS.

Discovery order at startup:

1. Bundled `$RESOURCE/app.toml` (release default)
2. `--config <path>` CLI flag
3. `./app.toml` at project root (dev fallback)

Path resolution — all relative to the directory containing the loaded `app.toml`:

- `icon` — icon file
- `contents` — UI directory, or an HTML entry file (if a file, its parent is the UI root; default entry is `index.html`)
- `dataPath` — writable data directory, independent of `contents` (dev and release)

Files:

- Dev config: [`app.toml`](app.toml)
- Bundled config: [`bundle/app.toml`](bundle/app.toml)
- Example contents: [`example/contents/index.html`](example/contents/index.html)

## Rust modules

| File | Responsibility |
|------|----------------|
| [`src-tauri/src/config.rs`](src-tauri/src/config.rs) | Load and parse `app.toml` |
| [`src-tauri/src/paths.rs`](src-tauri/src/paths.rs) | Resolve icon, contents, data paths |
| [`src-tauri/src/commands.rs`](src-tauri/src/commands.rs) | Invoke handlers for JS API (files, fetch, windows, screens) |
| [`src-tauri/src/db.rs`](src-tauri/src/db.rs) | SQLite query, execute, and close handlers; connection cache with idle timeout |
| [`src-tauri/src/auth.rs`](src-tauri/src/auth.rs) | Shared-listener authViaBrowser with concurrent flow dispatch |
| [`src-tauri/src/keyring.rs`](src-tauri/src/keyring.rs) | OS keychain secret set/get/delete |
| [`src-tauri/src/server.rs`](src-tauri/src/server.rs) | Embedded HTTP server and WebSocket server |
| [`src-tauri/src/process.rs`](src-tauri/src/process.rs) | Allowlisted subprocess execution (`shell_run`, `shell_spawn`, stdin, exit/kill, runtime timeout, allowlist introspection) |
| [`src-tauri/src/ai.rs`](src-tauri/src/ai.rs) | On-device AI (`shell.ai`): commands, tool bridge, JSON Schema translation, backend selection; backends in [`src-tauri/src/ai/`](src-tauri/src/ai/) (`backend_apple.rs` for macOS + the `ai-apple` feature, `backend_stub.rs` everywhere else) |
| [`src-tauri/src/menu.rs`](src-tauri/src/menu.rs) | Native app menu (Reload, Open DevTools) |
| [`src-tauri/src/cli.rs`](src-tauri/src/cli.rs) | Headless CLI (`app-ly ai`, `db`, `file`, `fetch`, `run`, `info`) — no window |
| [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) | App setup, `shell://` protocol, window creation, init script, state managers |

See [`_docs/project-structure.md`](_docs/project-structure.md) for the full repo layout, and
[`_docs/ai.md`](_docs/ai.md) for the on-device AI reference (platform requirements, reason codes,
supported JSON Schema subset, tool bridge).

## JS API

Injected as `window.shell` before page scripts run. Keyboard shortcuts are injected from [`src-tauri/scripts/shell-shortcuts.js`](src-tauri/scripts/shell-shortcuts.js). Full reference: [`_docs/js-api.md`](_docs/js-api.md).

- `saveFile(name, contents)`
- `readFile(name)`
- `log(message, level?)`
- `notify(title, body)`
- `fetch(url, opts?)`, `get(url, headers?)`, `post(url, body, headers?)`
- `getWindowPosition()`, `setWindowPosition(x, y)`, `getWindowSize()`, `setWindowSize(w, h)`, `minimize()`
- `getScreens()`, `getScreenAt(x, y)` — display sizes and multi-monitor info
- `dbQuery(dbName, query, params?)` — tabular SELECT results
- `dbExecute(dbName, query, params?)` — DML / scalar writes, returns changes + row id
- `dbClose(dbName?)` — close a cached SQLite connection and release the file (all open dbs if omitted); idle connections also close after 30s
- `secretSet(service, account, password)` — OS keychain store
- `secretGet(service, account)` — OS keychain retrieve
- `secretDelete(service, account)` — OS keychain delete
- `httpStart(options?)`, `httpRespond(id, status, headers?, body?)`, `httpStop()` — local HTTP server
- `onHttpRequest(callback)` — incoming HTTP request events
- `wsStart(options?)`, `wsSend(id, data)`, `wsClose(id)`, `wsStop()` — local WebSocket server
- `onWsConnection(callback)`, `onWsMessage(callback)`, `onWsClose(callback)` — WebSocket events
- `run(name, args?, options?)` — run an allowlisted program to completion, returns stdout/stderr/exit
- `spawn(name, args?, options?)` — streaming child process (`onStdout`/`onStderr`/`onExit`, `write`, `exit`/`kill`, `setTimeout`, `exited`, async iteration)
- `listCommands()` — the `[[allowedCommands]]` entries this app was configured with
- `ai.info()`, `ai.available()`, `ai.models()` — on-device model availability (never reject)
- `ai.generate(prompt, options?)` — one-shot text, returns `{ text, model, toolCalls }`
- `ai.generateObject(prompt, schema, options?)` — schema-constrained structured output
- `ai.stream(prompt, options?)` — streaming handle (`onText`, `completed`, `cancel`, async iteration)

Dev shortcuts (when `showDevMenu` is enabled):

- `Cmd/Ctrl + Shift + M` or `Cmd/Ctrl + Shift + I` — toggle the native Web Inspector ([Tauri debug docs](https://v2.tauri.app/develop/debug/))
- `Cmd/Ctrl + Shift + R` — reload contents page
- Right-click → **Inspect Element** — open the Web Inspector (platform shortcut: `Cmd + Option + I` on macOS, `Ctrl + Shift + I` elsewhere)

App menu **View**:

- **Reload** — reload contents (`Cmd/Ctrl + Shift + R`)
- **Open DevTools** — when `showDevMenu` is enabled (`Cmd/Ctrl + Shift + M`)

The shell uses Tauri’s built-in Web Inspector (`WebviewWindow::open_devtools`). Release builds enable it via the `devtools` Cargo feature on `tauri`; set `showDevMenu = true` in `app.toml` to expose the menu item and keyboard shortcuts. On macOS this uses a private API (not App Store–compatible).

## Commands

```bash
npm install
npm run tauri dev
npm run tauri build
npm run tauri dev -- --config ./path/to/app.toml
```

Headless CLI (same `app.toml` as the GUI: `--config`, folder containing the `.app`, then
bundled / cwd fallbacks). `[[allowedCommands]]` gates `run` and is what `ai` may call as
tools. Invoke the binary, not `open`:

```bash
app-ly.app/Contents/MacOS/app-ly --help
app-ly.app/Contents/MacOS/app-ly ai "say hi"
app-ly --config ./app.toml db query notes.db "select * from notes"
app-ly run git status
```

## Conventions

- Keep the shell generic; app-specific logic belongs in contents HTML
- Do not give the webview direct filesystem or network access
- File names passed to `saveFile`/`readFile` must be simple filenames (no subpaths)
- Prefer small, focused changes; avoid extra frameworks or abstractions