use chrono::Local;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use tauri::{AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, State};
use tauri_plugin_notification::NotificationExt;

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
pub fn shell_notify(app: AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
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
            let is_primary = primary.as_ref().is_some_and(|primary| monitors_equal(primary, monitor));
            let is_current = current.as_ref().is_some_and(|current| monitors_equal(current, monitor));
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
        primary.as_ref().is_some_and(|primary| monitors_equal(primary, &monitor)),
        current.as_ref().is_some_and(|current| monitors_equal(current, &monitor)),
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
        status_text: status
            .canonical_reason()
            .unwrap_or("Unknown")
            .to_string(),
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