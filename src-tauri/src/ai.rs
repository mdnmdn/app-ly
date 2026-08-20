//! On-device AI (`window.shell.ai`).
//!
//! The public surface here is deliberately vendor-neutral: command names,
//! event names, model ids and reason codes never name a platform vendor.
//! Vendor-specific code lives behind the `Backend` trait in `ai/backend_*.rs`,
//! selected once at compile time so no command body carries a `#[cfg]`.

use crate::config::AiConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[cfg(all(target_os = "macos", feature = "ai-apple"))]
mod backend_apple;
#[cfg(not(all(target_os = "macos", feature = "ai-apple")))]
mod backend_stub;

#[cfg(all(target_os = "macos", feature = "ai-apple"))]
use backend_apple::new_backend;
#[cfg(not(all(target_os = "macos", feature = "ai-apple")))]
use backend_stub::new_backend;

// ── Reason codes (closed set — the JS API and docs mirror these) ─────

/// Only the stub backend reports this one.
#[allow(dead_code)]
pub(crate) const REASON_UNSUPPORTED_PLATFORM: &str = "unsupported-platform";
pub(crate) const REASON_DISABLED_BY_CONFIG: &str = "disabled-by-config";

// The remaining codes describe states only an OS-level model reports, so they
// reach the wire through `reason_for` and a platform backend. A build without
// one leaves them unused by design, not by mistake.
#[allow(dead_code)]
pub(crate) const REASON_UNSUPPORTED_OS: &str = "unsupported-os";
#[allow(dead_code)]
pub(crate) const REASON_DEVICE_NOT_ELIGIBLE: &str = "device-not-eligible";
#[allow(dead_code)]
pub(crate) const REASON_NOT_ENABLED: &str = "not-enabled";
#[allow(dead_code)]
pub(crate) const REASON_MODEL_NOT_READY: &str = "model-not-ready";
#[allow(dead_code)]
pub(crate) const REASON_UNAVAILABLE: &str = "unavailable";

/// The single model id this shell exposes. The platform backend has no model
/// catalog, so `shell_ai_info` reports exactly one non-selectable entry.
const DEFAULT_MODEL_ID: &str = "default";

/// Reported as the `error` of a cancelled stream, and as the rejection of a
/// cancelled one-shot request.
const CANCELLED: &str = "cancelled";

/// Why the backend cannot generate right now.
pub(crate) struct Unavailable {
    pub reason: &'static str,
    pub detail: Option<String>,
}

/// The states an OS-level model can report. Kept separate from the wire
/// strings so the backend maps its vendor enum onto this once, and the
/// reason-code table stays unit-testable in builds without that backend.
///
/// Variants are constructed by the platform backend only; the stub build
/// exercises the mapping from tests.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OsUnavailable {
    DeviceNotEligible,
    NotEnabled,
    ModelNotReady,
    OsTooOld,
    Unknown,
}

#[allow(dead_code)]
pub(crate) fn reason_for(state: OsUnavailable) -> &'static str {
    match state {
        OsUnavailable::DeviceNotEligible => REASON_DEVICE_NOT_ELIGIBLE,
        OsUnavailable::NotEnabled => REASON_NOT_ENABLED,
        OsUnavailable::ModelNotReady => REASON_MODEL_NOT_READY,
        OsUnavailable::OsTooOld => REASON_UNSUPPORTED_OS,
        OsUnavailable::Unknown => REASON_UNAVAILABLE,
    }
}

/// The one place the rejection message for an unavailable backend is built.
pub(crate) fn unavailable_message(state: &Unavailable) -> String {
    match &state.detail {
        Some(detail) => format!("ai unavailable: {} — {detail}", state.reason),
        None => format!("ai unavailable: {}", state.reason),
    }
}

// ── Backend interface ───────────────────────────────────────────────

/// Runs one tool call on the JS side and returns whatever should be handed
/// back to the model. A failure is already folded into the returned value as
/// `{ "error": ... }`, so a backend never has to decide whether to abort.
pub(crate) type ToolDispatch = Arc<dyn Fn(&str, Value) -> Value + Send + Sync>;

/// Everything one generation needs. Built in `ai.rs`, consumed by a backend.
///
/// The stub backend rejects before reading any of it, so the fields look dead
/// in a build without a platform backend.
#[allow(dead_code)]
pub(crate) struct GenerateRequest {
    pub prompt: String,
    pub instructions: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    /// Tool declarations, already stripped of their JS handlers.
    pub tools: Vec<ToolSpec>,
    pub dispatch: ToolDispatch,
    /// Backend-dialect schema for structured output, `None` for plain text.
    pub schema: Option<String>,
}

pub(crate) trait Backend: Send + Sync {
    /// `None` when the backend can generate right now.
    fn availability(&self) -> Option<Unavailable>;
    /// Human-readable label for the single model entry.
    fn model_label(&self) -> &'static str;
    fn features(&self) -> AiFeatures;
    fn generate(&self, request: GenerateRequest) -> Result<String, String>;
    /// Returns the model's JSON output as a string.
    fn generate_object(&self, request: GenerateRequest) -> Result<String, String>;
    /// Blocks until generation finishes, calling `sink` with each text delta.
    /// Because it only returns after the last delta has been handed to `sink`,
    /// the caller can emit its completion event straight afterwards without
    /// racing a reader thread.
    fn stream(
        &self,
        request: GenerateRequest,
        sink: Box<dyn FnMut(&str) + Send + 'static>,
    ) -> Result<String, String>;
}

// ── Wire types ──────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiOptions {
    pub model: Option<String>,
    pub instructions: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub tools: Vec<ToolSpec>,
}

/// One tool the model may call. JS strips the `handler` before sending, so
/// Rust only ever sees the declaration. Only a platform backend reads the
/// fields; the stub rejects before it gets that far.
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiFeatures {
    pub text: bool,
    pub structured: bool,
    pub tools: bool,
    pub streaming: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiInfo {
    pub available: bool,
    pub reason: Option<String>,
    pub detail: Option<String>,
    pub models: Vec<AiModel>,
    pub features: AiFeatures,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: Value,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiResult {
    pub text: String,
    pub model: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiObjectResult {
    pub object: Value,
    pub model: String,
    pub tool_calls: Vec<ToolCallRecord>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStreamHandle {
    pub id: String,
}

// ── Settings ────────────────────────────────────────────────────────

pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 30_000;

/// `[ai]` with every default already applied.
#[derive(Debug, Clone)]
pub struct AiSettings {
    pub enabled: bool,
    pub instructions: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub tool_timeout: Duration,
}

impl Default for AiSettings {
    fn default() -> Self {
        AiSettings {
            enabled: true,
            instructions: None,
            temperature: None,
            max_tokens: None,
            tool_timeout: Duration::from_millis(DEFAULT_TOOL_TIMEOUT_MS),
        }
    }
}

impl AiSettings {
    pub fn from_config(config: Option<&AiConfig>) -> Self {
        let Some(config) = config else {
            return AiSettings::default();
        };
        AiSettings {
            enabled: config.enabled.unwrap_or(true),
            instructions: config.instructions.clone(),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            tool_timeout: Duration::from_millis(
                config.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS),
            ),
        }
    }
}

// ── State ───────────────────────────────────────────────────────────

/// One JS answer to a `shell://ai-tool-call`.
struct ToolReply {
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
}

/// Pending tool calls keyed by an unguessable call id — the same shape as
/// `EvalState` in `commands.rs`, but with a blocking receiver because the
/// waiter is the inference thread, not a future.
type PendingTools = Arc<Mutex<HashMap<String, mpsc::Sender<ToolReply>>>>;

/// Cancel flags for in-flight requests, keyed by the caller-supplied id.
type Requests = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;

pub struct AiState {
    settings: AiSettings,
    backend: Arc<dyn Backend>,
    pending_tools: PendingTools,
    requests: Requests,
}

impl AiState {
    pub fn new(settings: AiSettings) -> Self {
        AiState {
            settings,
            backend: new_backend(),
            pending_tools: Arc::new(Mutex::new(HashMap::new())),
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// `None` when a generation may proceed.
    fn availability(&self) -> Option<Unavailable> {
        if !self.settings.enabled {
            return Some(Unavailable {
                reason: REASON_DISABLED_BY_CONFIG,
                detail: Some("set [ai] enabled = true in app.toml to turn it on".into()),
            });
        }
        self.backend.availability()
    }

    fn require_available(&self) -> Result<(), String> {
        match self.availability() {
            Some(state) => Err(unavailable_message(&state)),
            None => Ok(()),
        }
    }
}

static CALL_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Tool call ids must be unguessable, not merely unique: any page that can
/// reach `shell_ai_tool_result` could otherwise answer someone else's call.
/// Request ids are *not* minted here — those come from the caller, so there
/// is exactly one source for them.
fn random_call_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(CALL_COUNTER.fetch_add(1, Ordering::Relaxed));
    format!("{:016x}", hasher.finish())
}

// ── Request registry ────────────────────────────────────────────────

/// Claims a caller-supplied request id and hands back its cancel flag.
/// Rejects a duplicate rather than overwriting: two live requests sharing an
/// id would cross-wire their tool handlers.
fn register_request(requests: &Requests, id: &str) -> Result<Arc<AtomicBool>, String> {
    let mut in_flight = requests.lock().unwrap();
    if in_flight.contains_key(id) {
        return Err(format!("ai request \"{id}\" is already in flight"));
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    in_flight.insert(id.to_string(), cancelled.clone());
    Ok(cancelled)
}

fn finish_request(requests: &Requests, id: &str) {
    requests.lock().unwrap().remove(id);
}

/// Releases the request id however the command exits, including on an early
/// `?` return.
struct RequestGuard {
    requests: Requests,
    id: String,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        finish_request(&self.requests, &self.id);
    }
}

// ── Tool bridge ─────────────────────────────────────────────────────

fn register_tool_call(pending: &PendingTools, call_id: &str) -> mpsc::Receiver<ToolReply> {
    let (tx, rx) = mpsc::channel::<ToolReply>();
    pending.lock().unwrap().insert(call_id.to_string(), tx);
    rx
}

/// Waits for the JS answer, or gives up after `timeout` so a hung handler
/// can never hang the app. Always de-registers the call id before returning.
fn await_tool_reply(
    pending: &PendingTools,
    call_id: &str,
    name: &str,
    rx: &mpsc::Receiver<ToolReply>,
    timeout: Duration,
) -> Result<Value, String> {
    let outcome = match rx.recv_timeout(timeout) {
        Ok(reply) if reply.ok => Ok(reply.value.unwrap_or(Value::Null)),
        Ok(reply) => Err(reply
            .error
            .unwrap_or_else(|| format!("tool \"{name}\" failed"))),
        Err(_) => Err(format!(
            "tool \"{name}\" did not answer within {}ms",
            timeout.as_millis()
        )),
    };
    pending.lock().unwrap().remove(call_id);
    outcome
}

fn deliver_tool_result(
    pending: &PendingTools,
    call_id: &str,
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
) {
    // An unknown or already-expired call id is a silent no-op.
    if let Some(tx) = pending.lock().unwrap().remove(call_id) {
        let _ = tx.send(ToolReply { ok, value, error });
    }
}

/// Builds the closure a backend calls when the model asks for a tool. Every
/// attempt lands in `records` with exactly one of `result` / `error` set.
fn tool_dispatcher(
    app: AppHandle,
    request_id: String,
    pending: PendingTools,
    timeout: Duration,
    records: Arc<Mutex<Vec<ToolCallRecord>>>,
) -> ToolDispatch {
    Arc::new(move |name: &str, arguments: Value| -> Value {
        let call_id = random_call_id();
        let rx = register_tool_call(&pending, &call_id);

        let _ = app.emit_to(
            "main",
            "shell://ai-tool-call",
            json!({
                "callId": call_id,
                "id": request_id,
                "name": name,
                "arguments": arguments.clone(),
            }),
        );

        let outcome = await_tool_reply(&pending, &call_id, name, &rx, timeout);
        let record = ToolCallRecord {
            name: name.to_string(),
            arguments,
            result: outcome.as_ref().ok().cloned(),
            error: outcome.as_ref().err().cloned(),
        };
        records.lock().unwrap().push(record);

        // A failed tool is reported to the model as data, never as an abort:
        // an unknown tool or a slow handler must not kill the generation.
        match outcome {
            Ok(value) => value,
            Err(error) => json!({ "error": error }),
        }
    })
}

// ── JSON Schema translation ─────────────────────────────────────────

/// Translates a caller-supplied JSON Schema into the dialect the backend's
/// dynamic-schema builder understands. The differences that matter:
///
/// - optionality is a per-property `optional` flag, not a `required` array
/// - array bounds are `min` / `max`, not `minItems` / `maxItems`
/// - `enum` / `anyOf` / `oneOf` become an `any_of` node with `choices`
/// - value constraints become `guides` entries
pub(crate) fn translate_schema(schema: &Value, name: &str) -> Result<Value, String> {
    let Some(object) = schema.as_object() else {
        return Err(format!("schema at \"{name}\" must be a JSON object"));
    };

    if let Some(choices) = object.get("anyOf").or_else(|| object.get("oneOf")) {
        return translate_any_of(object, choices, name);
    }

    if let Some(values) = object.get("enum") {
        return translate_enum(object, values, name);
    }

    match schema_type(object, name)? {
        "object" => translate_object(object, name),
        "array" => translate_array(object, name),
        primitive @ ("string" | "integer" | "number" | "boolean" | "null") => {
            translate_primitive(object, primitive)
        }
        other => Err(format!(
            "unsupported schema type \"{other}\" at \"{name}\""
        )),
    }
}

/// `type` may be missing (an object, matching the backend's own default) or a
/// union like `["string", "null"]`, in which case the `null` arm is dropped and
/// the first real type wins. Nullable is not the same as optional here: such a
/// property is still required unless `required`/`optional` says otherwise.
fn schema_type<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, String> {
    match object.get("type") {
        None => Ok("object"),
        Some(Value::String(text)) => Ok(text.as_str()),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(Value::as_str)
            .find(|entry| *entry != "null")
            .ok_or_else(|| format!("schema at \"{name}\" has no usable \"type\"")),
        Some(_) => Err(format!("\"type\" at \"{name}\" must be a string or array")),
    }
}

fn with_description(mut node: Map<String, Value>, object: &Map<String, Value>) -> Value {
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        node.insert("description".into(), json!(description));
    }
    Value::Object(node)
}

fn translate_any_of(
    object: &Map<String, Value>,
    choices: &Value,
    name: &str,
) -> Result<Value, String> {
    let choices = choices
        .as_array()
        .ok_or_else(|| format!("\"anyOf\" at \"{name}\" must be an array"))?;
    let translated = choices
        .iter()
        .enumerate()
        .map(|(index, choice)| translate_schema(choice, &format!("{name}Choice{index}")))
        .collect::<Result<Vec<_>, String>>()?;

    let mut node = Map::new();
    node.insert("type".into(), json!("any_of"));
    node.insert("name".into(), json!(name));
    node.insert("choices".into(), Value::Array(translated));
    Ok(with_description(node, object))
}

fn translate_enum(
    object: &Map<String, Value>,
    values: &Value,
    name: &str,
) -> Result<Value, String> {
    let values = values
        .as_array()
        .ok_or_else(|| format!("\"enum\" at \"{name}\" must be an array"))?;
    let choices = values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(|text| json!(text))
                .ok_or_else(|| format!("\"enum\" at \"{name}\" supports string values only"))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut node = Map::new();
    node.insert("type".into(), json!("any_of"));
    node.insert("name".into(), json!(name));
    node.insert("choices".into(), Value::Array(choices));
    Ok(with_description(node, object))
}

fn translate_object(object: &Map<String, Value>, name: &str) -> Result<Value, String> {
    let empty = Map::new();
    let properties = object
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    // JSON Schema leaves properties optional when `required` is absent, which
    // would let the model emit an empty object. A schema that never mentions
    // `required` therefore means "all of them" here.
    let required = object.get("required").and_then(Value::as_array);
    let is_required = |key: &str| match required {
        None => true,
        Some(entries) => entries.iter().any(|entry| entry.as_str() == Some(key)),
    };

    let mut translated = Map::new();
    for (key, value) in properties {
        let mut child = translate_schema(value, key)?;
        let explicit = value.get("optional").and_then(Value::as_bool).unwrap_or(false);
        let optional = explicit || !is_required(key);
        if let Some(child) = child.as_object_mut() {
            child.insert("optional".into(), json!(optional));
        }
        translated.insert(key.clone(), child);
    }

    let mut node = Map::new();
    node.insert("type".into(), json!("object"));
    node.insert("name".into(), json!(name));
    node.insert("properties".into(), Value::Object(translated));
    Ok(with_description(node, object))
}

fn translate_array(object: &Map<String, Value>, name: &str) -> Result<Value, String> {
    let items = object.get("items").cloned().unwrap_or(json!({"type": "string"}));
    let item = translate_schema(&items, &format!("{name}Item"))?;

    let mut node = Map::new();
    node.insert("type".into(), json!("array"));
    node.insert("items".into(), item);
    if let Some(minimum) = number_key(object, "minItems", "min") {
        node.insert("min".into(), json!(minimum));
    }
    if let Some(maximum) = number_key(object, "maxItems", "max") {
        node.insert("max".into(), json!(maximum));
    }
    Ok(with_description(node, object))
}

fn number_key(object: &Map<String, Value>, primary: &str, fallback: &str) -> Option<u64> {
    object
        .get(primary)
        .or_else(|| object.get(fallback))
        .and_then(Value::as_u64)
}

fn translate_primitive(object: &Map<String, Value>, primitive: &str) -> Result<Value, String> {
    let mut guides = Vec::new();

    if primitive == "string" {
        if let Some(pattern) = object.get("pattern").and_then(Value::as_str) {
            guides.push(json!({ "kind": "pattern", "pattern": pattern }));
        }
        if let Some(constant) = object.get("const").and_then(Value::as_str) {
            guides.push(json!({ "kind": "constant", "value": constant }));
        }
    }

    if primitive == "integer" || primitive == "number" {
        let minimum = object.get("minimum").and_then(Value::as_f64);
        let maximum = object.get("maximum").and_then(Value::as_f64);
        match (minimum, maximum) {
            (Some(min), Some(max)) => guides.push(json!({
                "kind": "range",
                "min": numeric(primitive, min),
                "max": numeric(primitive, max),
            })),
            (Some(min), None) => {
                guides.push(json!({ "kind": "minimum", "value": numeric(primitive, min) }))
            }
            (None, Some(max)) => {
                guides.push(json!({ "kind": "maximum", "value": numeric(primitive, max) }))
            }
            (None, None) => {}
        }
    }

    let mut node = Map::new();
    node.insert("type".into(), json!(primitive));
    if !guides.is_empty() {
        node.insert("guides".into(), Value::Array(guides));
    }
    Ok(with_description(node, object))
}

/// Integer bounds have to stay integers on the wire — the backend reads an
/// integer guide's value as an `Int` and silently defaults a float to 0.
fn numeric(primitive: &str, value: f64) -> Value {
    if primitive == "integer" {
        json!(value as i64)
    } else {
        json!(value)
    }
}

// ── Request assembly ────────────────────────────────────────────────

fn resolve_model(requested: Option<&str>) -> Result<String, String> {
    match requested {
        None | Some("") | Some(DEFAULT_MODEL_ID) => Ok(DEFAULT_MODEL_ID.to_string()),
        Some(other) => Err(format!(
            "unknown model \"{other}\" — this shell exposes one model id, \"{DEFAULT_MODEL_ID}\""
        )),
    }
}

fn build_request(
    settings: &AiSettings,
    prompt: String,
    options: AiOptions,
    schema: Option<String>,
    dispatch: ToolDispatch,
) -> GenerateRequest {
    GenerateRequest {
        prompt,
        instructions: options.instructions.or_else(|| settings.instructions.clone()),
        temperature: options.temperature.or(settings.temperature),
        max_tokens: options.max_tokens.or(settings.max_tokens),
        tools: options.tools,
        dispatch,
        schema,
    }
}

fn take_records(records: &Arc<Mutex<Vec<ToolCallRecord>>>) -> Vec<ToolCallRecord> {
    records.lock().unwrap().clone()
}

// ── Commands ────────────────────────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ai_info(state: State<'_, AiState>) -> AiInfo {
    match state.availability() {
        Some(unavailable) => AiInfo {
            available: false,
            reason: Some(unavailable.reason.to_string()),
            detail: unavailable.detail,
            models: Vec::new(),
            features: AiFeatures {
                text: false,
                structured: false,
                tools: false,
                streaming: false,
            },
        },
        None => AiInfo {
            available: true,
            reason: None,
            detail: None,
            models: vec![AiModel {
                id: DEFAULT_MODEL_ID.to_string(),
                name: state.backend.model_label().to_string(),
                default: true,
            }],
            features: state.backend.features(),
        },
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn shell_ai_generate(
    app: AppHandle,
    state: State<'_, AiState>,
    request_id: String,
    prompt: String,
    options: Option<AiOptions>,
) -> Result<AiResult, String> {
    state.require_available()?;
    let options = options.unwrap_or_default();
    let model = resolve_model(options.model.as_deref())?;

    let cancelled = register_request(&state.requests, &request_id)?;
    let _guard = RequestGuard {
        requests: state.requests.clone(),
        id: request_id.clone(),
    };

    let backend = state.backend.clone();
    let records = Arc::new(Mutex::new(Vec::new()));
    let dispatch = tool_dispatcher(
        app,
        request_id,
        state.pending_tools.clone(),
        state.settings.tool_timeout,
        records.clone(),
    );
    let request = build_request(&state.settings, prompt, options, None, dispatch);

    let text = tauri::async_runtime::spawn_blocking(move || backend.generate(request))
        .await
        .map_err(|e| format!("ai generate: {e}"))??;

    if cancelled.load(Ordering::Relaxed) {
        return Err(CANCELLED.to_string());
    }

    Ok(AiResult {
        text,
        model,
        tool_calls: take_records(&records),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn shell_ai_generate_object(
    app: AppHandle,
    state: State<'_, AiState>,
    request_id: String,
    prompt: String,
    schema: Value,
    options: Option<AiOptions>,
) -> Result<AiObjectResult, String> {
    state.require_available()?;
    let options = options.unwrap_or_default();
    let model = resolve_model(options.model.as_deref())?;

    let translated = translate_schema(&schema, "Root")?;
    let schema_json = serde_json::to_string(&json!({
        "root": translated,
        "dependencies": [],
    }))
    .map_err(|e| format!("encode schema: {e}"))?;

    let cancelled = register_request(&state.requests, &request_id)?;
    let _guard = RequestGuard {
        requests: state.requests.clone(),
        id: request_id.clone(),
    };

    let backend = state.backend.clone();
    let records = Arc::new(Mutex::new(Vec::new()));
    let dispatch = tool_dispatcher(
        app,
        request_id,
        state.pending_tools.clone(),
        state.settings.tool_timeout,
        records.clone(),
    );
    let request = build_request(
        &state.settings,
        prompt,
        options,
        Some(schema_json),
        dispatch,
    );

    let raw = tauri::async_runtime::spawn_blocking(move || backend.generate_object(request))
        .await
        .map_err(|e| format!("ai generate object: {e}"))??;

    if cancelled.load(Ordering::Relaxed) {
        return Err(CANCELLED.to_string());
    }

    let object = serde_json::from_str::<Value>(&raw)
        .map_err(|e| format!("model returned invalid JSON: {e}"))?;

    Ok(AiObjectResult {
        object,
        model,
        tool_calls: take_records(&records),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ai_stream(
    app: AppHandle,
    state: State<'_, AiState>,
    request_id: String,
    prompt: String,
    options: Option<AiOptions>,
) -> Result<AiStreamHandle, String> {
    state.require_available()?;
    let options = options.unwrap_or_default();
    let model = resolve_model(options.model.as_deref())?;

    let id = request_id;
    let cancelled = register_request(&state.requests, &id)?;

    let backend = state.backend.clone();
    let records = Arc::new(Mutex::new(Vec::new()));
    let dispatch = tool_dispatcher(
        app.clone(),
        id.clone(),
        state.pending_tools.clone(),
        state.settings.tool_timeout,
        records.clone(),
    );
    let request = build_request(&state.settings, prompt, options, None, dispatch);

    let sink_app = app.clone();
    let sink_id = id.clone();
    let sink_cancelled = cancelled.clone();
    let sink: Box<dyn FnMut(&str) + Send + 'static> = Box::new(move |delta: &str| {
        if sink_cancelled.load(Ordering::Relaxed) {
            return;
        }
        let _ = sink_app.emit_to(
            "main",
            "shell://ai-chunk",
            json!({ "id": sink_id, "text": delta }),
        );
    });

    let requests = state.requests.clone();
    let done_id = id.clone();

    // `Backend::stream` only returns once the last delta has been handed to
    // the sink, so the done event below cannot overtake a chunk event.
    std::thread::spawn(move || {
        let outcome = backend.stream(request, sink);
        finish_request(&requests, &done_id);

        let (text, error) = if cancelled.load(Ordering::Relaxed) {
            (String::new(), Some(CANCELLED.to_string()))
        } else {
            match outcome {
                Ok(text) => (text, None),
                Err(error) => (String::new(), Some(error)),
            }
        };

        let _ = app.emit_to(
            "main",
            "shell://ai-done",
            json!({
                "id": done_id,
                "text": text,
                "model": model,
                "toolCalls": take_records(&records),
                "error": error,
            }),
        );
    });

    Ok(AiStreamHandle { id })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_ai_tool_result(
    state: State<'_, AiState>,
    call_id: String,
    ok: bool,
    value: Option<Value>,
    error: Option<String>,
) {
    deliver_tool_result(&state.pending_tools, &call_id, ok, value, error);
}

/// Best effort: the underlying model exposes no cancel API, so this stops the
/// shell forwarding chunks and makes the request finish with `cancelled` — as
/// the `error` field of `shell://ai-done` for a stream, or as the rejection of
/// a one-shot `generate` / `generateObject`. An unknown or already-finished id
/// is a silent no-op.
#[tauri::command(rename_all = "camelCase")]
pub fn shell_ai_cancel(state: State<'_, AiState>, id: String) {
    if let Some(flag) = state.requests.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(toml: &str) -> AiConfig {
        toml::from_str(toml).expect("parse [ai] table")
    }

    // ── Reason codes ────────────────────────────────────────────────

    #[test]
    fn os_states_map_onto_the_documented_reason_codes() {
        assert_eq!(
            reason_for(OsUnavailable::DeviceNotEligible),
            "device-not-eligible"
        );
        assert_eq!(reason_for(OsUnavailable::NotEnabled), "not-enabled");
        assert_eq!(reason_for(OsUnavailable::ModelNotReady), "model-not-ready");
        assert_eq!(reason_for(OsUnavailable::OsTooOld), "unsupported-os");
        assert_eq!(reason_for(OsUnavailable::Unknown), "unavailable");
    }

    #[test]
    fn reason_codes_never_name_a_vendor() {
        let codes = [
            REASON_UNSUPPORTED_PLATFORM,
            REASON_UNSUPPORTED_OS,
            REASON_DISABLED_BY_CONFIG,
            REASON_DEVICE_NOT_ELIGIBLE,
            REASON_NOT_ENABLED,
            REASON_MODEL_NOT_READY,
            REASON_UNAVAILABLE,
        ];
        for code in codes {
            for vendor in ["apple", "mac", "foundation", "siri", "intelligence"] {
                assert!(!code.contains(vendor), "{code} leaks \"{vendor}\"");
            }
        }
    }

    #[test]
    fn unavailable_message_includes_the_detail_when_there_is_one() {
        assert_eq!(
            unavailable_message(&Unavailable {
                reason: REASON_MODEL_NOT_READY,
                detail: Some("still downloading".into()),
            }),
            "ai unavailable: model-not-ready — still downloading"
        );
        assert_eq!(
            unavailable_message(&Unavailable {
                reason: REASON_UNAVAILABLE,
                detail: None,
            }),
            "ai unavailable: unavailable"
        );
    }

    // ── Config ──────────────────────────────────────────────────────

    #[test]
    fn absent_config_uses_defaults() {
        let settings = AiSettings::from_config(None);
        assert!(settings.enabled);
        assert_eq!(settings.instructions, None);
        assert_eq!(settings.temperature, None);
        assert_eq!(settings.max_tokens, None);
        assert_eq!(
            settings.tool_timeout,
            Duration::from_millis(DEFAULT_TOOL_TIMEOUT_MS)
        );
    }

    #[test]
    fn empty_table_still_defaults_to_enabled() {
        let settings = AiSettings::from_config(Some(&config("")));
        assert!(settings.enabled);
        assert_eq!(
            settings.tool_timeout,
            Duration::from_millis(DEFAULT_TOOL_TIMEOUT_MS)
        );
    }

    #[test]
    fn config_values_override_the_defaults() {
        let settings = AiSettings::from_config(Some(&config(
            "enabled = false\n\
             instructions = \"be terse\"\n\
             temperature = 0.25\n\
             maxTokens = 128\n\
             toolTimeoutMs = 1500\n",
        )));
        assert!(!settings.enabled);
        assert_eq!(settings.instructions.as_deref(), Some("be terse"));
        assert_eq!(settings.temperature, Some(0.25));
        assert_eq!(settings.max_tokens, Some(128));
        assert_eq!(settings.tool_timeout, Duration::from_millis(1500));
    }

    #[test]
    fn unknown_config_keys_are_ignored() {
        let settings = AiSettings::from_config(Some(&config("enabled = true\nfuture = 1\n")));
        assert!(settings.enabled);
    }

    #[test]
    fn per_request_options_win_over_config_defaults() {
        let settings = AiSettings::from_config(Some(&config(
            "instructions = \"from config\"\ntemperature = 0.1\nmaxTokens = 64\n",
        )));
        let options = AiOptions {
            instructions: Some("from request".into()),
            temperature: Some(0.9),
            max_tokens: Some(256),
            ..AiOptions::default()
        };
        let request = build_request(
            &settings,
            "hello".into(),
            options,
            None,
            Arc::new(|_, _| Value::Null),
        );
        assert_eq!(request.instructions.as_deref(), Some("from request"));
        assert_eq!(request.temperature, Some(0.9));
        assert_eq!(request.max_tokens, Some(256));
    }

    #[test]
    fn config_defaults_fill_in_absent_request_options() {
        let settings = AiSettings::from_config(Some(&config(
            "instructions = \"from config\"\ntemperature = 0.1\nmaxTokens = 64\n",
        )));
        let request = build_request(
            &settings,
            "hello".into(),
            AiOptions::default(),
            None,
            Arc::new(|_, _| Value::Null),
        );
        assert_eq!(request.instructions.as_deref(), Some("from config"));
        assert_eq!(request.temperature, Some(0.1));
        assert_eq!(request.max_tokens, Some(64));
    }

    // ── Model selection ─────────────────────────────────────────────

    #[test]
    fn model_selection_accepts_default_and_rejects_anything_else() {
        assert_eq!(resolve_model(None).unwrap(), "default");
        assert_eq!(resolve_model(Some("default")).unwrap(), "default");
        let error = resolve_model(Some("gpt-9")).unwrap_err();
        assert!(error.starts_with("unknown model \"gpt-9\""), "{error}");
    }

    // ── Request registry ────────────────────────────────────────────

    #[test]
    fn a_request_id_can_only_be_in_flight_once() {
        let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
        register_request(&requests, "req-1").expect("the first claim wins");

        let error = register_request(&requests, "req-1")
            .expect_err("a duplicate id must be rejected, not silently overwritten");
        assert_eq!(error, "ai request \"req-1\" is already in flight");
    }

    #[test]
    fn a_finished_request_id_can_be_reused() {
        let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
        register_request(&requests, "req-2").expect("claim");
        finish_request(&requests, "req-2");
        register_request(&requests, "req-2").expect("the id is free again");
    }

    #[test]
    fn cancelling_flags_the_request_without_freeing_its_id() {
        let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
        let cancelled = register_request(&requests, "req-3").expect("claim");

        // What `shell_ai_cancel` does, without needing an AppHandle.
        requests
            .lock()
            .unwrap()
            .get("req-3")
            .expect("still in flight")
            .store(true, Ordering::Relaxed);

        assert!(cancelled.load(Ordering::Relaxed));
        assert!(
            register_request(&requests, "req-3").is_err(),
            "a cancelled-but-still-running request keeps its id"
        );
    }

    #[test]
    fn the_request_guard_releases_the_id_on_any_exit_path() {
        let requests: Requests = Arc::new(Mutex::new(HashMap::new()));
        {
            register_request(&requests, "req-4").expect("claim");
            let _guard = RequestGuard {
                requests: requests.clone(),
                id: "req-4".into(),
            };
            assert!(register_request(&requests, "req-4").is_err());
        }
        assert!(requests.lock().unwrap().is_empty());
    }

    // ── Tool bridge ─────────────────────────────────────────────────

    #[test]
    fn a_tool_result_resolves_the_waiting_call() {
        let pending: PendingTools = Arc::new(Mutex::new(HashMap::new()));
        let rx = register_tool_call(&pending, "call-1");
        deliver_tool_result(&pending, "call-1", true, Some(json!({ "ok": 1 })), None);

        let value = await_tool_reply(
            &pending,
            "call-1",
            "get_time",
            &rx,
            Duration::from_millis(500),
        )
        .expect("the delivered value should be returned");
        assert_eq!(value, json!({ "ok": 1 }));
        assert!(pending.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_tool_result_becomes_an_error() {
        let pending: PendingTools = Arc::new(Mutex::new(HashMap::new()));
        let rx = register_tool_call(&pending, "call-2");
        deliver_tool_result(
            &pending,
            "call-2",
            false,
            None,
            Some("unknown tool \"nope\"".into()),
        );

        let error = await_tool_reply(&pending, "call-2", "nope", &rx, Duration::from_millis(500))
            .expect_err("a failed reply must not look like a result");
        assert_eq!(error, "unknown tool \"nope\"");
    }

    #[test]
    fn a_hung_tool_handler_times_out_instead_of_blocking_forever() {
        let pending: PendingTools = Arc::new(Mutex::new(HashMap::new()));
        let rx = register_tool_call(&pending, "call-3");

        let started = std::time::Instant::now();
        let error = await_tool_reply(&pending, "call-3", "slow", &rx, Duration::from_millis(120))
            .expect_err("a silent handler must time out");
        assert!(error.contains("did not answer within 120ms"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            pending.lock().unwrap().is_empty(),
            "a timed-out call must be de-registered"
        );
    }

    #[test]
    fn a_result_for_an_unknown_call_id_is_a_no_op() {
        let pending: PendingTools = Arc::new(Mutex::new(HashMap::new()));
        deliver_tool_result(&pending, "never-registered", true, Some(json!(1)), None);
        assert!(pending.lock().unwrap().is_empty());
    }

    // ── Schema translation ──────────────────────────────────────────

    #[test]
    fn objects_translate_required_into_per_property_optional_flags() {
        let translated = translate_schema(
            &json!({
                "type": "object",
                "description": "a person",
                "properties": {
                    "name": { "type": "string", "description": "full name" },
                    "age": { "type": "integer" }
                },
                "required": ["name"]
            }),
            "Root",
        )
        .expect("translate");

        assert_eq!(translated["type"], json!("object"));
        assert_eq!(translated["name"], json!("Root"));
        assert_eq!(translated["description"], json!("a person"));
        assert_eq!(translated["properties"]["name"]["optional"], json!(false));
        assert_eq!(
            translated["properties"]["name"]["description"],
            json!("full name")
        );
        assert_eq!(translated["properties"]["age"]["optional"], json!(true));
    }

    #[test]
    fn a_schema_without_required_treats_every_property_as_required() {
        let translated = translate_schema(
            &json!({ "type": "object", "properties": { "a": { "type": "string" } } }),
            "Root",
        )
        .expect("translate");
        assert_eq!(translated["properties"]["a"]["optional"], json!(false));
    }

    #[test]
    fn an_explicit_optional_flag_is_honoured() {
        let translated = translate_schema(
            &json!({
                "type": "object",
                "properties": { "a": { "type": "string", "optional": true } },
                "required": ["a"]
            }),
            "Root",
        )
        .expect("translate");
        assert_eq!(translated["properties"]["a"]["optional"], json!(true));
    }

    #[test]
    fn arrays_translate_bounds_and_items() {
        let translated = translate_schema(
            &json!({
                "type": "array",
                "items": { "type": "string" },
                "minItems": 1,
                "maxItems": 5
            }),
            "Tags",
        )
        .expect("translate");
        assert_eq!(translated["type"], json!("array"));
        assert_eq!(translated["items"]["type"], json!("string"));
        assert_eq!(translated["min"], json!(1));
        assert_eq!(translated["max"], json!(5));
    }

    #[test]
    fn an_array_without_items_defaults_to_strings() {
        let translated = translate_schema(&json!({ "type": "array" }), "Tags").expect("translate");
        assert_eq!(translated["items"]["type"], json!("string"));
    }

    #[test]
    fn string_enums_become_any_of_choices() {
        let translated =
            translate_schema(&json!({ "type": "string", "enum": ["a", "b"] }), "Mood")
                .expect("translate");
        assert_eq!(translated["type"], json!("any_of"));
        assert_eq!(translated["name"], json!("Mood"));
        assert_eq!(translated["choices"], json!(["a", "b"]));
    }

    #[test]
    fn any_of_branches_translate_recursively() {
        let translated = translate_schema(
            &json!({ "anyOf": [{ "type": "string" }, { "type": "integer" }] }),
            "Value",
        )
        .expect("translate");
        assert_eq!(translated["type"], json!("any_of"));
        assert_eq!(translated["choices"][0]["type"], json!("string"));
        assert_eq!(translated["choices"][1]["type"], json!("integer"));
    }

    #[test]
    fn numeric_bounds_become_guides_with_the_right_value_type() {
        let integer = translate_schema(
            &json!({ "type": "integer", "minimum": 1, "maximum": 10 }),
            "Count",
        )
        .expect("translate");
        assert_eq!(
            integer["guides"],
            json!([{ "kind": "range", "min": 1, "max": 10 }])
        );

        let number = translate_schema(&json!({ "type": "number", "minimum": 0.5 }), "Score")
            .expect("translate");
        assert_eq!(
            number["guides"],
            json!([{ "kind": "minimum", "value": 0.5 }])
        );
    }

    #[test]
    fn string_patterns_become_guides() {
        let translated =
            translate_schema(&json!({ "type": "string", "pattern": "^[a-z]+$" }), "Slug")
                .expect("translate");
        assert_eq!(
            translated["guides"],
            json!([{ "kind": "pattern", "pattern": "^[a-z]+$" }])
        );
    }

    #[test]
    fn a_nullable_type_union_picks_the_real_type() {
        let translated =
            translate_schema(&json!({ "type": ["string", "null"] }), "Maybe").expect("translate");
        assert_eq!(translated["type"], json!("string"));
    }

    #[test]
    fn a_missing_type_is_treated_as_an_object() {
        let translated =
            translate_schema(&json!({ "properties": { "a": { "type": "string" } } }), "Root")
                .expect("translate");
        assert_eq!(translated["type"], json!("object"));
    }

    #[test]
    fn nested_objects_and_arrays_translate_all_the_way_down() {
        let translated = translate_schema(
            &json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": { "id": { "type": "integer" } },
                            "required": ["id"]
                        }
                    }
                },
                "required": ["items"]
            }),
            "Root",
        )
        .expect("translate");
        let item = &translated["properties"]["items"]["items"];
        assert_eq!(item["type"], json!("object"));
        assert_eq!(item["properties"]["id"]["optional"], json!(false));
    }

    #[test]
    fn unsupported_shapes_are_rejected_with_a_useful_message() {
        let error = translate_schema(&json!("nope"), "Root").unwrap_err();
        assert_eq!(error, "schema at \"Root\" must be a JSON object");

        let error = translate_schema(&json!({ "type": "widget" }), "Root").unwrap_err();
        assert!(error.contains("unsupported schema type \"widget\""), "{error}");

        let error =
            translate_schema(&json!({ "type": "string", "enum": [1, 2] }), "Root").unwrap_err();
        assert!(error.contains("string values only"), "{error}");
    }
}
