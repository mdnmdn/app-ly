# Building an app-ly app

## What `app-ly` is for

A prebuilt desktop shell. You supply an `app.toml`, a folder of static HTML/JS/CSS, and an icon. Launching the binary *is* the app.

`window.shell` is injected before page scripts run and covers the things a plain web page cannot do: files, SQLite, CORS-free HTTP, notifications, windows, the OS keychain, local servers, allowlisted programs, file drop and native clipboard, and an on-device model. No npm, bundler, framework, or compile step.

## Minimum viable app

```
myapp/
├── app-ly.app        # (or the platform executable)
├── app.toml
├── icon.png
└── contents/
    └── index.html
```

```toml
icon = "icon.png"
name = "My App"
contents = "contents"
dataPath = "data"
```

Launch `app-ly.app` sitting in `myapp/` — it finds `app.toml` next to it.

```javascript
await shell.log("app started");
```

## `app.toml`

Paths are relative to this file's directory. Keep `icon`, `contents`, and `dataPath` inside the same folder. `contents` is the UI directory (or an HTML file; its parent is then the UI root). `dataPath` is the writable data dir — independent of `contents`, created at startup with a `logs/` subdirectory.

```toml
icon = "icon.png"
name = "My App"
contents = "contents"
dataPath = "data"
showDevMenu = true            # optional. true while building; omit / false to ship without DevTools
keychainPrefix = "my-app"     # optional. OS keychain prefix, default "app-ly"

[settings]                    # optional. string-only map, exposed as shell.settings
apiBaseUrl = "https://api.example.com"

[[allowedCommands]]           # optional. programs shell.run / shell.spawn may start
name = "git"
program = "git"
args = ["^(status|log)$"]
timeoutMs = 30000
```

- `[settings]` values are TOML strings. A `.env` next to `app.toml` is merged on top and wins on collisions — use it for local overrides and secrets, and gitignore it.
- No `[[allowedCommands]]` means process execution is off. `program`, `cwd`, and `env` live here, never in JS.

## Path rules

`saveFile`, `readFile`, `deleteFile`, `renameFile`, `openFile`, `openFileLocation`, `dbQuery`, `dbExecute`, and `dbClose` take **simple filenames only** — no `/`, `\`, `..`, or empty names.

```javascript
await shell.saveFile("settings.json", "...");    // ok
await shell.saveFile("notes/today.json", "..."); // rejected
await shell.saveFile("../escape.json", "...");   // rejected
```

Need structure? Encode it in the filename, or use SQLite.

## CLI

The same binary can run shell features without opening a window — useful for probing AI, SQLite, files, or an allowlisted program. It loads the **same** `app.toml` as the GUI (`--config`, the folder containing the `.app`, then bundled / cwd fallbacks). `dataPath`, `[ai]`, and `[[allowedCommands]]` all apply.

Invoke the executable inside the bundle (Finder / `open` still launches the GUI, and stdout would have nowhere to go):

```bash
app-ly.app/Contents/MacOS/app-ly --help
app-ly.app/Contents/MacOS/app-ly ai "say hi"
app-ly.app/Contents/MacOS/app-ly run git status
app-ly.app/Contents/MacOS/app-ly --config ./app.toml db query notes.db "select * from notes"
```

- `run` is `shell.run`: only names in `[[allowedCommands]]`, with that entry's `args` / `cwd` / `env` / `timeoutMs`. No name lists the allowlist.
- `ai` uses `[ai]` defaults. There is no JS, so page tool handlers are not available — instead each `[[allowedCommands]]` entry is exposed to the model as a tool (same allowlist as `run`).
- HTTP/WebSocket servers, `spawn`, windows, and keychain still need the window.

No command → desktop app, as before.

## `window.shell`

Available on `window` before your scripts run. Methods return a `Promise` that **rejects with a string**. `shell.settings` is the exception — a plain object, no `await`.

### `settings`

`[settings]` from `app.toml`, merged with `.env` (`.env` wins). Deployment config, not user data.

```javascript
const res = await shell.get(`${shell.settings.apiBaseUrl}/items`);
```

Values are always strings. Read-only, fixed at startup. Keep secrets in `.env`, not in a committed `app.toml`.

### `saveFile` / `readFile`

Text files in `dataPath`. Key-value storage by filename — use SQLite once you need queries.

```javascript
await shell.saveFile("settings.json", JSON.stringify({ theme: "dark" }));
const settings = JSON.parse(await shell.readFile("settings.json")); // rejects if missing
```

### `deleteFile` / `renameFile` / `openFile` / `openFileLocation`

```javascript
await shell.saveFile("report.csv", csv);
await shell.openFile("report.csv");           // OS default app
await shell.openFileLocation("report.csv");   // reveal in Finder / Explorer
await shell.renameFile("report.csv", "report-final.csv");
await shell.deleteFile("report-final.csv");
```

`openFile` / `openFileLocation` resolve once the OS has been asked, not once the other app launches. On Linux, `openFileLocation` opens the folder rather than selecting the file. Both names stay inside `dataPath`.

### `onFileDrop` / `readClipboard` / `writeClipboard`

Native file drop and pasteboard. Paths stay in Rust; JS gets `{ name, mime, size, body, encoding }`. UTF-8 (no NUL) is `encoding: "text"`, anything else `"base64"`. Bodies over 8 MiB are `null` on read (name/mime/size remain) and reject the whole write. Directories are skipped.

```javascript
const unlisten = await shell.onFileDrop((event) => {
  if (event.type !== "drop") return;
  for (const file of event.files) {
    if (file.body == null) continue;
  }
});

const clip = await shell.readClipboard();
// { text, html, files } — empty clipboard is nulls/[], never rejects

await shell.writeClipboard({ text: "hello" });
await shell.writeClipboard({
  files: [{ name: "export.csv", body: "a,b\n1,2", encoding: "text" }],
});
```

- `onFileDrop` types: `"enter"` | `"over"` | `"drop"` | `"leave"`. Files (metadata only) on `enter`; bodies on `drop`. HTML5 `dataTransfer.files` from Finder is not available — the native handler owns the drop.
- `readClipboard` is for a Paste button (no user gesture). `⌘V` in text fields already works via the Edit menu. Finder-copied files come through `files`; `text`/`html` are then `null` so paths are not leaked.
- `writeClipboard` is for a Copy button. Replaces the pasteboard; empty input clears it. File names are simple filenames; bodies are staged in a temp dir.

### `dbQuery` / `dbExecute` / `dbClose`

A SQLite file in `dataPath`, created on first use.

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
// { changes, lastInsertRowid }

const { columns, rows } = await shell.dbQuery(
  "app.db",
  "SELECT id, title FROM notes ORDER BY id DESC",
);
const idIdx = columns.indexOf("id");
const records = rows.map((r) => ({ id: r[idIdx] }));

await shell.dbClose("app.db"); // or dbClose() for every open db
```

- `CREATE TABLE IF NOT EXISTS` on every startup — there is no migrator.
- Rows are arrays aligned with `columns`, not objects.
- Params: `null`, boolean, number, string. No blobs (they come back as `null`). Use `?` placeholders.
- Close before deleting, renaming, or copying the file. Idle connections close after 30s.

### `log`

Appends to `dataPath/logs/shell.log`. For built apps without DevTools, not a `console.log` replacement.

```javascript
await shell.log("user clicked save");
await shell.log("save failed: " + err, "error");
```

### `notify`

Native OS notification. Use for things the user is not watching the window for.

```javascript
await shell.notify("Export finished", "report.csv saved");
```

### `fetch` / `get` / `post`

Proxied through the shell, so CORS does not apply. `http://` and `https://` only.

```javascript
const res = await shell.get("https://api.example.com/items");
if (res.ok) {
  const data = JSON.parse(res.body); // body is always a string
}

await shell.post(
  "https://api.example.com/items",
  JSON.stringify({ name: "x" }),
  { "Content-Type": "application/json" },
);

await shell.fetch(url, { method: "PATCH", headers: { ... }, body: "..." });
```

No streaming, binary bodies, multipart, or WebSockets on this path.

### Window and screen

Physical pixels. Most apps never need these.

```javascript
const { x, y } = await shell.getWindowPosition();
const { width, height } = await shell.getWindowSize();
await shell.setWindowPosition(x, y);
await shell.setWindowSize(width, height);
await shell.minimize();

const { screens, primaryIndex } = await shell.getScreens();
const screen = await shell.getScreenAt(x, y);
// divide by screen.scaleFactor for CSS pixels
```

### `openWindow` / `closeWindow` / `onWindowNavigated` / `onWindowLoaded` / `onWindowClosed` / `getWindowBody` / `evalWindow`

Child webviews for external flows (OAuth in a popup). `http(s)` only. If the provider refuses embedded webviews, use `authViaBrowser` instead.

```javascript
const { id } = await shell.openWindow("https://accounts.example.com/oauth/authorize?...", {
  title: "Sign in",
  width: 480,
  height: 640,
});

const unlisten = await shell.onWindowNavigated((windowId, url) => {
  if (windowId !== id) return;
  if (!url.startsWith("https://yourapp.example.com/callback")) return;
  const code = new URL(url).searchParams.get("code");
  shell.closeWindow(id);
  unlisten();
});

await shell.onWindowClosed((windowId) => {
  if (windowId === id) { /* user abandoned the flow */ }
});

const title = await shell.evalWindow(id, "return document.title;");
const text = await shell.getWindowBody(id);
```

- `openWindow` options: `{ title?, width?, height? }` (default `480×640`). Returns `{ id }`. You cannot close `"main"`.
- Filter events by `id`. `onWindowLoaded` fires when the DOM is settled — prefer it before `getWindowBody` / `evalWindow`.
- `evalWindow` runs `code` as an `async` function body; the result must be JSON-serializable. Don't open untrusted URLs.

### `authViaBrowser`

System-browser sign-in. Opens `authUrl`, waits for your backend to redirect to a local callback with `?authCode=` or `?error=`.

```javascript
const authCode = await shell.authViaBrowser(
  "https://idp.example.com/saml/login?service=myapp",
);

const res = await shell.post(
  "https://api.example.com/auth/exchange",
  JSON.stringify({ authCode }),
  { "Content-Type": "application/json" },
);
const { token } = JSON.parse(res.body);
```

- Default timeout is 2 minutes (`timeoutMs`). Pass `returnUrl` only if the provider requires a fixed redirect URI.
- One-shot: no `id`, no `evalWindow`. Exchange the code with `shell.post`.

### `secretSet` / `secretGet` / `secretDelete`

OS keychain (macOS Keychain, Linux Secret Service, Windows Credential Manager).

```javascript
await shell.secretSet("myapp", "api-key", "sk-abc123...");
const key = await shell.secretGet("myapp", "api-key"); // rejects if missing
await shell.secretDelete("myapp", "api-key");
```

`service` groups secrets; `account` names one. No `secretExists` — catch the error.

### `httpStart` / `httpRespond` / `httpStop` / `onHttpRequest`

Local HTTP server on `127.0.0.1`. One at a time. Text bodies only.

```javascript
const { port } = await shell.httpStart({ port: 0 });

await shell.onHttpRequest(async (req) => {
  await shell.httpRespond(req.id, 200, { "Content-Type": "text/plain" }, "Hello");
});

await shell.httpStop();
```

`port: 0` picks a free port. The server thread waits until `httpRespond`. Handle "already running".

### `wsStart` / `wsSend` / `wsClose` / `wsStop` / `onWsConnection` / `onWsMessage` / `onWsClose`

Local WebSocket server on `127.0.0.1`. Text frames only.

```javascript
const { port } = await shell.wsStart({ port: 0 });

await shell.onWsConnection(({ id }) => shell.wsSend(id, "Welcome!"));
await shell.onWsMessage(({ id, data }) => { /* ... */ });
await shell.onWsClose(({ id }) => { /* ... */ });

await shell.wsClose(id);
await shell.wsStop();
```

### `run` / `spawn` / `listCommands`

Only programs listed in `[[allowedCommands]]`. JS supplies the `name` and arguments — never `program`, `cwd`, or `env`.

```toml
[[allowedCommands]]
name = "git"
program = "git"
args = ["^(status|log|diff)$", "^--oneline$"]
extraArgs = "^[\\w./-]+$"
maxArgs = 8
cwd = "repo"
timeoutMs = 30000
env = { GIT_PAGER = "cat" }
```

```javascript
const { stdout, stderr, code, timedOut } = await shell.run("git", ["status"], {
  timeoutMs: 5000,
});
if (code !== 0) { /* show stderr */ }

const proc = await shell.spawn("git", ["log", "--oneline"]);
proc.onStdout((data) => { /* ... */ });
const { code: exitCode } = await proc.exited;

for await (const { stream, data } of await shell.spawn("git", ["log"])) {
  // stream is "stdout" or "stderr"
}

await proc.write("y\n");
await proc.closeStdin();
await proc.setTimeout(10_000); // re-arm; null clears
await proc.exit();             // SIGTERM; await proc.exited to know it's gone
await proc.kill();             // force

const commands = await shell.listCommands();
// [{ name, program, argsRestricted, timeoutMs }]
```

- Omitting both `args` and `extraArgs` accepts any arguments. Patterns are fully anchored. Nothing runs through a shell (no pipes, globs, `&&`).
- Unknown `name` / bad args / missing binary **reject**. Non-zero exit and timeout **resolve** — check `code` and `timedOut`.
- Timeout: call `options.timeoutMs`, else the entry's `timeoutMs`, else none. `run` buffers all output; `spawn` for anything long or interactive.

### AI — `generate` / `generateObject` / `stream`

On-device model — no API key, no network. Not always present (macOS 26+, Apple silicon, Apple Intelligence on). Treat as an enhancement.

```toml
[ai]
enabled = true
instructions = "Answer briefly and in plain text."
temperature = 0.7
maxTokens = 512
toolTimeoutMs = 30000
```

```javascript
const info = await shell.ai.info(); // never rejects
if (!info.available) return;

const { text } = await shell.ai.generate("Summarise this note in one line.", {
  instructions: "You are concise.",
  maxTokens: 200,
  toolTimeoutMs: 5000,
});

const { object } = await shell.ai.generateObject("Tag this note.", {
  type: "object",
  properties: {
    title: { type: "string" },
    tags: { type: "array", items: { type: "string" }, maxItems: 5 },
  },
  required: ["title", "tags"],
});

const stream = await shell.ai.stream("Write a short poem about the sea.");
for await (const delta of stream) { /* ... */ }
await stream.completed; // { text, model, toolCalls }
stream.cancel();        // stops delivery, not the model
```

- `info` / `available` / `models` never reject. Generate calls reject with `ai unavailable: <reason>`.
- Reasons: `unsupported-platform`, `unsupported-os`, `disabled-by-config`, `device-not-eligible`, `not-enabled`, `model-not-ready`, `unavailable`.
- No chat history. One model (`"default"`). Per-request `options` (`instructions`, `temperature`, `maxTokens`, `toolTimeoutMs`, `tools`) override `[ai]`.
- Tools: `tools: [{ name, description, parameters, handler }]`. Handler failures go to the model, not to your catch.
- Schema subset and tool bridge: [`ai.md`](ai.md).

### Built-in, no API

- DevTools / reload: `Cmd/Ctrl+Shift+M` or `I`, `Cmd/Ctrl+Shift+R`, View menu.
- Edit menu: Cut / Copy / Paste / Select All with the platform shortcuts.

## Errors worth handling

- Missing file on `readFile` / `deleteFile` / `renameFile` / `openFile` / `openFileLocation`.
- `fetch` network failures.
- Non-zero `code` or `timedOut` from `run` / `spawn` (these resolve). A rejected `run` / `spawn` is a config/argument error.
- `shell.ai.*` generate calls when the model is unavailable — check `info()` first.
- Everything else (bad filename, bad SQL, bad URL scheme) is a programming error.

## Checklist

1. Paths in `app.toml` stay inside that file's folder.
2. File and database names are simple filenames, never paths.
3. SQLite tables use `CREATE TABLE IF NOT EXISTS` on every startup; SQL uses `?` placeholders.
4. `res.body` is a string — parse JSON yourself.
5. Secrets in `.env` / `secretSet`, not in committed `[settings]` or `saveFile`.
6. HTTP / WebSocket "already running" is handled.
7. `[[allowedCommands]]` is as narrow as it can be; the UI checks `code` and `timedOut`.
