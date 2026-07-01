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
- The platform opener binary is missing (e.g. `xdg-open` not installed) in `openFile`/`openFileLocation`

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
- No WebSocket or multipart helpers
- `shell.settings` values are strings only; no nested objects, numbers, or booleans
- `.env` parsing has no multi-line values, `\n` escapes, or variable interpolation
- Child windows (`openWindow`) are plain webviews with no `window.shell` injected into them — they're for external content only, not a place to run more of your app's JS. Use `getWindowBody`/`evalWindow` from the main window to read or drive them instead
- `evalWindow` result values must be JSON-serializable (like `dbQuery`/`fetch` payloads) — functions, DOM nodes, etc. come back as `null`
- `openFile`/`openFileLocation` resolve once the OS has been asked to open the item, not once it's actually open — a missing default app or file manager failure won't surface as a rejected promise
- `openFileLocation` "selects" the file on macOS/Windows; on Linux it can only open the enclosing folder, not select the file within it