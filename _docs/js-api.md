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

The Web Inspector is available when `showDevMenu` is enabled in `app.toml` (default `true` in debug builds, `false` in release). Set `showDevMenu = true` in the external `app.toml` beside your `.app` bundle to enable the menu item and shortcuts in release.

The shell calls Tauri’s `WebviewWindow::open_devtools` / `close_devtools` (via `shell_toggle_devtools`). Release builds include the `devtools` Cargo feature on `tauri`. On macOS, programmatic inspector access uses a private API and is not App Store–compatible.

The same actions are in the native app menu under **View** → **Reload** / **Open DevTools** (DevTools follows `showDevMenu`).

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
- Missing file on `readFile`
- Unsupported URL scheme in `fetch`
- Network failure in `fetch`
- Invalid database name or SQL error in `dbQuery` / `dbExecute`
- Window not available yet when calling window APIs during very early page load

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