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
keychainPrefix = "my-app"     # optional. prefix for OS keychain keys, default "app-ly"

[settings]                    # optional. string-only key/value map, exposed as shell.settings
apiBaseUrl = "https://api.example.com"

[[allowedCommands]]           # optional, repeatable. programs shell.run/shell.spawn may start
name = "git"                  # the alias your JS passes to run/spawn
program = "git"               # bare name (resolved via PATH) or absolute path
args = ["^(status|log)$"]     # optional. one anchored regex per argument position
timeoutMs = 30000             # optional. default timeout for this command
```

Rules and gotchas:

- **All paths in `app.toml` are relative to the directory containing that `app.toml`** — not the project root, not cwd. Keep `icon`, `contents`, and `dataPath` inside (or under) the same folder as the `app.toml` that references them. Don't use absolute paths or `..` to reach outside your app folder.
- `contents` must point at a single HTML **file**. Everything else referenced from that HTML (JS, CSS, images) is resolved relative to that file's directory by the browser as normal — put your whole frontend under one `contents/` folder so it travels as a unit.
- `dataPath` is always relative to the directory containing `app.toml` — both in dev and release. The directory (and a `logs/` subdirectory inside it) is created automatically at startup. Don't try to create it yourself.
- `showDevMenu`: leave it `true` while building; an app you intend to ship without DevTools exposed should set it `false` (or omit it, since release already defaults to `false`).
- `[settings]` values must be TOML strings (`key = "value"`) — this is an env-var-shaped map, not a general config tree. Quote numbers/booleans too if you put them here; your JS gets them back as strings either way.
- A `.env` file (plain `KEY=VALUE` lines, `#` comments, optional quotes) placed next to `app.toml` is merged on top of `[settings]` and **wins on key collisions**. Use `[settings]` for checked-in defaults, `.env` for local overrides and secrets you don't want in version control — and make sure `.env` is in `.gitignore`.
- Discovery order: `--config <path>` flag → folder containing the `app-ly.app` bundle/executable (this is the normal case — your `app.toml` sits right next to the binary you copied in) → bundled fallback resource baked into the binary itself → (dev-only) `./app.toml` in cwd → project root `app.toml`. As an app author you don't need `--config` at all: just keep `app.toml` next to the binary and it's found automatically. `--config` is only useful for testing multiple app folders against one shared binary without copying it around.
- `[[allowedCommands]]` is the only way your contents HTML can start a program, and every field of it (`program`, `cwd`, `env`, the argument patterns) lives here in `app.toml`, never in JS. No entries at all means process execution is fully disabled — which is the right setting unless you actually need it. See [Running programs](#running-programs--run--spawn--listcommands) below.
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

Available immediately on `window` before your page scripts run. Every method returns a `Promise` that **rejects with a string** on failure — always wrap calls in `try/catch` or `.catch()` where failure is expected (e.g. `readFile` on a file that doesn't exist yet). `shell.settings` is the one exception — it's a plain object, not a method, available synchronously with no `await`.

### Summary

| Method | Signature | Purpose |
|---|---|---|
| `settings` | `{ [key: string]: string }` (property, not a call) | `[settings]` from `app.toml`, merged with `.env` |
| `saveFile` | `(name, contents) → void` | Write a text file to `dataPath` |
| `readFile` | `(name) → string` | Read a text file from `dataPath`; rejects if missing |
| `deleteFile` | `(name) → void` | Delete a file from `dataPath` |
| `renameFile` | `(name, newName) → void` | Rename/move a file within `dataPath` |
| `openFile` | `(name) → void` | Open a file in `dataPath` with the OS default app |
| `openFileLocation` | `(name) → void` | Reveal a file in `dataPath` in the OS file manager |
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
| `openWindow` | `(url, options?) → { id }` | Open a child webview window (e.g. an external auth flow) |
| `closeWindow` | `(id) → void` | Close a window opened via `openWindow` |
| `onWindowNavigated` | `((id, url) => void) → unlisten` | Subscribe to navigation events across all child windows |
| `onWindowLoaded` | `((id, url) => void) → unlisten` | Subscribe to page-load-finished events across all child windows |
| `onWindowClosed` | `((id) => void) → unlisten` | Subscribe to child windows closing |
| `getWindowBody` | `(id) → string` | Get `document.body.innerText` from a child window |
| `evalWindow` | `(id, code) → any` | Run JS in a child window (as an `async` function body) and return its result |
| `authViaBrowser` | `(authUrl, options?) → authCode` | Run a sign-in flow in the system browser, wait for the redirect back, return the auth code |
| `secretSet` | `(service, account, password) → void` | Store a secret in the OS keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager) |
| `secretGet` | `(service, account) → string` | Retrieve a secret from the OS keychain |
| `secretDelete` | `(service, account) → void` | Delete a secret from the OS keychain |
| `httpStart` | `(options?) → { port }` | Start a local HTTP server on `127.0.0.1` |
| `httpRespond` | `(id, status, headers?, body?) → void` | Send an HTTP response for a received request |
| `httpStop` | `() → void` | Stop the HTTP server |
| `onHttpRequest` | `(callback) → unlisten` | Subscribe to incoming HTTP request events |
| `wsStart` | `(options?) → { port }` | Start a local WebSocket server on `127.0.0.1` |
| `wsSend` | `(id, data) → void` | Send a text message to a WebSocket client |
| `wsClose` | `(id) → void` | Close a WebSocket connection |
| `wsStop` | `() → void` | Stop the WebSocket server |
| `onWsConnection` | `(callback) → unlisten` | Subscribe to new WebSocket client connections |
| `onWsMessage` | `(callback) → unlisten` | Subscribe to WebSocket text messages |
| `onWsClose` | `(callback) → unlisten` | Subscribe to WebSocket client disconnections |
| `run` | `(name, args?, options?) → { stdout, stderr, code, signal, timedOut }` | Run an allowlisted program to completion |
| `spawn` | `(name, args?, options?) → ChildProcess` | Start an allowlisted program and stream its output |
| `listCommands` | `() → [{ name, program, argsRestricted, timeoutMs }]` | List the `[[allowedCommands]]` entries this app was configured with |

`name`/`dbName` arguments are always simple filenames — see [path rules](#path-rules-inside-the-app-filenames-not-paths) above. Window/screen methods are rarely needed — see [below](#window-and-screen--mostly-skip-these). Child-window methods are covered [below](#child-windows--openwindow--closewindow--onwindownavigated--onwindowclosed).

### Settings — `shell.settings`

A plain object, populated once at startup from `app.toml`'s `[settings]` table merged with a `.env` file beside it (`.env` wins on conflicts). Use it for configuration that varies per deployment — API base URLs, feature flags, environment name — the same role `process.env` plays in a Node app.

```toml
# app.toml
[settings]
apiBaseUrl = "https://api.example.com"
```

```
# .env, next to app.toml — not checked into git
apiBaseUrl = "https://staging.api.example.com"
```

```javascript
const res = await shell.get(`${shell.settings.apiBaseUrl}/items`);
```

Practice:

- All values are strings, always — same as OS environment variables. Parse yourself (`Number(...)`, `=== "true"`) if you need something else.
- It's read-only and fixed at startup — there's no `setSetting`. If you need runtime-writable app state, use `saveFile`/SQLite instead; `settings` is for deployment-time configuration, not user data.
- Don't put real secrets in `[settings]` if `app.toml` is committed to a repo — put them in `.env` and gitignore it.

### Files — `saveFile` / `readFile`

Plain text files in `dataPath`. Good for settings, small exports, anything you'd otherwise put in `localStorage` but want to survive as a real file.

```javascript
await shell.saveFile("settings.json", JSON.stringify({ theme: "dark" }));
const raw = await shell.readFile("settings.json"); // throws if missing
const settings = JSON.parse(raw);
```

Practice: treat this as key-value storage keyed by filename, not a filesystem. For anything relational or queryable, use SQLite instead.

### Managing files — `deleteFile` / `renameFile` / `openFile` / `openFileLocation`

For the common pattern of "generate a file in `dataPath`, then let the user open it or find it on disk" — e.g. exporting a report and giving the user a link to view it or reveal it in Finder/Explorer.

```javascript
await shell.saveFile("report.csv", csvContents);

// render as a clickable UI affordance — not a literal <a href="file://...">,
// since navigating the webview to file:// isn't reliable cross-platform
openLink.onclick = () => shell.openFile("report.csv");
revealLink.onclick = () => shell.openFileLocation("report.csv");

// later
await shell.renameFile("report.csv", "report-final.csv");
await shell.deleteFile("report-final.csv");
```

Practice:

- `openFile`/`openFileLocation` shell out to the OS's own opener (`open`/`explorer`/`xdg-open`) — there's no in-app file viewer or preview. The promise resolves once the OS has been asked to open the item, not once the other application has actually launched.
- `openFileLocation` selects the file within its folder on macOS/Windows; on Linux, where there's no universal "select in file manager" action, it opens the enclosing folder instead — don't rely on the file being visibly highlighted there.
- `renameFile`/`deleteFile` operate within `dataPath` only — both `name` and `newName` follow the same simple-filename rule as everything else here, so you can't rename a file to escape `dataPath` either.

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

### Child windows — `openWindow` / `closeWindow` / `onWindowNavigated` / `onWindowClosed` / `getWindowBody` / `evalWindow`

The main window can't navigate away to run an external flow (there's no browser chrome, and doing so would lose your app). Use a child window for that — the canonical case is an OAuth/login flow you need to drive and observe from your JS app. If the identity provider refuses to run inside an embedded webview (common for SAML/SSO), use [`authViaBrowser`](#system-browser-sign-in--authviabrowser) instead, which runs the flow in the system browser.

```javascript
const { id } = await shell.openWindow("https://accounts.example.com/oauth/authorize?...", {
  title: "Sign in",
  width: 480,
  height: 640,
});

const unlisten = await shell.onWindowNavigated((windowId, url) => {
  if (windowId !== id) return;
  if (url.startsWith("https://yourapp.example.com/callback")) {
    const code = new URL(url).searchParams.get("code");
    shell.closeWindow(id);
    unlisten();
    // exchange `code` for a token via shell.post(...)
  }
});

// optional: react if the user closes the popup themselves without finishing
const unlistenClosed = await shell.onWindowClosed((windowId) => {
  if (windowId === id) unlistenClosed();
});
```

- `openWindow(url, options?)` — `options: { title?, width?, height? }` (defaults `480×640`, a typical auth-popup size). Only `http://`/`https://` URLs are allowed, same rule as `fetch`. Resolves to `{ id }` — an internal window label, not a DOM handle; use it to filter events and to `closeWindow`.
- `closeWindow(id)` — closes a window opened via `openWindow`. You cannot close `"main"` this way.
- `onWindowNavigated((id, url) => ...)` — fires on every navigation in every child window, including redirects. Always filter by `id`, since multiple child windows can be open at once. Returns a promise resolving to an unlisten function — call it once you're done watching.
- `onWindowClosed((id) => ...)` — fires when a child window closes, whether via `closeWindow` or the user closing it manually. Use it to clean up state if the user abandons the flow.
- `getWindowBody(id)` — returns `document.body.innerText` from the child window as a string. Handy for scraping a status message off a page you don't control (e.g. "did the OAuth consent screen show an error?").
- `evalWindow(id, code)` — runs `code` as an `async` function body inside the child window and returns its (JSON-serializable) result; `code` can `await` and `return` a value, and a thrown error becomes a rejected promise on the caller's side:
  ```javascript
  const title = await shell.evalWindow(id, "return document.title;");
  ```
- There's no sandboxing between a child window and your main window beyond being separate native windows — don't open untrusted URLs you wouldn't want the user pointed at outside your app either. `evalWindow` runs arbitrary JS with the same lack of sandboxing, so only point it at windows you opened yourself.
- `onWindowLoaded((id, url) => ...)` — like `onWindowNavigated`, but fires once the page has actually finished loading rather than on navigation start/redirect. Prefer this over `onWindowNavigated` when you need the DOM settled before calling `getWindowBody`/`evalWindow`.

### System-browser sign-in — `authViaBrowser`

Some identity providers (most SAML/SSO setups) detect and refuse to run inside an embedded webview like `openWindow`'s child window — they only work in the user's actual default browser. `authViaBrowser` covers that case: it opens the URL in the system browser, spins up a one-time local HTTP listener, and resolves once your backend redirects the browser back to it.

```javascript
// authUrl is your backend's "start sign-in" URL — it should accept a returnUrl
// query param and redirect the browser there (with ?authCode=... or ?error=...)
// once the identity provider flow completes.
const authCode = await shell.authViaBrowser(
  "https://idp.example.com/saml/login?service=myapp",
);

const res = await shell.post(
  "https://api.example.com/auth/exchange",
  JSON.stringify({ authCode }),
  { "Content-Type": "application/json" },
);
if (res.ok) {
  const { token } = JSON.parse(res.body);
  // store token, e.g. via saveFile or in memory
}
```

How it works, in order:

1. A single shared background listener on `127.0.0.1` handles all callbacks (started lazily on first use). Each flow embeds a unique `sid` in its return URL so concurrent auth requests are safely disambiguated.
2. It opens `authUrl` in the OS default browser, with `returnUrl=<the callback URL>` appended as a query param.
3. Your backend runs its normal SSO flow, then 302-redirects the browser to `<returnUrl>?authCode=<value>` (or `?error=<value>` on failure).
4. The shell's local listener catches that one request, shows a static "you can close this tab" page, and resolves the promise with `authCode` (or rejects with the `error` value).

Practice:

- Prefer the default auto-picked port (omit `options.returnUrl`) unless your identity provider requires a fixed, pre-registered redirect URI — only then pass a fixed `returnUrl` (e.g. `"http://127.0.0.1:41417/callback"`), and make sure that same URL is registered with the provider.
- Set `options.timeoutMs` generously for flows that involve MFA or an approval step; the default is 2 minutes (`120000`).
- This is a one-shot flow, not a window you control — there's no `id`, no `evalWindow`/`getWindowBody` into it, and nothing to `closeWindow`. If you need to script/observe the login page itself, use `openWindow` instead (accepting that some providers will block it).
- The call only returns an `authCode` string; exchanging it for a session/token is your backend's job, typically via `shell.post`.

### Secure store — `secretSet` / `secretGet` / `secretDelete`

Stores secrets in the OS system keychain using keyring-rs. The keychain is the right place for API tokens, credentials, encryption keys — anything you'd store in `/etc/` or a `.env` file but want properly encrypted at rest by the OS.

```javascript
await shell.secretSet("myapp", "api-key", "sk-abc123...");
const key = await shell.secretGet("myapp", "api-key");
await shell.secretDelete("myapp", "api-key");
```

- `service` is a logical grouping name (e.g. your app name). `account` is the specific identifier for this secret (e.g. `"api-key"` or `"user@example.com"`). Both are strings.
- `secretGet` rejects if the entry doesn't exist — there's no `secretExists` helper; catch the error.
- Uses the native keychain: **macOS** Keychain, **Linux** Secret Service (libsecret/gnome-keyring), **Windows** Credential Manager. No file paths, no config.

### HTTP Server — `httpStart` / `httpRespond` / `httpStop` / `onHttpRequest`

Runs a local HTTP server inside your app. Useful for receiving webhooks, exposing a local API to other processes on the machine, or embedding a control UI.

```javascript
const { port } = await shell.httpStart({ port: 0 });
console.log("HTTP server on port", port);

await shell.onHttpRequest(async (req) => {
  console.log(req.method, req.url, req.headers, req.body);
  await shell.httpRespond(req.id, 200, { "Content-Type": "text/plain" }, "Hello");
});

// later
await shell.httpStop();
```

- `httpStart({ port })` — binds to `127.0.0.1`. Default port `0` picks any free port. Returns `{ port }` with the actual bound port. Rejects if a server is already running.
- `onHttpRequest(callback)` — fires `{ id, method, url, headers, body }` for each incoming request. The server thread blocks until you call `httpRespond`. Return an `UnlistenFn` to stop listening.
- `httpRespond(id, status, headers?, body?)` — sends the response. `id` must match a pending request. Rejects if the id is unknown or already responded.
- `httpStop()` — stops the server. Pending unanswered requests will error.
- One server at a time. Only text bodies are supported (no streaming).

### WebSocket Server — `wsStart` / `wsSend` / `wsClose` / `wsStop` / `onWsConnection` / `onWsMessage` / `onWsClose`

Runs a local WebSocket server for real-time bidirectional communication with other processes on the machine.

```javascript
const { port } = await shell.wsStart({ port: 0 });
console.log("WS server on port", port);

await shell.onWsConnection(({ id }) => {
  console.log("client connected:", id);
  shell.wsSend(id, "Welcome!");
});

await shell.onWsMessage(({ id, data }) => {
  console.log("received from", id, data);
});

await shell.onWsClose(({ id }) => {
  console.log("client disconnected:", id);
});

// later
await shell.wsStop();
```

- `wsStart({ port })` — binds to `127.0.0.1`. Default port `0` picks any free port. Returns `{ port }`. One server at a time.
- `wsSend(id, data)` — sends a text message to a connected client by id.
- `wsClose(id)` — gracefully closes a connection.
- `wsStop()` — stops the server, closing all active connections.
- Events: `onWsConnection({ id })`, `onWsMessage({ id, data })`, `onWsClose({ id })`. Each returns an `UnlistenFn`.
- Only text messages are supported (binary frames are silently ignored).

### Running programs — `run` / `spawn` / `listCommands`

Your app can shell out to local programs — but only to programs *you* listed in `app.toml`, by the `name` you gave them. There is no "run this command line" call: the page picks a listed entry and supplies arguments, nothing more.

```toml
# app.toml
[[allowedCommands]]
name = "git"                                  # the alias your JS passes to run/spawn
program = "git"                               # bare name (found on PATH) or an absolute path
args = ["^(status|log|diff)$", "^--oneline$"] # optional: one regex per argument position
extraArgs = "^[\\w./-]+$"                     # optional: pattern for every argument past `args`
maxArgs = 8                                   # optional: hard cap on argument count
cwd = "repo"                                  # optional: relative to this app.toml's directory
timeoutMs = 30000                             # optional: default timeout for this command
env = { GIT_PAGER = "cat" }                   # optional: extra env vars for the child
```

```javascript
// one-shot: wait for it, get everything it printed
const { stdout, stderr, code, timedOut } = await shell.run("git", ["status"], {
  timeoutMs: 5000,
});
if (code !== 0) show(stderr);

// streaming: react to output as it arrives
const proc = await shell.spawn("git", ["log", "--oneline"]);
proc.onStdout((data) => (out.textContent += data));
const { code: exitCode } = await proc.exited;

// ...or consume the output as an async iterable instead of with handlers
for await (const { stream, data } of await shell.spawn("git", ["log"])) {
  out.textContent += data;
}
```

Practice:

- **The allowlist is the security boundary, and it is only as tight as you write it.** Omitting both `args` and `extraArgs` means *any* arguments are accepted for that program — an unrestricted `git` entry allows `git push --force`. Add patterns (and `maxArgs`) to anything that isn't harmless.
- **Patterns are fully anchored for you.** Each one is compiled as `^(?:P)$`, so `"status"` matches `status` and not `xstatusy`. Writing your own `^...$` is fine and changes nothing.
- Positional patterns line up with argument indexes; arguments beyond the list are rejected unless `extraArgs` is set, and passing fewer arguments than there are patterns is always fine.
- **Nothing runs through a shell.** No pipes, globs, `&&`, redirection, or quoting — arguments reach the program verbatim. That removes shell injection as a concern, and it also means "one command line" tricks don't work: sequence the steps in JS instead.
- **`program`, `cwd`, and `env` are config-only** and can never be passed from JS. That's deliberate. If your app needs a different working directory, add a second `[[allowedCommands]]` entry for it.
- **Know what rejects and what doesn't.** An unknown `name`, an argument that fails a pattern, a bad regex in your config, or a missing executable all **reject**. A program that runs and exits non-zero **resolves** — you must check `code` yourself. So does a timeout: the child is killed and the call resolves with `timedOut: true` plus whatever output it had produced.
- Timeouts resolve in this order: the call's `options.timeoutMs`, else the entry's `timeoutMs`, else no timeout at all. Give anything long-running one, or a hung child sticks around for the life of the app.
- `code` is `null` when the process was killed or signalled instead of exiting normally; `signal` only ever has a value on Unix (always `null` on Windows).
- `spawn` gives you a process object: `id`, `pid`, `onStdout`/`onStderr`/`onExit` (each returning an unsubscribe function), `write(data)` / `closeStdin()` for stdin, `exit()` / `kill()` / `setTimeout(ms)` for controlling it, an `exited` promise, and `for await (const { stream, data } of proc)`. Output produced before you attach a handler is buffered and replayed, so you can't lose the first chunk by attaching late.
- Stop a process with `exit()` (asks politely — `SIGTERM`, so the child can clean up) and reach for `kill()` only when it must die now. `exit()` resolves when the signal was *sent*; await `exited` to know the process is actually gone.
- You are not stuck with the timeout you spawned with: `setTimeout(ms)` re-arms the deadline from the moment you call it, so you can extend a job that turned out to be slow, put a deadline on a process spawned without one, or call it on every chunk to get an inactivity timeout. `setTimeout(null)` clears it.
- `listCommands()` returns `{ name, program, argsRestricted, timeoutMs }` for each entry — use it to grey out features when an app deployment wasn't configured with the command it needs. `cwd`/`env` values are never exposed to JS.
- Use `run` for anything that finishes quickly and `spawn` for anything long, chatty, or interactive. `run` buffers everything in memory before it resolves.

### What you get for free, unprompted

- Keyboard shortcuts (`Cmd/Ctrl+Shift+M/I` devtools toggle, `Cmd/Ctrl+Shift+R` reload) and the View menu are injected automatically — don't build your own reload/devtools UI.
- A native **Edit** menu (Cut/Copy/Paste/Select All) is wired up with the platform's standard shortcuts (`Cmd/Ctrl+X/C/V/A`) — text inputs and `contenteditable` regions in your HTML get working copy/paste without any JS on your part.
- `withGlobalTauri` is on, but you should not need raw `window.__TAURI__` — everything supported is exposed via `window.shell`. Reaching past `shell` into Tauri internals means you're outside what this shell promises to keep stable.

## Errors you should actually handle

- `readFile`/`deleteFile`/`renameFile`/`openFile`/`openFileLocation` on a missing file — expected whenever the file might not have been created yet, catch it and fall back to defaults.
- `fetch` network failures — catch and show the user something, don't let it crash silent.
- A non-zero `code` (or `timedOut: true`) from `run`/`spawn` — these *resolve*, so nothing throws; check the result and tell the user. A rejected `run`/`spawn`, by contrast, means the command isn't allowlisted or your arguments don't match the configured patterns — that's a config/programming error.
- Everything else (invalid filename, invalid SQL, bad URL scheme) is a programming error on your part — fix the call, don't defensively swallow it.

## Full reference example

```html
<!doctype html>
<html>
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Shell Example</title>
  </head>
  <body>
    <h1>app-ly Shell Demo</h1>

    <section>
      <h2>Files</h2>
      <button id="save">Save note</button>
      <button id="load">Load note</button>
    </section>

    <section>
      <h2>Network</h2>
      <button id="fetch">Fetch JSON</button>
    </section>

    <section>
      <h2>Secure Store</h2>
      <button id="secret-set">Store secret</button>
      <button id="secret-get">Get secret</button>
      <button id="secret-delete">Delete secret</button>
    </section>

    <section>
      <h2>HTTP Server</h2>
      <button id="http-start">Start HTTP server</button>
      <button id="http-stop">Stop HTTP server</button>
    </section>

    <section>
      <h2>WebSocket Server</h2>
      <button id="ws-start">Start WS server</button>
      <button id="ws-stop">Stop WS server</button>
    </section>

    <pre id="out"></pre>

    <script>
      const out = document.getElementById("out");

      function show(value) {
        out.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
      }

      document.getElementById("save").onclick = async () => {
        await shell.saveFile("note.txt", "hello " + new Date().toISOString());
        await shell.log("saved note");
        show("Saved.");
      };

      document.getElementById("load").onclick = async () => {
        try {
          const text = await shell.readFile("note.txt");
          show(text);
        } catch (e) {
          show(String(e));
        }
      };

      document.getElementById("fetch").onclick = async () => {
        const res = await shell.get("https://jsonplaceholder.typicode.com/todos/1");
        show(res);
      };

      document.getElementById("secret-set").onclick = async () => {
        await shell.secretSet("demo", "test-key", "my-secret-value");
        show("Secret stored.");
      };

      document.getElementById("secret-get").onclick = async () => {
        try {
          const value = await shell.secretGet("demo", "test-key");
          show(value);
        } catch (e) {
          show(String(e));
        }
      };

      document.getElementById("secret-delete").onclick = async () => {
        await shell.secretDelete("demo", "test-key");
        show("Secret deleted.");
      };

      let httpRunning = false;
      document.getElementById("http-start").onclick = async () => {
        if (httpRunning) return show("Already running");
        const { port } = await shell.httpStart({ port: 0 });
        httpRunning = true;
        show(`HTTP server on port ${port}`);
        await shell.onHttpRequest(async (req) => {
          await shell.httpRespond(req.id, 200, { "Content-Type": "application/json" }, JSON.stringify({ ok: true, url: req.url }));
        });
      };

      document.getElementById("http-stop").onclick = async () => {
        await shell.httpStop();
        httpRunning = false;
        show("HTTP server stopped.");
      };

      let wsRunning = false;
      document.getElementById("ws-start").onclick = async () => {
        if (wsRunning) return show("Already running");
        const { port } = await shell.wsStart({ port: 0 });
        wsRunning = true;
        show(`WS server on port ${port}`);
        await shell.onWsConnection(({ id }) => {
          show(`Client connected: ${id}`);
          shell.wsSend(id, "Welcome!");
        });
        await shell.onWsMessage(({ id, data }) => {
          show(`Message from ${id}: ${data}`);
        });
      };

      document.getElementById("ws-stop").onclick = async () => {
        await shell.wsStop();
        wsRunning = false;
        show("WS server stopped.");
      };
    </script>
  </body>
</html>
```

## Checklist before calling an app "done"

1. `app.toml` paths all resolve relative to the `app.toml` itself — no leakage outside the app folder.
2. Filenames passed to `saveFile`/`readFile`/`deleteFile`/`renameFile`/`openFile`/`openFileLocation`/`dbQuery`/`dbExecute` are simple names, never paths.
3. Any SQLite table creation uses `CREATE TABLE IF NOT EXISTS` and runs on every startup.
4. SQL parameters are passed via `?` placeholders, never concatenated.
5. `fetch`/`get`/`post` responses are JSON-parsed only if you know the API returns JSON — `res.body` is always a raw string.
6. Secrets/local overrides live in `.env` next to `app.toml` (gitignored), not in `[settings]` if `app.toml` is committed.
7. Tested by launching the `app-ly` binary from your app's folder (or `npm run tauri dev -- --config ./yourapp/app.toml` if working from a shell checkout), including the cold-start case (no existing `dataPath` files).
8. Sensitive credentials use `secretSet`/`secretGet` (OS keychain) instead of `saveFile` for anything you'd call a secret.
9. If using the HTTP or WebSocket server, handle the "already running" error gracefully in your UI.
10. Any `[[allowedCommands]]` entry is as narrow as it can be (argument patterns, `maxArgs`, a `timeoutMs`), and the UI handles both a non-zero exit code and `timedOut`.
