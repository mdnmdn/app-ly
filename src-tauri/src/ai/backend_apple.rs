//! macOS backend, built on Apple's FoundationModels framework via the
//! `foundation-models` crate. Compiled only for
//! `all(target_os = "macos", feature = "ai-apple")`.
//!
//! Nothing vendor-specific escapes this file: availability states are mapped
//! onto the shell's own `OsUnavailable` codes, and the model is reported to JS
//! as the neutral `"default"` id defined in `ai.rs`.

use super::{
    reason_for, translate_schema, AiFeatures, Backend, GenerateRequest, OsUnavailable,
    ToolDispatch, ToolSpec, Unavailable,
};
use foundation_models::{
    Availability, GeneratedContent, GenerationOptions, GenerationSchema, LanguageModelSession,
    StreamEvent, SystemLanguageModel, Tool, ToolOutput, Unavailability,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub(crate) fn new_backend() -> Arc<dyn Backend> {
    Arc::new(AppleBackend)
}

struct AppleBackend;

// ── Availability ────────────────────────────────────────────────────

/// The vendor enum crosses into the shell's own vocabulary exactly here.
fn os_state(reason: Unavailability) -> OsUnavailable {
    match reason {
        Unavailability::DeviceNotEligible => OsUnavailable::DeviceNotEligible,
        Unavailability::AppleIntelligenceNotEnabled => OsUnavailable::NotEnabled,
        Unavailability::ModelNotReady => OsUnavailable::ModelNotReady,
        Unavailability::OsTooOld => OsUnavailable::OsTooOld,
        _ => OsUnavailable::Unknown,
    }
}

/// Written here rather than forwarded from the OS so the text stays
/// vendor-neutral, as the JS-visible surface requires.
fn detail_for(state: OsUnavailable) -> &'static str {
    match state {
        OsUnavailable::DeviceNotEligible => "this device does not support on-device AI",
        OsUnavailable::NotEnabled => "on-device AI is turned off in system settings",
        OsUnavailable::ModelNotReady => "the on-device model is still downloading or preparing",
        OsUnavailable::OsTooOld => "this operating system has no on-device model API",
        OsUnavailable::Unknown => "the system reports the on-device model as unavailable",
    }
}

// ── Session assembly ────────────────────────────────────────────────

fn options_for(request: &GenerateRequest) -> GenerationOptions {
    let mut options = GenerationOptions::new();
    if let Some(temperature) = request.temperature {
        options = options.with_temperature(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        options = options.with_maximum_response_tokens(max_tokens);
    }
    options
}

/// Wraps a translated schema the way the dynamic-schema builder expects it.
fn schema_request(root: Value) -> Result<String, String> {
    serde_json::to_string(&json!({ "root": root, "dependencies": [] }))
        .map_err(|e| format!("encode schema: {e}"))
}

fn build_tool(spec: &ToolSpec, dispatch: ToolDispatch) -> Result<Tool, String> {
    let root = translate_schema(&spec.parameters, &spec.name)
        .map_err(|e| format!("tool \"{}\": {e}", spec.name))?;
    let schema = GenerationSchema::from_json_schema(schema_request(root)?)
        .map_err(|e| format!("tool \"{}\": invalid parameter schema: {e}", spec.name))?;

    let name = spec.name.clone();
    Ok(Tool::new(
        spec.name.clone(),
        spec.description.clone(),
        schema,
        move |arguments: GeneratedContent| {
            let decoded = arguments
                .json_string()
                .ok()
                .and_then(|json| serde_json::from_str::<Value>(&json).ok())
                .unwrap_or(Value::Null);
            // `dispatch` folds a failing or unanswered JS handler into an
            // `{ "error": ... }` value, so the model always gets an answer and
            // the generation is never aborted from here.
            Ok(ToolOutput::text(dispatch(&name, decoded).to_string()))
        },
    ))
}

/// A fresh session per request: per-request instructions and tools cannot be
/// changed on an existing one, and the framework rejects concurrent requests
/// on a single session anyway.
fn open_session(request: &GenerateRequest) -> Result<LanguageModelSession, String> {
    let mut builder = LanguageModelSession::builder();

    if let Some(instructions) = &request.instructions {
        builder = builder
            .instructions(instructions.as_str())
            .map_err(|e| format!("ai instructions: {}", e.message()))?;
    }

    for spec in &request.tools {
        builder = builder.tool(build_tool(spec, request.dispatch.clone())?);
    }

    builder
        .build()
        .map_err(|e| format!("ai session: {}", e.message()))
}

// ── Backend ─────────────────────────────────────────────────────────

impl Backend for AppleBackend {
    fn availability(&self) -> Option<Unavailable> {
        match SystemLanguageModel::availability() {
            Availability::Available => None,
            Availability::Unavailable(reason) => {
                let state = os_state(reason);
                Some(Unavailable {
                    reason: reason_for(state),
                    detail: Some(detail_for(state).to_string()),
                })
            }
            _ => Some(Unavailable {
                reason: reason_for(OsUnavailable::Unknown),
                detail: Some(detail_for(OsUnavailable::Unknown).to_string()),
            }),
        }
    }

    fn model_label(&self) -> &'static str {
        "On-device model"
    }

    fn features(&self) -> AiFeatures {
        AiFeatures {
            text: true,
            structured: true,
            tools: true,
            streaming: true,
        }
    }

    fn generate(&self, request: GenerateRequest) -> Result<String, String> {
        let session = open_session(&request)?;
        session
            .respond_with(&request.prompt, options_for(&request))
            .map_err(|e| format!("ai generate: {}", e.message()))
    }

    fn generate_object(&self, request: GenerateRequest) -> Result<String, String> {
        let schema = request
            .schema
            .as_deref()
            .ok_or_else(|| "ai generate object: no schema was supplied".to_string())?;
        let session = open_session(&request)?;
        // `respond_with_schema_options`, never `respond_with_json_schema`:
        // only this form reaches the framework's guided decoding. The other
        // one just pastes the schema into the prompt and hopes.
        session
            .respond_with_schema_options(&request.prompt, schema, true, options_for(&request))
            .map_err(|e| format!("ai generate object: {}", e.message()))
    }

    fn stream(
        &self,
        request: GenerateRequest,
        sink: Box<dyn FnMut(&str) + Send + 'static>,
    ) -> Result<String, String> {
        let session = open_session(&request)?;

        let text = Arc::new(Mutex::new(String::new()));
        let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let collected = text.clone();
        let failed = failure.clone();
        let mut sink = sink;

        // `stream_with` blocks until the stream terminates, and the deltas
        // reach `sink` from inside it — so by the time it returns, everything
        // this request will ever emit has already been emitted.
        session
            .stream_with(
                &request.prompt,
                options_for(&request),
                move |event| match event {
                    StreamEvent::Chunk(delta) => {
                        collected.lock().unwrap().push_str(delta);
                        sink(delta);
                    }
                    StreamEvent::Error(error) => {
                        *failed.lock().unwrap() = Some(error.message().to_string());
                    }
                    _ => {}
                },
            )
            .map_err(|e| format!("ai stream: {}", e.message()))?;

        if let Some(error) = failure.lock().unwrap().take() {
            return Err(format!("ai stream: {error}"));
        }
        let text = text.lock().unwrap().clone();
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailability_maps_onto_the_documented_reason_codes() {
        let cases = [
            (Unavailability::DeviceNotEligible, "device-not-eligible"),
            (Unavailability::AppleIntelligenceNotEnabled, "not-enabled"),
            (Unavailability::ModelNotReady, "model-not-ready"),
            (Unavailability::OsTooOld, "unsupported-os"),
            (Unavailability::Unknown, "unavailable"),
        ];
        for (reason, expected) in cases {
            assert_eq!(reason_for(os_state(reason)), expected);
        }
    }

    #[test]
    fn availability_details_never_name_a_vendor() {
        let states = [
            OsUnavailable::DeviceNotEligible,
            OsUnavailable::NotEnabled,
            OsUnavailable::ModelNotReady,
            OsUnavailable::OsTooOld,
            OsUnavailable::Unknown,
        ];
        for state in states {
            let detail = detail_for(state).to_lowercase();
            for vendor in ["apple", "mac", "foundation", "siri", "intelligence"] {
                assert!(!detail.contains(vendor), "{detail} leaks \"{vendor}\"");
            }
        }
    }

    #[test]
    fn a_schema_request_is_wrapped_for_the_dynamic_schema_builder() {
        let wrapped = schema_request(json!({ "type": "string" })).expect("encode");
        let parsed: Value = serde_json::from_str(&wrapped).expect("parse");
        assert_eq!(parsed["root"]["type"], json!("string"));
        assert_eq!(parsed["dependencies"], json!([]));
    }
}
