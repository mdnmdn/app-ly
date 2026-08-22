//! A W3C WebDriver endpoint served from inside the shell.
//!
//! Tauri's official automation story is `tauri-driver`, an external process
//! that wraps the platform's native WebDriver (and has no macOS support at
//! all). This module takes the other route: the shell itself listens on a
//! loopback port and speaks enough of the WebDriver protocol that a Selenium
//! or WebdriverIO client — or plain `curl` — can drive the running app on
//! every platform the shell builds for. Commands are carried out by
//! evaluating the harness in [`scripts/webdriver-harness.js`] inside the
//! session's window, so what the endpoint can see is exactly what the page
//! can see.
//!
//! It is off unless `[webdriver] enabled = true` in `app.toml` or `--webdriver`
//! is passed, and it binds loopback by default.

use crate::commands::{eval_in_window, EvalState, ShellState};
use crate::config::WebDriverConfig;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};

const HARNESS: &str = include_str!("../scripts/webdriver-harness.js");

pub const DEFAULT_PORT: u16 = 4444;
pub const DEFAULT_HOST: &str = "127.0.0.1";

const DEFAULT_SCRIPT_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_PAGE_LOAD_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_IMPLICIT_WAIT_MS: u64 = 0;

// Every eval is given the command's own budget plus this, so the JS side —
// which polls its own deadline for implicit waits — is the one that reports a
// timeout, with the right W3C error code, instead of the Rust side guessing.
const EVAL_GRACE_MS: u64 = 2_000;

// ── Settings ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WebDriverSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
}

impl Default for WebDriverSettings {
    fn default() -> Self {
        WebDriverSettings {
            enabled: false,
            host: DEFAULT_HOST.into(),
            port: DEFAULT_PORT,
            token: None,
        }
    }
}

/// CLI overrides, in the order they were parsed. Kept separate from the
/// config so `resolve_settings` can apply "flag beats file" without caring
/// where either came from.
#[derive(Debug, Clone, Default)]
pub struct WebDriverCliOverrides {
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub token: Option<String>,
}

/// Reads `--webdriver`, `--no-webdriver`, `--webdriver-port`,
/// `--webdriver-host` and `--webdriver-token`. Each value flag accepts both
/// `--flag value` and `--flag=value`. Passing a port or host without
/// `--webdriver` still turns the endpoint on — asking for a port you don't
/// want listened on isn't a thing anyone means.
pub fn parse_cli_overrides<I: IntoIterator<Item = String>>(args: I) -> WebDriverCliOverrides {
    let args: Vec<String> = args.into_iter().collect();
    let mut overrides = WebDriverCliOverrides::default();
    let mut explicitly_off = false;

    let value_for = |index: usize, flag: &str, arg: &str| -> Option<String> {
        if let Some(inline) = arg.strip_prefix(&format!("{flag}=")) {
            return Some(inline.to_string());
        }
        args.get(index + 1).cloned()
    };

    for (index, arg) in args.iter().enumerate() {
        let flag = arg.split('=').next().unwrap_or(arg);
        match flag {
            "--webdriver" if arg == "--webdriver" => overrides.enabled = Some(true),
            "--no-webdriver" => {
                overrides.enabled = Some(false);
                explicitly_off = true;
            }
            "--webdriver-port" => {
                if let Some(port) = value_for(index, "--webdriver-port", arg)
                    .and_then(|value| value.parse::<u16>().ok())
                {
                    overrides.port = Some(port);
                }
            }
            "--webdriver-host" => {
                overrides.host = value_for(index, "--webdriver-host", arg);
            }
            "--webdriver-token" => {
                overrides.token = value_for(index, "--webdriver-token", arg);
            }
            // `--webdriver=4444` is a natural shorthand to reach for.
            "--webdriver" => {
                overrides.enabled = Some(true);
                if let Some(port) =
                    value_for(index, "--webdriver", arg).and_then(|value| value.parse::<u16>().ok())
                {
                    overrides.port = Some(port);
                }
            }
            _ => {}
        }
    }

    if !explicitly_off
        && overrides.enabled.is_none()
        && (overrides.port.is_some() || overrides.host.is_some() || overrides.token.is_some())
    {
        overrides.enabled = Some(true);
    }

    overrides
}

pub fn resolve_settings(
    config: Option<&WebDriverConfig>,
    overrides: &WebDriverCliOverrides,
) -> WebDriverSettings {
    let mut settings = WebDriverSettings::default();

    if let Some(config) = config {
        // A `[webdriver]` table that says nothing about `enabled` still means
        // "I want this on" — nobody writes the table to leave it off.
        settings.enabled = config.enabled.unwrap_or(true);
        if let Some(host) = &config.host {
            settings.host = host.clone();
        }
        if let Some(port) = config.port {
            settings.port = port;
        }
        settings.token = config.token.clone();
    }

    if let Some(enabled) = overrides.enabled {
        settings.enabled = enabled;
    }
    if let Some(host) = &overrides.host {
        settings.host = host.clone();
    }
    if let Some(port) = overrides.port {
        settings.port = port;
    }
    if let Some(token) = &overrides.token {
        settings.token = Some(token.clone());
    }

    settings
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

// ── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WdError {
    pub code: String,
    pub message: String,
    pub stacktrace: String,
}

impl WdError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        WdError {
            code: code.into(),
            message: message.into(),
            stacktrace: String::new(),
        }
    }

    /// HTTP status per the W3C error table. Anything unrecognised is treated
    /// as an unknown error, which is also what the spec says to do.
    pub fn http_status(&self) -> u16 {
        match self.code.as_str() {
            "element click intercepted"
            | "element not interactable"
            | "insecure certificate"
            | "invalid argument"
            | "invalid cookie domain"
            | "invalid element state"
            | "invalid selector" => 400,
            "invalid session id"
            | "no such alert"
            | "no such cookie"
            | "no such element"
            | "no such frame"
            | "no such window"
            | "stale element reference"
            | "unknown command" => 404,
            "unknown method" => 405,
            _ => 500,
        }
    }

    fn body(&self) -> Value {
        json!({
            "value": {
                "error": self.code,
                "message": self.message,
                "stacktrace": self.stacktrace,
            }
        })
    }
}

fn invalid_argument(message: impl Into<String>) -> WdError {
    WdError::new("invalid argument", message)
}

fn unknown_error(message: impl Into<String>) -> WdError {
    WdError::new("unknown error", message)
}

// ── Session state ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Session {
    id: String,
    window: String,
    script_timeout_ms: u64,
    page_load_timeout_ms: u64,
    implicit_wait_ms: u64,
}

impl Session {
    fn timeouts(&self) -> Value {
        json!({
            "implicit": self.implicit_wait_ms,
            "pageLoad": self.page_load_timeout_ms,
            "script": self.script_timeout_ms,
        })
    }
}

struct Context {
    app: AppHandle,
    app_name: String,
    settings: WebDriverSettings,
    session: Mutex<Option<Session>>,
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(SESSION_COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", hasher.finish())
}

// ── JS plumbing ─────────────────────────────────────────────────────

fn lit<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

/// Substitutes `$NAME$` placeholders in a JS template. Single-pass over the
/// template only, so a substituted value (a selector, a user script) is never
/// rescanned for placeholders of its own.
fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 128);
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('$') {
            Some(end) => match vars.iter().find(|(name, _)| *name == &after[..end]) {
                Some((_, value)) => {
                    out.push_str(value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('$');
                    rest = after;
                }
            },
            None => {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Evaluates a command body in `label`'s window and unwraps the harness's
/// tagged result. The body runs as an async arrow function; `WD` is the
/// harness and `return` hands back the command's value.
fn eval(context: &Context, label: &str, body: &str, timeout_ms: u64) -> Result<Value, WdError> {
    let window = context
        .app
        .get_webview_window(label)
        .ok_or_else(|| WdError::new("no such window", format!("no window named {label}")))?;

    let code = format!(
        "{HARNESS}\nconst WD = window.__APPLY_WD__;\nreturn await WD.run(async () => {{ {body} }});"
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let app = context.app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<EvalState>();
        let _ = tx.send(eval_in_window(&state, &window, &code).await);
    });

    let raw = match rx.recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => return Err(unknown_error(message)),
        Err(_) => {
            return Err(WdError::new(
                "timeout",
                format!("the page did not answer within {timeout_ms}ms"),
            ))
        }
    };

    match raw.get("status").and_then(Value::as_str) {
        Some("ok") => Ok(raw.get("value").cloned().unwrap_or(Value::Null)),
        Some("error") => Err(WdError {
            code: raw
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("javascript error")
                .to_string(),
            message: raw
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            stacktrace: raw
                .get("stacktrace")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        _ => Err(unknown_error("the harness returned an unrecognised result")),
    }
}

fn eval_in_session(context: &Context, session: &Session, body: &str) -> Result<Value, WdError> {
    let budget = session
        .script_timeout_ms
        .max(session.implicit_wait_ms)
        .saturating_add(EVAL_GRACE_MS);
    eval(context, &session.window, body, budget)
}

// ── Command bodies ──────────────────────────────────────────────────

const FIND_JS: &str = r#"
  const deadline = Date.now() + $WAIT$;
  const root = $ROOT$;
  for (;;) {
    const found = WD.locate($USING$, $VALUE$, root, $ALL$);
    if (found.length) return $ALL$ ? found.map(WD.ref) : WD.ref(found[0]);
    if (Date.now() >= deadline) {
      if ($ALL$) return [];
      throw WD.fail('no such element', 'no element matching ' + $USING$ + ' ' + $VALUE$);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
"#;

fn find_body(session: &Session, using: &str, value: &str, root: Option<&str>, all: bool) -> String {
    let root_expr = match root {
        Some(id) => format!("WD.deref({})", lit(&id)),
        None => "document".into(),
    };
    render(
        FIND_JS,
        &[
            ("WAIT", &session.implicit_wait_ms.to_string()),
            ("ROOT", &root_expr),
            ("USING", &lit(&using)),
            ("VALUE", &lit(&value)),
            ("ALL", if all { "true" } else { "false" }),
        ],
    )
}

// Built as an async function rather than a plain one so a script body can use
// top-level `await` — the same affordance `shell.evalWindow` gives, and the
// thing anyone debugging an app reaches for first. `arguments` still works:
// an AsyncFunction is a normal function, not an arrow.
const ASYNC_FUNCTION_JS: &str =
    "const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;";

const EXECUTE_SYNC_JS: &str = r#"
  const args = WD.deserialize($ARGS$);
  $ASYNC_FUNCTION$
  const fn = new AsyncFunction($SCRIPT$);
  return await fn.apply(window, args);
"#;

const EXECUTE_ASYNC_JS: &str = r#"
  const args = WD.deserialize($ARGS$);
  $ASYNC_FUNCTION$
  const fn = new AsyncFunction($SCRIPT$);
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(WD.fail('script timeout', 'async script did not call its callback within $TIMEOUT$ms')), $TIMEOUT$);
    const done = (value) => { clearTimeout(timer); resolve(value); };
    try {
      fn.apply(window, args.concat([done]));
    } catch (err) {
      clearTimeout(timer);
      reject(err);
    }
  });
"#;

// Navigation is scheduled rather than awaited: the page is torn down mid-eval,
// so the harness's own callback would never make it back. `wait_for_load`
// below picks the window back up once it settles.
const NAVIGATE_JS: &str = r#"
  setTimeout(() => { $ACTION$; }, 10);
  return null;
"#;

fn navigate_body(action: &str) -> String {
    render(NAVIGATE_JS, &[("ACTION", action)])
}

/// Polls `document.readyState` until the window has finished loading. Eval
/// errors are expected while the document is being swapped, so they only
/// matter once the deadline passes.
fn wait_for_load(context: &Context, session: &Session) -> Result<(), WdError> {
    let deadline = Instant::now() + Duration::from_millis(session.page_load_timeout_ms);
    let mut last;
    loop {
        last = match eval(
            context,
            &session.window,
            "return document.readyState;",
            2_000,
        ) {
            Ok(Value::String(state)) if state == "complete" => return Ok(()),
            Ok(_) => None,
            Err(error) => Some(error),
        };
        if Instant::now() >= deadline {
            return Err(last.unwrap_or_else(|| {
                WdError::new(
                    "timeout",
                    format!(
                        "page did not finish loading within {}ms",
                        session.page_load_timeout_ms
                    ),
                )
            }));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

// ── Window helpers ──────────────────────────────────────────────────

fn window_handles(app: &AppHandle) -> Vec<String> {
    let mut handles: Vec<String> = app.webview_windows().keys().cloned().collect();
    handles.sort_by_key(|handle| (handle != "main", handle.clone()));
    handles
}

fn require_window(context: &Context, label: &str) -> Result<tauri::WebviewWindow, WdError> {
    context
        .app
        .get_webview_window(label)
        .ok_or_else(|| WdError::new("no such window", format!("no window named {label}")))
}

/// A move or resize is dispatched to the platform's event loop, so reading the
/// geometry straight back reports the old rect. Wait briefly for it to land so
/// the response describes the window the caller now has.
fn settled_window_rect(window: &tauri::WebviewWindow, expected: &Value) -> Result<Value, WdError> {
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        let rect = window_rect(window)?;
        let matched = ["x", "y", "width", "height"].iter().all(|key| {
            match (
                expected.get(key).and_then(Value::as_i64),
                rect.get(key).and_then(Value::as_i64),
            ) {
                (Some(want), Some(got)) => want == got,
                (Some(_), None) => false,
                (None, _) => true,
            }
        });
        if matched || Instant::now() >= deadline {
            return Ok(rect);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn window_rect(window: &tauri::WebviewWindow) -> Result<Value, WdError> {
    let position = window
        .outer_position()
        .map_err(|e| unknown_error(format!("get window position: {e}")))?;
    let size = window
        .outer_size()
        .map_err(|e| unknown_error(format!("get window size: {e}")))?;
    Ok(json!({
        "x": position.x,
        "y": position.y,
        "width": size.width,
        "height": size.height,
    }))
}

// ── Request handling ────────────────────────────────────────────────

struct Request {
    method: String,
    segments: Vec<String>,
    query: HashMap<String, String>,
    body: Value,
}

fn parse_target(target: &str) -> (Vec<String>, HashMap<String, String>) {
    let (path, query_string) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };

    let segments = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect();

    let mut query = HashMap::new();
    if let Some(query_string) = query_string {
        for pair in query_string.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            query.insert(percent_decode(key), percent_decode(value));
        }
    }

    (segments, query)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&text[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn body_str(body: &Value, key: &str) -> Result<String, WdError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| invalid_argument(format!("missing string field \"{key}\"")))
}

fn body_u64(body: &Value, key: &str) -> Option<u64> {
    body.get(key).and_then(Value::as_u64)
}

fn current_session(context: &Context, id: &str) -> Result<Session, WdError> {
    let session = context.session.lock().unwrap();
    match session.as_ref() {
        Some(session) if session.id == id => Ok(session.clone()),
        Some(_) => Err(WdError::new(
            "invalid session id",
            "another session is active",
        )),
        None => Err(WdError::new("invalid session id", "no active session")),
    }
}

fn update_session<F: FnOnce(&mut Session)>(context: &Context, update: F) {
    if let Some(session) = context.session.lock().unwrap().as_mut() {
        update(session);
    }
}

fn dispatch(context: &Context, request: &Request) -> Result<Value, WdError> {
    let method = request.method.as_str();
    let segments: Vec<&str> = request.segments.iter().map(String::as_str).collect();

    match (method, segments.as_slice()) {
        ("GET", ["status"]) => {
            let busy = context.session.lock().unwrap().is_some();
            Ok(json!({
                "ready": !busy,
                "message": if busy { "a session is already running" } else { "ready for a new session" },
            }))
        }
        ("POST", ["session"]) => new_session(context, &request.body),
        ("DELETE", ["session", id]) => {
            current_session(context, id)?;
            *context.session.lock().unwrap() = None;
            Ok(Value::Null)
        }
        (_, ["status"]) => Err(WdError::new("unknown method", "/status is read with GET")),
        (_, ["session"]) => Err(WdError::new(
            "unknown method",
            "sessions are created with POST /session",
        )),
        (_, []) => Err(WdError::new(
            "unknown command",
            "no command at the endpoint root; start with GET /status",
        )),
        (_, ["session", id, rest @ ..]) => {
            let session = current_session(context, id)?;
            session_command(context, &session, method, rest, request)
        }
        _ => Err(WdError::new(
            "unknown command",
            format!("{method} /{}", request.segments.join("/")),
        )),
    }
}

fn new_session(context: &Context, body: &Value) -> Result<Value, WdError> {
    let mut slot = context.session.lock().unwrap();
    if let Some(existing) = slot.as_ref() {
        return Err(WdError::new(
            "session not created",
            format!("session {} is already running", existing.id),
        ));
    }

    // Clients split capabilities across `alwaysMatch` and the `firstMatch`
    // array in ways that vary by library, so read both and let `alwaysMatch`
    // win — that is the direction the spec's matching algorithm resolves in.
    let always_match = body
        .pointer("/capabilities/alwaysMatch")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let first_match = body
        .pointer("/capabilities/firstMatch/0")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let capability = |name: &str| {
        always_match
            .get(name)
            .or_else(|| first_match.get(name))
            .cloned()
    };

    let requested_window = capability("app-ly:window")
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "main".into());

    let handles = window_handles(&context.app);
    if !handles.iter().any(|handle| handle == &requested_window) {
        return Err(WdError::new(
            "session not created",
            format!("no window named {requested_window}; open windows: {handles:?}"),
        ));
    }

    let timeouts = capability("timeouts").unwrap_or(Value::Null);
    let session = Session {
        id: new_session_id(),
        window: requested_window,
        script_timeout_ms: body_u64(&timeouts, "script").unwrap_or(DEFAULT_SCRIPT_TIMEOUT_MS),
        page_load_timeout_ms: body_u64(&timeouts, "pageLoad")
            .unwrap_or(DEFAULT_PAGE_LOAD_TIMEOUT_MS),
        implicit_wait_ms: body_u64(&timeouts, "implicit").unwrap_or(DEFAULT_IMPLICIT_WAIT_MS),
    };

    let response = json!({
        "sessionId": session.id,
        "capabilities": {
            "browserName": "app-ly",
            "browserVersion": env!("CARGO_PKG_VERSION"),
            "platformName": std::env::consts::OS,
            "acceptInsecureCerts": false,
            "setWindowRect": true,
            "timeouts": session.timeouts(),
            "app-ly:name": context.app_name,
            "app-ly:window": session.window,
            "app-ly:windows": window_handles(&context.app),
        }
    });

    *slot = Some(session);
    Ok(response)
}

fn session_command(
    context: &Context,
    session: &Session,
    method: &str,
    segments: &[&str],
    request: &Request,
) -> Result<Value, WdError> {
    let body = &request.body;

    match (method, segments) {
        // ── Timeouts ──
        ("GET", ["timeouts"]) => Ok(session.timeouts()),
        ("POST", ["timeouts"]) => {
            update_session(context, |session| {
                if let Some(value) = body_u64(body, "script") {
                    session.script_timeout_ms = value;
                }
                if let Some(value) = body_u64(body, "pageLoad") {
                    session.page_load_timeout_ms = value;
                }
                if let Some(value) = body_u64(body, "implicit") {
                    session.implicit_wait_ms = value;
                }
            });
            Ok(Value::Null)
        }

        // ── Navigation ──
        ("POST", ["url"]) => {
            let url = body_str(body, "url")?;
            eval_in_session(
                context,
                session,
                &navigate_body(&format!("window.location.assign({})", lit(&url))),
            )?;
            wait_for_load(context, session)?;
            Ok(Value::Null)
        }
        ("GET", ["url"]) => eval_in_session(context, session, "return window.location.href;"),
        ("GET", ["title"]) => eval_in_session(context, session, "return document.title;"),
        ("GET", ["source"]) => eval_in_session(
            context,
            session,
            "return document.documentElement ? document.documentElement.outerHTML : '';",
        ),
        ("POST", ["back"]) => {
            eval_in_session(context, session, &navigate_body("window.history.back()"))?;
            wait_for_load(context, session)?;
            Ok(Value::Null)
        }
        ("POST", ["forward"]) => {
            eval_in_session(context, session, &navigate_body("window.history.forward()"))?;
            wait_for_load(context, session)?;
            Ok(Value::Null)
        }
        ("POST", ["refresh"]) => {
            eval_in_session(context, session, &navigate_body("window.location.reload()"))?;
            wait_for_load(context, session)?;
            Ok(Value::Null)
        }

        // ── Windows ──
        ("GET", ["window"]) => Ok(json!(session.window)),
        ("GET", ["window", "handles"]) => Ok(json!(window_handles(&context.app))),
        ("POST", ["window"]) => {
            let handle = body_str(body, "handle")?;
            let window = require_window(context, &handle)?;
            let _ = window.set_focus();
            update_session(context, |session| session.window = handle);
            Ok(Value::Null)
        }
        ("DELETE", ["window"]) => {
            if session.window == "main" {
                return Err(WdError::new(
                    "unsupported operation",
                    "closing the main window would quit the app; use DELETE /session instead",
                ));
            }
            let window = require_window(context, &session.window)?;
            window
                .close()
                .map_err(|e| unknown_error(format!("close window: {e}")))?;
            update_session(context, |session| session.window = "main".into());
            // Closing is dispatched to the event loop, so give the handle a
            // moment to disappear before reporting what is left.
            let deadline = Instant::now() + Duration::from_millis(750);
            let closed = session.window.clone();
            loop {
                let handles = window_handles(&context.app);
                if !handles.contains(&closed) || Instant::now() >= deadline {
                    return Ok(json!(handles));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
        ("GET", ["window", "rect"]) => window_rect(&require_window(context, &session.window)?),
        ("POST", ["window", "rect"]) => {
            let window = require_window(context, &session.window)?;
            if let (Some(width), Some(height)) = (
                body.get("width").and_then(Value::as_u64),
                body.get("height").and_then(Value::as_u64),
            ) {
                window
                    .set_size(PhysicalSize::new(width as u32, height as u32))
                    .map_err(|e| unknown_error(format!("set window size: {e}")))?;
            }
            if let (Some(x), Some(y)) = (
                body.get("x").and_then(Value::as_i64),
                body.get("y").and_then(Value::as_i64),
            ) {
                window
                    .set_position(PhysicalPosition::new(x as i32, y as i32))
                    .map_err(|e| unknown_error(format!("set window position: {e}")))?;
            }
            settled_window_rect(&window, body)
        }
        ("POST", ["window", "minimize"]) => {
            let window = require_window(context, &session.window)?;
            window
                .minimize()
                .map_err(|e| unknown_error(format!("minimize window: {e}")))?;
            window_rect(&window)
        }
        ("POST", ["window", "maximize"]) => {
            let window = require_window(context, &session.window)?;
            window
                .maximize()
                .map_err(|e| unknown_error(format!("maximize window: {e}")))?;
            window_rect(&window)
        }
        ("POST", ["window", "fullscreen"]) => {
            let window = require_window(context, &session.window)?;
            window
                .set_fullscreen(true)
                .map_err(|e| unknown_error(format!("fullscreen window: {e}")))?;
            window_rect(&window)
        }

        // ── Elements ──
        ("POST", ["element"]) => {
            let (using, value) = locator(body)?;
            eval_in_session(
                context,
                session,
                &find_body(session, &using, &value, None, false),
            )
        }
        ("POST", ["elements"]) => {
            let (using, value) = locator(body)?;
            eval_in_session(
                context,
                session,
                &find_body(session, &using, &value, None, true),
            )
        }
        ("POST", ["element", id, "element"]) => {
            let (using, value) = locator(body)?;
            eval_in_session(
                context,
                session,
                &find_body(session, &using, &value, Some(id), false),
            )
        }
        ("POST", ["element", id, "elements"]) => {
            let (using, value) = locator(body)?;
            eval_in_session(
                context,
                session,
                &find_body(session, &using, &value, Some(id), true),
            )
        }
        ("GET", ["element", "active"]) => eval_in_session(
            context,
            session,
            "const el = document.activeElement; \
             if (!el) throw WD.fail('no such element', 'no active element'); \
             return WD.ref(el);",
        ),
        ("GET", ["element", id, "text"]) => element_get(
            context,
            session,
            id,
            "return el.innerText !== undefined ? el.innerText : (el.textContent || '');",
        ),
        ("GET", ["element", id, "name"]) => {
            element_get(context, session, id, "return el.tagName.toLowerCase();")
        }
        ("GET", ["element", id, "rect"]) => {
            element_get(context, session, id, "return WD.rect(el);")
        }
        ("GET", ["element", id, "displayed"]) => {
            element_get(context, session, id, "return WD.displayed(el);")
        }
        ("GET", ["element", id, "enabled"]) => {
            element_get(context, session, id, "return WD.enabled(el);")
        }
        ("GET", ["element", id, "selected"]) => element_get(
            context,
            session,
            id,
            "return el.selected === true || el.checked === true;",
        ),
        ("GET", ["element", id, "attribute", name]) => element_get(
            context,
            session,
            id,
            &render(
                "const value = el.getAttribute($NAME$); \
                 if (value !== null) return value; \
                 return el[$NAME$] === true ? 'true' : null;",
                &[("NAME", &lit(name))],
            ),
        ),
        ("GET", ["element", id, "property", name]) => element_get(
            context,
            session,
            id,
            &render(
                "const value = el[$NAME$]; return value === undefined ? null : value;",
                &[("NAME", &lit(name))],
            ),
        ),
        ("GET", ["element", id, "css", name]) => element_get(
            context,
            session,
            id,
            &render(
                "return window.getComputedStyle(el).getPropertyValue($NAME$);",
                &[("NAME", &lit(name))],
            ),
        ),
        ("POST", ["element", id, "click"]) => {
            element_get(context, session, id, "WD.click(el); return null;")
        }
        ("POST", ["element", id, "clear"]) => {
            element_get(context, session, id, "WD.clear(el); return null;")
        }
        ("POST", ["element", id, "value"]) => {
            let text = match body.get("text").and_then(Value::as_str) {
                Some(text) => text.to_string(),
                // Older clients send `value` as an array of key chunks.
                None => body
                    .get("value")
                    .and_then(Value::as_array)
                    .map(|chunks| {
                        chunks
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .ok_or_else(|| invalid_argument("missing string field \"text\""))?,
            };
            element_get(
                context,
                session,
                id,
                &render(
                    "WD.sendKeys(el, $TEXT$); return null;",
                    &[("TEXT", &lit(&text))],
                ),
            )
        }

        // ── Scripts ──
        ("POST", ["execute", "sync"]) => {
            let script = body_str(body, "script")?;
            let args = body.get("args").cloned().unwrap_or_else(|| json!([]));
            eval_in_session(
                context,
                session,
                &render(
                    EXECUTE_SYNC_JS,
                    &[
                        ("ARGS", &lit(&args)),
                        ("SCRIPT", &lit(&script)),
                        ("ASYNC_FUNCTION", ASYNC_FUNCTION_JS),
                    ],
                ),
            )
        }
        ("POST", ["execute", "async"]) => {
            let script = body_str(body, "script")?;
            let args = body.get("args").cloned().unwrap_or_else(|| json!([]));
            eval_in_session(
                context,
                session,
                &render(
                    EXECUTE_ASYNC_JS,
                    &[
                        ("ARGS", &lit(&args)),
                        ("SCRIPT", &lit(&script)),
                        ("TIMEOUT", &session.script_timeout_ms.to_string()),
                        ("ASYNC_FUNCTION", ASYNC_FUNCTION_JS),
                    ],
                ),
            )
        }

        // ── app-ly extensions ──
        ("GET", ["app-ly", "windows"]) => app_ly_windows(context),
        ("GET", ["app-ly", "logs"]) => app_ly_logs(context, request),
        ("POST", ["app-ly", "devtools"]) => {
            let window = require_window(context, &session.window)?;
            if window.is_devtools_open() {
                window.close_devtools();
                Ok(json!({ "open": false }))
            } else {
                window.open_devtools();
                Ok(json!({ "open": true }))
            }
        }

        // ── Explicitly out of scope ──
        ("GET", ["screenshot"]) | ("GET", ["element", _, "screenshot"]) => Err(WdError::new(
            "unsupported operation",
            "the shell has no webview capture API; take a screenshot from the OS instead",
        )),
        (_, ["alert_text", ..]) | (_, ["cookie", ..]) | (_, ["frame", ..]) | (_, ["actions"]) => {
            Err(WdError::new(
                "unsupported operation",
                format!(
                    "/{} is not implemented by this endpoint",
                    segments.join("/")
                ),
            ))
        }

        _ => Err(WdError::new(
            "unknown command",
            format!("{method} /session/{{id}}/{}", segments.join("/")),
        )),
    }
}

fn locator(body: &Value) -> Result<(String, String), WdError> {
    Ok((body_str(body, "using")?, body_str(body, "value")?))
}

/// Runs `body` with the element bound to `el`, so every per-element command
/// shares one deref (and one stale-reference check).
fn element_get(
    context: &Context,
    session: &Session,
    id: &str,
    body: &str,
) -> Result<Value, WdError> {
    let code = render(
        "const el = WD.deref($ID$);\n$BODY$",
        &[("ID", &lit(&id)), ("BODY", body)],
    );
    eval_in_session(context, session, &code)
}

fn app_ly_windows(context: &Context) -> Result<Value, WdError> {
    let mut windows = Vec::new();
    for handle in window_handles(&context.app) {
        let Some(window) = context.app.get_webview_window(&handle) else {
            continue;
        };
        windows.push(json!({
            "handle": handle,
            "title": window.title().unwrap_or_default(),
            "url": window.url().map(|url| url.to_string()).unwrap_or_default(),
            "visible": window.is_visible().unwrap_or(false),
            "focused": window.is_focused().unwrap_or(false),
            "minimized": window.is_minimized().unwrap_or(false),
            "devtoolsOpen": window.is_devtools_open(),
            "rect": window_rect(&window).unwrap_or(Value::Null),
        }));
    }
    Ok(json!(windows))
}

/// Tail of the app's `shell.log`, so a driver can read what the app logged
/// without reaching into `dataPath` itself.
fn app_ly_logs(context: &Context, request: &Request) -> Result<Value, WdError> {
    let lines: usize = request
        .query
        .get("lines")
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);

    let path = context
        .app
        .state::<ShellState>()
        .data_root
        .join("logs/shell.log");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(unknown_error(format!("read log: {error}"))),
    };

    let all: Vec<&str> = text.lines().collect();
    let tail = all[all.len().saturating_sub(lines)..].to_vec();
    Ok(json!({ "path": path.to_string_lossy(), "lines": tail }))
}

// ── HTTP server ─────────────────────────────────────────────────────

/// Compares in constant time. The token exists for the case where the endpoint
/// is reachable from off-box, and there a `==` that returns early on the first
/// wrong byte lets an attacker walk the secret out one character at a time.
fn secret_eq(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    // Length is not the secret, so branching on it is fine; the content compare
    // below still visits every byte either way.
    let mut difference = u8::from(left.len() != right.len());
    for index in 0..left.len().max(right.len()) {
        let l = left.get(index).copied().unwrap_or(0);
        let r = right.get(index).copied().unwrap_or(0);
        difference |= l ^ r;
    }
    difference == 0
}

/// Strips one `Bearer` scheme, matched case-insensitively as RFC 7235 requires.
/// A bare token with no scheme is accepted too — it is what `curl -H` users
/// tend to send, and there is no other scheme to confuse it with.
fn presented_token(value: &str) -> &str {
    let value = value.trim();
    match value.get(..7) {
        Some(scheme) if scheme.eq_ignore_ascii_case("bearer ") => value[7..].trim_start(),
        _ => value,
    }
}

fn authorized(settings: &WebDriverSettings, request: &tiny_http::Request) -> bool {
    let Some(expected) = &settings.token else {
        return true;
    };
    request.headers().iter().any(|header| {
        let field = header.field.as_str().as_str().to_ascii_lowercase();
        let value = header.value.as_str();
        match field.as_str() {
            "authorization" => secret_eq(presented_token(value), expected),
            "x-auth-token" => secret_eq(value.trim(), expected),
            _ => false,
        }
    })
}

fn respond(request: tiny_http::Request, status: u16, body: &Value) {
    let payload = serde_json::to_string(body).unwrap_or_else(|_| "{}".into());
    let response = tiny_http::Response::from_string(payload)
        .with_status_code(status)
        .with_header(
            tiny_http::Header::from_bytes(
                &b"Content-Type"[..],
                &b"application/json; charset=utf-8"[..],
            )
            .expect("static header"),
        )
        .with_header(
            tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..])
                .expect("static header"),
        );
    let _ = request.respond(response);
}

/// Starts the endpoint on a background thread. A bind failure is reported and
/// swallowed: the app is still usable without automation, and taking the
/// window down over an occupied port would be worse than losing the endpoint.
pub fn start(app: AppHandle, app_name: String, settings: WebDriverSettings) {
    if !settings.enabled {
        return;
    }

    let address = format!("{}:{}", settings.host, settings.port);
    let server = match tiny_http::Server::http(&address) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("webdriver: could not listen on {address}: {error}");
            return;
        }
    };

    if !is_loopback(&settings.host) && settings.token.is_none() {
        eprintln!(
            "webdriver: WARNING — listening on {address} with no token. \
             Anyone who can reach this port can run arbitrary JavaScript in the app."
        );
    }

    eprintln!("webdriver: listening on http://{address} (POST /session to start)");

    let context = Arc::new(Context {
        app,
        app_name,
        settings,
        session: Mutex::new(None),
    });

    thread::spawn(move || {
        // Commands are handled one at a time, which is what the protocol
        // expects of a session anyway, and keeps the eval bridge honest.
        for mut request in server.incoming_requests() {
            let method = request.method().to_string();
            let target = request.url().to_string();

            if method == "OPTIONS" {
                respond(request, 200, &json!({ "value": null }));
                continue;
            }

            if !authorized(&context.settings, &request) {
                // The spec has no auth error code; 401 plus a plain message
                // says more than borrowing an unrelated one.
                let error = unknown_error("missing or invalid webdriver token");
                respond(request, 401, &error.body());
                continue;
            }

            let mut raw = String::new();
            if request.as_reader().read_to_string(&mut raw).is_err() {
                let error = invalid_argument("could not read the request body");
                respond(request, error.http_status(), &error.body());
                continue;
            }

            let body = if raw.trim().is_empty() {
                json!({})
            } else {
                match serde_json::from_str::<Value>(&raw) {
                    Ok(body) => body,
                    Err(error) => {
                        let error = invalid_argument(format!("invalid JSON body: {error}"));
                        respond(request, error.http_status(), &error.body());
                        continue;
                    }
                }
            };

            let (segments, query) = parse_target(&target);
            let parsed = Request {
                method,
                segments,
                query,
                body,
            };

            match dispatch(&context, &parsed) {
                Ok(value) => respond(request, 200, &json!({ "value": value })),
                Err(error) => respond(request, error.http_status(), &error.body()),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> WebDriverCliOverrides {
        parse_cli_overrides(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn cli_flag_enables_the_endpoint() {
        let overrides = cli(&["app-ly", "--webdriver"]);
        assert_eq!(overrides.enabled, Some(true));
        assert_eq!(overrides.port, None);
    }

    #[test]
    fn cli_accepts_split_and_inline_values() {
        assert_eq!(
            cli(&["app-ly", "--webdriver-port", "9515"]).port,
            Some(9515)
        );
        assert_eq!(cli(&["app-ly", "--webdriver-port=9515"]).port, Some(9515));
        assert_eq!(cli(&["app-ly", "--webdriver=9515"]).port, Some(9515));
        assert_eq!(
            cli(&["app-ly", "--webdriver-host=0.0.0.0"]).host.as_deref(),
            Some("0.0.0.0")
        );
    }

    #[test]
    fn a_port_alone_implies_enabled_but_no_webdriver_still_wins() {
        assert_eq!(
            cli(&["app-ly", "--webdriver-port", "9515"]).enabled,
            Some(true)
        );
        let off = cli(&["app-ly", "--webdriver-port", "9515", "--no-webdriver"]);
        assert_eq!(off.enabled, Some(false));
        assert_eq!(off.port, Some(9515));
    }

    #[test]
    fn config_table_without_enabled_turns_it_on() {
        let config = WebDriverConfig {
            enabled: None,
            host: None,
            port: Some(5555),
            token: None,
        };
        let settings = resolve_settings(Some(&config), &WebDriverCliOverrides::default());
        assert!(settings.enabled);
        assert_eq!(settings.port, 5555);
        assert_eq!(settings.host, DEFAULT_HOST);
    }

    #[test]
    fn absent_config_leaves_it_off() {
        let settings = resolve_settings(None, &WebDriverCliOverrides::default());
        assert!(!settings.enabled);
        assert_eq!(settings.port, DEFAULT_PORT);
    }

    #[test]
    fn cli_overrides_beat_the_config_file() {
        let config = WebDriverConfig {
            enabled: Some(true),
            host: Some("127.0.0.1".into()),
            port: Some(4444),
            token: Some("from-config".into()),
        };
        let settings =
            resolve_settings(Some(&config), &cli(&["app-ly", "--webdriver-port", "9999"]));
        assert_eq!(settings.port, 9999);
        assert_eq!(settings.token.as_deref(), Some("from-config"));

        let off = resolve_settings(Some(&config), &cli(&["app-ly", "--no-webdriver"]));
        assert!(!off.enabled);
    }

    #[test]
    fn error_codes_map_to_the_w3c_http_statuses() {
        assert_eq!(WdError::new("no such element", "").http_status(), 404);
        assert_eq!(WdError::new("invalid argument", "").http_status(), 400);
        assert_eq!(WdError::new("unknown method", "").http_status(), 405);
        assert_eq!(WdError::new("javascript error", "").http_status(), 500);
        assert_eq!(WdError::new("script timeout", "").http_status(), 500);
        assert_eq!(
            WdError::new("stale element reference", "").http_status(),
            404
        );
    }

    #[test]
    fn targets_split_into_segments_and_query() {
        let (segments, query) = parse_target("/session/abc/element/e1/attribute/data-id?lines=10");
        assert_eq!(
            segments,
            vec!["session", "abc", "element", "e1", "attribute", "data-id"]
        );
        assert_eq!(query.get("lines").map(String::as_str), Some("10"));

        let (segments, query) = parse_target("/status");
        assert_eq!(segments, vec!["status"]);
        assert!(query.is_empty());
    }

    #[test]
    fn percent_escapes_survive_the_path() {
        let (segments, _) = parse_target("/session/a/element/e1/css/background%2Dcolor");
        assert_eq!(
            segments.last().map(String::as_str),
            Some("background-color")
        );
    }

    #[test]
    fn render_substitutes_only_template_placeholders() {
        assert_eq!(render("a $X$ b", &[("X", "1")]), "a 1 b");
        // A substituted value that itself looks like a placeholder is left alone.
        assert_eq!(render("$X$", &[("X", "$X$"), ("Y", "2")]), "$X$");
        // Unknown placeholders and lone dollars pass through untouched.
        assert_eq!(
            render("cost: $5 and $Y$", &[("X", "1")]),
            "cost: $5 and $Y$"
        );
    }

    #[test]
    fn find_body_embeds_the_locator_as_json() {
        let session = Session {
            id: "s".into(),
            window: "main".into(),
            script_timeout_ms: 30_000,
            page_load_timeout_ms: 300_000,
            implicit_wait_ms: 500,
        };
        let body = find_body(&session, "css selector", "a[href=\"x\"]", None, false);
        assert!(body.contains("Date.now() + 500"));
        assert!(body.contains(r#""css selector""#));
        assert!(body.contains(r#""a[href=\"x\"]""#));
        assert!(body.contains("document"));
        assert!(!body.contains("$USING$"));
    }

    #[test]
    fn find_body_scopes_to_an_element_when_given_one() {
        let session = Session {
            id: "s".into(),
            window: "main".into(),
            script_timeout_ms: 30_000,
            page_load_timeout_ms: 300_000,
            implicit_wait_ms: 0,
        };
        let body = find_body(&session, "tag name", "li", Some("e7"), true);
        assert!(body.contains(r#"WD.deref("e7")"#));
        assert!(body.contains("found.map(WD.ref)"));
    }

    #[test]
    fn a_bearer_scheme_is_stripped_exactly_once() {
        assert_eq!(presented_token("Bearer s3cret"), "s3cret");
        assert_eq!(presented_token("bearer s3cret"), "s3cret");
        assert_eq!(presented_token("BEARER  s3cret"), "s3cret");
        // A bare token still works.
        assert_eq!(presented_token("  s3cret  "), "s3cret");
        // Only one scheme comes off, so a token that starts with the word
        // survives instead of being eaten.
        assert_eq!(presented_token("Bearer Bearer s3cret"), "Bearer s3cret");
    }

    #[test]
    fn secret_comparison_matches_only_the_exact_token() {
        assert!(secret_eq("s3cret", "s3cret"));
        assert!(!secret_eq("s3cret", "s3crey"));
        assert!(!secret_eq("s3cret", "s3cre"));
        assert!(!secret_eq("s3cret", "s3crett"));
        assert!(!secret_eq("", "s3cret"));
        // A prefix of nul bytes must not pass the padded compare.
        assert!(!secret_eq("s3cret\u{0}", "s3cret"));
        assert!(secret_eq("", ""));
    }

    #[test]
    fn loopback_hosts_are_recognised() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("localhost"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("0.0.0.0"));
        assert!(!is_loopback("192.168.1.10"));
    }
}
