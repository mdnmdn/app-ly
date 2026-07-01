use crate::paths::deploy_folder;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::path::BaseDirectory;
use tauri::{App, Manager};

#[derive(Debug, Clone, Deserialize)]
pub struct ShellConfig {
    pub icon: String,
    pub name: String,
    pub contents: String,
    #[serde(rename = "dataPath")]
    pub data_path: String,
    #[serde(rename = "showDevMenu", default)]
    pub show_dev_menu: Option<bool>,
    #[serde(default)]
    pub settings: Option<HashMap<String, String>>,
}

pub fn default_show_dev_menu() -> bool {
    cfg!(debug_assertions)
}

pub fn effective_show_dev_menu(config: &ShellConfig) -> bool {
    config.show_dev_menu.unwrap_or_else(default_show_dev_menu)
}

impl ShellConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read config: {e}"))?;
        toml::from_str(&text).map_err(|e| format!("parse config: {e}"))
    }
}

/// `KEY=VALUE` lines only — no multi-line values, no `\n`-style escapes.
/// Good enough for the local-secrets use case `.env` exists for; reach for a
/// real dotenv crate if you need more.
fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let mut value = value.trim();
        if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            value = &value[1..value.len() - 1];
        }
        values.insert(key.trim().to_string(), value.to_string());
    }
    values
}

/// Merges `[settings]` from `app.toml` with a `.env` file beside it, if present.
/// `.env` wins on key collisions — it's the local-override layer.
pub fn load_settings(config: &ShellConfig, config_dir: &Path) -> HashMap<String, String> {
    let mut settings = config.settings.clone().unwrap_or_default();
    if let Ok(text) = std::fs::read_to_string(config_dir.join(".env")) {
        settings.extend(parse_dotenv(&text));
    }
    settings
}

#[derive(Debug, Clone)]
pub struct ConfigSearch {
    pub label: String,
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct ConfigDiscovery {
    pub config: ShellConfig,
    pub config_dir: PathBuf,
}

fn config_dir_for(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn display_path(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn record_search(
    searched: &mut Vec<ConfigSearch>,
    label: impl Into<String>,
    path: PathBuf,
) -> bool {
    let exists = path.exists();
    searched.push(ConfigSearch {
        label: label.into(),
        path: display_path(path.clone()),
        exists,
    });
    exists
}

fn loaded(config: ShellConfig, path: PathBuf) -> ConfigDiscovery {
    let config_dir = config_dir_for(&display_path(path));
    ConfigDiscovery { config, config_dir }
}

fn try_cli_config(searched: &mut Vec<ConfigSearch>) -> Result<Option<ConfigDiscovery>, String> {
    let args: Vec<String> = std::env::args().collect();
    for (index, arg) in args.iter().enumerate() {
        if arg == "--config" {
            let Some(path_value) = args.get(index + 1) else {
                return Err("--config requires a path".into());
            };
            let path = PathBuf::from(path_value);
            record_search(searched, "--config flag", path.clone());
            let config = ShellConfig::load(&path)?;
            return Ok(Some(loaded(config, path)));
        }
    }
    Ok(None)
}

fn try_deploy_config(searched: &mut Vec<ConfigSearch>) -> Result<Option<ConfigDiscovery>, String> {
    let folder = match deploy_folder() {
        Ok(folder) => folder,
        Err(error) => {
            searched.push(ConfigSearch {
                label: "folder containing app-ly.app (external app.toml)".into(),
                path: PathBuf::from(format!("<unresolved> ({error})")),
                exists: false,
            });
            return Ok(None);
        }
    };

    let path = folder.join("app.toml");
    if record_search(
        searched,
        "folder containing app-ly.app (external app.toml)",
        path.clone(),
    ) {
        let config = ShellConfig::load(&path)?;
        return Ok(Some(loaded(config, path)));
    }
    Ok(None)
}

fn try_bundled_config(
    app: &App,
    searched: &mut Vec<ConfigSearch>,
) -> Result<Option<ConfigDiscovery>, String> {
    let path = match app.path().resolve("app.toml", BaseDirectory::Resource) {
        Ok(path) => path,
        Err(error) => {
            searched.push(ConfigSearch {
                label: "bundled resource (fallback)".into(),
                path: PathBuf::from(format!("<unresolved> ({error})")),
                exists: false,
            });
            return Ok(None);
        }
    };

    if record_search(searched, "bundled resource (fallback)", path.clone()) {
        let config = ShellConfig::load(&path)?;
        return Ok(Some(loaded(config, path)));
    }
    Ok(None)
}

fn try_cwd_config(searched: &mut Vec<ConfigSearch>) -> Result<Option<ConfigDiscovery>, String> {
    let Ok(cwd) = std::env::current_dir() else {
        return Ok(None);
    };
    let path = cwd.join("app.toml");
    if record_search(searched, "current directory (./app.toml)", path.clone()) {
        let config = ShellConfig::load(&path)?;
        return Ok(Some(loaded(config, path)));
    }
    Ok(None)
}

fn try_dev_fallback(searched: &mut Vec<ConfigSearch>) -> Result<ConfigDiscovery, String> {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../app.toml");
    record_search(searched, "project root (dev fallback)", dev_path.clone());
    let config = ShellConfig::load(&dev_path)?;
    Ok(loaded(config, dev_path))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn config_fallback_html(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title}</title>
  <style>
    :root {{ color-scheme: light dark; }}
    body {{
      margin: 0;
      min-height: 100vh;
      display: grid;
      place-items: center;
      font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      background: #f4f4f5;
      color: #18181b;
    }}
    .card {{
      width: min(720px, calc(100vw - 48px));
      background: #fff;
      border: 1px solid #e4e4e7;
      border-radius: 12px;
      padding: 28px 32px;
      box-shadow: 0 10px 30px rgba(0, 0, 0, 0.06);
    }}
    h1 {{ margin: 0 0 12px; font-size: 1.35rem; }}
    p {{ margin: 0 0 16px; color: #52525b; }}
    pre {{
      margin: 0;
      padding: 16px;
      border-radius: 8px;
      background: #fafafa;
      border: 1px solid #e4e4e7;
      white-space: pre-wrap;
      word-break: break-word;
      font: 13px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace;
    }}
    @media (prefers-color-scheme: dark) {{
      body {{ background: #09090b; color: #fafafa; }}
      .card {{ background: #18181b; border-color: #3f3f46; box-shadow: none; }}
      p {{ color: #a1a1aa; }}
      pre {{ background: #09090b; border-color: #3f3f46; }}
    }}
  </style>
</head>
<body>
  <main class="card">
    <h1>{title}</h1>
    <p>The shell could not start because configuration is missing or invalid.</p>
    <pre>{body}</pre>
  </main>
</body>
</html>"#,
        title = escape_html(title),
        body = escape_html(message),
    )
}

pub fn missing_config_message(searched: &[ConfigSearch]) -> String {
    let mut message = String::from(
        "Missing app.toml\n\n\
         Place app.toml in the folder that contains app-ly.app (not inside the bundle).\n\
         The bundled copy inside the app is only used as a fallback.\n\
         Do not edit files inside the .app bundle — macOS code signing will block the app.\n",
    );

    message.push_str("\nSearched:\n");
    for entry in searched {
        let status = if entry.exists { "found" } else { "not found" };
        message.push_str(&format!(
            "  • {} — {}\n    {}\n",
            entry.label,
            status,
            entry.path.display()
        ));
    }

    message
}

pub enum DiscoverError {
    Missing(Vec<ConfigSearch>),
    Failed(String),
}

fn found(
    result: Result<Option<ConfigDiscovery>, String>,
) -> Result<Option<ConfigDiscovery>, DiscoverError> {
    result.map_err(DiscoverError::Failed)
}

pub fn discover_config(app: &App) -> Result<ConfigDiscovery, DiscoverError> {
    let mut searched = Vec::new();

    if let Some(discovery) = found(try_cli_config(&mut searched))? {
        return Ok(discovery);
    }

    if !cfg!(debug_assertions) {
        if let Some(discovery) = found(try_deploy_config(&mut searched))? {
            return Ok(discovery);
        }

        if let Some(discovery) = found(try_bundled_config(app, &mut searched))? {
            return Ok(discovery);
        }

        return Err(DiscoverError::Missing(searched));
    }

    if let Ok(path) = app.path().resolve("app.toml", BaseDirectory::Resource) {
        searched.push(ConfigSearch {
            label: "bundled resource (skipped in dev)".into(),
            path: display_path(path),
            exists: false,
        });
    }

    if let Some(discovery) = found(try_cwd_config(&mut searched))? {
        return Ok(discovery);
    }

    try_dev_fallback(&mut searched).map_err(DiscoverError::Failed)
}

#[cfg(test)]
mod tests {
    use super::parse_dotenv;

    #[test]
    fn parses_dotenv_lines() {
        let text = "\
# comment
export FOO=bar
BAZ = \"quoted value\"
SINGLE='also quoted'
EMPTY=

MALFORMED_LINE_NO_EQUALS
";
        let values = parse_dotenv(text);
        assert_eq!(values.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(values.get("BAZ").map(String::as_str), Some("quoted value"));
        assert_eq!(
            values.get("SINGLE").map(String::as_str),
            Some("also quoted")
        );
        assert_eq!(values.get("EMPTY").map(String::as_str), Some(""));
        assert_eq!(values.len(), 4);
    }
}
