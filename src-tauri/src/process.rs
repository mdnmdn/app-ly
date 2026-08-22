use crate::config::CommandEntry;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

// ── Process state ───────────────────────────────────────────────────

pub struct ProcessState {
    pub commands: Vec<CommandEntry>,
    pub config_dir: PathBuf,
    pub running: Arc<Mutex<HashMap<String, RunningProcess>>>,
}

pub struct RunningProcess {
    pub stdin: Option<ChildStdin>,
    pub child: Arc<Mutex<Child>>,
    pub deadline: Deadline,
    pub pid: u32,
}

impl ProcessState {
    pub fn new(commands: Vec<CommandEntry>, config_dir: PathBuf) -> Self {
        ProcessState {
            commands,
            config_dir,
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn entry(&self, name: &str) -> Result<&CommandEntry, String> {
        if let Some(entry) = self.commands.iter().find(|e| e.name == name) {
            return Ok(entry);
        }
        if self.commands.is_empty() {
            return Err(format!(
                "no allowed command named \"{name}\" — this app.toml has no [[allowedCommands]]"
            ));
        }
        let names = self
            .commands
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "no allowed command named \"{name}\" — app.toml allows: {names}"
        ))
    }

    pub fn run_sync(
        &self,
        name: &str,
        args: Vec<String>,
        timeout_ms: Option<u64>,
        stdin: Option<String>,
    ) -> Result<RunResult, String> {
        let entry = self.entry(name)?.clone();
        run_blocking(entry, self.config_dir.clone(), args, timeout_ms, stdin)
    }

    pub fn list(&self) -> Vec<CommandInfo> {
        self.commands
            .iter()
            .map(|entry| CommandInfo {
                name: entry.name.clone(),
                program: entry.program.clone(),
                args_restricted: entry.args.is_some()
                    || entry.extra_args.is_some()
                    || entry.max_args.is_some(),
                timeout_ms: entry.timeout_ms,
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnResult {
    pub id: String,
    pub pid: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub name: String,
    pub program: String,
    pub args_restricted: bool,
    pub timeout_ms: Option<u64>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_id() -> String {
    format!("proc-{}", ID_COUNTER.fetch_add(1, Ordering::Relaxed))
}

// ── Argument allowlist ──────────────────────────────────────────────

/// Patterns are implicitly fully anchored: `P` is compiled as `^(?:P)$`.
fn compile_pattern(entry: &CommandEntry, pattern: &str) -> Result<Regex, String> {
    Regex::new(&format!("^(?:{pattern})$")).map_err(|e| {
        format!(
            "command \"{}\": invalid regex \"{}\" in allowedCommands: {e}",
            entry.name, pattern
        )
    })
}

/// Validates a caller-supplied argument list against the entry's allowlist.
/// No `args` and no `extraArgs` means any arguments are accepted.
fn validate_args(entry: &CommandEntry, args: &[String]) -> Result<(), String> {
    if let Some(max) = entry.max_args {
        if args.len() > max {
            return Err(format!(
                "command \"{}\": {} arguments given, maxArgs is {}",
                entry.name,
                args.len(),
                max
            ));
        }
    }

    if entry.args.is_none() && entry.extra_args.is_none() {
        return Ok(());
    }

    let positional: &[String] = entry.args.as_deref().unwrap_or(&[]);

    for (index, arg) in args.iter().enumerate() {
        let pattern = positional.get(index).or(entry.extra_args.as_ref());
        let Some(pattern) = pattern else {
            return Err(format!(
                "command \"{}\": argument {} (\"{}\") is not allowed — the allowlist defines {} positional pattern(s) and no extraArgs",
                entry.name,
                index + 1,
                arg,
                positional.len()
            ));
        };

        let regex = compile_pattern(entry, pattern)?;
        if !regex.is_match(arg) {
            return Err(format!(
                "command \"{}\": argument {} (\"{}\") does not match allowed pattern {}",
                entry.name,
                index + 1,
                arg,
                pattern
            ));
        }
    }

    Ok(())
}

// ── Spawning ────────────────────────────────────────────────────────

fn build_command(
    entry: &CommandEntry,
    config_dir: &Path,
    args: &[String],
) -> Result<Command, String> {
    validate_args(entry, args)?;

    let mut command = Command::new(&entry.program);
    command.args(args);

    let cwd = match &entry.cwd {
        Some(cwd) => {
            let path = Path::new(cwd);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                config_dir.join(path)
            }
        }
        None => config_dir.to_path_buf(),
    };
    command.current_dir(cwd);

    if let Some(env) = &entry.env {
        for (key, value) in env {
            command.env(key, value);
        }
    }

    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    Ok(command)
}

fn effective_timeout(call_timeout: Option<u64>, entry_timeout: Option<u64>) -> Option<Duration> {
    call_timeout.or(entry_timeout).map(Duration::from_millis)
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

enum Wait {
    Exited(ExitStatus),
    TimedOut,
    Failed(String),
}

/// The instant a process should be killed at, or `None` for "no timeout".
/// Shared so `shell_process_set_timeout` can move it while the child runs.
pub type Deadline = Arc<Mutex<Option<Instant>>>;

fn deadline_in(timeout: Option<Duration>) -> Deadline {
    Arc::new(Mutex::new(timeout.map(|d| Instant::now() + d)))
}

/// Polls rather than blocking on a fixed timeout, so the deadline is re-read
/// on every tick and `kill` still gets at the child between polls.
fn wait_for_exit(child: &Arc<Mutex<Child>>, deadline: &Deadline) -> Wait {
    loop {
        let polled = child.lock().unwrap().try_wait();
        match polled {
            Ok(Some(status)) => return Wait::Exited(status),
            Ok(None) => {}
            Err(e) => return Wait::Failed(format!("wait for process: {e}")),
        }

        if let Some(at) = *deadline.lock().unwrap() {
            if Instant::now() >= at {
                return Wait::TimedOut;
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn kill_and_reap(child: &Arc<Mutex<Child>>) {
    let mut child = child.lock().unwrap();
    let _ = child.kill();
    let _ = child.wait();
}

fn write_stdin(child: &mut Child, stdin: Option<String>) {
    match stdin {
        Some(data) => {
            if let Some(mut pipe) = child.stdin.take() {
                // On its own thread: a large payload can fill the pipe buffer
                // before the child has read any of it.
                thread::spawn(move || {
                    let _ = pipe.write_all(data.as_bytes());
                });
            }
        }
        None => {
            child.stdin.take();
        }
    }
}

// ── Readers ─────────────────────────────────────────────────────────

fn collect_reader<R: Read + Send + 'static>(
    mut reader: R,
    buffer: Arc<Mutex<String>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => buffer
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&chunk[..read])),
            }
        }
    })
}

/// How long the supervisor waits for the stdout/stderr readers to hit EOF
/// before emitting `shell://process-exit` anyway. Normally they finish the
/// moment the child's pipes close; the cap only matters when a surviving
/// grandchild still holds the write end open.
const DRAIN_GRACE: Duration = Duration::from_millis(1000);

/// Returns a receiver that disconnects once the reader has drained the pipe,
/// so the supervisor can order the exit event after the final chunk.
fn stream_reader<R: Read + Send + 'static>(
    mut reader: R,
    app: AppHandle,
    id: String,
    event: &'static str,
) -> mpsc::Receiver<()> {
    let (done_tx, done_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let _done = done_tx;
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let data = String::from_utf8_lossy(&chunk[..read]).to_string();
                    let _ =
                        app.emit_to("main", event, serde_json::json!({ "id": id, "data": data }));
                }
            }
        }
    });
    done_rx
}

fn take_buffer(buffer: &Arc<Mutex<String>>) -> String {
    buffer.lock().unwrap().clone()
}

// ── Commands ────────────────────────────────────────────────────────

fn run_blocking(
    entry: CommandEntry,
    config_dir: PathBuf,
    args: Vec<String>,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
) -> Result<RunResult, String> {
    let mut command = build_command(&entry, &config_dir, &args)?;
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn \"{}\": {e}", entry.program))?;

    write_stdin(&mut child, stdin);

    let out_buffer = Arc::new(Mutex::new(String::new()));
    let err_buffer = Arc::new(Mutex::new(String::new()));
    let out_reader = child
        .stdout
        .take()
        .map(|pipe| collect_reader(pipe, out_buffer.clone()));
    let err_reader = child
        .stderr
        .take()
        .map(|pipe| collect_reader(pipe, err_buffer.clone()));

    let child = Arc::new(Mutex::new(child));
    let deadline = deadline_in(effective_timeout(timeout_ms, entry.timeout_ms));

    match wait_for_exit(&child, &deadline) {
        Wait::Exited(status) => {
            if let Some(reader) = out_reader {
                let _ = reader.join();
            }
            if let Some(reader) = err_reader {
                let _ = reader.join();
            }
            Ok(RunResult {
                stdout: take_buffer(&out_buffer),
                stderr: take_buffer(&err_buffer),
                code: status.code(),
                signal: exit_signal(&status),
                timed_out: false,
            })
        }
        Wait::TimedOut => {
            kill_and_reap(&child);
            // Small grace period so the readers can drain what the child
            // already wrote; joining could hang on a surviving grandchild.
            thread::sleep(Duration::from_millis(50));
            Ok(RunResult {
                stdout: take_buffer(&out_buffer),
                stderr: take_buffer(&err_buffer),
                code: None,
                signal: None,
                timed_out: true,
            })
        }
        Wait::Failed(error) => {
            kill_and_reap(&child);
            Err(error)
        }
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn shell_run(
    state: State<'_, ProcessState>,
    name: String,
    args: Option<Vec<String>>,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
) -> Result<RunResult, String> {
    let entry = state.entry(&name)?.clone();
    let config_dir = state.config_dir.clone();
    let args = args.unwrap_or_default();

    tauri::async_runtime::spawn_blocking(move || {
        run_blocking(entry, config_dir, args, timeout_ms, stdin)
    })
    .await
    .map_err(|e| format!("run command \"{name}\": {e}"))?
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_spawn(
    app: AppHandle,
    state: State<'_, ProcessState>,
    name: String,
    args: Option<Vec<String>>,
    timeout_ms: Option<u64>,
) -> Result<SpawnResult, String> {
    let entry = state.entry(&name)?.clone();
    let args = args.unwrap_or_default();
    let mut command = build_command(&entry, &state.config_dir, &args)?;
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn \"{}\": {e}", entry.program))?;

    let id = next_id();
    let pid = Some(child.id());
    let stdin = child.stdin.take();

    let mut drained = Vec::new();
    if let Some(pipe) = child.stdout.take() {
        drained.push(stream_reader(
            pipe,
            app.clone(),
            id.clone(),
            "shell://process-stdout",
        ));
    }
    if let Some(pipe) = child.stderr.take() {
        drained.push(stream_reader(
            pipe,
            app.clone(),
            id.clone(),
            "shell://process-stderr",
        ));
    }

    let child = Arc::new(Mutex::new(child));
    let deadline = deadline_in(effective_timeout(timeout_ms, entry.timeout_ms));
    let running = state.running.clone();
    running.lock().unwrap().insert(
        id.clone(),
        RunningProcess {
            stdin,
            child: child.clone(),
            deadline: deadline.clone(),
            pid: pid.unwrap_or_default(),
        },
    );

    let supervisor_id = id.clone();

    thread::spawn(move || {
        let (code, signal, timed_out) = match wait_for_exit(&child, &deadline) {
            Wait::Exited(status) => (status.code(), exit_signal(&status), false),
            Wait::TimedOut => {
                kill_and_reap(&child);
                (None, None, true)
            }
            Wait::Failed(error) => {
                eprintln!("process {supervisor_id}: {error}");
                kill_and_reap(&child);
                (None, None, false)
            }
        };

        running.lock().unwrap().remove(&supervisor_id);

        // Let the readers finish emitting before the exit event, so a consumer
        // that stops on exit can't miss the tail of the output.
        for done in &drained {
            let _ = done.recv_timeout(DRAIN_GRACE);
        }

        let _ = app.emit_to(
            "main",
            "shell://process-exit",
            serde_json::json!({
                "id": supervisor_id,
                "code": code,
                "signal": signal,
                "timedOut": timed_out,
            }),
        );
    });

    Ok(SpawnResult { id, pid })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_process_write(
    state: State<'_, ProcessState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let mut running = state.running.lock().unwrap();
    let process = running
        .get_mut(&id)
        .ok_or_else(|| format!("process {id} not found or already exited"))?;
    let stdin = process
        .stdin
        .as_mut()
        .ok_or_else(|| format!("process {id} stdin is already closed"))?;
    stdin
        .write_all(data.as_bytes())
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("write to process {id}: {e}"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_process_close_stdin(state: State<'_, ProcessState>, id: String) -> Result<(), String> {
    let mut running = state.running.lock().unwrap();
    let process = running
        .get_mut(&id)
        .ok_or_else(|| format!("process {id} not found or already exited"))?;
    process.stdin.take();
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_process_kill(state: State<'_, ProcessState>, id: String) -> Result<(), String> {
    let child = {
        let running = state.running.lock().unwrap();
        running.get(&id).map(|process| process.child.clone())
    }
    .ok_or_else(|| format!("process {id} not found or already exited"))?;

    let mut child = child.lock().unwrap();
    child.kill().map_err(|e| format!("kill process {id}: {e}"))
}

/// Asks the process to exit so it can run its own shutdown path. On Unix this
/// is `SIGTERM`, which the child may trap or ignore — follow up with a timeout
/// or `kill` if it must go. Windows has no SIGTERM, so it falls back to a
/// forceful kill and the call reports that it did.
#[cfg(unix)]
fn signal_exit(pid: u32, id: &str) -> Result<bool, String> {
    // Safety: `kill(2)` with a pid we own; failure is reported via errno.
    if unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) } == 0 {
        return Ok(true);
    }
    Err(format!(
        "signal process {id}: {}",
        std::io::Error::last_os_error()
    ))
}

#[cfg(not(unix))]
fn signal_exit(_pid: u32, _id: &str) -> Result<bool, String> {
    Ok(false)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitRequest {
    /// `false` when the platform has no graceful signal and the child was
    /// killed outright instead.
    pub graceful: bool,
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_process_exit(
    state: State<'_, ProcessState>,
    id: String,
) -> Result<ExitRequest, String> {
    let (child, pid) = {
        let running = state.running.lock().unwrap();
        running
            .get(&id)
            .map(|process| (process.child.clone(), process.pid))
    }
    .ok_or_else(|| format!("process {id} not found or already exited"))?;

    if signal_exit(pid, &id)? {
        return Ok(ExitRequest { graceful: true });
    }

    let mut child = child.lock().unwrap();
    child
        .kill()
        .map_err(|e| format!("kill process {id}: {e}"))
        .map(|_| ExitRequest { graceful: false })
}

/// Moves a running process's deadline to `timeoutMs` from now, or clears it
/// when `timeoutMs` is null. The supervisor re-reads it on its next poll.
#[tauri::command(rename_all = "camelCase")]
pub fn shell_process_set_timeout(
    state: State<'_, ProcessState>,
    id: String,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    let deadline = {
        let running = state.running.lock().unwrap();
        running.get(&id).map(|process| process.deadline.clone())
    }
    .ok_or_else(|| format!("process {id} not found or already exited"))?;

    *deadline.lock().unwrap() = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_list_commands(state: State<'_, ProcessState>) -> Vec<CommandInfo> {
    state.list()
}

#[cfg(test)]
mod tests {
    use super::validate_args;
    use crate::config::CommandEntry;

    fn entry(
        args: Option<&[&str]>,
        extra_args: Option<&str>,
        max_args: Option<usize>,
    ) -> CommandEntry {
        CommandEntry {
            name: "git".into(),
            program: "git".into(),
            args: args.map(|patterns| patterns.iter().map(|p| p.to_string()).collect()),
            extra_args: extra_args.map(str::to_string),
            max_args,
            cwd: None,
            timeout_ms: None,
            env: None,
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn no_patterns_allows_anything() {
        let entry = entry(None, None, None);
        assert!(validate_args(&entry, &args(&["push", "--force", "origin"])).is_ok());
    }

    #[test]
    fn positional_patterns_match_and_mismatch() {
        let entry = entry(Some(&["^(status|log|diff)$", "^--oneline$"]), None, None);
        assert!(validate_args(&entry, &args(&["log", "--oneline"])).is_ok());

        let error = validate_args(&entry, &args(&["push"])).unwrap_err();
        assert_eq!(
            error,
            "command \"git\": argument 1 (\"push\") does not match allowed pattern ^(status|log|diff)$"
        );
    }

    #[test]
    fn extra_args_rejected_without_extra_args_pattern() {
        let entry = entry(Some(&["^status$"]), None, None);
        let error = validate_args(&entry, &args(&["status", "--short"])).unwrap_err();
        assert!(error.contains("argument 2 (\"--short\") is not allowed"));
    }

    #[test]
    fn extra_args_accepted_with_extra_args_pattern() {
        let entry = entry(Some(&["^status$"]), Some(r"^[\w./-]+$"), None);
        assert!(validate_args(&entry, &args(&["status", "src/main.rs"])).is_ok());
        assert!(validate_args(&entry, &args(&["status", "; rm -rf /"])).is_err());
    }

    #[test]
    fn extra_args_alone_applies_to_every_argument() {
        let entry = entry(None, Some("^[a-z]+$"), None);
        assert!(validate_args(&entry, &args(&["one", "two"])).is_ok());
        assert!(validate_args(&entry, &args(&["one", "TWO"])).is_err());
    }

    #[test]
    fn max_args_caps_argument_count() {
        let entry = entry(None, None, Some(2));
        assert!(validate_args(&entry, &args(&["a", "b"])).is_ok());
        let error = validate_args(&entry, &args(&["a", "b", "c"])).unwrap_err();
        assert!(error.contains("maxArgs is 2"));
    }

    #[test]
    fn patterns_are_implicitly_anchored() {
        let entry = entry(Some(&["status"]), None, None);
        assert!(validate_args(&entry, &args(&["status"])).is_ok());
        assert!(validate_args(&entry, &args(&["xstatusy"])).is_err());
    }

    #[test]
    fn invalid_regex_surfaces_an_error() {
        let entry = entry(Some(&["^(unclosed"]), None, None);
        let error = validate_args(&entry, &args(&["anything"])).unwrap_err();
        assert!(error.starts_with("command \"git\": invalid regex \"^(unclosed\""));
    }

    #[test]
    fn fewer_args_than_patterns_is_ok() {
        let entry = entry(Some(&["^status$", "^--short$"]), None, None);
        assert!(validate_args(&entry, &args(&["status"])).is_ok());
        assert!(validate_args(&entry, &[]).is_ok());
    }
}

// ── Real-process behaviour (Unix: relies on sh/cat/sleep) ───────────
#[cfg(all(test, unix))]
mod process_tests {
    use super::run_blocking;
    use crate::config::CommandEntry;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn sh(script: &str) -> (CommandEntry, Vec<String>) {
        (
            CommandEntry {
                name: "sh".into(),
                program: "sh".into(),
                args: None,
                extra_args: None,
                max_args: None,
                cwd: None,
                timeout_ms: None,
                env: None,
            },
            vec!["-c".to_string(), script.to_string()],
        )
    }

    fn run(
        entry: CommandEntry,
        args: Vec<String>,
        timeout: Option<u64>,
        stdin: Option<&str>,
    ) -> super::RunResult {
        run_blocking(
            entry,
            PathBuf::from("."),
            args,
            timeout,
            stdin.map(str::to_string),
        )
        .expect("run_blocking should not error")
    }

    #[test]
    fn captures_stdout_stderr_and_exit_code() {
        let (entry, args) = sh("echo out; echo err 1>&2; exit 3");
        let result = run(entry, args, None, None);
        assert_eq!(result.stdout.trim(), "out");
        assert_eq!(result.stderr.trim(), "err");
        assert_eq!(result.code, Some(3));
        assert_eq!(result.signal, None);
        assert!(!result.timed_out);
    }

    #[test]
    fn stdin_is_written_and_closed() {
        // `cat` only terminates if the stdin pipe is closed after writing.
        let entry = CommandEntry {
            name: "cat".into(),
            program: "cat".into(),
            args: None,
            extra_args: None,
            max_args: None,
            cwd: None,
            timeout_ms: Some(5000),
            env: None,
        };
        let result = run(entry, vec![], None, Some("piped input\n"));
        assert_eq!(result.stdout.trim(), "piped input");
        assert_eq!(result.code, Some(0));
        assert!(!result.timed_out, "cat must see EOF, not hit the timeout");
    }

    #[test]
    fn timeout_kills_and_reports_partial_output() {
        let (entry, args) = sh("echo early; sleep 30");
        let result = run(entry, args, Some(400), None);
        assert!(result.timed_out);
        assert_eq!(result.code, None, "a killed process has no exit code");
        assert_eq!(
            result.stdout.trim(),
            "early",
            "output before the kill survives"
        );
    }

    #[test]
    fn per_call_timeout_overrides_the_entry_timeout() {
        let (mut entry, args) = sh("sleep 30");
        entry.timeout_ms = Some(60_000);
        let result = run(entry, args, Some(300), None);
        assert!(result.timed_out, "the per-call timeout must win");
    }

    #[test]
    fn config_cwd_and_env_are_applied() {
        let (mut entry, args) = sh("pwd; echo $SHELL_TEST_VAR");
        entry.cwd = Some("src".into());
        entry.env = Some(HashMap::from([(
            "SHELL_TEST_VAR".to_string(),
            "from-config".to_string(),
        )]));
        // cwd resolves relative to the config dir, here the crate root.
        let result = run_blocking(
            entry,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            args,
            None,
            None,
        )
        .expect("run_blocking should not error");
        assert!(
            result.stdout.contains("src-tauri/src"),
            "cwd: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("from-config"),
            "env: {}",
            result.stdout
        );
    }

    #[test]
    fn deadline_can_be_extended_while_the_process_runs() {
        use super::{deadline_in, wait_for_exit, Wait};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let (entry, args) = sh("sleep 1; echo survived");
        let mut command =
            super::build_command(&entry, &PathBuf::from("."), &args).expect("build_command");
        let child = Arc::new(Mutex::new(command.spawn().expect("spawn")));

        // Starts with a deadline that would fire well before the child exits.
        let deadline = deadline_in(Some(Duration::from_millis(200)));

        // Push it out before it fires — the poller must pick up the new value.
        let extend = deadline.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            *extend.lock().unwrap() = Some(std::time::Instant::now() + Duration::from_secs(10));
        });

        match wait_for_exit(&child, &deadline) {
            Wait::Exited(status) => assert!(status.success(), "child should run to completion"),
            Wait::TimedOut => panic!("extending the deadline at runtime did not take effect"),
            Wait::Failed(e) => panic!("wait failed: {e}"),
        }
    }

    #[test]
    fn deadline_can_be_shortened_while_the_process_runs() {
        use super::{deadline_in, wait_for_exit, Wait};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let (entry, args) = sh("sleep 30");
        let mut command =
            super::build_command(&entry, &PathBuf::from("."), &args).expect("build_command");
        let child = Arc::new(Mutex::new(command.spawn().expect("spawn")));

        // No timeout at spawn time; one is set at runtime instead.
        let deadline = deadline_in(None);
        let shorten = deadline.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            *shorten.lock().unwrap() = Some(std::time::Instant::now() + Duration::from_millis(50));
        });

        let started = std::time::Instant::now();
        match wait_for_exit(&child, &deadline) {
            Wait::TimedOut => {}
            other => panic!(
                "expected a runtime-set deadline to fire, got {}",
                match other {
                    Wait::Exited(_) => "exit",
                    Wait::Failed(_) => "failure",
                    Wait::TimedOut => unreachable!(),
                }
            ),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "should have fired at the runtime deadline, not waited for the child"
        );
        super::kill_and_reap(&child);
    }

    #[cfg(unix)]
    #[test]
    fn exit_sends_a_signal_the_child_can_trap() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        // Traps SIGTERM, prints, and exits 42 — proving a graceful path ran
        // rather than an untrappable SIGKILL.
        let (entry, args) =
            sh("trap 'echo cleaned; exit 42' TERM; while true; do sleep 0.05; done");
        let mut command =
            super::build_command(&entry, &PathBuf::from("."), &args).expect("build_command");
        let mut spawned = command.spawn().expect("spawn");
        let pid = spawned.id();
        let out = spawned.stdout.take().expect("stdout");
        let buffer = Arc::new(Mutex::new(String::new()));
        let reader = super::collect_reader(out, buffer.clone());

        std::thread::sleep(Duration::from_millis(200));
        assert!(
            super::signal_exit(pid, "test").expect("signal"),
            "SIGTERM sent"
        );

        let status = spawned.wait().expect("wait");
        let _ = reader.join();
        assert_eq!(
            status.code(),
            Some(42),
            "the trap handler chose the exit code"
        );
        assert_eq!(buffer.lock().unwrap().trim(), "cleaned");
    }

    #[test]
    fn missing_executable_is_an_error_not_a_result() {
        let entry = CommandEntry {
            name: "nope".into(),
            program: "definitely-not-a-real-program-xyz".into(),
            args: None,
            extra_args: None,
            max_args: None,
            cwd: None,
            timeout_ms: None,
            env: None,
        };
        let error = run_blocking(entry, PathBuf::from("."), vec![], None, None)
            .expect_err("a missing executable must reject");
        assert!(
            error.contains("definitely-not-a-real-program-xyz"),
            "{error}"
        );
    }
}
