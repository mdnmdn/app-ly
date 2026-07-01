# Building an app-ly app 


## What `app-ly` is for

`app-ly` exists so you can build a real, working **desktop application** using nothing but JS + HTML — no Tauri, no Rust, no Electron, no native toolchain. It's a ready-to-run shell binary: you write a folder of static HTML/JS/CSS plus an `app.toml`, drop the pre-built `app-ly.app` (macOS) / executable (other platforms) next to them, and launching that binary *is* your desktop app. The shell gives that HTML a native window plus a `window.shell` API for the things plain web pages can't do (persistent files, SQLite, CORS-free HTTP, notifications, window control) — everything a small desktop app typically needs, none of the platform-specific glue normally required to get it.

No npm, no bundler, no framework, no build step, no compiling — plain `<script>` tags work, and the binary you copy in is already compiled. The `npm run tauri dev/build` toolchain is only relevant to someone modifying `app-ly` itself, never to someone authoring an app on top of it.

## Minimum viable app

```
myapp/
├── app-ly.app        # (or platform executable) — the pre-built shell binary, copied in
├── app.toml
├── icon.png
└── contents/
    └── index.html
```

`myapp/app.toml`:

```toml
icon = "icon.png"
name = "My App"
contents = "contents/index.html"
dataPath = "data"
```

Run it by launching `app-ly.app` (or the executable) sitting in `myapp/` — it auto-discovers the `app.toml` next to it. No build step, no flags, no install.

`myapp/contents/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>My App</title>
  </head>
  <body>
    <h1>Hello</h1>
    <script>
      window.addEventListener("DOMContentLoaded", async () => {
        await shell.log("app started");
      });
    </script>
  </body>
</html>
```

Run it by launching the `app-ly` binary placed in `myapp/`.

If you're iterating on the `app-ly` shell itself (not just authoring contents), you can instead run from a checkout of this repo with hot toolchain access:

```bash
npm install
npm run tauri dev -- --config ./myapp/app.toml
```

## `app.toml` reference and best practices

```toml
icon = "icon.png"            # path, relative to this app.toml's directory
name = "My App"               # window title
contents = "contents/index.html"  # entry HTML, relative to this app.toml's directory
dataPath = "data"             # writable data dir, relative to this app.toml's directory
showDevMenu = true            # optional. default: true in dev, false in release
```

Rules and gotchas:

- **All paths in `app.toml` are relative to the directory containing that `app.toml`** — not the project root, not cwd. Keep `icon`, `contents`, and `dataPath` inside (or under) the same folder as the `app.toml` that references them. Don't use absolute paths or `..` to reach outside your app folder.
- `contents` must point at a single HTML **file**. Everything else referenced from that HTML (JS, CSS, images) is resolved relative to that file's directory by the browser as normal — put your whole frontend under one `contents/` folder so it travels as a unit.
- `dataPath` behaves differently in dev vs release — don't hardcode logic that assumes one or the other:
  - **Dev** (`tauri dev`): `<app.toml dir>/<dataPath>` — lands inside your project folder, easy to inspect.
  - **Release** (`tauri build` bundle): `<OS app-data-dir>/<dataPath>` — a writable, sandboxed location outside the read-only app bundle. Never assume `dataPath` is next to your HTML in release.
  - The directory (and a `logs/` subdirectory inside it) is created automatically at startup. Don't try to create it yourself.
- `showDevMenu`: leave it `true` while building; an app you intend to ship without DevTools exposed should set it `false` (or omit it, since release already defaults to `false`).
- Discovery order: `--config <path>` flag → folder containing the `app-ly.app` bundle/executable (this is the normal case — your `app.toml` sits right next to the binary you copied in) → bundled fallback resource baked into the binary itself → (dev-only) `./app.toml` in cwd → project root `app.toml`. As an app author you don't need `--config` at all: just keep `app.toml` next to the binary and it's found automatically. `--config` is only useful for testing multiple app folders against one shared binary without copying it around.
- For a real release, you also need to point `src-tauri/tauri.conf.json`'s `bundle.resources` at your `contents/`, `icon`, and a copy of your `app.toml` (as `bundle/app.toml`), per [`_docs/README.md`](README.md). That's a shell-repo change, not something your contents HTML controls.

## Path rules inside the app (filenames, not paths)

`saveFile`, `readFile`, `dbQuery`, `dbExecute` all take **simple filenames only** — no subdirectories, ever. The shell rejects any name containing `/`, `\`, `..`, or a null byte, and also rejects empty names.

```javascript
await shell.saveFile("settings.json", "...");   // ✅
await shell.saveFile("notes/today.json", "..."); // ❌ rejected — no nested paths
await shell.saveFile("../escape.json", "...");   // ❌ rejected — path traversal
```

If you need structure, encode it in the filename (`notes-2024-01-01.json`) or put multiple logical records inside one SQLite database (preferred for anything beyond a couple of files — see below).

## `window.shell` API

Available immediately on `window` before your page scripts run. Every method returns a `Promise` that **rejects with a string** on failure — always wrap calls in `try/catch` or `.catch()` where failure is expected (e.g. `readFile` on a file that doesn't exist yet).

### Summary

| Method | Signature | Purpose |
|---|---|---|
| `saveFile` | `(name, contents) → void` | Write a text file to `dataPath` |
| `readFile` | `(name) → string` | Read a text file from `dataPath`; rejects if missing |
| `dbQuery` | `(dbName, query, params?) → { columns, rows }` | Run a SQL `SELECT` against a SQLite file in `dataPath` |
| `dbExecute` | `(dbName, query, params?) → { changes, lastInsertRowid }` | Run a SQL write/DDL statement |
| `log` | `(message, level?) → void` | Append a line to `dataPath/logs/shell.log` |
| `notify` | `(title, body) → void` | Show a native OS notification |
| `fetch` | `(url, options?) → { ok, status, statusText, headers, body }` | CORS-free HTTP request (full control) |
| `get` | `(url, headers?) → response` | `fetch` shorthand, method `GET` |
| `post` | `(url, body, headers?) → response` | `fetch` shorthand, method `POST` |
| `getWindowPosition` | `() → { x, y }` | Outer window position, physical pixels |
| `setWindowPosition` | `(x, y) → void` | Move the window |
| `getWindowSize` | `() → { width, height }` | Window content size, physical pixels |
| `setWindowSize` | `(width, height) → void` | Resize the window |
| `minimize` | `() → void` | Minimize to dock/taskbar |
| `getScreens` | `() → { screens, primaryIndex, currentIndex }` | List displays and their geometry |
| `getScreenAt` | `(x, y) → screen` | Display containing a screen point |

`name`/`dbName` arguments are always simple filenames — see [path rules](#path-rules-inside-the-app-filenames-not-paths) above. Window/screen methods are rarely needed — see [below](#window-and-screen--mostly-skip-these).

### Files — `saveFile` / `readFile`

Plain text files in `dataPath`. Good for settings, small exports, anything you'd otherwise put in `localStorage` but want to survive as a real file.

```javascript
await shell.saveFile("settings.json", JSON.stringify({ theme: "dark" }));
const raw = await shell.readFile("settings.json"); // throws if missing
const settings = JSON.parse(raw);
```

Practice: treat this as key-value storage keyed by filename, not a filesystem. For anything relational or queryable, use SQLite instead.

### SQLite — `dbQuery` / `dbExecute`

A SQLite file in `dataPath`, created on first use. This is the right tool once you have more than a handful of records or need to query/filter/sort.

```javascript
await shell.dbExecute(
  "app.db",
  "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, title TEXT, created_at TEXT)",
);

const write = await shell.dbExecute(
  "app.db",
  "INSERT INTO notes (title, created_at) VALUES (?, ?)",
  ["First note", new Date().toISOString()],
);
// write: { changes, lastInsertRowid }

const result = await shell.dbQuery("app.db", "SELECT id, title FROM notes ORDER BY id DESC");
// result: { columns: ["id", "title"], rows: [[1, "First note"], ...] }
```

Practice:

- Always run `CREATE TABLE IF NOT EXISTS ...` at startup — there's no separate migration step, your app owns its own schema.
- `dbQuery` returns `{ columns, rows }` with rows as arrays, not objects. Map columns to indices once and reuse:
  ```javascript
  const { columns, rows } = result;
  const idIdx = columns.indexOf("id");
  const records = rows.map((r) => ({ id: r[idIdx] /* ... */ }));
  ```
- Parameters support `null`, boolean, number, string only — no arrays/objects, no blobs in or out (blob columns come back as `null`). Don't design a schema around storing binary data.
- Use `?` placeholders, never string-concatenate values into SQL — this is the one place classic injection bugs are possible.
- One database file is enough for almost every app; reach for a second `dbName` only if you genuinely have unrelated datasets.

### Logging — `log`

Appends to `dataPath/logs/shell.log`. Use it for diagnosing issues in built apps where there's no devtools console available (release with `showDevMenu = false`), not as a replacement for `console.log` during dev.

```javascript
await shell.log("user clicked save");        // level defaults to "info"
await shell.log("save failed: " + err, "error");
```

### Notifications — `notify`

Native OS notification. Use sparingly — for things the user isn't actively watching the window for (background task finished, export done).

```javascript
await shell.notify("Export finished", "report.csv saved to disk");
```

### Networking — `fetch` / `get` / `post`

Proxied through Rust, so it bypasses the webview's CORS restrictions — call any `http(s)` API directly without a backend or CORS proxy.

```javascript
const res = await shell.get("https://api.example.com/items");
if (res.ok) {
  const data = JSON.parse(res.body);
}

await shell.post(
  "https://api.example.com/items",
  JSON.stringify({ name: "x" }),
  { "Content-Type": "application/json" },
);

// full control:
await shell.fetch(url, { method: "PATCH", headers: {...}, body: "..." });
```

Practice:

- `res.body` is always a **string** — `JSON.parse` it yourself; there's no automatic content-type handling.
- Only `http://`/`https://` are allowed — no `file://`, no relative URLs.
- No streaming, no binary bodies, no multipart/form-data, no WebSockets. If you need any of those, this isn't the right transport — that's a real ceiling of v1, not a config option.

### Window and screen — mostly skip these

`getWindowPosition/setWindowPosition/getWindowSize/setWindowSize/minimize/getScreens/getScreenAt` exist for apps that need to manage their own window placement (e.g. restoring a saved position, snapping to a specific monitor). All sizes/positions are **physical pixels** — divide by `scaleFactor` (from `getScreens`) if you need logical/CSS pixels. Most apps never need these; don't reach for them unless you have an actual multi-monitor or window-persistence requirement.

### What you get for free, unprompted

- Keyboard shortcuts (`Cmd/Ctrl+Shift+M/I` devtools toggle, `Cmd/Ctrl+Shift+R` reload) and the View menu are injected automatically — don't build your own reload/devtools UI.
- `withGlobalTauri` is on, but you should not need raw `window.__TAURI__` — everything supported is exposed via `window.shell`. Reaching past `shell` into Tauri internals means you're outside what this shell promises to keep stable.

## Errors you should actually handle

- `readFile` on a missing file — expected on first run, catch it and fall back to defaults.
- `fetch` network failures — catch and show the user something, don't let it crash silent.
- Everything else (invalid filename, invalid SQL, bad URL scheme) is a programming error on your part — fix the call, don't defensively swallow it.

## Full reference example

```html
<!doctype html>
<html>
  <body>
    <button id="save">Save</button>
    <button id="fetch">Fetch</button>
    <pre id="out"></pre>
    <script>
      const out = document.getElementById("out");

      document.getElementById("save").onclick = async () => {
        await shell.saveFile("note.txt", "hello");
        await shell.log("saved note");
        await shell.notify("Saved", "note.txt updated");
        out.textContent = "saved";
      };

      document.getElementById("fetch").onclick = async () => {
        const res = await shell.get("https://jsonplaceholder.typicode.com/todos/1");
        out.textContent = JSON.stringify(res, null, 2);
      };
    </script>
  </body>
</html>
```

## Checklist before calling an app "done"

1. `app.toml` paths all resolve relative to the `app.toml` itself — no leakage outside the app folder.
2. Filenames passed to `saveFile`/`readFile`/`dbQuery`/`dbExecute` are simple names, never paths.
3. Any SQLite table creation uses `CREATE TABLE IF NOT EXISTS` and runs on every startup.
4. SQL parameters are passed via `?` placeholders, never concatenated.
5. `fetch`/`get`/`post` responses are JSON-parsed only if you know the API returns JSON — `res.body` is always a raw string.
6. Tested by launching the `app-ly` binary from your app's folder (or `npm run tauri dev -- --config ./yourapp/app.toml` if working from a shell checkout), including the cold-start case (no existing `dataPath` files).
