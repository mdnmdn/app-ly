//! Headless CLI for the same shell features as `window.shell`.
//!
//! `app-ly ai "say hi"` runs the command and exits — no window. With no
//! recognised command the desktop app starts as usual.

use crate::ai::{AiOptions, AiSettings, AiState, ToolDispatch, ToolSpec};
use crate::commands::{self, ShellState};
use crate::config::{
    discover_config_headless, load_settings, missing_config_message, CommandEntry, DiscoverError,
    ShellConfig,
};
use crate::db::{self, DbState};
use crate::paths::resolve_paths;
use crate::process::ProcessState;
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const USAGE: &str = "\
Usage: app-ly [--config PATH] [--json] <command> [args]

Headless access to the same shell as window.shell. No window is opened.
With no command, the desktop app launches as usual.

Commands:
  info                         Loaded app.toml, paths, AI availability
  ai <prompt>                  On-device generate; [[allowedCommands]] are tools
  db query <db> <sql>          SELECT → JSON
  db exec  <db> <sql>          INSERT/UPDATE/DELETE → JSON
  file read <name>
  file write <name> [text]     Text from args, or stdin if omitted
  file delete <name>
  fetch <url>
  run [name] [args...]         Allowlisted program from this app.toml (no name → list)
  help

Options:
  --config PATH                Same as the GUI flag
  --json                       Machine-readable output where it applies

ai options:
  --stream                     Write tokens as they arrive
  --instructions TEXT
  --temperature N
  --max-tokens N
  --schema JSON                Structured output (generateObject)

db / fetch options:
  --params JSON                Bind parameters as a JSON array (db)
  --method GET|POST|...
  --body TEXT

Examples:
  app-ly.app/Contents/MacOS/app-ly ai \"say hi\"
  app-ly --config ./app.toml db query notes.db \"select * from notes\"
  app-ly run git status
";

#[derive(Debug, PartialEq)]
enum Invocation {
    Gui,
    Cli(Cli),
}

#[derive(Debug, PartialEq)]
struct Cli {
    config: Option<PathBuf>,
    json: bool,
    command: Command,
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Info,
    Ai {
        prompt: String,
        stream: bool,
        instructions: Option<String>,
        temperature: Option<f64>,
        max_tokens: Option<u32>,
        schema: Option<String>,
    },
    DbQuery {
        db: String,
        sql: String,
        params: Option<String>,
    },
    DbExec {
        db: String,
        sql: String,
        params: Option<String>,
    },
    FileRead {
        name: String,
    },
    FileWrite {
        name: String,
        contents: Option<String>,
    },
    FileDelete {
        name: String,
    },
    Fetch {
        url: String,
        method: Option<String>,
        body: Option<String>,
    },
    Run {
        name: Option<String>,
        args: Vec<String>,
    },
}

fn is_cli_command(name: &str) -> bool {
    matches!(
        name,
        "ai" | "db" | "file" | "fetch" | "run" | "info" | "help" | "-h" | "--help"
    )
}

fn parse<I, S>(args: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .filter(|s| !s.starts_with("-psn_"))
        .collect();

    let mut config = None;
    let mut json = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                let path = args
                    .get(i + 1)
                    .ok_or_else(|| "--config requires a path".to_string())?;
                config = Some(PathBuf::from(path));
                i += 2;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            "--help" | "-h" => {
                return Ok(Invocation::Cli(Cli {
                    config,
                    json,
                    command: Command::Help,
                }));
            }
            flag if flag.starts_with('-') => {
                // OS / Tauri may pass unknown flags; without a command, launch GUI.
                return Ok(Invocation::Gui);
            }
            _ => break,
        }
    }

    if i >= args.len() {
        return Ok(Invocation::Gui);
    }

    if !is_cli_command(&args[i]) {
        return Ok(Invocation::Gui);
    }

    parse_command(&args[i..], config, json)
}

fn take_flag_value(
    rest: &[String],
    index: &mut usize,
    flag: &str,
) -> Result<Option<String>, String> {
    if rest.get(*index).map(String::as_str) != Some(flag) {
        return Ok(None);
    }
    let value = rest
        .get(*index + 1)
        .ok_or_else(|| format!("{flag} requires a value"))?;
    *index += 2;
    Ok(Some(value.clone()))
}

fn parse_command(
    args: &[String],
    mut config: Option<PathBuf>,
    mut json: bool,
) -> Result<Invocation, String> {
    let cmd = args[0].as_str();
    let rest = &args[1..];

    let cli = |command| {
        Invocation::Cli(Cli {
            config: config.clone(),
            json,
            command,
        })
    };

    match cmd {
        "help" | "-h" | "--help" => Ok(cli(Command::Help)),
        "info" => {
            let mut i = 0;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--json" => {
                        json = true;
                        i += 1;
                    }
                    "--config" => {
                        config = Some(PathBuf::from(
                            rest.get(i + 1)
                                .ok_or_else(|| "--config requires a path".to_string())?,
                        ));
                        i += 2;
                    }
                    other => return Err(format!("unexpected argument: {other}")),
                }
            }
            Ok(Invocation::Cli(Cli {
                config,
                json,
                command: Command::Info,
            }))
        }
        "ai" => parse_ai(rest, config, json),
        "db" => parse_db(rest, config, json),
        "file" => parse_file(rest, config, json),
        "fetch" => parse_fetch(rest, config, json),
        "run" => {
            let name = rest.first().cloned();
            let args = if rest.is_empty() {
                Vec::new()
            } else {
                rest[1..].to_vec()
            };
            Ok(cli(Command::Run { name, args }))
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn parse_ai(
    rest: &[String],
    mut config: Option<PathBuf>,
    mut json: bool,
) -> Result<Invocation, String> {
    let mut stream = false;
    let mut instructions = None;
    let mut temperature = None;
    let mut max_tokens = None;
    let mut schema = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--json" => {
                json = true;
                i += 1;
            }
            "--stream" => {
                stream = true;
                i += 1;
            }
            "--config" => {
                config = Some(PathBuf::from(
                    rest.get(i + 1)
                        .ok_or_else(|| "--config requires a path".to_string())?,
                ));
                i += 2;
            }
            "--instructions" => {
                instructions = take_flag_value(rest, &mut i, "--instructions")?;
            }
            "--temperature" => {
                let raw = take_flag_value(rest, &mut i, "--temperature")?
                    .ok_or_else(|| "--temperature requires a value".to_string())?;
                temperature = Some(
                    raw.parse::<f64>()
                        .map_err(|_| format!("invalid --temperature: {raw}"))?,
                );
            }
            "--max-tokens" => {
                let raw = take_flag_value(rest, &mut i, "--max-tokens")?
                    .ok_or_else(|| "--max-tokens requires a value".to_string())?;
                max_tokens = Some(
                    raw.parse::<u32>()
                        .map_err(|_| format!("invalid --max-tokens: {raw}"))?,
                );
            }
            "--schema" => {
                schema = take_flag_value(rest, &mut i, "--schema")?;
            }
            "--" => {
                i += 1;
                break;
            }
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(format!("unknown ai flag: {flag}"));
            }
            _ => break,
        }
    }

    if stream && json {
        return Err("--stream and --json cannot be used together".into());
    }
    if stream && schema.is_some() {
        return Err("--stream cannot be used with --schema".into());
    }

    let prompt = rest[i..].join(" ");
    Ok(Invocation::Cli(Cli {
        config,
        json,
        command: Command::Ai {
            prompt,
            stream,
            instructions,
            temperature,
            max_tokens,
            schema,
        },
    }))
}

fn parse_db(
    rest: &[String],
    mut config: Option<PathBuf>,
    json: bool,
) -> Result<Invocation, String> {
    let action = rest
        .first()
        .map(String::as_str)
        .ok_or_else(|| "db requires `query` or `exec`".to_string())?;
    if action != "query" && action != "exec" {
        return Err(format!(
            "unknown db action: {action} (expected query or exec)"
        ));
    }

    let mut params = None;
    let mut positional = Vec::new();
    let mut i = 1;
    while i < rest.len() {
        match rest[i].as_str() {
            "--params" => {
                params = take_flag_value(rest, &mut i, "--params")?;
            }
            "--config" => {
                config = Some(PathBuf::from(
                    rest.get(i + 1)
                        .ok_or_else(|| "--config requires a path".to_string())?,
                ));
                i += 2;
            }
            "--" => {
                positional.extend_from_slice(&rest[i + 1..]);
                break;
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown db flag: {flag}"));
            }
            _ => {
                positional.push(rest[i].clone());
                i += 1;
            }
        }
    }

    let db = positional
        .first()
        .cloned()
        .ok_or_else(|| "db requires a database file name".to_string())?;
    let sql = positional[1..].join(" ");
    if sql.is_empty() {
        return Err("db requires an SQL statement".into());
    }

    let command = if action == "query" {
        Command::DbQuery { db, sql, params }
    } else {
        Command::DbExec { db, sql, params }
    };
    Ok(Invocation::Cli(Cli {
        config,
        json,
        command,
    }))
}

fn parse_file(rest: &[String], config: Option<PathBuf>, json: bool) -> Result<Invocation, String> {
    let action = rest
        .first()
        .map(String::as_str)
        .ok_or_else(|| "file requires `read`, `write`, or `delete`".to_string())?;
    let name = rest
        .get(1)
        .cloned()
        .ok_or_else(|| format!("file {action} requires a file name"))?;
    let command = match action {
        "read" => Command::FileRead { name },
        "delete" => Command::FileDelete { name },
        "write" => {
            let extra = &rest[2..];
            let contents = if extra.is_empty() {
                None
            } else {
                Some(extra.join(" "))
            };
            Command::FileWrite { name, contents }
        }
        other => return Err(format!("unknown file action: {other}")),
    };
    Ok(Invocation::Cli(Cli {
        config,
        json,
        command,
    }))
}

fn parse_fetch(
    rest: &[String],
    mut config: Option<PathBuf>,
    json: bool,
) -> Result<Invocation, String> {
    let mut method = None;
    let mut body = None;
    let mut url = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--method" => {
                method = take_flag_value(rest, &mut i, "--method")?;
            }
            "--body" => {
                body = take_flag_value(rest, &mut i, "--body")?;
            }
            "--config" => {
                config = Some(PathBuf::from(
                    rest.get(i + 1)
                        .ok_or_else(|| "--config requires a path".to_string())?,
                ));
                i += 2;
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown fetch flag: {flag}"));
            }
            _ => {
                if url.is_some() {
                    return Err("fetch takes a single URL".into());
                }
                url = Some(rest[i].clone());
                i += 1;
            }
        }
    }
    let url = url.ok_or_else(|| "fetch requires a URL".to_string())?;
    Ok(Invocation::Cli(Cli {
        config,
        json,
        command: Command::Fetch { url, method, body },
    }))
}

fn read_stdin() -> Result<String, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("read stdin: {e}"))?;
    Ok(buf)
}

fn parse_params(raw: Option<&str>) -> Result<Option<Vec<Value>>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value: Value =
        serde_json::from_str(raw).map_err(|e| format!("--params must be a JSON array: {e}"))?;
    match value {
        Value::Array(items) => Ok(Some(items)),
        _ => Err("--params must be a JSON array".into()),
    }
}

struct AppCtx {
    config: ShellConfig,
    config_dir: PathBuf,
    config_path: PathBuf,
    data_root: PathBuf,
    contents_dir: PathBuf,
}

fn load_ctx(config_override: Option<&Path>) -> Result<AppCtx, String> {
    let discovery = match discover_config_headless(config_override) {
        Ok(discovery) => discovery,
        Err(DiscoverError::Missing(searched)) => {
            return Err(missing_config_message(&searched));
        }
        Err(DiscoverError::Failed(message)) => return Err(message),
    };
    let resolved = resolve_paths(&discovery.config, &discovery.config_dir)?;
    Ok(AppCtx {
        config: discovery.config,
        config_dir: discovery.config_dir,
        config_path: discovery.config_path,
        data_root: resolved.data_root,
        contents_dir: resolved.contents_dir,
    })
}

fn print_json(value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("encode json: {e}"))?;
    println!("{text}");
    Ok(())
}

fn cmd_info(ctx: &AppCtx, json: bool) -> Result<(), String> {
    let settings = load_settings(&ctx.config, &ctx.config_dir);
    let ai = AiState::new(AiSettings::from_config(ctx.config.ai.as_ref()));
    let info = ai.info();
    let commands: Vec<String> = ctx
        .config
        .allowed_commands
        .iter()
        .map(|entry| entry.name.clone())
        .collect();

    if json {
        return print_json(&json!({
            "name": ctx.config.name,
            "config": ctx.config_path,
            "configDir": ctx.config_dir,
            "contentsDir": ctx.contents_dir,
            "dataPath": ctx.data_root,
            "settings": settings,
            "allowedCommands": commands,
            "ai": info,
        }));
    }

    println!("name            {}", ctx.config.name);
    println!("config          {}", ctx.config_path.display());
    println!("configDir       {}", ctx.config_dir.display());
    println!("contentsDir     {}", ctx.contents_dir.display());
    println!("dataPath        {}", ctx.data_root.display());
    if commands.is_empty() {
        println!("allowedCommands  (none)");
    } else {
        println!("allowedCommands  {}", commands.join(", "));
    }
    match (info.available, info.reason, info.detail) {
        (true, _, _) => {
            let label = info
                .models
                .first()
                .map(|model| model.name.as_str())
                .unwrap_or("On-device model");
            println!("ai              available ({label})");
        }
        (false, Some(reason), Some(detail)) => {
            println!("ai              unavailable ({reason}) — {detail}");
        }
        (false, Some(reason), None) => {
            println!("ai              unavailable ({reason})");
        }
        (false, None, _) => println!("ai              unavailable"),
    }
    Ok(())
}

fn command_tools(commands: &[CommandEntry]) -> Vec<ToolSpec> {
    commands
        .iter()
        .map(|entry| ToolSpec {
            name: entry.name.clone(),
            description: format!(
                "Run the allowlisted program `{}` (app.toml name \"{}\"). Pass only arguments the allowlist accepts.",
                entry.program, entry.name
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments to pass to the program"
                    }
                },
                "required": ["args"],
            }),
        })
        .collect()
}

fn tool_args(arguments: &Value) -> Result<Vec<String>, String> {
    let as_strings = |items: &[Value]| -> Result<Vec<String>, String> {
        items
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "args must be an array of strings".to_string())
            })
            .collect()
    };
    match arguments {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => as_strings(items),
        Value::Object(map) => match map.get("args") {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => as_strings(items),
            Some(Value::String(text)) => Ok(vec![text.clone()]),
            Some(_) => Err("args must be an array of strings".into()),
        },
        Value::String(text) => Ok(vec![text.clone()]),
        _ => Err("tool arguments must be an object with args[]".into()),
    }
}

fn command_dispatch(processes: Arc<ProcessState>) -> ToolDispatch {
    Arc::new(move |name: &str, arguments: Value| {
        let args = match tool_args(&arguments) {
            Ok(args) => args,
            Err(error) => return json!({ "error": error }),
        };
        match processes.run_sync(name, args, None, None) {
            Ok(result) => json!({
                "stdout": result.stdout,
                "stderr": result.stderr,
                "code": result.code,
                "timedOut": result.timed_out,
            }),
            Err(error) => json!({ "error": error }),
        }
    })
}

fn cli_ai_bridge(ctx: &AppCtx) -> (Vec<ToolSpec>, Option<ToolDispatch>) {
    if ctx.config.allowed_commands.is_empty() {
        return (Vec::new(), None);
    }
    let processes = Arc::new(ProcessState::new(
        ctx.config.allowed_commands.clone(),
        ctx.config_dir.clone(),
    ));
    (
        command_tools(&ctx.config.allowed_commands),
        Some(command_dispatch(processes)),
    )
}

fn cmd_ai(
    ctx: &AppCtx,
    prompt: String,
    stream: bool,
    instructions: Option<String>,
    temperature: Option<f64>,
    max_tokens: Option<u32>,
    schema: Option<String>,
    json: bool,
) -> Result<(), String> {
    let prompt = if prompt.is_empty() || prompt == "-" {
        let text = read_stdin()?;
        if text.trim().is_empty() {
            return Err("ai requires a prompt (argument or stdin)".into());
        }
        text
    } else {
        prompt
    };

    let ai = AiState::new(AiSettings::from_config(ctx.config.ai.as_ref()));
    let (tools, dispatch) = cli_ai_bridge(ctx);
    let options = AiOptions {
        instructions,
        temperature,
        max_tokens,
        tools,
        ..AiOptions::default()
    };

    if let Some(schema) = schema {
        let schema: Value =
            serde_json::from_str(&schema).map_err(|e| format!("--schema must be JSON: {e}"))?;
        let result = ai.generate_object_sync(prompt, schema, options, dispatch)?;
        if json {
            return print_json(&json!({
                "object": result.object,
                "model": result.model,
                "toolCalls": result.tool_calls,
            }));
        }
        let text = serde_json::to_string_pretty(&result.object)
            .map_err(|e| format!("encode object: {e}"))?;
        println!("{text}");
        return Ok(());
    }

    if stream {
        let mut stdout = io::stdout();
        let result = ai.stream_sync(
            prompt,
            options,
            Box::new(move |delta: &str| {
                let _ = write!(stdout, "{delta}");
                let _ = stdout.flush();
            }),
            dispatch,
        )?;
        if !result.text.ends_with('\n') {
            println!();
        }
        return Ok(());
    }

    let result = ai.generate_sync(prompt, options, dispatch)?;
    if json {
        return print_json(&json!({
            "text": result.text,
            "model": result.model,
            "toolCalls": result.tool_calls,
        }));
    }
    println!("{}", result.text);
    Ok(())
}

fn cmd_db(
    ctx: &AppCtx,
    exec: bool,
    db_name: String,
    sql: String,
    params: Option<String>,
) -> Result<(), String> {
    let shell = ShellState {
        data_root: ctx.data_root.clone(),
    };
    let db = DbState::new();
    let params = parse_params(params.as_deref())?;
    let outcome = if exec {
        let result = db::db_execute(&shell, &db, db_name.clone(), sql, params)?;
        json!({
            "changes": result.changes,
            "lastInsertRowid": result.last_insert_rowid,
        })
    } else {
        let result = db::db_query(&shell, &db, db_name.clone(), sql, params)?;
        json!({
            "columns": result.columns,
            "rows": result.rows,
        })
    };
    let _ = db::db_close(&db, Some(&db_name));
    print_json(&outcome)
}

fn cmd_file(ctx: &AppCtx, command: &Command, json: bool) -> Result<(), String> {
    let shell = ShellState {
        data_root: ctx.data_root.clone(),
    };
    match command {
        Command::FileRead { name } => {
            let contents = commands::read_file(&shell, name)?;
            if json {
                return print_json(&json!({ "name": name, "contents": contents }));
            }
            print!("{contents}");
            if !contents.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        Command::FileWrite { name, contents } => {
            let text = match contents {
                Some(text) => text.clone(),
                None => read_stdin()?,
            };
            commands::save_file(&shell, name, &text)?;
            if json {
                return print_json(&json!({ "ok": true, "name": name }));
            }
            Ok(())
        }
        Command::FileDelete { name } => {
            commands::delete_file(&shell, name)?;
            if json {
                return print_json(&json!({ "ok": true, "name": name }));
            }
            Ok(())
        }
        _ => unreachable!("cmd_file called with non-file command"),
    }
}

fn cmd_fetch(
    url: String,
    method: Option<String>,
    body: Option<String>,
    json: bool,
) -> Result<(), String> {
    let response = commands::fetch_blocking(url, method, None, body)?;
    if json {
        print_json(&json!({
            "ok": response.ok,
            "status": response.status,
            "statusText": response.status_text,
            "headers": response.headers,
            "body": response.body,
        }))?;
    } else {
        print!("{}", response.body);
        if !response.body.ends_with('\n') {
            println!();
        }
    }
    if response.ok {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status))
    }
}

fn cmd_run(
    ctx: &AppCtx,
    name: Option<String>,
    args: Vec<String>,
    json: bool,
) -> Result<i32, String> {
    let processes = ProcessState::new(ctx.config.allowed_commands.clone(), ctx.config_dir.clone());
    let Some(name) = name else {
        let list = processes.list();
        if json {
            print_json(&json!({
                "config": ctx.config_path,
                "allowedCommands": list,
            }))?;
        } else if list.is_empty() {
            println!("no [[allowedCommands]] in {}", ctx.config_path.display());
        } else {
            println!("allowedCommands from {}:", ctx.config_path.display());
            for command in list {
                println!("  {}  ({})", command.name, command.program);
            }
        }
        return Ok(0);
    };
    let result = processes.run_sync(&name, args, None, None)?;
    if json {
        print_json(&json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "code": result.code,
            "signal": result.signal,
            "timedOut": result.timed_out,
        }))?;
    } else {
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
    }
    if result.timed_out {
        return Ok(124);
    }
    Ok(result.code.unwrap_or(1))
}

fn execute(cli: Cli) -> Result<i32, String> {
    match cli.command {
        Command::Help => {
            print!("{USAGE}");
            Ok(0)
        }
        Command::Info => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_info(&ctx, cli.json)?;
            Ok(0)
        }
        Command::Ai {
            prompt,
            stream,
            instructions,
            temperature,
            max_tokens,
            schema,
        } => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_ai(
                &ctx,
                prompt,
                stream,
                instructions,
                temperature,
                max_tokens,
                schema,
                cli.json,
            )?;
            Ok(0)
        }
        Command::DbQuery { db, sql, params } => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_db(&ctx, false, db, sql, params)?;
            Ok(0)
        }
        Command::DbExec { db, sql, params } => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_db(&ctx, true, db, sql, params)?;
            Ok(0)
        }
        command @ (Command::FileRead { .. }
        | Command::FileWrite { .. }
        | Command::FileDelete { .. }) => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_file(&ctx, &command, cli.json)?;
            Ok(0)
        }
        Command::Fetch { url, method, body } => {
            let _ctx = load_ctx(cli.config.as_deref())?;
            cmd_fetch(url, method, body, cli.json)?;
            Ok(0)
        }
        Command::Run { name, args } => {
            let ctx = load_ctx(cli.config.as_deref())?;
            cmd_run(&ctx, name, args, cli.json)
        }
    }
}

#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn AllocConsole() -> i32;
        fn GetConsoleWindow() -> *mut core::ffi::c_void;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
    unsafe {
        if GetConsoleWindow().is_null() {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                let _ = AllocConsole();
            }
        }
    }
}

/// `Some(code)` means this process was a CLI invocation and should exit.
/// `None` means launch the desktop app.
pub fn maybe_run() -> Option<i32> {
    let invocation = match parse(std::env::args().skip(1)) {
        Ok(invocation) => invocation,
        Err(message) => {
            #[cfg(windows)]
            attach_parent_console();
            eprintln!("{message}");
            eprintln!();
            eprint!("{USAGE}");
            return Some(2);
        }
    };

    match invocation {
        Invocation::Gui => None,
        Invocation::Cli(cli) => {
            #[cfg(windows)]
            attach_parent_console();
            match execute(cli) {
                Ok(code) => Some(code),
                Err(message) => {
                    eprintln!("{message}");
                    Some(1)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_line(line: &str) -> Invocation {
        parse(line.split_whitespace()).expect("parse")
    }

    #[test]
    fn no_args_launches_gui() {
        assert_eq!(parse(Vec::<String>::new()).unwrap(), Invocation::Gui);
    }

    #[test]
    fn unknown_positional_launches_gui() {
        assert_eq!(parse_line("not-a-command"), Invocation::Gui);
    }

    #[test]
    fn finder_psn_arg_is_ignored() {
        assert_eq!(parse_line("-psn_0_12345"), Invocation::Gui);
    }

    #[test]
    fn help_is_cli() {
        match parse_line("--help") {
            Invocation::Cli(cli) => assert_eq!(cli.command, Command::Help),
            other => panic!("expected cli, got {other:?}"),
        }
    }

    #[test]
    fn config_without_command_is_gui() {
        assert_eq!(parse_line("--config ./app.toml"), Invocation::Gui);
    }

    #[test]
    fn ai_prompt_and_flags() {
        let Invocation::Cli(cli) = parse(
            [
                "--config",
                "./app.toml",
                "ai",
                "--instructions",
                "be brief",
                "--temperature",
                "0.2",
                "--max-tokens",
                "64",
                "say",
                "hi",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap() else {
            panic!("expected cli");
        };
        assert_eq!(cli.config, Some(PathBuf::from("./app.toml")));
        match cli.command {
            Command::Ai {
                prompt,
                stream,
                instructions,
                temperature,
                max_tokens,
                schema,
            } => {
                assert_eq!(prompt, "say hi");
                assert!(!stream);
                assert_eq!(instructions.as_deref(), Some("be brief"));
                assert_eq!(temperature, Some(0.2));
                assert_eq!(max_tokens, Some(64));
                assert!(schema.is_none());
            }
            other => panic!("expected ai, got {other:?}"),
        }
    }

    #[test]
    fn run_forwards_flags_to_the_child() {
        let Invocation::Cli(cli) = parse_line("run git status --oneline") else {
            panic!("expected cli");
        };
        match cli.command {
            Command::Run { name, args } => {
                assert_eq!(name.as_deref(), Some("git"));
                assert_eq!(args, vec!["status", "--oneline"]);
            }
            other => panic!("expected run, got {other:?}"),
        }
        assert!(cli.config.is_none());
    }

    #[test]
    fn db_query_parses_sql_and_params() {
        let Invocation::Cli(cli) = parse(
            [
                "db",
                "query",
                "notes.db",
                "--params",
                "[1]",
                "select * from notes where id = ?",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .unwrap() else {
            panic!("expected cli");
        };
        match cli.command {
            Command::DbQuery { db, sql, params } => {
                assert_eq!(db, "notes.db");
                assert_eq!(sql, "select * from notes where id = ?");
                assert_eq!(params.as_deref(), Some("[1]"));
            }
            other => panic!("expected db query, got {other:?}"),
        }
    }

    #[test]
    fn headless_config_override_loads_toml() {
        let dir = std::env::temp_dir().join(format!(
            "app-ly-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/index.html"), "<html></html>").unwrap();
        let toml_path = dir.join("app.toml");
        std::fs::write(
            &toml_path,
            r#"
icon = "icon.png"
name = "Cli Test"
contents = "ui"
dataPath = "data"
"#,
        )
        .unwrap();

        let ctx = load_ctx(Some(&toml_path)).expect("load_ctx");
        assert_eq!(ctx.config.name, "Cli Test");
        assert_eq!(ctx.config_dir, dir.canonicalize().unwrap_or(dir.clone()));
        assert!(ctx.data_root.ends_with("data"));
        assert!(ctx.config_path.ends_with("app.toml"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_without_name_lists_commands() {
        let Invocation::Cli(cli) = parse_line("run") else {
            panic!("expected cli");
        };
        match cli.command {
            Command::Run { name, args } => {
                assert!(name.is_none());
                assert!(args.is_empty());
            }
            other => panic!("expected run, got {other:?}"),
        }
    }

    fn write_cli_app(dir: &std::path::Path, extra: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir.join("ui")).unwrap();
        std::fs::write(dir.join("ui/index.html"), "<html></html>").unwrap();
        let toml_path = dir.join("app.toml");
        std::fs::write(
            &toml_path,
            format!(
                r#"
icon = "icon.png"
name = "Cli Test"
contents = "ui"
dataPath = "data"
{extra}
"#
            ),
        )
        .unwrap();
        toml_path
    }

    #[test]
    fn run_honors_allowlist_from_app_toml() {
        let dir = std::env::temp_dir().join(format!(
            "app-ly-cli-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let toml_path = write_cli_app(
            &dir,
            r#"
[[allowedCommands]]
name = "echo"
program = "echo"
"#,
        );
        let ctx = load_ctx(Some(&toml_path)).expect("load_ctx");

        let code = cmd_run(&ctx, Some("echo".into()), vec!["ok".into()], false).unwrap();
        assert_eq!(code, 0);

        let error = cmd_run(&ctx, Some("git".into()), vec!["status".into()], false).unwrap_err();
        assert!(
            error.contains("git") && error.contains("echo"),
            "unexpected error: {error}"
        );

        let processes = std::sync::Arc::new(ProcessState::new(
            ctx.config.allowed_commands.clone(),
            ctx.config_dir.clone(),
        ));
        let dispatch = command_dispatch(processes);
        let value = dispatch("echo", json!({ "args": ["from-tool"] }));
        assert_eq!(value["stdout"].as_str().unwrap().trim(), "from-tool");
        let denied = dispatch("git", json!({ "args": ["status"] }));
        assert!(
            denied["error"]
                .as_str()
                .is_some_and(|text| text.contains("git")),
            "unexpected tool error: {denied}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
