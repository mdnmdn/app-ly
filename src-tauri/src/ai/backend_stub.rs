//! Fallback backend for builds with no on-device model: every platform other
//! than macOS, and macOS built without the `ai-apple` feature.
//!
//! It reports itself unavailable so `shell.ai.info()` still answers honestly,
//! and rejects every generate call with the same shared message the available
//! path would use.

use super::{
    unavailable_message, AiFeatures, Backend, GenerateRequest, Unavailable,
    REASON_UNSUPPORTED_PLATFORM,
};
use std::sync::Arc;

pub(crate) fn new_backend() -> Arc<dyn Backend> {
    Arc::new(StubBackend)
}

struct StubBackend;

fn unsupported() -> Unavailable {
    Unavailable {
        reason: REASON_UNSUPPORTED_PLATFORM,
        detail: Some("this build has no on-device model backend".into()),
    }
}

impl Backend for StubBackend {
    fn availability(&self) -> Option<Unavailable> {
        Some(unsupported())
    }

    fn model_label(&self) -> &'static str {
        "On-device model"
    }

    fn features(&self) -> AiFeatures {
        AiFeatures {
            text: false,
            structured: false,
            tools: false,
            streaming: false,
        }
    }

    fn generate(&self, _request: GenerateRequest) -> Result<String, String> {
        Err(unavailable_message(&unsupported()))
    }

    fn generate_object(&self, _request: GenerateRequest) -> Result<String, String> {
        Err(unavailable_message(&unsupported()))
    }

    fn stream(
        &self,
        _request: GenerateRequest,
        _sink: Box<dyn FnMut(&str) + Send + 'static>,
    ) -> Result<String, String> {
        Err(unavailable_message(&unsupported()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> GenerateRequest {
        GenerateRequest {
            prompt: "hello".into(),
            instructions: None,
            temperature: None,
            max_tokens: None,
            tools: Vec::new(),
            dispatch: Arc::new(|_, _| serde_json::Value::Null),
            schema: None,
        }
    }

    #[test]
    fn the_stub_is_always_unavailable() {
        let backend = new_backend();
        let state = backend.availability().expect("the stub is never available");
        assert_eq!(state.reason, "unsupported-platform");
    }

    #[test]
    fn the_stub_advertises_no_features() {
        let features = new_backend().features();
        assert!(!features.text);
        assert!(!features.structured);
        assert!(!features.tools);
        assert!(!features.streaming);
    }

    #[test]
    fn every_generate_call_rejects_with_the_shared_message() {
        let backend = new_backend();
        let expected =
            "ai unavailable: unsupported-platform — this build has no on-device model backend";

        assert_eq!(backend.generate(request()).unwrap_err(), expected);
        assert_eq!(backend.generate_object(request()).unwrap_err(), expected);
        assert_eq!(
            backend.stream(request(), Box::new(|_| {})).unwrap_err(),
            expected
        );
    }
}
