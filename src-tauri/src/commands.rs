use chrono::Local;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{
    AppHandle, Emitter, Manager, Monitor, PhysicalPosition, PhysicalSize, State, Url, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri::webview::PageLoadEvent;

#[derive(Clone)]
pub struct ShellState {
    pub data_root: PathBuf,
}

fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err("invalid file name".into());
    }
    Ok(())
}

fn data_file_path(state: &ShellState, name: &str) -> Result<PathBuf, String> {
    validate_filename(name)?;
    let path = state.data_root.join(name);
    if !path.starts_with(&state.data_root) {
        return Err("path escape".into());
    }
    Ok(path)
}

#[tauri::command]
pub fn shell_save_file(
    state: State<'_, ShellState>,
    name: String,
    contents: String,
) -> Result<(), String> {
    let path = data_file_path(&state, &name)?;
    std::fs::write(path, contents).map_err(|e| format!("write file: {e}"))
}

#[tauri::command]
pub fn shell_read_file(state: State<'_, ShellState>, name: String) -> Result<String, String> {
    let path = data_file_path(&state, &name)?;
    std::fs::read_to_string(path).map_err(|e| format!("read file: {e}"))
}

#[tauri::command]
pub fn shell_delete_file(state: State<'_, ShellState>, name: String) -> Result<(), String> {
    let path = data_file_path(&state, &name)?;
    std::fs::remove_file(path).map_err(|e| format!("delete file: {e}"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_rename_file(
    state: State<'_, ShellState>,
    name: String,
    new_name: String,
) -> Result<(), String> {
    let from = data_file_path(&state, &name)?;
    let to = data_file_path(&state, &new_name)?;
    std::fs::rename(from, to).map_err(|e| format!("rename file: {e}"))
}

fn require_exists(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        Ok(())
    } else {
        Err("file not found".into())
    }
}

/// "Open"/"reveal" have no cross-platform API in std — shell out to each
/// platform's own file-opener rather than pull in a plugin for two verbs.

#[cfg(target_os = "macos")]
fn open_path(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open file: {e}"))
}

#[cfg(target_os = "macos")]
fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("reveal file: {e}"))
}

#[cfg(target_os = "windows")]
fn open_path(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open file: {e}"))
}

#[cfg(target_os = "windows")]
fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("reveal file: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_path(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open file: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reveal_path(path: &std::path::Path) -> Result<(), String> {
    // No universal "select in file manager" verb on Linux; open the enclosing folder instead.
    let dir = path.parent().unwrap_or(path);
    std::process::Command::new("xdg-open")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("reveal file: {e}"))
}

#[tauri::command]
pub fn shell_open_file(state: State<'_, ShellState>, name: String) -> Result<(), String> {
    let path = data_file_path(&state, &name)?;
    require_exists(&path)?;
    open_path(&path)
}

#[tauri::command]
pub fn shell_open_file_location(state: State<'_, ShellState>, name: String) -> Result<(), String> {
    let path = data_file_path(&state, &name)?;
    require_exists(&path)?;
    reveal_path(&path)
}

#[tauri::command]
pub fn shell_log(
    state: State<'_, ShellState>,
    message: String,
    level: Option<String>,
) -> Result<(), String> {
    let level = level.unwrap_or_else(|| "info".to_string());
    let log_path = state.data_root.join("logs/shell.log");
    let line = format!(
        "{} [{}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        level,
        message
    );

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| format!("open log: {e}"))?;
    file.write_all(line.as_bytes())
        .map_err(|e| format!("write log: {e}"))
}

#[tauri::command]
pub fn shell_notify(title: String, body: String) -> Result<(), String> {
    notify_rust::Notification::new()
        .summary(&title)
        .body(&body)
        .show()
        .map(|_| ())
        .map_err(|e| format!("notify: {e}"))
}

#[derive(Debug, Serialize)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

fn main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window("main")
        .ok_or_else(|| "main window not found".into())
}

#[tauri::command]
pub fn shell_get_window_position(app: AppHandle) -> Result<WindowPosition, String> {
    let window = main_window(&app)?;
    let position = window
        .outer_position()
        .map_err(|e| format!("get window position: {e}"))?;
    Ok(WindowPosition {
        x: position.x,
        y: position.y,
    })
}

#[tauri::command]
pub fn shell_set_window_position(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let window = main_window(&app)?;
    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|e| format!("set window position: {e}"))
}

#[tauri::command]
pub fn shell_get_window_size(app: AppHandle) -> Result<WindowSize, String> {
    let window = main_window(&app)?;
    let size = window
        .inner_size()
        .map_err(|e| format!("get window size: {e}"))?;
    Ok(WindowSize {
        width: size.width,
        height: size.height,
    })
}

#[tauri::command]
pub fn shell_set_window_size(app: AppHandle, width: u32, height: u32) -> Result<(), String> {
    let window = main_window(&app)?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|e| format!("set window size: {e}"))
}

#[tauri::command]
pub fn shell_minimize_window(app: AppHandle) -> Result<(), String> {
    let window = main_window(&app)?;
    window
        .minimize()
        .map_err(|e| format!("minimize window: {e}"))
}

#[derive(Debug, Serialize)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize)]
pub struct ScreenSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct ShellScreen {
    pub name: Option<String>,
    pub size: ScreenSize,
    pub position: ScreenPoint,
    #[serde(rename = "workArea")]
    pub work_area: ScreenRect,
    #[serde(rename = "scaleFactor")]
    pub scale_factor: f64,
    #[serde(rename = "isPrimary")]
    pub is_primary: bool,
    #[serde(rename = "isCurrent")]
    pub is_current: bool,
}

#[derive(Debug, Serialize)]
pub struct ScreensInfo {
    pub screens: Vec<ShellScreen>,
    #[serde(rename = "primaryIndex")]
    pub primary_index: Option<usize>,
    #[serde(rename = "currentIndex")]
    pub current_index: Option<usize>,
}

fn monitors_equal(a: &Monitor, b: &Monitor) -> bool {
    a.name() == b.name()
        && a.size().width == b.size().width
        && a.size().height == b.size().height
        && a.position().x == b.position().x
        && a.position().y == b.position().y
        && a.scale_factor() == b.scale_factor()
}

fn monitor_to_screen(monitor: &Monitor, is_primary: bool, is_current: bool) -> ShellScreen {
    let size = monitor.size();
    let position = monitor.position();
    let work_area = monitor.work_area();

    ShellScreen {
        name: monitor.name().cloned(),
        size: ScreenSize {
            width: size.width,
            height: size.height,
        },
        position: ScreenPoint {
            x: position.x,
            y: position.y,
        },
        work_area: ScreenRect {
            x: work_area.position.x,
            y: work_area.position.y,
            width: work_area.size.width,
            height: work_area.size.height,
        },
        scale_factor: monitor.scale_factor(),
        is_primary,
        is_current,
    }
}

#[tauri::command]
pub fn shell_get_screens(app: AppHandle) -> Result<ScreensInfo, String> {
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("list screens: {e}"))?;
    let primary = app
        .primary_monitor()
        .map_err(|e| format!("get primary screen: {e}"))?;
    let current = main_window(&app)?
        .current_monitor()
        .map_err(|e| format!("get current screen: {e}"))?;

    let screens = monitors
        .iter()
        .map(|monitor| {
            let is_primary = primary
                .as_ref()
                .is_some_and(|primary| monitors_equal(primary, monitor));
            let is_current = current
                .as_ref()
                .is_some_and(|current| monitors_equal(current, monitor));
            monitor_to_screen(monitor, is_primary, is_current)
        })
        .collect::<Vec<_>>();

    Ok(ScreensInfo {
        primary_index: screens.iter().position(|screen| screen.is_primary),
        current_index: screens.iter().position(|screen| screen.is_current),
        screens,
    })
}

#[tauri::command]
pub fn shell_get_screen_at(app: AppHandle, x: f64, y: f64) -> Result<ShellScreen, String> {
    let monitor = app
        .monitor_from_point(x, y)
        .map_err(|e| format!("get screen at point: {e}"))?
        .ok_or_else(|| format!("no screen at ({x}, {y})"))?;
    let primary = app
        .primary_monitor()
        .map_err(|e| format!("get primary screen: {e}"))?;
    let current = main_window(&app)?
        .current_monitor()
        .map_err(|e| format!("get current screen: {e}"))?;

    Ok(monitor_to_screen(
        &monitor,
        primary
            .as_ref()
            .is_some_and(|primary| monitors_equal(primary, &monitor)),
        current
            .as_ref()
            .is_some_and(|current| monitors_equal(current, &monitor)),
    ))
}

#[derive(Debug, Serialize)]
pub struct FetchResponse {
    pub ok: bool,
    pub status: u16,
    #[serde(rename = "statusText")]
    pub status_text: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[tauri::command]
pub async fn shell_fetch(
    url: String,
    method: Option<String>,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
) -> Result<FetchResponse, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("only http and https URLs are allowed".into());
    }

    let method = method.unwrap_or_else(|| "GET".to_string());
    let client = reqwest::Client::new();
    let mut request = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "PATCH" => client.patch(&url),
        "DELETE" => client.delete(&url),
        other => return Err(format!("unsupported method: {other}")),
    };

    if let Some(headers) = headers {
        let mut header_map = HeaderMap::new();
        for (key, value) in headers {
            let name = HeaderName::from_str(&key).map_err(|e| format!("invalid header: {e}"))?;
            let value =
                HeaderValue::from_str(&value).map_err(|e| format!("invalid header value: {e}"))?;
            header_map.insert(name, value);
        }
        request = request.headers(header_map);
    }

    if let Some(body) = body {
        request = request.body(body);
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    let response_headers = response
        .headers()
        .iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect::<HashMap<_, _>>();
    let body = response
        .text()
        .await
        .map_err(|e| format!("read response: {e}"))?;

    Ok(FetchResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("Unknown").to_string(),
        headers: response_headers,
        body,
    })
}

#[tauri::command]
pub fn shell_toggle_devtools(app: AppHandle) -> Result<(), String> {
    let window = main_window(&app)?;
    if window.is_devtools_open() {
        window.close_devtools();
    } else {
        window.open_devtools();
    }
    Ok(())
}

static CHILD_WINDOW_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
pub struct OpenWindowOptions {
    pub title: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct OpenedWindow {
    pub id: String,
}

#[derive(Debug, Serialize, Clone)]
struct WindowNavigatedPayload {
    id: String,
    url: String,
}

#[derive(Debug, Serialize, Clone)]
struct WindowClosedPayload {
    id: String,
}

/// Child windows are for things a `<a target="_blank">` can't do here (CSP blocks
/// navigating the main window away, and there's no browser chrome to pop up) —
/// namely external OAuth/auth flows the JS app needs to watch and drive.
#[tauri::command]
pub fn shell_open_window(
    app: AppHandle,
    url: String,
    options: Option<OpenWindowOptions>,
) -> Result<OpenedWindow, String> {
    let parsed = Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("only http and https URLs are allowed".into());
    }

    let options = options.unwrap_or(OpenWindowOptions {
        title: None,
        width: None,
        height: None,
    });
    let label = format!(
        "shell-window-{}",
        CHILD_WINDOW_COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    let nav_app = app.clone();
    let nav_label = label.clone();
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .inner_size(
            options.width.unwrap_or(480.0),
            options.height.unwrap_or(640.0),
        )
        .on_navigation(move |url| {
            let _ = nav_app.emit_to(
                "main",
                "shell://window-navigated",
                WindowNavigatedPayload {
                    id: nav_label.clone(),
                    url: url.to_string(),
                },
            );
            true
        });

    if let Some(title) = &options.title {
        builder = builder.title(title);
    }

    let load_app = app.clone();
    let load_label = label.clone();
    builder = builder.on_page_load(move |_window, payload| {
        if payload.event() == PageLoadEvent::Finished {
            let _ = load_app.emit_to(
                "main",
                "shell://window-loaded",
                WindowNavigatedPayload {
                    id: load_label.clone(),
                    url: payload.url().to_string(),
                },
            );
        }
    });

    let window = builder.build().map_err(|e| format!("open window: {e}"))?;

    let closed_app = app.clone();
    let closed_label = label.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = closed_app.emit_to(
                "main",
                "shell://window-closed",
                WindowClosedPayload {
                    id: closed_label.clone(),
                },
            );
        }
    });

    Ok(OpenedWindow { id: label })
}

#[tauri::command]
pub fn shell_close_window(app: AppHandle, id: String) -> Result<(), String> {
    if id == "main" {
        return Err("cannot close the main window".into());
    }
    let window = app
        .get_webview_window(&id)
        .ok_or_else(|| "window not found".to_string())?;
    window.close().map_err(|e| format!("close window: {e}"))
}

// `WebviewWindow::eval_with_callback` only serializes a completion value that
// WebKit/WebView2 already resolved from a Promise, and on macOS an unresolved
// Promise bridges to a null object (empty result) rather than being awaited.
// So instead of trusting eval's own completion value, the evaluated script
// calls back into `shell_eval_result` via `invoke` once it's actually done,
// keyed by a request id, and this state holds the sender waiting on it.
#[derive(Default)]
pub struct EvalState {
    pending: std::sync::Mutex<HashMap<String, tauri::async_runtime::Sender<String>>>,
}

// Child windows can load arbitrary remote content, and the capability that
// lets them call back into `shell_eval_result` (necessarily) grants that to
// any origin — so request ids need to be unguessable, not just unique, or a
// page in one child window could spoof another window's pending eval result.
fn random_request_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(EVAL_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", hasher.finish())
}

static EVAL_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn find_window(app: &AppHandle, id: &str) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window(id)
        .ok_or_else(|| "window not found".to_string())
}

#[derive(Debug, Deserialize, Serialize)]
struct EvalOutcome {
    ok: bool,
    value: Option<serde_json::Value>,
    error: Option<String>,
}

// Runs `code` as the body of an async function in `window` and awaits the
// result reported back through `shell_eval_result`. Building the function via
// `AsyncFunction(code)` (rather than splicing `code` into the script text)
// means a syntax error in `code` throws where it's caught below, instead of
// breaking the surrounding script.
async fn eval_in_window(
    state: &EvalState,
    window: &tauri::WebviewWindow,
    code: &str,
) -> Result<serde_json::Value, String> {
    let request_id = random_request_id();
    let (tx, mut rx) = tauri::async_runtime::channel::<String>(1);
    state.pending.lock().unwrap().insert(request_id.clone(), tx);

    let code_literal = serde_json::to_string(code).map_err(|e| format!("encode eval code: {e}"))?;
    let request_id_literal = serde_json::to_string(&request_id).unwrap();
    let script = format!(
        "(function() {{ (async () => {{ \
           let payload; \
           try {{ \
             const AsyncFunction = Object.getPrototypeOf(async function(){{}}).constructor; \
             const fn = new AsyncFunction({code_literal}); \
             payload = {{ ok: true, value: await fn() }}; \
           }} catch (err) {{ \
             payload = {{ ok: false, error: String(err && err.message ? err.message : err) }}; \
           }} \
           window.__TAURI__.core.invoke('shell_eval_result', {{ requestId: {request_id_literal}, payload: JSON.stringify(payload) }}); \
         }})(); }})()"
    );

    if let Err(e) = window.eval(script) {
        state.pending.lock().unwrap().remove(&request_id);
        return Err(format!("eval: {e}"));
    }

    let raw = rx
        .recv()
        .await
        .ok_or_else(|| "eval: window closed before returning a result".to_string())?;
    let outcome: EvalOutcome =
        serde_json::from_str(&raw).map_err(|e| format!("parse eval result: {e}"))?;
    if outcome.ok {
        Ok(outcome.value.unwrap_or(serde_json::Value::Null))
    } else {
        Err(outcome.error.unwrap_or_else(|| "eval failed".into()))
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_eval_result(state: State<'_, EvalState>, request_id: String, payload: String) {
    if let Some(tx) = state.pending.lock().unwrap().remove(&request_id) {
        let _ = tx.try_send(payload);
    }
}

// document.body.innerText is always a plain synchronous string, never a
// Promise, so the WKWebView "unresolved-Promise-bridges-to-null" issue that
// forced evalWindow onto the invoke round-trip doesn't apply here — this can
// use eval_with_callback directly and skip the ACL-gated bridge entirely.
#[tauri::command]
pub async fn shell_get_window_body(app: AppHandle, id: String) -> Result<String, String> {
    let window = find_window(&app, &id)?;
    let (tx, mut rx) = tauri::async_runtime::channel::<String>(1);
    window
        .eval_with_callback(
            "document.body ? document.body.innerText : ''",
            move |result| {
                let _ = tx.try_send(result);
            },
        )
        .map_err(|e| format!("eval: {e}"))?;
    let raw = rx
        .recv()
        .await
        .ok_or_else(|| "eval: window closed before returning a result".to_string())?;
    Ok(serde_json::from_str::<serde_json::Value>(&raw)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default())
}

#[tauri::command]
pub async fn shell_eval_window(
    app: AppHandle,
    state: State<'_, EvalState>,
    id: String,
    code: String,
) -> Result<serde_json::Value, String> {
    let window = find_window(&app, &id)?;
    eval_in_window(&state, &window, &code).await
}

fn percent_encode_return_url(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn write_auth_callback_response(stream: &mut std::net::TcpStream) {
    let body = "<!DOCTYPE html><html><body><p>Sign-in complete. You can close this tab and return to the desktop app.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn parse_auth_code_from_http(stream: &mut std::net::TcpStream) -> Result<Option<String>, String> {
    use std::io::Read;

    let mut buffer = [0u8; 4096];
    let read = stream
        .read(&mut buffer)
        .map_err(|e| format!("read callback request: {e}"))?;
    if read == 0 {
        return Ok(None);
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let _method = parts.next();
    let target = parts.next().unwrap_or_default();
    let path = target.split('#').next().unwrap_or(target);
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();

    let mut auth_code = None;
    let mut error = None;
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key == "authCode" && !value.is_empty() {
            auth_code = Some(value.to_string());
        } else if key == "error" && !value.is_empty() {
            error = Some(value.to_string());
        }
    }

    write_auth_callback_response(stream);

    if let Some(err) = error {
        return Err(format!("authentication error: {err}"));
    }

    Ok(auth_code)
}

fn is_loopback_callback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]" || host == "::1"
}

fn bind_host_for_callback(host: &str) -> &str {
    if host == "[::1]" || host == "::1" {
        "[::1]"
    } else {
        "127.0.0.1"
    }
}

fn resolve_auth_callback(return_url: Option<String>) -> Result<(std::net::TcpListener, String), String> {
    use std::net::TcpListener;

    let return_url = match return_url {
        Some(url) if !url.trim().is_empty() => url.trim().to_string(),
        _ => {
            let listener = TcpListener::bind("127.0.0.1:0")
                .map_err(|e| format!("bind callback listener: {e}"))?;
            let port = listener
                .local_addr()
                .map_err(|e| format!("callback listener addr: {e}"))?
                .port();
            return Ok((listener, format!("http://127.0.0.1:{port}/callback")));
        }
    };

    let parsed = Url::parse(&return_url).map_err(|e| format!("invalid returnUrl: {e}"))?;
    if parsed.scheme() != "http" {
        return Err("returnUrl must use http".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "returnUrl must include a host".to_string())?;
    if !is_loopback_callback_host(host) {
        return Err("returnUrl host must be localhost or 127.0.0.1".into());
    }
    let port = parsed
        .port()
        .ok_or_else(|| "returnUrl must include an explicit port".to_string())?;
    if port == 0 {
        return Err("returnUrl port cannot be 0".into());
    }

    let bind_addr = format!("{}:{port}", bind_host_for_callback(host));
    let listener = TcpListener::bind(&bind_addr).map_err(|e| {
        format!("bind callback listener on {bind_addr} (returnUrl={return_url}): {e}")
    })?;

    Ok((listener, return_url))
}

fn auth_via_browser_blocking(
    auth_url: &str,
    timeout_ms: u64,
    return_url: Option<String>,
) -> Result<String, String> {
    use std::time::{Duration, Instant};

    let (listener, return_url) = resolve_auth_callback(return_url)?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("callback listener nonblocking: {e}"))?;

    let sep = if auth_url.contains('?') { '&' } else { '?' };
    let browser_url = format!(
        "{auth_url}{sep}returnUrl={}",
        percent_encode_return_url(&return_url)
    );

    open::that(&browser_url).map_err(|e| format!("open browser: {e}"))?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if Instant::now() >= deadline {
            return Err("authentication timed out waiting for browser callback".into());
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                match parse_auth_code_from_http(&mut stream)? {
                    Some(code) => return Ok(code),
                    None => continue,
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(err) => return Err(format!("callback accept: {err}")),
        }
    }
}

/// Opens the system browser for SAML auth and waits for the backend redirect to a
/// transient loopback listener. Returns the one-time authCode for token exchange.
#[tauri::command(rename_all = "camelCase")]
pub async fn shell_auth_via_browser(
    auth_url: String,
    timeout_ms: Option<u64>,
    return_url: Option<String>,
) -> Result<String, String> {
    let timeout = timeout_ms.unwrap_or(120_000);
    tauri::async_runtime::spawn_blocking(move || {
        auth_via_browser_blocking(&auth_url, timeout, return_url)
    })
    .await
    .map_err(|e| format!("auth browser task: {e}"))?
}
