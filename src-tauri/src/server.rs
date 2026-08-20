use serde::Serialize;
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

// ── HTTP Server state ───────────────────────────────────────────────

pub struct HttpServerState {
    pub port: Mutex<Option<u16>>,
    pub shutdown_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub pending: Arc<Mutex<HashMap<String, PendingHttpRequest>>>,
}

pub struct PendingHttpRequest {
    pub request: Option<tiny_http::Request>,
}

impl Default for HttpServerState {
    fn default() -> Self {
        HttpServerState {
            port: Mutex::new(None),
            shutdown_tx: Mutex::new(None),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

// ── WebSocket Server state ──────────────────────────────────────────

pub struct WsServerState {
    pub port: Mutex<Option<u16>>,
    pub shutdown_tx: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub connections: Arc<Mutex<HashMap<String, WsConnectionHandle>>>,
}

pub struct WsConnectionHandle {
    pub cmd_tx: std::sync::mpsc::Sender<WsCommand>,
}

pub enum WsCommand {
    Send(String),
    Close,
}

impl Default for WsServerState {
    fn default() -> Self {
        WsServerState {
            port: Mutex::new(None),
            shutdown_tx: Mutex::new(None),
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Serialize)]
pub struct ServerStarted {
    pub port: u16,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_id(prefix: &str) -> String {
    format!("{}-{}", prefix, ID_COUNTER.fetch_add(1, Ordering::Relaxed))
}

// ── HTTP Server commands ────────────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub fn shell_http_start(
    app: AppHandle,
    state: State<'_, HttpServerState>,
    port: Option<u16>,
) -> Result<ServerStarted, String> {
    let mut port_lock = state.port.lock().unwrap();
    if port_lock.is_some() {
        return Err("HTTP server is already running".into());
    }

    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let server = tiny_http::Server::http(&addr).map_err(|e| format!("start HTTP server: {e}"))?;
    let addr_str = format!("{}", server.server_addr());
    let actual_port = addr_str
        .rsplit(':')
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    *state.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

    let pending = state.pending.clone();

    thread::spawn(move || loop {
        if shutdown_rx.try_recv().is_ok() {
            break;
        }

        match server.try_recv() {
            Ok(Some(mut request)) => {
                let id = next_id("http-req");
                let method = request.method().to_string();
                let url = request.url().to_string();
                let headers: HashMap<String, String> = request
                    .headers()
                    .iter()
                    .map(|h| (h.field.to_string(), h.value.to_string()))
                    .collect();
                let mut body_buf = String::new();
                request.as_reader().read_to_string(&mut body_buf).ok();

                {
                    let mut p = pending.lock().unwrap();
                    p.insert(
                        id.clone(),
                        PendingHttpRequest {
                            request: Some(request),
                        },
                    );
                }

                let _ = app.emit_to(
                    "main",
                    "shell://http-request",
                    serde_json::json!({
                        "id": id,
                        "method": method,
                        "url": url,
                        "headers": headers,
                        "body": body_buf,
                    }),
                );
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                eprintln!("HTTP server recv error: {e}");
            }
        }
    });

    *port_lock = Some(actual_port);
    Ok(ServerStarted { port: actual_port })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_http_respond(
    state: State<'_, HttpServerState>,
    id: String,
    status: u16,
    headers: Option<HashMap<String, String>>,
    body: Option<String>,
) -> Result<(), String> {
    let mut pending = state.pending.lock().unwrap();
    let entry = pending
        .remove(&id)
        .ok_or_else(|| format!("HTTP request {id} not found or already responded"))?;
    let request = entry
        .request
        .ok_or_else(|| String::from("request already consumed"))?;

    let mut response =
        tiny_http::Response::from_string(body.unwrap_or_default()).with_status_code(status);

    if let Some(hdrs) = headers {
        for (key, value) in hdrs {
            if let Ok(header) = tiny_http::Header::from_bytes(key.as_bytes(), value.as_bytes()) {
                response = response.with_header(header);
            }
        }
    }

    request
        .respond(response)
        .map_err(|e| format!("send HTTP response: {e}"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_http_stop(state: State<'_, HttpServerState>) -> Result<(), String> {
    let mut port = state.port.lock().unwrap();
    *port = None;
    let mut shutdown_tx = state.shutdown_tx.lock().unwrap();
    shutdown_tx.take();
    Ok(())
}

// ── WebSocket Server commands ───────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ws_start(
    app: AppHandle,
    state: State<'_, WsServerState>,
    port: Option<u16>,
) -> Result<ServerStarted, String> {
    let mut port_lock = state.port.lock().unwrap();
    if port_lock.is_some() {
        return Err("WebSocket server is already running".into());
    }

    let addr = format!("127.0.0.1:{}", port.unwrap_or(0));
    let listener = TcpListener::bind(&addr).map_err(|e| format!("start WS server: {e}"))?;
    let actual_port = listener
        .local_addr()
        .map_err(|e| format!("get WS addr: {e}"))?
        .port();

    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();
    *state.shutdown_tx.lock().unwrap() = Some(shutdown_tx);

    let connections = state.connections.clone();

    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .map_err(|e| eprintln!("WS listener nonblocking: {e}"))
            .ok();

        loop {
            if shutdown_rx.try_recv().is_ok() {
                let mut conns = connections.lock().unwrap();
                for (_, handle) in conns.drain() {
                    let _ = handle.cmd_tx.send(WsCommand::Close);
                }
                break;
            }

            match listener.accept() {
                Ok((stream, _)) => {
                    let connections = connections.clone();
                    let app = app.clone();
                    thread::spawn(move || {
                        handle_ws_connection(stream, connections, app);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    eprintln!("WS accept error: {e}");
                }
            }
        }
    });

    *port_lock = Some(actual_port);
    Ok(ServerStarted { port: actual_port })
}

fn handle_ws_connection(
    stream: std::net::TcpStream,
    connections: Arc<Mutex<HashMap<String, WsConnectionHandle>>>,
    app: AppHandle,
) {
    let mut ws = match tungstenite::accept(stream) {
        Ok(ws) => ws,
        Err(e) => {
            eprintln!("WS accept error: {e}");
            return;
        }
    };

    let conn_id = next_id("ws");
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<WsCommand>();

    {
        let mut conns = connections.lock().unwrap();
        conns.insert(
            conn_id.clone(),
            WsConnectionHandle {
                cmd_tx: cmd_tx.clone(),
            },
        );
    }

    let _ = app.emit_to(
        "main",
        "shell://ws-connection",
        serde_json::json!({ "id": conn_id }),
    );

    if let Err(e) = ws.get_ref().set_nonblocking(true) {
        eprintln!("WS nonblocking: {e}");
    }

    let mut closed = false;

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                WsCommand::Send(data) => {
                    if ws.send(tungstenite::Message::Text(data)).is_err() {
                        closed = true;
                    }
                }
                WsCommand::Close => {
                    let _ = ws.close(None);
                    closed = true;
                }
            }
        }

        if closed {
            break;
        }

        match ws.read() {
            Ok(tungstenite::Message::Text(text)) => {
                let _ = app.emit_to(
                    "main",
                    "shell://ws-message",
                    serde_json::json!({ "id": conn_id, "data": text }),
                );
            }
            Ok(tungstenite::Message::Close(_)) => {
                closed = true;
            }
            Ok(tungstenite::Message::Ping(_)) => {}
            Ok(tungstenite::Message::Pong(_)) => {}
            Ok(tungstenite::Message::Binary(_)) => {}
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                closed = true;
            }
        }

        if closed {
            break;
        }
    }

    connections.lock().unwrap().remove(&conn_id);
    let _ = app.emit_to(
        "main",
        "shell://ws-close",
        serde_json::json!({ "id": conn_id }),
    );
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ws_send(
    state: State<'_, WsServerState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let connections = state.connections.lock().unwrap();
    let handle = connections
        .get(&id)
        .ok_or_else(|| format!("WebSocket connection {id} not found"))?;
    handle
        .cmd_tx
        .send(WsCommand::Send(data))
        .map_err(|_| format!("WebSocket connection {id} closed"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ws_close(state: State<'_, WsServerState>, id: String) -> Result<(), String> {
    let connections = state.connections.lock().unwrap();
    let handle = connections
        .get(&id)
        .ok_or_else(|| format!("WebSocket connection {id} not found"))?;
    handle
        .cmd_tx
        .send(WsCommand::Close)
        .map_err(|_| format!("WebSocket connection {id} closed"))
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ws_stop(state: State<'_, WsServerState>) -> Result<(), String> {
    let mut port = state.port.lock().unwrap();
    *port = None;
    let mut shutdown_tx = state.shutdown_tx.lock().unwrap();
    shutdown_tx.take();
    Ok(())
}
