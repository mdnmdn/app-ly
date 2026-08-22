use crate::paths::{bundled_resource_app_toml, deploy_folder};
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
    #[serde(rename = "keychainPrefix", default)]
    pub keychain_prefix: Option<String>,
    #[serde(default)]
    pub settings: Option<HashMap<String, String>>,
    #[serde(rename = "allowedCommands", default)]
    pub allowed_commands: Vec<CommandEntry>,
    #[serde(default)]
    pub ai: Option<AiConfig>,
    #[serde(default)]
    pub webdriver: Option<WebDriverConfig>,
}

/// The optional `[ai]` table. Absent means "all defaults, feature on".
/// Every field is optional so a partial table still parses, and unknown keys
/// are ignored so a newer app.toml stays loadable by an older shell.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub tool_timeout_ms: Option<u64>,
}

/// The optional `[webdriver]` table. Absent means the endpoint stays off;
/// present but without `enabled` means on, since writing the table at all is
/// how you ask for it. Every field is optional so a partial table still
/// parses, and CLI flags override whatever lands here.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDriverConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub token: Option<String>,
}

/// One `[[allowedCommands]]` entry. `program`, `cwd` and `env` come from
/// config only — the webview can never supply them.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEntry {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub extra_args: Option<String>,
    #[serde(default)]
    pub max_args: Option<usize>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
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
    pub config_path: PathBuf,
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
    let config_path = display_path(path);
    let config_dir = config_dir_for(&config_path);
    ConfigDiscovery {
        config,
        config_dir,
        config_path,
    }
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

fn discover_rest(
    mut searched: Vec<ConfigSearch>,
    bundled: Option<PathBuf>,
) -> Result<ConfigDiscovery, DiscoverError> {
    // Next to the .app / executable — same identity the GUI uses when deployed.
    // Tried in debug too so `app-ly.app/Contents/MacOS/app-ly run …` picks up
    // that folder's app.toml instead of the shell checkout's.
    if let Some(discovery) = found(try_deploy_config(&mut searched))? {
        return Ok(discovery);
    }

    if !cfg!(debug_assertions) {
        if let Some(path) = bundled {
            if record_search(&mut searched, "bundled resource (fallback)", path.clone()) {
                let config = ShellConfig::load(&path).map_err(DiscoverError::Failed)?;
                return Ok(loaded(config, path));
            }
        } else {
            searched.push(ConfigSearch {
                label: "bundled resource (fallback)".into(),
                path: PathBuf::from("<unresolved>"),
                exists: false,
            });
        }

        return Err(DiscoverError::Missing(searched));
    }

    if let Some(path) = bundled {
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

/// Same search order as the GUI, without needing a Tauri `App`.
/// `config_override` is `--config` already parsed by the CLI.
pub fn discover_config_headless(
    config_override: Option<&Path>,
) -> Result<ConfigDiscovery, DiscoverError> {
    let mut searched = Vec::new();

    if let Some(path) = config_override {
        let path = path.to_path_buf();
        record_search(&mut searched, "--config flag", path.clone());
        let config = ShellConfig::load(&path).map_err(DiscoverError::Failed)?;
        return Ok(loaded(config, path));
    }

    discover_rest(searched, bundled_resource_app_toml())
}

pub fn discover_config(app: &App) -> Result<ConfigDiscovery, DiscoverError> {
    let mut searched = Vec::new();

    if let Some(discovery) = found(try_cli_config(&mut searched))? {
        return Ok(discovery);
    }

    let bundled = match app.path().resolve("app.toml", BaseDirectory::Resource) {
        Ok(path) => Some(path),
        Err(_) => bundled_resource_app_toml(),
    };

    discover_rest(searched, bundled)
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
