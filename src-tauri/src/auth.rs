use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub struct AuthState {
    pub port: Mutex<u16>,
    pub shutdown_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub pending: Arc<Mutex<HashMap<String, PendingAuth>>>,
}

pub struct PendingAuth {
    result_tx: Option<std::sync::mpsc::Sender<Result<String, String>>>,
}

impl AuthState {
    pub fn new() -> Self {
        AuthState {
            port: Mutex::new(0),
            shutdown_tx: Mutex::new(None),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for AuthState {
    fn default() -> Self {
        Self::new()
    }
}

static AUTH_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_auth_id() -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(AUTH_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", hasher.finish())
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

fn response_html() -> &'static [u8] {
    b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n<!DOCTYPE html><html><body><p>Sign-in complete. You can close this tab and return to the desktop app.</p></body></html>"
}

fn response_404() -> &'static [u8] {
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
}

fn extract_query_params(target: &str) -> Vec<(String, String)> {
    let path = target.split('#').next().unwrap_or(target);
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or_default();
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.to_string();
            let value = parts.next().unwrap_or_default().to_string();
            Some((key, value))
        })
        .collect()
}

fn parse_incoming_request(
    stream: &mut TcpStream,
) -> Result<(String, String, Vec<(String, String)>), String> {
    let mut buffer = [0u8; 4096];
    let read = stream
        .read(&mut buffer)
        .map_err(|e| format!("read auth callback: {e}"))?;
    if read == 0 {
        return Err("empty request".into());
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let params = extract_query_params(&target);

    Ok((method, target, params))
}

fn ensure_auth_listener(app: &AppHandle) -> Result<u16, String> {
    let state = app.state::<AuthState>();
    let mut port = state.port.lock().unwrap();
    if *port != 0 {
        return Ok(*port);
    }

    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind auth listener: {e}"))?;
    let actual_port = listener
        .local_addr()
        .map_err(|e| format!("get auth listener addr: {e}"))?
        .port();

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    *state.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

    let pending = state.pending.clone();

    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .map_err(|e| eprintln!("auth listener nonblocking: {e}"))
            .ok();

        loop {
            match shutdown_rx.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }

            match listener.accept() {
                Ok((mut stream, _)) => {
                    let request_line = match parse_incoming_request(&mut stream) {
                        Ok((_, target, params)) => (target, params),
                        Err(_) => {
                            let _ = stream.write_all(response_404());
                            continue;
                        }
                    };

                    let (_target, params) = request_line;

                    let sid = params
                        .iter()
                        .find(|(k, _)| k == "sid")
                        .map(|(_, v)| v.clone());

                    let mut pending_lock = pending.lock().unwrap();

                    let matched_key = sid.as_ref().and_then(|sid| {
                        for key in pending_lock.keys() {
                            if key.contains(&format!("sid={}", sid)) {
                                return Some(key.clone());
                            }
                        }
                        None
                    });

                    if let Some(key) = matched_key {
                        let auth_code = params
                            .iter()
                            .find(|(k, _)| k == "authCode")
                            .map(|(_, v)| v.clone());

                        let error = params
                            .iter()
                            .find(|(k, _)| k == "error")
                            .map(|(_, v)| v.clone());

                        let result = if let Some(err) = error {
                            Err(format!("authentication error: {err}"))
                        } else if let Some(code) = auth_code {
                            Ok(code)
                        } else {
                            continue;
                        };

                        let _ = stream.write_all(response_html());

                        if let Some(mut entry) = pending_lock.remove(&key) {
                            if let Some(tx) = entry.result_tx.take() {
                                let _ = tx.send(result);
                            }
                        }
                    } else {
                        let _ = stream.write_all(response_404());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("auth accept error: {e}");
                }
            }
        }
    });

    *port = actual_port;
    Ok(actual_port)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn shell_auth_via_browser(
    app: AppHandle,
    auth_url: String,
    timeout_ms: Option<u64>,
    return_url: Option<String>,
) -> Result<String, String> {
    let port = ensure_auth_listener(&app)?;
    let state = app.state::<AuthState>();
    let timeout = timeout_ms.unwrap_or(120_000);

    let sid = random_auth_id();
    let effective_return_url = match return_url {
        Some(url) if !url.trim().is_empty() => {
            let base = url.trim().to_string();
            let sep = if base.contains('?') { '&' } else { '?' };
            format!("{base}{sep}sid={sid}")
        }
        _ => format!("http://127.0.0.1:{port}/callback?sid={sid}"),
    };

    let (tx, rx) = std::sync::mpsc::channel::<Result<String, String>>();

    {
        let mut pending = state.pending.lock().unwrap();
        pending.insert(
            effective_return_url.clone(),
            PendingAuth {
                result_tx: Some(tx),
            },
        );
    }

    let sep = if auth_url.contains('?') { '&' } else { '?' };
    let browser_url = format!(
        "{auth_url}{sep}returnUrl={}",
        percent_encode_return_url(&effective_return_url)
    );

    open::that(&browser_url).map_err(|e| format!("open browser: {e}"))?;

    let app_clone = app.clone();
    let url_clone = effective_return_url.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let channel_result = rx.recv_timeout(Duration::from_millis(timeout));

        if channel_result.is_err() {
            app_clone
                .state::<AuthState>()
                .pending
                .lock()
                .unwrap()
                .remove(&url_clone);
        }

        channel_result
            .map_err(|e| match e {
                RecvTimeoutError::Timeout => {
                    "authentication timed out waiting for browser callback".to_string()
                }
                RecvTimeoutError::Disconnected => "auth flow cancelled".to_string(),
            })?
    })
    .await
    .map_err(|e| format!("auth task: {e}"))?
}
