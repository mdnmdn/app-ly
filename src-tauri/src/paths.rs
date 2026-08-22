use crate::config::ShellConfig;
use std::path::{Path, PathBuf};

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

/// Resolve `contents` independently of `dataPath`. Both are joined to the
/// directory that contains `app.toml`.
///
/// `contents` may be a UI directory (entry `index.html`) or an HTML file
/// (the file's parent is the UI root — kept so existing app.toml files work).
fn resolve_contents(
    config_dir: &Path,
    contents: &str,
) -> Result<(PathBuf, PathBuf, String), String> {
    let path = config_dir.join(contents);
    let as_file = if path.is_dir() {
        false
    } else if path.is_file() {
        true
    } else {
        path.extension().is_some()
    };

    if as_file {
        let contents_dir = path
            .parent()
            .ok_or_else(|| "contents path has no parent".to_string())?
            .to_path_buf();
        let entry_filename = path
            .file_name()
            .ok_or_else(|| "contents path has no file name".to_string())?
            .to_string_lossy()
            .to_string();
        Ok((path, contents_dir, entry_filename))
    } else {
        Ok((path.clone(), path, "index.html".into()))
    }
}

pub fn resolve_paths(config: &ShellConfig, config_dir: &Path) -> Result<ResolvedPaths, String> {
    let icon = config_dir.join(&config.icon);
    let (contents, contents_dir, entry_filename) = resolve_contents(config_dir, &config.contents)?;
    let data_root = config_dir.join(&config.data_path);

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

#[cfg(test)]
mod tests {
    use super::resolve_paths;
    use crate::config::ShellConfig;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_config(contents: &str, data_path: &str) -> ShellConfig {
        ShellConfig {
            icon: "icon.png".into(),
            name: "Test".into(),
            contents: contents.into(),
            data_path: data_path.into(),
            show_dev_menu: None,
            keychain_prefix: None,
            settings: None,
            allowed_commands: Vec::new(),
            ai: None,
            webdriver: None,
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "app-ly-paths-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    #[test]
    fn contents_dir_and_data_path_are_independent_siblings() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("ui")).unwrap();
        std::fs::write(root.join("ui/index.html"), "<html></html>").unwrap();

        let resolved = resolve_paths(&test_config("ui", "data"), &root).unwrap();

        assert_eq!(
            canonical(&resolved.contents_dir),
            canonical(&root.join("ui"))
        );
        assert_eq!(
            canonical(&resolved.data_root),
            canonical(&root.join("data"))
        );
        assert_eq!(resolved.entry_filename, "index.html");
        assert!(root.join("data/logs").is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn contents_file_path_still_uses_parent_as_ui_root() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("ui")).unwrap();
        std::fs::write(root.join("ui/app.html"), "<html></html>").unwrap();

        let resolved = resolve_paths(&test_config("ui/app.html", "data"), &root).unwrap();

        assert_eq!(
            canonical(&resolved.contents_dir),
            canonical(&root.join("ui"))
        );
        assert_eq!(
            canonical(&resolved.data_root),
            canonical(&root.join("data"))
        );
        assert_eq!(resolved.entry_filename, "app.html");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn contents_is_not_resolved_relative_to_data_path() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("ui")).unwrap();
        std::fs::write(root.join("ui/index.html"), "<html></html>").unwrap();

        let resolved = resolve_paths(&test_config("ui", "store"), &root).unwrap();

        assert_eq!(
            canonical(&resolved.contents_dir),
            canonical(&root.join("ui"))
        );
        assert_eq!(
            canonical(&resolved.data_root),
            canonical(&root.join("store"))
        );
        assert_ne!(
            canonical(&resolved.contents_dir),
            canonical(&resolved.data_root)
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
