# WebDriver

`app-ly` can serve a [W3C WebDriver](https://www.w3.org/TR/webdriver2/) endpoint from inside the
shell process, so Selenium, WebdriverIO, or plain `curl` can drive the running app. It is off by
default and exists purely as a debugging/automation aid for app authors — not a feature contents
HTML can turn on for itself.

## What it is

Tauri's official automation route is [`tauri-driver`](https://v2.tauri.app/develop/tests/webdriver/),
an external binary that wraps the platform's native WebDriver implementation — and it has no macOS
support at all. `app-ly` takes a different route: the shell itself listens on a loopback HTTP port
and speaks enough of the WebDriver protocol to be driven on every platform the shell builds for,
with no extra binary to install.

Commands are carried out by evaluating a small JS harness
([`src-tauri/scripts/webdriver-harness.js`](../src-tauri/scripts/webdriver-harness.js)) inside the
session's window, through the same eval bridge `shell.evalWindow` uses for child windows. The
endpoint therefore sees exactly what the page sees — the real DOM, the real `window.shell` — and
can only do what page JS can do: no OS-level screenshots, no cookie jar outside the page, no
browser chrome to inspect.

Implemented in [`src-tauri/src/webdriver.rs`](../src-tauri/src/webdriver.rs), wired up in
[`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs), config type in
[`src-tauri/src/config.rs`](../src-tauri/src/config.rs).

## Enabling it

### `app.toml`

```toml
[webdriver]
enabled = true        # optional; a present [webdriver] table means "on" unless this is false
host = "127.0.0.1"    # optional; default "127.0.0.1"
port = 4444           # optional; default 4444
token = "s3cret"      # optional; when set, requests must carry it — see Security
```

No `[webdriver]` table at all means the endpoint is off. A `[webdriver]` table with no `enabled`
key still turns it on — nobody writes the table just to leave it disabled.

### CLI flags

CLI flags override `app.toml`, field by field:

| Flag | Meaning |
|---|---|
| `--webdriver` | turn it on |
| `--webdriver=4444` | turn it on, on that port |
| `--webdriver-port 4444` / `--webdriver-port=4444` | set the port (implies on) |
| `--webdriver-host 127.0.0.1` / `--webdriver-host=…` | set the bind host (implies on) |
| `--webdriver-token s3cret` / `--webdriver-token=…` | set the shared token (implies on) |
| `--no-webdriver` | force off — beats every other flag and `app.toml` |

Every value flag accepts both `--flag value` and `--flag=value`. Passing a port, host, or token
without `--webdriver` still turns the endpoint on — asking for a specific port isn't something
you'd do if you didn't want it listening.

Precedence overall: **CLI flags > `[webdriver]` in `app.toml` > built-in defaults**
(`127.0.0.1:4444`, no token, off).

```bash
npm run tauri dev -- --webdriver
./app-ly --webdriver-port 9515
./app-ly --config ./app.toml --webdriver-host 0.0.0.0 --webdriver-token s3cret
```

### Startup behavior

On success it logs to stderr:

```
webdriver: listening on http://127.0.0.1:4444 (POST /session to start)
```

If the port is already taken, the bind fails, that failure is logged, and **the app keeps running
without the endpoint** — a busy port never takes the window down:

```
webdriver: could not listen on 127.0.0.1:4444: <os error>
```

## Security

- **Off by default.** Nothing listens unless `[webdriver]` is configured or a `--webdriver*` flag
  is passed.
- **Loopback by default.** `host` defaults to `127.0.0.1`.
- **Token-gated when configured.** If `token` is set, every request must carry either
  `Authorization: Bearer <token>` or `X-Auth-Token: <token>`, or it gets HTTP 401. With no token,
  no auth is required at all — which is exactly why the default bind is loopback-only.
- **Binding a non-loopback host with no token prints a warning** at startup:

  ```
  webdriver: WARNING — listening on 0.0.0.0:4444 with no token. Anyone who can reach this port
  can run arbitrary JavaScript in the app.
  ```

State this plainly to anyone deploying it: **reaching the port means running arbitrary JavaScript
in the app**, with the app's own permissions (file access via `saveFile`/`readFile`, whatever
`shell.fetch` can reach, whatever `[[allowedCommands]]` allows, the OS keychain, and so on). This
is a debugging/automation surface, not a supported remote-control feature for end users. Ship
release builds with it off, or gated behind an explicit CLI flag you control.

## Sessions

**One session at a time.** `POST /session` while a session is already running returns
`session not created`; end it with `DELETE /session/{id}` before starting another.

Capabilities are read from `capabilities.alwaysMatch` in the `POST /session` body, falling back to
the first entry of `capabilities.firstMatch` — `alwaysMatch` wins where both set the same key. Which
of the two a client populates varies by library, so either works.

| Capability | Effect |
|---|---|
| `"app-ly:window"` | Which window to drive, by handle. Default `"main"`. Rejects with `session not created` if no window has that handle |
| `timeouts.script` | Initial script timeout, ms. Default `30000` |
| `timeouts.pageLoad` | Initial page-load timeout, ms. Default `300000` |
| `timeouts.implicit` | Initial implicit wait, ms. Default `0` |

`POST /session` returns:

```json
{
  "sessionId": "0000000000000001",
  "capabilities": {
    "browserName": "app-ly",
    "browserVersion": "0.1.0",
    "platformName": "macos",
    "acceptInsecureCerts": false,
    "setWindowRect": true,
    "timeouts": { "implicit": 0, "pageLoad": 300000, "script": 30000 },
    "app-ly:name": "My App",
    "app-ly:window": "main",
    "app-ly:windows": ["main"]
  }
}
```

**Window handles are Tauri window labels**, not synthetic ids: `"main"` for the main window, and
`shell-window-N` for each child window opened via `shell.openWindow`. `GET /session/{id}/window/handles`
lists every open handle with `main` sorted first.

## Endpoint reference

Responses follow W3C shape. Success: `200 {"value": …}`. Failure:
`{"value": {"error": "<code>", "message": "…", "stacktrace": "…"}}`, with the HTTP status the W3C
spec assigns to that error code — 404 for `no such element` / `no such window` /
`stale element reference` / `invalid session id`, 400 for `invalid argument` / `invalid selector` /
`element not interactable`, 405 for `unknown method` (a known path reached with the wrong HTTP
method), 404 for `unknown command` (a path with no route at all), and 500 for everything else
(including `unknown error`, `javascript error`, `script timeout`, `unsupported operation`).

Session-independent:

| Method + path | Notes |
|---|---|
| `GET /status` | `{ready, message}` — `ready` is `false` while a session is live |
| `POST /session` | Create a session (see [Sessions](#sessions)) |
| `DELETE /session/{id}` | End the session |

Under `/session/{id}`:

| Method + path | Notes |
|---|---|
| `GET`/`POST /timeouts` | `{implicit, pageLoad, script}` in ms |
| `POST /url` | `{url}` — navigates, then waits for `document.readyState === "complete"` |
| `GET /url` | Current `location.href` |
| `GET /title` | `document.title` |
| `GET /source` | `documentElement.outerHTML` |
| `POST /back` / `/forward` / `/refresh` | Each waits for load, same as `/url` |
| `GET /window` | The current window handle |
| `GET /window/handles` | All open handles, `main` first |
| `POST /window` | `{handle}` — switch the driven window and focus it |
| `DELETE /window` | Closes the current window and returns the remaining handles; refuses with `unsupported operation` on `main` (closing it would quit the app — use `DELETE /session` instead) |
| `GET`/`POST /window/rect` | `{x, y, width, height}` in physical pixels, read/set via the native window, not JS |
| `POST /window/minimize`, `/maximize`, `/fullscreen` | Returns the new rect |
| `POST /element`, `POST /elements` | `{using, value}` — find one / all, from the document root |
| `POST /element/{id}/element`, `/elements` | Same, scoped to that element |
| `GET /element/active` | `document.activeElement` |
| `GET /element/{id}/text` | `innerText`, falling back to `textContent` |
| `GET /element/{id}/name` | Tag name, lowercased |
| `GET /element/{id}/rect` | Page coordinates (`getBoundingClientRect` plus scroll offset) |
| `GET /element/{id}/displayed`, `/enabled`, `/selected` | Booleans |
| `GET /element/{id}/attribute/{name}` | The attribute, or `"true"` for a true boolean DOM property with no matching attribute, else `null` |
| `GET /element/{id}/property/{name}` | The DOM property, `null` if `undefined` |
| `GET /element/{id}/css/{name}` | Computed style value |
| `POST /element/{id}/click` | Scrolls into view, dispatches mouseover/mousemove/mousedown, focuses, mouseup, then a native `.click()` |
| `POST /element/{id}/clear` | Inputs, textareas, and `contenteditable` |
| `POST /element/{id}/value` | `{text}` (also accepts the legacy `{value: [...]}` array of key chunks) |
| `POST /execute/sync` | `{script, args}` — script is an **async** function body, so `await` works at the top level; `arguments[0]…` for the args; a returned promise is awaited |
| `POST /execute/async` | `{script, args}` — same async body, with the completion callback as the last argument; bounded by the session's `script` timeout |

`app-ly` extensions — see [their own section](#the-app-ly-extensions) below:

| Method + path | Notes |
|---|---|
| `GET /app-ly/windows` | Every shell window: handle, title, url, visible, focused, minimized, devtoolsOpen, rect |
| `GET /app-ly/logs?lines=N` | Tail of `dataPath/logs/shell.log` — what `shell.log()` writes. Default 200 lines |
| `POST /app-ly/devtools` | Toggles the Web Inspector on the session's window, returns `{open}` |

## Locator strategies and implicit wait

`using` accepts five strategies:

| `using` | Matches |
|---|---|
| `css selector` | `querySelector`/`querySelectorAll` |
| `tag name` | `getElementsByTagName` |
| `link text` | `<a>` elements whose trimmed text equals `value` exactly |
| `partial link text` | `<a>` elements whose trimmed text contains `value` |
| `xpath` | `document.evaluate`, ordered snapshot |

The session's **implicit wait** (`timeouts.implicit`, default `0`) applies to element lookups:
`POST /element` (and its scoped/`/elements` siblings for a single match) polls every 50ms until the
deadline passes before returning `no such element`. `POST /elements` (find-all) never errors on
timeout — it returns `[]` once the deadline passes.

## Element references and staleness

Element references use the standard W3C wire key `element-6066-11e4-a52e-4f735466cecc`, both in
responses and in anything you pass back into `execute/sync`/`execute/async` args. Internally the
harness keeps a `WeakMap` from live DOM elements to opaque ids (`e1`, `e2`, …); a reference is a
`{ "element-6066-11e4-a52e-4f735466cecc": "e7" }` object, resolvable only within that document.

A reference goes **stale** — `stale element reference` — the moment either is true:

- the element is detached from the document (`!element.isConnected`), or
- the document it came from has been replaced (navigation re-injects the harness fresh, resetting
  the whole registry).

There is no re-attach or refresh: re-locate the element after any navigation.

## Text entry

`POST /element/{id}/value` writes character-by-character through the input/textarea prototype's
**native value setter**, not the instance property — frameworks like React shadow the instance
property with their own getter/setter, so a plain `element.value = x` is invisible to them, but
writing through `Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set` is not.
For each character it dispatches `keydown` → (value write) → `input` → `keyup`, then a final
`change` once the whole string has been typed. `<select>` elements accept an option's `value` or
its trimmed label text instead of being typed character-by-character.

Special WebDriver key code points are recognized inline in the text — these are the standard Unicode PUA characters (invisible when rendered; shown here as escapes):

| Code point | Key |
|---|---|
| `\uE003` | Backspace |
| `\uE004` | Tab |
| `\uE006` / `\uE007` | Enter |
| `\uE00C` | Escape |

Anything else is typed as literal text. `POST /element/{id}/clear` clears inputs/textareas/
`contenteditable` the same way (native setter, then `input`/`change`).

## The `app-ly/*` extensions

These are not part of W3C WebDriver — they exist because a shell app has state a browser doesn't,
and reaching it without them would mean writing bespoke `execute/sync` scripts every time. This is
the biggest practical win of driving `app-ly` over driving a browser: you get the app's logs and
its whole window layout for free.

### `GET /app-ly/windows`

Every open shell window, independent of which one the session is currently pointed at — handy for
picking a `handle` to `POST /window` to, or for confirming a child window opened by
`shell.openWindow` actually appeared.

```bash
curl -s -H "X-Auth-Token: s3cret" localhost:4444/session/$SID/app-ly/windows | jq
```

```json
[
  {
    "handle": "main",
    "title": "My App",
    "url": "shell://localhost/index.html",
    "visible": true,
    "focused": true,
    "minimized": false,
    "devtoolsOpen": false,
    "rect": { "x": 120, "y": 80, "width": 1024, "height": 768 }
  }
]
```

### `GET /app-ly/logs?lines=N`

Tails `dataPath/logs/shell.log` — exactly what `shell.log()` writes from contents JS — so a driver
can assert on app-side logging without reaching into `dataPath` itself. `lines` defaults to 200.

```bash
curl -s localhost:4444/session/$SID/app-ly/logs?lines=50 | jq -r '.value.lines[]'
```

### `POST /app-ly/devtools`

Toggles the native Web Inspector on the session's window and reports the new state — useful for
popping DevTools open right before a failure to eyeball the page, without going near the app's own
`showDevMenu` shortcut.

```bash
curl -s -X POST localhost:4444/session/$SID/app-ly/devtools
# {"value":{"open":true}}
```

## What is deliberately not implemented

Each of these returns `unsupported operation` (HTTP 500):

| Not implemented | Use instead |
|---|---|
| `GET /screenshot`, `GET /element/{id}/screenshot` | The shell has no webview capture API — take a screenshot from the OS |
| `/cookie` (all methods) | Read/write cookies via `execute/sync`, e.g. `return document.cookie` |
| `/frame`, `/frame/parent` | Not applicable — there is one document per window; use `execute/sync` if the page has iframes of its own |
| `/alert_text`, `/alert/accept`, `/alert/dismiss` | The shell has no native alert bridge; if your app uses `window.confirm`/`alert`, stub or intercept them from the page instead |
| `/actions` (the W3C Actions API — synthesized pointer/key sequences) | Use the element commands (`click`, `value`, `clear`) or synthesize events yourself in `execute/sync` |

Also worth knowing: `shell.*` APIs are reachable from `execute/sync` exactly like any other page
script, which is often the fastest way to inspect app state directly —
`return await window.shell.dbQuery("app.db", "select * from notes")` beats scraping the DOM for
data the page already has in a database.

## Driving the app

### curl walkthrough

A full session: status, create, find, click, run a script, tear down. `SID` holds the session id
throughout.

```bash
# is the endpoint up, and is a session free?
curl -s localhost:4444/status
# {"value":{"ready":true,"message":"ready for a new session"}}

# create a session
SID=$(curl -s -X POST localhost:4444/session \
  -H 'content-type: application/json' \
  -d '{"capabilities":{"alwaysMatch":{}}}' | jq -r .value.sessionId)

# find an element
EID=$(curl -s -X POST localhost:4444/session/$SID/element \
  -H 'content-type: application/json' \
  -d '{"using":"css selector","value":"#save"}' | jq -r '.value["element-6066-11e4-a52e-4f735466cecc"]')

# click it
curl -s -X POST localhost:4444/session/$SID/element/$EID/click

# run a script and read something back
curl -s -X POST localhost:4444/session/$SID/execute/sync \
  -H 'content-type: application/json' \
  -d '{"script":"return document.title;","args":[]}'
# {"value":"My App"}

# end the session
curl -s -X DELETE localhost:4444/session/$SID
```

With a token configured, add `-H "Authorization: Bearer s3cret"` (or `X-Auth-Token: s3cret`) to
every call above.

### WebdriverIO

```javascript
import { remote } from "webdriverio";

const browser = await remote({
  hostname: "127.0.0.1",
  port: 4444,
  path: "/",
  capabilities: { browserName: "app-ly" },
});

const button = await browser.$("#save");
await button.click();

console.log(await browser.getTitle());

await browser.deleteSession();
```

### Python Selenium

```python
from selenium import webdriver
from selenium.webdriver.common.by import By

options = webdriver.ChromeOptions()  # any Options subclass — its values are ignored
driver = webdriver.Remote(command_executor="http://127.0.0.1:4444", options=options)

driver.find_element(By.CSS_SELECTOR, "#save").click()
print(driver.title)

driver.quit()
```

Selenium's `Remote` client speaks W3C WebDriver in general, and works for everything this endpoint
implements — but Selenium clients routinely probe endpoints this shell does not implement (session
capability negotiation extras, `/se/...` vendor paths) as part of normal setup, and those calls will
come back as `unsupported operation` or `unknown command` rather than being silently skipped. `curl`
and WebdriverIO are the smoothest paths; a Selenium client works but is the least forgiving of the
three.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Nothing answers on the port at all | The endpoint isn't enabled — confirm `[webdriver]` is present (and `enabled` isn't `false`) or a `--webdriver*` flag was passed; check stderr for the `webdriver: listening on …` line or a bind-failure line |
| `session not created` on `POST /session` | A session is already running — `DELETE /session/{id}` first, or check `GET /status` |
| `invalid session id` | Either no session exists yet, or the `{id}` in the URL doesn't match the one active session's id (only one session runs at a time) |
| `stale element reference` | The element was detached, or the page navigated/reloaded since you found it — re-run the `find` |
| `no such window` | The `handle` doesn't match any currently open window — check `GET /session/{id}/window/handles` or `GET /app-ly/windows` |
| `401` on every request | A `token` is configured and your request is missing `Authorization: Bearer <token>` / `X-Auth-Token: <token>`, or it doesn't match. The W3C error set has no auth code, so the body reads `unknown error` with the message `missing or invalid webdriver token` |
| `element not interactable` on click/value/clear | The element isn't displayed (zero size, `display:none`, `visibility:hidden`, `opacity:0`) or is disabled — check `GET /element/{id}/displayed` and `/enabled` first |
