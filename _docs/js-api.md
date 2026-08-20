# JavaScript API

Contents HTML receives a global `window.shell` object injected by the Tauri shell before page scripts run.

No npm packages or build step are required in contents HTML.

The shell also injects [`src-tauri/scripts/shell-shortcuts.js`](../src-tauri/scripts/shell-shortcuts.js) automatically.

## Keyboard shortcuts

Injected into every contents page:

| Shortcut | Action |
|----------|--------|
| `Cmd + Shift + M` or `Cmd + Shift + I` (macOS) / `Ctrl + Shift + M` or `Ctrl + Shift + I` (others) | Toggle the native [Web Inspector](https://v2.tauri.app/develop/debug/) |
| `Cmd + Shift + R` (macOS) / `Ctrl + Shift + R` (others) | Reload the contents page |
| Right-click → **Inspect Element** | Open the Web Inspector |
| `Cmd/Ctrl + X` / `+ C` / `+ V` / `+ A` | Cut / Copy / Paste / Select All, via a native **Edit** menu |

The Web Inspector is available when `showDevMenu` is enabled in `app.toml` (default `true` in debug builds, `false` in release). Set `showDevMenu = true` in the external `app.toml` beside your `.app` bundle to enable the menu item and shortcuts in release.

The shell calls Tauri’s `WebviewWindow::open_devtools` / `close_devtools` (via `shell_toggle_devtools`). Release builds include the `devtools` Cargo feature on `tauri`. On macOS, programmatic inspector access uses a private API and is not App Store–compatible.

The same actions are in the native app menu under **View** → **Reload** / **Open DevTools** (DevTools follows `showDevMenu`).

The **Edit** menu (Cut/Copy/Paste/Select All) is always present, unconditionally — it's what gives standard text inputs and `contenteditable` regions working clipboard shortcuts. Nothing to configure.

## `shell.settings`

A plain object (not a function — no `await` needed), available synchronously as soon as `window.shell` exists. Populated at startup from the `[settings]` table in `app.toml`, merged with a `.env` file in the same directory as `app.toml` (`.env` wins on key collisions).

- All values are strings, same as OS environment variables — parse yourself if you need numbers/booleans.
- Read-only; there is no setter. It reflects `app.toml`/`.env` at process start, not live state.

```toml
# app.toml
[settings]
apiBaseUrl = "https://api.example.com"
```

```
# .env, next to app.toml
apiBaseUrl = "https://staging.api.example.com"
```

```javascript
console.log(shell.settings.apiBaseUrl); // "https://staging.api.example.com"
```

`.env` parsing supports `KEY=VALUE` lines, blank lines, `#` comments, an optional `export ` prefix, and matching `'single'` or `"double"` quotes around the value. No multi-line values, no `\n` escape sequences, no variable interpolation.

## `shell.saveFile(name, contents)`

Writes a text file into the configured `dataPath`.

- `name` — simple filename only (e.g. `"settings.json"`)
- `contents` — string to write
- Returns: `Promise<void>`

```javascript
await shell.saveFile("settings.json", JSON.stringify({ theme: "dark" }));
```

## `shell.readFile(name)`

Reads a text file from `dataPath`.

- `name` — simple filename only
- Returns: `Promise<string>`

```javascript
const raw = await shell.readFile("settings.json");
const settings = JSON.parse(raw);
```

## `shell.deleteFile(name)`

Deletes a file from `dataPath`.

- `name` — simple filename only
- Returns: `Promise<void>` — rejects if the file doesn't exist

```javascript
await shell.deleteFile("old-export.csv");
```

## `shell.renameFile(name, newName)`

Renames/moves a file within `dataPath` (both names are simple filenames, so this cannot move a file outside `dataPath`).

- `name` — current simple filename
- `newName` — new simple filename
- Returns: `Promise<void>` — rejects if `name` doesn't exist or `newName` is invalid

```javascript
await shell.renameFile("draft.csv", "report-2024-01-01.csv");
```

## `shell.openFile(name)`

Opens a file in `dataPath` with the OS's default application for its type (e.g. a `.csv` opens in the default spreadsheet app). Use this to back a "view file" link in your UI.

- `name` — simple filename only
- Returns: `Promise<void>` — rejects if the file doesn't exist; resolves once the OS has been asked to open it (doesn't wait for the other app to launch)

```javascript
await shell.openFile("report.csv");
```

## `shell.openFileLocation(name)`

Reveals a file in the OS's file manager (Finder/Explorer), selecting it. On Linux, where there's no universal "select in file manager" action, this opens the enclosing folder instead. Use this to back an "open containing folder" link in your UI.

- `name` — simple filename only
- Returns: `Promise<void>` — rejects if the file doesn't exist

```javascript
await shell.openFileLocation("report.csv");
```

## `shell.log(message, level?)`

Appends a line to `dataPath/logs/shell.log`.

- `message` — log text
- `level` — optional level string, default `"info"`
- Returns: `Promise<void>`

```javascript
await shell.log("user clicked save", "info");
```

## `shell.notify(title, body)`

Shows a native desktop notification.

- `title` — notification title
- `body` — notification body
- Returns: `Promise<void>`

```javascript
await shell.notify("Done", "Export finished");
```

## `shell.fetch(url, options?)`

HTTP/HTTPS client proxied through Rust. Bypasses browser CORS limits of the `shell://` webview origin.

- `url` — `http://` or `https://` URL
- `options` — optional object:
  - `method` — `GET`, `POST`, `PUT`, `PATCH`, `DELETE` (default `GET`)
  - `headers` — object of header name → value
  - `body` — request body string
- Returns: `Promise<{ ok, status, statusText, headers, body }>`

```javascript
const response = await shell.fetch("https://api.example.com/items", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ q: "test" }),
});

if (response.ok) {
  const data = JSON.parse(response.body);
}
```

## `shell.get(url, headers?)`

Convenience wrapper for `shell.fetch` with `GET`.

```javascript
const response = await shell.get("https://httpbin.org/get");
```

## `shell.post(url, body, headers?)`

Convenience wrapper for `shell.fetch` with `POST`.

```javascript
const response = await shell.post(
  "https://httpbin.org/post",
  JSON.stringify({ hello: "world" }),
  { "Content-Type": "application/json" },
);
```

## `shell.getWindowPosition()`

Returns the main window position in physical screen pixels (top-left of the outer frame, including title bar).

- Returns: `Promise<{ x: number, y: number }>`

```javascript
const { x, y } = await shell.getWindowPosition();
```

## `shell.setWindowPosition(x, y)`

Moves the main window. Coordinates are physical screen pixels, matching `getWindowPosition`.

- `x` — horizontal position
- `y` — vertical position
- Returns: `Promise<void>`

```javascript
await shell.setWindowPosition(120, 80);
```

## `shell.getWindowSize()`

Returns the main window client area size in physical pixels (content region, excluding title bar and borders). Matches the size used by `setWindowSize`.

- Returns: `Promise<{ width: number, height: number }>`

```javascript
const { width, height } = await shell.getWindowSize();
```

## `shell.setWindowSize(width, height)`

Resizes the main window client area in physical pixels.

- `width` — content width
- `height` — content height
- Returns: `Promise<void>`

```javascript
await shell.setWindowSize(1024, 768);
```

## `shell.minimize()`

Minimizes the main window to the dock/taskbar.

- Returns: `Promise<void>`

```javascript
await shell.minimize();
```

## `shell.getScreens()`

Lists all connected displays and marks which one is primary and which one contains the main window.

All sizes and positions are in physical screen pixels. Use `scaleFactor` on each entry to convert to logical pixels (`logical = physical / scaleFactor`).

- Returns: `Promise<{ screens, primaryIndex, currentIndex }>`

Each `screens` entry:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string \| null` | Display name when available |
| `size` | `{ width, height }` | Full monitor resolution |
| `position` | `{ x, y }` | Monitor top-left in the virtual desktop |
| `workArea` | `{ x, y, width, height }` | Usable area (excludes menu bar, dock, etc.) |
| `scaleFactor` | `number` | Device pixel ratio |
| `isPrimary` | `boolean` | OS primary display |
| `isCurrent` | `boolean` | Display containing the main window |

```javascript
const { screens, primaryIndex, currentIndex } = await shell.getScreens();

for (const screen of screens) {
  console.log(screen.name, screen.workArea.width, screen.workArea.height);
}

const primary = screens[primaryIndex];
```

## `shell.getScreenAt(x, y)`

Returns the display that contains a point in physical screen coordinates.

- `x` — horizontal position
- `y` — vertical position
- Returns: `Promise<screen>` — same shape as one `screens` entry from `getScreens()`

```javascript
const screen = await shell.getScreenAt(1200, 400);
```

## `shell.openWindow(url, options?)`

Opens a new child webview window — for flows the main window can't run itself, e.g. an external OAuth/login page you need to observe and drive.

- `url` — `http://` or `https://` URL only, same rule as `fetch`
- `options` — optional object:
  - `title` — window title (default: platform default, untitled)
  - `width` — logical pixel width (default `480`)
  - `height` — logical pixel height (default `640`)
- Returns: `Promise<{ id: string }>` — `id` is an internal window label, not a DOM handle

```javascript
const { id } = await shell.openWindow("https://accounts.example.com/oauth/authorize?...", {
  title: "Sign in",
  width: 480,
  height: 640,
});
```

## `shell.closeWindow(id)`

Closes a window previously opened with `openWindow`.

- `id` — the id returned by `openWindow`; `"main"` is rejected
- Returns: `Promise<void>`

```javascript
await shell.closeWindow(id);
```

## `shell.onWindowNavigated((id, url) => void)`

Subscribes to navigation events from every child window opened via `openWindow` (including redirects). Fires for all child windows — filter by `id` yourself.

- Returns: `Promise<UnlistenFn>` — call the resolved function to stop listening

```javascript
const unlisten = await shell.onWindowNavigated((windowId, url) => {
  if (windowId !== id) return;
  if (url.startsWith("https://yourapp.example.com/callback")) {
    const code = new URL(url).searchParams.get("code");
    shell.closeWindow(id);
    unlisten();
  }
});
```

## `shell.onWindowLoaded((id, url) => void)`

Subscribes to page-load-finished events from every child window opened via `openWindow`. Unlike `onWindowNavigated` (which fires as navigation starts/redirects), this fires once the page has actually finished loading — useful when you need the DOM settled before calling `getWindowBody`/`evalWindow`.

- Returns: `Promise<UnlistenFn>` — call the resolved function to stop listening

```javascript
const unlisten = await shell.onWindowLoaded((windowId, url) => {
  if (windowId !== id) return;
  console.log("child window finished loading", url);
});
```

## `shell.getWindowBody(id)`

Returns the `innerText` of `document.body` in a child window opened via `openWindow`. Useful for reading what an external page (e.g. a login flow) is currently showing.

- `id` — the id returned by `openWindow`
- Returns: `Promise<string>` — empty string if the window has no body yet

```javascript
const text = await shell.getWindowBody(id);
```

## `shell.evalWindow(id, code)`

Runs `code` as a function body inside a child window and returns its result. `code` may use `return` and `await` — it always runs as if inside an `async` function, so a returned Promise is resolved before the result comes back to your JS.

- `id` — the id returned by `openWindow`
- `code` — JS source, executed as the body of an `async` function in the child window
- Returns: `Promise<any>` — rejects with the thrown error's message if `code` throws

```javascript
const title = await shell.evalWindow(id, "return document.title;");

const status = await shell.evalWindow(
  id,
  "const res = await fetch('/api/status'); return res.status;",
);
```

## `shell.onWindowClosed((id) => void)`

Subscribes to child windows closing, whether via `closeWindow` or the user closing the window manually. Useful for cleaning up if the user abandons a flow (e.g. closes an OAuth popup without completing it).

- Returns: `Promise<UnlistenFn>` — call the resolved function to stop listening

```javascript
const unlisten = await shell.onWindowClosed((windowId) => {
  if (windowId === id) unlisten();
});
```

## `shell.authViaBrowser(authUrl, options?)`

Runs a browser-based sign-in flow (e.g. SAML/SSO) in the user's **system** default browser instead of a child webview, then waits for the backend to redirect back to a one-time local HTTP callback and returns the resulting auth code. Use this instead of `openWindow` + `onWindowNavigated` when the identity provider blocks embedded webviews (common for SSO/SAML providers).

- `authUrl` — the `http(s)://` URL that starts the sign-in flow. It gets `returnUrl=<encoded callback>` appended as a query parameter (joined with `&` if `authUrl` already has a `?`) — your backend must redirect the browser back to that exact URL, with an `authCode=<value>` query parameter, once sign-in completes.
- `options` — optional. Either a plain number (treated as `timeoutMs`), or an object:
  - `timeoutMs` — how long to wait for the callback before giving up. Default `120000` (2 minutes).
  - `returnUrl` — override the callback URL instead of the auto-generated one. Must be `http://` on `localhost`/`127.0.0.1`/`::1`, with an explicit port. Use this only if your identity provider requires a fixed, pre-registered redirect URL — leave it unset otherwise, which lets the shell pick any free local port.
- Returns: `Promise<string>` — the `authCode` value read off the callback request's query string. Exchange it for a real token yourself (e.g. via `shell.post` to your backend) — this call does not do the token exchange.

```javascript
const authCode = await shell.authViaBrowser(
  "https://idp.example.com/saml/login?service=myapp",
);

const res = await shell.post(
  "https://api.example.com/auth/exchange",
  JSON.stringify({ authCode }),
  { "Content-Type": "application/json" },
);
```

With a fixed, pre-registered redirect URL and a longer timeout:

```javascript
const authCode = await shell.authViaBrowser("https://idp.example.com/saml/login", {
  timeoutMs: 300_000,
  returnUrl: "http://127.0.0.1:41417/callback",
});
```

Behavior notes:

- Opens the URL with the OS's default browser opener (same mechanism as `openFile`), not a Tauri window — there is no `id` to track, close, or `evalWindow` into.
- A single shared background TCP listener handles all auth callbacks, so multiple `authViaBrowser` calls can run concurrently without port conflicts. Each flow is identified by a unique `sid` embedded in the return URL.
- The callback listener accepts exactly one real request per flow (it ignores stray/empty requests without an `error` or `authCode` param, such as browser favicon fetches) and shows the caller a static "you can close this tab" HTML page — it does not redirect anywhere.
- If the backend redirects with `?error=...` instead of `?authCode=...`, the promise rejects with `"authentication error: <value>"`.
- If nothing hits the callback before `timeoutMs` elapses, the promise rejects with `"authentication timed out waiting for browser callback"`.
- `returnUrl`, when provided, should be loopback-only (`localhost`/`127.0.0.1`/`::1`). A unique `sid` is automatically appended so multiple concurrent flows with the same `returnUrl` are safely disambiguated.

## `shell.secretSet(service, account, password)`

Stores a secret in the OS system keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager) via keyring-rs.

- `service` — logical service name, e.g. `"myapp"`
- `account` — account/user identifier, e.g. `"api-key"` or `"user@example.com"`
- `password` — the secret value to store
- Returns: `Promise<void>`

```javascript
await shell.secretSet("myapp", "api-key", "sk-abc123...");
```

## `shell.secretGet(service, account)`

Retrieves a secret from the OS system keychain.

- `service` — logical service name
- `account` — account/user identifier
- Returns: `Promise<string>` — the stored password; rejects if the entry does not exist

```javascript
const key = await shell.secretGet("myapp", "api-key");
```

## `shell.secretDelete(service, account)`

Deletes a secret from the OS system keychain.

- `service` — logical service name
- `account` — account/user identifier
- Returns: `Promise<void>` — rejects if the entry does not exist

```javascript
await shell.secretDelete("myapp", "api-key");
```

## `shell.httpStart(options?)`

Starts a local HTTP server on `127.0.0.1`. Incoming requests are forwarded to your JS code via the `shell://http-request` event.

- `options` — optional object:
  - `port` — port to bind (default `0` = any available port)
- Returns: `Promise<{ port: number }>` — the actual bound port

You must listen for `shell://http-request` events before or shortly after starting the server, and respond via `shell.httpRespond()` for each request. The server thread blocks until a response is received.

```javascript
const { port } = await shell.httpStart({ port: 0 });
console.log("HTTP server on port", port);

await shell.onHttpRequest(async (req) => {
  console.log(req.method, req.url, req.headers);
  await shell.httpRespond(req.id, 200, { "Content-Type": "text/plain" }, "Hello");
});
```

## `shell.httpRespond(id, status, headers?, body?)`

Sends an HTTP response for a previously received request.

- `id` — the request id from the `shell://http-request` event payload
- `status` — HTTP status code (e.g. `200`, `404`)
- `headers` — optional object of response header names to values
- `body` — optional response body string
- Returns: `Promise<void>` — rejects if the request id is unknown or already responded

```javascript
await shell.httpRespond(req.id, 200, { "Content-Type": "application/json" }, JSON.stringify({ ok: true }));
```

## `shell.httpStop()`

Stops the HTTP server. Pending requests will error on respond.

- Returns: `Promise<void>`

```javascript
await shell.httpStop();
```

## `shell.onHttpRequest(callback)`

Subscribes to incoming HTTP requests.

- `callback` — function receiving `{ id, method, url, headers, body }`
- Returns: `Promise<UnlistenFn>` — call the resolved function to stop listening

```javascript
const unlisten = await shell.onHttpRequest((req) => {
  shell.httpRespond(req.id, 200, {}, "OK");
  unlisten(); // handle only one request
});
```

## `shell.wsStart(options?)`

Starts a local WebSocket server on `127.0.0.1`. New connections, incoming messages, and disconnections are forwarded to your JS code via events.

- `options` — optional object:
  - `port` — port to bind (default `0` = any available port)
- Returns: `Promise<{ port: number }>` — the actual bound port

```javascript
const { port } = await shell.wsStart({ port: 0 });
console.log("WS server on port", port);
```

## `shell.wsSend(id, data)`

Sends a text message to a connected WebSocket client.

- `id` — the connection id from the `shell://ws-connection` event
- `data` — text string to send
- Returns: `Promise<void>` — rejects if the connection is not found or closed

```javascript
await shell.wsSend(connId, "Reply");
```

## `shell.wsClose(id)`

Closes a WebSocket connection gracefully.

- `id` — the connection id from the `shell://ws-connection` event
- Returns: `Promise<void>`

```javascript
await shell.wsClose(connId);
```

## `shell.wsStop()`

Stops the WebSocket server. All active connections are closed.

- Returns: `Promise<void>`

```javascript
await shell.wsStop();
```

## `shell.onWsConnection(callback)`

Subscribes to new WebSocket client connections.

- `callback` — function receiving `{ id }` each time a client connects
- Returns: `Promise<UnlistenFn>`

```javascript
const unlisten = await shell.onWsConnection(({ id }) => {
  console.log("client connected:", id);
});
```

## `shell.onWsMessage(callback)`

Subscribes to WebSocket text messages from any connected client.

- `callback` — function receiving `{ id, data }` where `id` is the connection id and `data` is the text payload
- Returns: `Promise<UnlistenFn>`

```javascript
await shell.onWsMessage(({ id, data }) => {
  console.log(`message from ${id}:`, data);
});
```

## `shell.onWsClose(callback)`

Subscribes to WebSocket client disconnections.

- `callback` — function receiving `{ id }`
- Returns: `Promise<UnlistenFn>`

```javascript
await shell.onWsClose(({ id }) => {
  console.log("client disconnected:", id);
});
```

## Running programs (`[[allowedCommands]]`)

`shell.run` and `shell.spawn` execute local programs. There is no general "run this command line" API — a program can only be started if the app author listed it in `app.toml` under `[[allowedCommands]]`, and JS refers to it by the `name` given there.

```toml
[[allowedCommands]]
name = "git"                                  # required. Alias JS passes to shell.run/spawn. Unique.
program = "git"                               # required. Executable: bare name (resolved via PATH) or absolute path.
args = ["^(status|log|diff)$", "^--oneline$"] # optional. Positional regex allowlist, one pattern per arg index.
extraArgs = "^[\\w./-]+$"                     # optional. Pattern for every arg beyond the ones in `args`.
maxArgs = 8                                   # optional. Hard cap on argument count.
cwd = "repo"                                  # optional. Working directory, relative to the app.toml directory (absolute paths allowed). Default: the app.toml directory.
timeoutMs = 30000                             # optional. Default timeout when the JS caller does not pass one. Absent = no timeout.
env = { GIT_PAGER = "cat" }                   # optional. Extra env vars, merged over the inherited environment.
```

Repeat the `[[allowedCommands]]` block once per program. Omitting the table entirely means no process execution is possible at all — every `run`/`spawn` call rejects.

### Security model

- **The allowlist is the entire policy.** Only a listed `program` can run, and only under its own `name`. Nothing else on the machine is reachable through this API.
- **Nothing goes through a shell.** `app-ly` always spawns the executable directly (`Command::new(program).args(...)`), never `sh -c` / `cmd /c`. There is no shell-injection surface, and equally no globbing, pipes, redirection, `&&`, or quoting rules — every argument reaches the program verbatim. Chain steps in JS instead of in a command line.
- **`program`, `cwd`, and `env` come from `app.toml` only.** JS can never supply them. This is a deliberate limitation: a page can pick a listed command and pass arguments, and that is all. Per call, JS supplies exactly the entry `name`, an argument array, and `timeoutMs` / `stdin`.
- **Patterns are implicitly fully anchored.** Each configured pattern `P` is compiled as `^(?:P)$` ([Rust `regex` syntax](https://docs.rs/regex/latest/regex/#syntax)). So `"status"` matches only `status` — it does **not** match `xstatusy`. Writing `"^status$"` yourself is fine too; the anchors nest harmlessly.
- **Omitting both `args` and `extraArgs` leaves arguments unrestricted** for that program — the allowlist then constrains only *which* executable runs, not what it is asked to do. This is the loosest possible setting: an unrestricted `git` entry permits `git push --force`. Regex limiting is opt-in, so add at least `extraArgs` (and ideally `maxArgs`) to anything that isn't trivially harmless.
- `env` values are never exposed to JS — `shell.listCommands()` reports only `name`, `program`, `argsRestricted`, and `timeoutMs`.

### Argument matching rules

- Neither `args` nor `extraArgs` present → any arguments are accepted (still subject to `maxArgs`).
- `args` present → the argument at index `i` must match `args[i]`.
- Arguments at index ≥ `args.length` are **rejected**, unless `extraArgs` is set — then each of them must match `extraArgs`.
- `extraArgs` present without `args` → every argument must match `extraArgs`.
- Passing **fewer** arguments than there are patterns is allowed; trailing positional patterns are optional.
- More arguments than `maxArgs` → rejected.
- An invalid regex in the config does not crash startup: `run`/`spawn` for that one entry reject with an error naming the entry and the bad pattern.

Rejection messages are actionable, e.g. `command "git": argument 1 ("push") does not match allowed pattern ^(status|log|diff)$`.

### What rejects vs. what resolves

The promise rejects only on **policy or spawn** failures. A program that runs and fails is a normal result, not an error.

| Situation | Outcome |
|-----------|---------|
| No `[[allowedCommands]]` entry with that `name` | **Rejects** — `no allowed command named "curl" — add an [[allowedCommands]] entry to app.toml` |
| An argument fails its pattern, or `maxArgs` is exceeded | **Rejects** |
| Invalid regex in that entry's config | **Rejects** |
| Executable not found / cannot be spawned | **Rejects** |
| Program ran and exited non-zero | **Resolves** — check `code` yourself |
| Timeout elapsed and the child was killed | **Resolves** with `timedOut: true` and the output collected so far |
| `write` / `closeStdin` / `exit` / `kill` / `setTimeout` on a process that already exited | **Rejects** — `process proc-3 not found or already exited`. Guard with `exited` if the process may have finished |

### Timeouts

The effective timeout is the per-call `options.timeoutMs` if given, otherwise the entry's `timeoutMs`, otherwise none (the child may run forever). On timeout the child is killed and the call **resolves** with `timedOut: true` — it never rejects — carrying whatever output had been collected. `code` is `null` whenever the process was killed or signalled rather than exiting on its own. `signal` is Unix-only and is always `null` on Windows.

For a spawned process the timeout is not fixed at spawn time — `proc.setTimeout(ms)` re-arms it from the moment you call it, and `proc.setTimeout(null)` removes it. See [`shell.spawn`](#shellspawnname-args-options).

## `shell.run(name, args?, options?)`

Runs an allowlisted program to completion and returns everything it produced. The wait happens off the UI thread, so the webview stays responsive.

- `name` — the `name` of an `[[allowedCommands]]` entry
- `args` — optional array of string arguments (default `[]`), validated against that entry
- `options` — optional object:
  - `timeoutMs` — kill the child after this many milliseconds; overrides the entry's `timeoutMs`
  - `stdin` — string written to the child's stdin, which is then closed
- Returns: `Promise<{ stdout, stderr, code, signal, timedOut }>`

| Field | Type | Description |
|-------|------|-------------|
| `stdout` | `string` | Everything written to stdout |
| `stderr` | `string` | Everything written to stderr |
| `code` | `number \| null` | Exit status; `null` if the process was killed or signalled |
| `signal` | `number \| null` | Terminating signal (Unix only; always `null` on Windows) |
| `timedOut` | `boolean` | `true` if the child was killed because the timeout elapsed |

When the second argument is a non-array object it is treated as `options`, so `shell.run("echo", { timeoutMs: 1000 })` runs the command with no arguments.

```javascript
const { stdout, stderr, code, timedOut } = await shell.run("git", ["status"], {
  timeoutMs: 5000,
});

if (timedOut) {
  console.warn("git status took too long");
} else if (code !== 0) {
  console.error("git failed:", code, stderr);
} else {
  console.log(stdout);
}
```

Feeding the child stdin:

```javascript
const { stdout } = await shell.run("wc", ["-l"], { stdin: "a\nb\nc\n" });
```

## `shell.spawn(name, args?, options?)`

Starts an allowlisted program and streams its output as it arrives, instead of waiting for it to finish. Use it for long-running or chatty commands, or when you need to write to stdin while the process runs.

- `name` — the `name` of an `[[allowedCommands]]` entry
- `args` — optional array of string arguments (default `[]`), validated exactly as in `shell.run`
- `options` — optional object:
  - `timeoutMs` — kill the child after this many milliseconds; overrides the entry's `timeoutMs`
- Returns: `Promise<ChildProcess>` — resolves once the process has been spawned

The same 2-argument overload applies: `shell.spawn("ping", { timeoutMs: 10000 })`.

The resolved `ChildProcess` object:

| Member | Type | Description |
|--------|------|-------------|
| `id` | `string` | Shell-assigned process id (e.g. `"proc-0"`), used by the underlying events |
| `pid` | `number \| null` | OS process id, when the platform reports one |
| `onStdout(cb)` | `(data: string) => void` → `unsubscribe` | Called with each stdout chunk as it arrives |
| `onStderr(cb)` | `(data: string) => void` → `unsubscribe` | Called with each stderr chunk as it arrives |
| `onExit(cb)` | `({ code, signal, timedOut }) => void` → `unsubscribe` | Called once, when the process ends |
| `write(data)` | `(string) => Promise<void>` | Writes to the child's stdin |
| `closeStdin()` | `() => Promise<void>` | Closes stdin, so the child sees EOF |
| `kill()` | `() => Promise<void>` | Forcefully kills the child (`SIGKILL` on Unix) — it cannot clean up |
| `exit()` | `() => Promise<{ graceful }>` | Asks the child to exit (`SIGTERM` on Unix) so it can run its own shutdown |
| `setTimeout(ms)` | `(number \| null) => Promise<void>` | Sets the timeout **while the process runs**, counted from now; `null` clears it |
| `exited` | `Promise<{ code, signal, timedOut }>` | Resolves once the process ends; never rejects |
| `[Symbol.asyncIterator]` | yields `{ stream, data }` | `for await` over all output; `stream` is `"stdout"` or `"stderr"` |

Behavior worth relying on:

- Each `onStdout` / `onStderr` / `onExit` returns an **unsubscribe function** — call it to stop receiving that stream.
- **No output is lost.** Chunks that arrive before you attach a handler are queued and flushed to the first handler registered for that stream, and an `onExit` handler attached after the process already ended still fires.
- `exited` resolves the same way whether you await it before or after the process ends, and resolves (never rejects) even on kill or timeout — `code` is then `null` and `timedOut` tells you which it was.
- Async iteration yields chunks in arrival order and ends when the process exits; chunks produced while you are awaiting the next value are queued, and starting iteration after some output has arrived still replays it.
- `exit()` is the polite stop and `kill()` is the forceful one. `exit()` sends `SIGTERM`, which the child may trap, delay, or ignore entirely — so it resolves when the *signal was sent*, not when the process is gone. Await `exited` to know it actually stopped. It resolves `{ graceful: true }` on Unix; on Windows there is no `SIGTERM`, so it falls back to a forceful kill and reports `{ graceful: false }`.
- `setTimeout(ms)` re-arms the deadline from the moment you call it, so it both extends and shortens: it works on a process spawned with no timeout at all, and calling it repeatedly (say, on each chunk of output) gives you an **inactivity timeout**. `setTimeout(null)` removes the deadline. A timeout that fires kills the process the same way a spawn-time timeout does — `exited` resolves with `timedOut: true`.
- Under the hood `app-ly` listens on the `shell://process-stdout`, `shell://process-stderr`, and `shell://process-exit` events and dispatches them by `id` — `shell.spawn` subscribes for you, so there is no `onProcess*` API to call yourself.

Streaming output into the DOM:

```javascript
const proc = await shell.spawn("git", ["log", "--oneline"], { timeoutMs: 10000 });
const out = document.getElementById("out");

const stopOut = proc.onStdout((data) => {
  out.textContent += data;
});
proc.onStderr((data) => console.warn("stderr:", data));

const { code, signal, timedOut } = await proc.exited;
stopOut();
out.textContent += `\n[exit code=${code} signal=${signal} timedOut=${timedOut}]`;
```

The same thing as an async iteration, with a cancel button:

```javascript
const proc = await shell.spawn("ping", ["-c", "5", "127.0.0.1"]);

cancelButton.onclick = () => proc.kill();

for await (const { stream, data } of proc) {
  out.textContent += stream === "stderr" ? `! ${data}` : data;
}

const { code, timedOut } = await proc.exited;
```

Driving a process through stdin:

```javascript
const proc = await shell.spawn("sort");

proc.onStdout((data) => (out.textContent += data));

await proc.write("banana\n");
await proc.write("apple\n");
await proc.closeStdin(); // sort only produces output once its input ends

await proc.exited;
```

Stopping a process politely, with a deadline as the fallback:

```javascript
const proc = await shell.spawn("server", ["--watch"]);

stopButton.onclick = async () => {
  await proc.exit();          // ask it to shut down cleanly
  await proc.setTimeout(5000); // but don't wait forever
};

const { code, timedOut } = await proc.exited;
if (timedOut) console.warn("it ignored SIGTERM and was killed");
```

An inactivity timeout — re-arm the deadline on every chunk, so the process is
killed only after it has been quiet for 10 seconds:

```javascript
const proc = await shell.spawn("tailer", ["app.log"]);
await proc.setTimeout(10000);
proc.onStdout(async (data) => {
  out.textContent += data;
  await proc.setTimeout(10000); // reset the clock
});
```

## `shell.listCommands()`

Lists the `[[allowedCommands]]` entries the running app was configured with — for building a UI (a picker, a diagnostics panel) or for checking whether a command is available before offering it.

- Returns: `Promise<Array<{ name, program, argsRestricted, timeoutMs }>>`

| Field | Type | Description |
|-------|------|-------------|
| `name` | `string` | The alias to pass to `run` / `spawn` |
| `program` | `string` | The configured executable |
| `argsRestricted` | `boolean` | `true` if the entry sets any of `args`, `extraArgs`, or `maxArgs` |
| `timeoutMs` | `number \| null` | The entry's default timeout, if it has one |

`cwd` and `env` are never reported.

```javascript
const commands = await shell.listCommands();
// [{ name: "git", program: "git", argsRestricted: true, timeoutMs: 30000 }]

if (!commands.some((c) => c.name === "git")) {
  show("This app was configured without git access.");
}
```

## On-device AI (`[ai]`)

`shell.ai` runs a language model **on the device** — no API key, no network request, no endpoint. Prompts and completions never leave the machine through this API.

It is not always available: it needs macOS 26 or newer on Apple silicon with Apple Intelligence turned on, and a shell built with its AI backend. Everywhere else the API still answers, reporting itself unavailable. Full reference, including the complete JSON Schema subset and the architecture: [`ai.md`](ai.md).

An optional `[ai]` table in `app.toml` sets defaults for every request:

```toml
[ai]
enabled = true                                      # optional. false => reason "disabled-by-config"
instructions = "Answer briefly and in plain text."  # optional. Default system prompt
temperature = 0.7                                   # optional. Default sampling temperature
maxTokens = 512                                     # optional. Default response length cap
toolTimeoutMs = 30000                               # optional, default 30000. How long the shell waits
                                                    # for a JS tool handler before answering the model
                                                    # with an error. Config-only, no per-request override.
```

Per-request `options` override these field by field.

### Availability

`shell.ai.info()`, `available()` and `models()` **never reject**. `generate()`, `generateObject()` and `stream()` **do** reject when the model is unusable, with `ai unavailable: <reason> — <detail>` (the detail half is omitted when there is none). Check first:

```javascript
const info = await shell.ai.info();
if (!info.available) {
  status.textContent = `AI unavailable: ${info.reason}`;
} else {
  const { text } = await shell.ai.generate("Say hello in five words.");
  show(text);
}
```

`reason` is one of a closed set:

| Code | Meaning |
|------|---------|
| `unsupported-platform` | Not macOS, or a build without the AI backend |
| `unsupported-os` | The OS is too old to have an on-device model API |
| `disabled-by-config` | `[ai] enabled = false` in `app.toml` |
| `device-not-eligible` | The hardware/region does not support on-device AI |
| `not-enabled` | The user has not turned the OS AI feature on |
| `model-not-ready` | The model is still downloading or preparing |
| `unavailable` | Any other state the OS reports |

### Request options

Accepted by `generate`, `generateObject`, and `stream` alike. All optional; each overrides the matching `[ai]` default.

| Option | Type | Description |
|--------|------|-------------|
| `model` | `string` | Must be `"default"` (or omitted) — this shell exposes one model |
| `instructions` | `string` | System prompt for this request |
| `temperature` | `number` | Sampling temperature |
| `maxTokens` | `number` | Response length cap |
| `tools` | `array` | `{ name, description, parameters, handler }` entries — see [`shell.ai.generate`](#shellaigenerateprompt-options) |

## `shell.ai.info()`

Reports whether the model can be used right now, and what it supports. Never rejects.

- Returns: `Promise<{ available, reason, detail, models, features }>`

| Field | Type | Description |
|-------|------|-------------|
| `available` | `boolean` | `true` when generation may proceed |
| `reason` | `string \| null` | `null` when available; otherwise a code from the table above |
| `detail` | `string \| null` | Human-readable explanation; may be `null` |
| `models` | `array` | `[]` when unavailable; otherwise `[{ id, name, default }]` |
| `features` | `object` | `{ text, structured, tools, streaming }` — all `false` when unavailable |

## `shell.ai.available()`

Shorthand for `info().available`. Never rejects.

- Returns: `Promise<boolean>`

## `shell.ai.models()`

The models this shell exposes — exactly one, `{ id: "default", name: "On-device model", default: true }`, or `[]` when unavailable. Never rejects.

- Returns: `Promise<Array<{ id, name, default }>>`

## `shell.ai.generate(prompt, options?)`

One-shot text generation. Runs off the UI thread, so the webview stays responsive.

- `prompt` — the text to answer
- `options` — optional, see [Request options](#request-options)
- Returns: `Promise<{ text, model, toolCalls }>`

| Field | Type | Description |
|-------|------|-------------|
| `text` | `string` | The generated text |
| `model` | `string` | The model id actually used (always `"default"`) |
| `toolCalls` | `array` | Every tool call attempted: `{ name, arguments, result, error }`. `[]` if none |

```javascript
const { text, model } = await shell.ai.generate("Write a haiku about desktop apps.", {
  instructions: "You are concise.",
  maxTokens: 200,
});
show(`${text}\n\n-- model: ${model}`);
```

Tools let the model call back into your JavaScript mid-generation. The `handler` never leaves the page — only `{ name, description, parameters }` is sent:

```javascript
const result = await shell.ai.generate("What time is it in Tokyo?", {
  tools: [
    {
      name: "get_time",
      description: "Return the current time in an IANA time zone.",
      parameters: {
        type: "object",
        properties: { zone: { type: "string", description: "e.g. Asia/Tokyo" } },
        required: ["zone"],
      },
      handler: ({ zone }) =>
        new Date().toLocaleTimeString("en-GB", { timeZone: zone }),
    },
  ],
});
```

Every attempted call lands in `toolCalls` with exactly one of `result` / `error` set. A handler that throws, a tool name the request never registered, and a handler that never returns (after `toolTimeoutMs`) are all reported *to the model* as errors — none of them aborts generation.

## `shell.ai.generateObject(prompt, schema, options?)`

Structured output. The model is grammatically constrained to the schema as it decodes, so the result parses — this is real constrained decoding, not "please reply in JSON" prompting.

- `prompt` — what to produce
- `schema` — a JSON Schema object
- `options` — optional, see [Request options](#request-options)
- Returns: `Promise<{ object, model, toolCalls }>` — `object` is already-parsed JSON

```javascript
const { object } = await shell.ai.generateObject("Describe the app-ly desktop shell.", {
  type: "object",
  properties: {
    title: { type: "string", description: "Short title" },
    tags: { type: "array", items: { type: "string" }, maxItems: 5 },
    rating: { type: "integer", minimum: 1, maximum: 5 },
  },
  required: ["title", "tags", "rating"],
});
```

Only a subset of JSON Schema is supported: `type`, `description`, `properties`, `required`, `items`, `minItems`/`maxItems`, string `enum`, `anyOf`/`oneOf`, `pattern`, string `const`, and `minimum`/`maximum`. There is no `$ref` support, and **an absent `required` means every property is required** — the opposite of standard JSON Schema. Unsupported keywords are ignored; malformed ones reject before generation starts. The full table of what is supported, ignored, and rejected is in [`ai.md`](ai.md#structured-output).

## `shell.ai.stream(prompt, options?)`

Streaming text generation. Resolves once the listeners are live and the backend has accepted the request, so no delta can be missed by subscribing late.

- `prompt`, `options` — as `generate`
- Returns: `Promise<AiStream>`

| Member | Type | Description |
|--------|------|-------------|
| `id` | `string` | Request id, echoed on every event for this request |
| `onText(cb)` | `(text: string) => void` → `unsubscribe` | Each text delta; the first handler drains anything buffered before it |
| `completed` | `Promise<{ text, model, toolCalls }>` | Resolves when generation finishes; rejects on error or cancellation |
| `cancel()` | `() => Promise<void>` | Best-effort stop; resolves even if already finished |
| `[Symbol.asyncIterator]` | yields `string` | `for await` over the deltas |

```javascript
const stream = await shell.ai.stream("Write a short poem about the sea.");

let text = "";
const stop = stream.onText((delta) => {
  text += delta;
  out.textContent = text;
});

try {
  await stream.completed;
} catch (error) {
  console.warn("stream failed:", error.message); // "cancelled" if you cancelled it
} finally {
  stop();
}
```

Behavior worth relying on:

- **No delta is lost.** Deltas arriving before a consumer exists are buffered and replayed to the first `onText` handler or the first iterator. The backlog goes to whoever claims it first — not to every consumer.
- Every chunk for a request is delivered before its completion, so `completed` never resolves ahead of text you have not seen.
- **`cancel()` is best-effort and does not stop the model.** The inference runs to completion in the background and its output is discarded; the shell stops forwarding deltas and `completed` rejects with `cancelled`.
- `cancel()` exists on the stream handle only — `generate` and `generateObject` cannot be cancelled.
- On error or cancellation the completion carries no text. If you need partial output, accumulate it from `onText` yourself.
- Under the hood the shell listens on `shell://ai-chunk`, `shell://ai-done`, and `shell://ai-tool-call` and dispatches by request id — `shell.ai` subscribes for you, so there is no `onAi*` API to call.

## `shell.dbQuery(dbName, query, params?)`

Runs a read query against a SQLite database stored in `dataPath`. The database file is created on first use if it does not exist.

- `dbName` — simple database filename only (e.g. `"app.db"`)
- `query` — SQL string with `?` placeholders
- `params` — optional array of parameter values (`null`, boolean, number, string)
- Returns: `Promise<{ columns: string[], rows: any[][] }>`

`rows` is an array of arrays aligned with `columns`. This shape is compact and maps directly from SQLite.

```javascript
const result = await shell.dbQuery(
  "app.db",
  "SELECT id, title FROM notes WHERE id = ?",
  [1],
);

const [idIndex, titleIndex] = [
  result.columns.indexOf("id"),
  result.columns.indexOf("title"),
];

for (const row of result.rows) {
  console.log(row[idIndex], row[titleIndex]);
}
```

## `shell.dbExecute(dbName, query, params?)`

Runs a write/query that returns a single result — `INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE`, counts, etc.

- `dbName` — simple database filename only
- `query` — SQL string with `?` placeholders
- `params` — optional array of parameter values
- Returns: `Promise<{ changes: number, lastInsertRowid: number }>`

```javascript
await shell.dbExecute(
  "app.db",
  "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, title TEXT)",
);

await shell.dbExecute("app.db", "INSERT INTO notes (title) VALUES (?)", ["First note"]);

const write = await shell.dbExecute("app.db", "UPDATE notes SET title = ? WHERE id = ?", [
  "Updated",
  1,
]);

console.log(write.changes, write.lastInsertRowid);
```

## Errors

All methods return promises that reject with a string error message on failure.

Common cases:

- Invalid filename (contains `/`, `\\`, or `..`)
- Missing file on `readFile`, `deleteFile`, `renameFile`, `openFile`, `openFileLocation`
- Unsupported URL scheme in `fetch` or `openWindow` (only `http`/`https` allowed)
- Network failure in `fetch`
- Invalid database name or SQL error in `dbQuery` / `dbExecute`
- Window not available yet when calling window APIs during very early page load
- Unknown window `id`, or `id: "main"`, passed to `closeWindow`
- The platform opener binary is missing (e.g. `xdg-open` not installed) in `openFile`/`openFileLocation` or `authViaBrowser`
- Invalid/non-loopback `returnUrl`, backend redirect with `?error=...`, or timeout in `authViaBrowser`
- Unknown command name, an argument rejected by an `[[allowedCommands]]` pattern, an invalid regex in that entry, or a missing executable in `run`/`spawn` — note that a non-zero exit status is *not* an error, it resolves with `code` set
- Unknown or already-exited process id passed to a `ChildProcess`'s `write`/`closeStdin`/`kill`
- `ai unavailable: <reason> — <detail>` from `shell.ai.generate`/`generateObject`/`stream` when the on-device model cannot be used — check `shell.ai.info()` first (it never rejects)
- An unsupported or malformed schema passed to `shell.ai.generateObject`, or `options.model` set to anything other than `"default"`
- `cancelled` as the rejection of a cancelled stream's `completed` — note that a failing tool handler is *not* an error, it is reported to the model and recorded in `toolCalls`

## Full example

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

## Limitations (v1)

- File and database APIs accept simple filenames only, not nested paths
- `fetch` returns text bodies only (no streaming or binary)
- SQLite parameter values support `null`, boolean, number, and string only
- SQLite blob columns are returned as `null` in `dbQuery`
- `shell.settings` values are strings only; no nested objects, numbers, or booleans
- `.env` parsing has no multi-line values, `\n` escapes, or variable interpolation
- Child windows (`openWindow`) are plain webviews with no `window.shell` injected into them — they're for external content only, not a place to run more of your app's JS. Use `getWindowBody`/`evalWindow` from the main window to read or drive them instead
- `evalWindow` result values must be JSON-serializable (like `dbQuery`/`fetch` payloads) — functions, DOM nodes, etc. come back as `null`
- `openFile`/`openFileLocation` resolve once the OS has been asked to open the item, not once it's actually open — a missing default app or file manager failure won't surface as a rejected promise
- `openFileLocation` "selects" the file on macOS/Windows; on Linux it can only open the enclosing folder, not select the file within it
- `authViaBrowser` runs the flow in the system browser, not a `shell`-controlled webview — there's no `evalWindow`/`getWindowBody` equivalent for it; the calling backend must redirect to the local callback with `authCode`/`error` in the query string itself
- `secretGet` rejects if the keyring entry doesn't exist — there's no `secretExists` helper; catch the error or try `secretGet` directly
- The HTTP server reads the full request body into a string before emitting the event (no streaming support)
- The WebSocket server only supports text messages — binary frames are silently ignored
- Only one HTTP server and one WebSocket server can run at a time; calling `httpStart`/`wsStart` while one is already running returns an error
- `run`/`spawn` can only start programs listed in `[[allowedCommands]]`; `program`, `cwd`, and `env` come from `app.toml` only and can never be supplied from JS
- No shell interpreter is involved in `run`/`spawn` — no pipes, globs, redirection, or `&&`; compose steps in JS instead
- Child process output is decoded as UTF-8 (lossily) into strings — binary stdout/stderr is not supported
- `proc.exit()` sends `SIGTERM` on Unix, which a child may trap, delay, or ignore — it reports that the signal was sent, not that the process stopped. Windows has no `SIGTERM`, so it falls back to a forceful kill and resolves `{ graceful: false }`
- Timeouts are checked on a short poll rather than a precise timer, so a deadline can fire up to ~20ms late
- `signal` on a process result is Unix-only and always `null` on Windows
- `shell.ai` needs macOS 26 or newer on Apple silicon with Apple Intelligence enabled, and a shell built with its AI backend; every other build reports `unsupported-platform` and rejects generation
- `shell.ai` has no chat history — each call is a fresh session, so multi-turn behavior means putting earlier turns in the prompt yourself
- One model only (`"default"`); no images, embeddings, token counts, or finish reasons
- `generateObject` supports a subset of JSON Schema with no `$ref`, and treats an absent `required` as "every property is required"
- `generateObject` does not stream, and `stream` takes no schema — structured output and streaming are mutually exclusive
- Cancelling a stream stops delivery but not the inference itself, and `cancel()` exists on the stream handle only — `generate`/`generateObject` cannot be cancelled
- `toolTimeoutMs` is set in `app.toml` only; there is no per-request or per-tool override, and no overall deadline on a generation