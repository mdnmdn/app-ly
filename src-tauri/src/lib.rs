mod commands;
mod config;
mod db;
mod menu;
mod paths;

use commands::{
    shell_auth_via_browser, shell_close_window, shell_delete_file, shell_eval_result, shell_eval_window, shell_fetch,
    shell_get_screen_at, shell_get_screens, shell_get_window_body, shell_get_window_position,
    shell_get_window_size, shell_log, shell_minimize_window, shell_notify, shell_open_file,
    shell_open_file_location, shell_open_window, shell_read_file, shell_rename_file,
    shell_save_file, shell_set_window_position, shell_set_window_size, shell_toggle_devtools,
    EvalState, ShellState,
};
use config::{
    config_fallback_html, default_show_dev_menu, discover_config, effective_show_dev_menu,
    load_settings, missing_config_message, DiscoverError,
};
use db::{shell_db_execute, shell_db_query};
use http::{header::CONTENT_TYPE, Response, StatusCode};
use paths::resolve_paths;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn shell_init_script(show_dev_menu: bool, settings: &HashMap<String, String>) -> String {
    let settings_json = serde_json::to_string(settings).unwrap_or_else(|_| "{}".into());
    format!(
        "window.__SHELL_DEV__ = {};\nwindow.__SHELL_SETTINGS__ = {};\n{}\n{}",
        show_dev_menu,
        settings_json,
        include_str!("../scripts/shell-api.js"),
        include_str!("../scripts/shell-shortcuts.js"),
    )
}

#[derive(Clone)]
struct ProtocolState {
    contents_dir: PathBuf,
    fallback_html: Option<String>,
}

fn load_window_icon(path: &std::path::Path) -> Result<tauri::image::Image<'static>, String> {
    let image = image::open(path).map_err(|e| format!("open icon: {e}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok(tauri::image::Image::new_owned(
        rgba.into_raw(),
        width,
        height,
    ))
}

fn mime_for_path(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn serve_shell_request(state: &ProtocolState, request_path: &str) -> Response<Vec<u8>> {
    let path = request_path.trim_start_matches('/');

    if let Some(html) = &state.fallback_html {
        if path.is_empty() || path == "index.html" {
            return Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, "text/html")
                .body(html.as_bytes().to_vec())
                .unwrap();
        }

        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap();
    }

    let requested = if path.is_empty() {
        state.contents_dir.join("index.html")
    } else {
        state.contents_dir.join(path)
    };

    let canonical_root =
        std::fs::canonicalize(&state.contents_dir).unwrap_or_else(|_| state.contents_dir.clone());
    let file_path = match std::fs::canonicalize(&requested) {
        Ok(path) if path.starts_with(&canonical_root) => path,
        _ => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Vec::new())
                .unwrap();
        }
    };

    if !file_path.is_file() {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap();
    }

    let bytes = match std::fs::read(&file_path) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Vec::new())
                .unwrap();
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime_for_path(&file_path))
        .body(bytes)
        .unwrap()
}

struct StartupPlan {
    window_title: String,
    entry_filename: String,
    icon: Option<PathBuf>,
    data_root: PathBuf,
    contents_dir: PathBuf,
    fallback_html: Option<String>,
    show_dev_menu: bool,
    settings: HashMap<String, String>,
}

fn fallback_data_root(app: &tauri::App) -> Result<PathBuf, String> {
    let data_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&data_root).map_err(|e| format!("create data dir: {e}"))?;
    std::fs::create_dir_all(data_root.join("logs")).map_err(|e| format!("create logs dir: {e}"))?;
    Ok(data_root)
}

fn plan_startup(app: &tauri::App) -> Result<StartupPlan, String> {
    let discovery = match discover_config(app) {
        Ok(discovery) => discovery,
        Err(DiscoverError::Missing(searched)) => {
            let message = missing_config_message(&searched);
            eprintln!("{message}");
            return Ok(StartupPlan {
                window_title: "Missing app.toml".into(),
                entry_filename: "index.html".into(),
                icon: None,
                data_root: fallback_data_root(app)?,
                contents_dir: PathBuf::from("."),
                fallback_html: Some(config_fallback_html("Missing app.toml", &message)),
                show_dev_menu: default_show_dev_menu(),
                settings: HashMap::new(),
            });
        }
        Err(DiscoverError::Failed(message)) => {
            eprintln!("{message}");
            return Ok(StartupPlan {
                window_title: "Config error".into(),
                entry_filename: "index.html".into(),
                icon: None,
                data_root: fallback_data_root(app)?,
                contents_dir: PathBuf::from("."),
                fallback_html: Some(config_fallback_html("Config error", &message)),
                show_dev_menu: default_show_dev_menu(),
                settings: HashMap::new(),
            });
        }
    };

    let show_dev_menu = effective_show_dev_menu(&discovery.config);

    let resolved = match resolve_paths(app, &discovery.config, &discovery.config_dir) {
        Ok(resolved) => resolved,
        Err(message) => {
            eprintln!("{message}");
            return Ok(StartupPlan {
                window_title: "Config error".into(),
                entry_filename: "index.html".into(),
                icon: None,
                data_root: fallback_data_root(app)?,
                contents_dir: PathBuf::from("."),
                fallback_html: Some(config_fallback_html("Config error", &message)),
                show_dev_menu,
                settings: HashMap::new(),
            });
        }
    };

    let settings = load_settings(&discovery.config, &discovery.config_dir);

    Ok(StartupPlan {
        window_title: discovery.config.name,
        entry_filename: resolved.entry_filename,
        icon: if resolved.icon.exists() {
            Some(resolved.icon)
        } else {
            None
        },
        data_root: resolved.data_root,
        contents_dir: resolved.contents_dir,
        fallback_html: None,
        show_dev_menu,
        settings,
    })
}

const DEFAULT_WINDOW_SCREEN_FRACTION: f64 = 0.6;

fn default_inner_size(app: &tauri::App) -> (f64, f64) {
    let Some(monitor) = app.primary_monitor().ok().flatten() else {
        return (1024.0, 768.0);
    };

    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = work_area.size.width as f64 / scale * DEFAULT_WINDOW_SCREEN_FRACTION;
    let height = work_area.size.height as f64 / scale * DEFAULT_WINDOW_SCREEN_FRACTION;
    (width, height)
}

fn create_builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    create_builder()
        .register_uri_scheme_protocol("shell", |ctx, request| {
            let state = ctx.app_handle().state::<ProtocolState>();
            serve_shell_request(&state, request.uri().path())
        })
        .setup(|app| {
            let plan = plan_startup(app)?;

            app.manage(ShellState {
                data_root: plan.data_root.clone(),
            });
            app.manage(EvalState::default());
            app.manage(ProtocolState {
                contents_dir: plan.contents_dir.clone(),
                fallback_html: plan.fallback_html.clone(),
            });

            let entry_url = format!("shell://localhost/{}", plan.entry_filename);
            let url = entry_url
                .parse()
                .map_err(|e| format!("invalid shell url: {e}"))?;

            let (width, height) = default_inner_size(app);

            let mut window_builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                    .title(&plan.window_title)
                    .inner_size(width, height)
                    .center()
                    .initialization_script(&shell_init_script(plan.show_dev_menu, &plan.settings));

            if let Some(icon_path) = &plan.icon {
                if let Ok(icon) = load_window_icon(icon_path) {
                    window_builder = window_builder.icon(icon)?;
                }
            }

            window_builder.build()?;
            menu::setup_app_menu(app, &plan.window_title, plan.show_dev_menu)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            shell_save_file,
            shell_read_file,
            shell_delete_file,
            shell_rename_file,
            shell_open_file,
            shell_open_file_location,
            shell_log,
            shell_notify,
            shell_fetch,
            shell_get_window_position,
            shell_set_window_position,
            shell_get_window_size,
            shell_set_window_size,
            shell_minimize_window,
            shell_get_screens,
            shell_get_screen_at,
            shell_db_query,
            shell_db_execute,
            shell_toggle_devtools,
            shell_open_window,
            shell_close_window,
            shell_get_window_body,
            shell_eval_window,
            shell_eval_result,
            shell_auth_via_browser
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
