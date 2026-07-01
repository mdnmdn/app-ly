use crate::config::ShellConfig;
use std::path::{Path, PathBuf};
use tauri::{App, Manager};

/// Folder where a deployed `app.toml` lives: next to the `.app` on macOS,
/// or the directory containing the executable elsewhere.
pub fn deploy_folder() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("resolve executable: {e}"))?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;

    #[cfg(target_os = "macos")]
    {
        let in_macos_dir = exe_dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "MacOS");
        let in_app_bundle = exe_dir
            .parent()
            .and_then(|contents| contents.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "Contents");

        if in_macos_dir && in_app_bundle {
            // MacOS → Contents → app-ly.app → folder containing the .app bundle
            let deploy_dir = exe_dir
                .parent()
                .and_then(|contents| contents.parent())
                .and_then(|app_bundle| app_bundle.parent());
            if let Some(deploy_dir) = deploy_dir {
                return Ok(deploy_dir.to_path_buf());
            }
        }
    }

    Ok(exe_dir.to_path_buf())
}

#[derive(Debug, Clone)]
pub struct ResolvedPaths {
    pub icon: PathBuf,
    pub _contents: PathBuf,
    pub contents_dir: PathBuf,
    pub entry_filename: String,
    pub data_root: PathBuf,
}

pub fn resolve_paths(
    app: &App,
    config: &ShellConfig,
    config_dir: &Path,
) -> Result<ResolvedPaths, String> {
    let icon = config_dir.join(&config.icon);
    let contents = config_dir.join(&config.contents);
    let contents_dir = contents
        .parent()
        .ok_or_else(|| "contents path has no parent".to_string())?
        .to_path_buf();
    let entry_filename = contents
        .file_name()
        .ok_or_else(|| "contents path has no file name".to_string())?
        .to_string_lossy()
        .to_string();

    let data_root = if cfg!(debug_assertions) {
        config_dir.join(&config.data_path)
    } else {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("resolve app data dir: {e}"))?;
        app_data.join(&config.data_path)
    };

    std::fs::create_dir_all(&data_root).map_err(|e| format!("create data dir: {e}"))?;
    std::fs::create_dir_all(data_root.join("logs")).map_err(|e| format!("create logs dir: {e}"))?;

    Ok(ResolvedPaths {
        icon,
        _contents: contents,
        contents_dir,
        entry_filename,
        data_root,
    })
}
