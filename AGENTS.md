# AGENTS.md

Generic Tauri desktop shell that loads app identity and UI from `app.toml`.

## What this project is

`app-ly` is a reusable container binary. Each deployment provides:

- `app.toml` — icon, title, HTML entrypoint, data directory
- `contents/` — static HTML/JS/CSS loaded into the window
- optional icon asset

The shell exposes `window.shell` to contents HTML for persistence, logging, notifications, CORS-free HTTP, and SQLite databases stored in `dataPath`.

## Config

TOML schema:

```toml
icon = "icon.png"
name = "My App"
contents = "contents/index.html"
dataPath = "data"
showDevMenu = true  # optional; default true in debug, false in release
```

Discovery order at startup:

1. Bundled `$RESOURCE/app.toml` (release default)
2. `--config <path>` CLI flag
3. `./app.toml` at project root (dev fallback)

Path resolution:

- `icon` and `contents` are relative to the directory containing the loaded `app.toml`
- `dataPath` in dev: relative to config directory
- `dataPath` in release: under the app data directory (writable)

Files:

- Dev config: [`app.toml`](app.toml)
- Bundled config: [`bundle/app.toml`](bundle/app.toml)
- Example contents: [`example/contents/index.html`](example/contents/index.html)

## Rust modules

| File | Responsibility |
|------|----------------|
| [`src-tauri/src/config.rs`](src-tauri/src/config.rs) | Load and parse `app.toml` |
| [`src-tauri/src/paths.rs`](src-tauri/src/paths.rs) | Resolve icon, contents, data paths |
| [`src-tauri/src/commands.rs`](src-tauri/src/commands.rs) | Invoke handlers for JS API |
| [`src-tauri/src/db.rs`](src-tauri/src/db.rs) | SQLite query and execute handlers |
| [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs) | App setup, `shell://` protocol, window creation, init script |

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

## Conventions

- Keep the shell generic; app-specific logic belongs in contents HTML
- Do not give the webview direct filesystem or network access
- File names passed to `saveFile`/`readFile` must be simple filenames (no subpaths)
- Prefer small, focused changes; avoid extra frameworks or abstractions