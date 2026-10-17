use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

use serde::Serialize;
use url::Url;

pub(crate) const MAX_UPSTREAM_ATTEMPT_BODY_BYTES: usize = 64 * 1024;
pub(crate) const MAX_UPSTREAM_ATTEMPT_TOTAL_BODY_BYTES: usize = 256 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpstreamAttempt {
    pub(crate) sequence: usize,
    pub(crate) phase: &'static str,
    pub(crate) method: String,
    pub(crate) endpoint: String,
    pub(crate) status: Option<u16>,
    pub(crate) duration_ms: u64,
    pub(crate) response_body: Option<String>,
    pub(crate) body_kind: &'static str,
    pub(crate) body_truncated: bool,
    pub(crate) response_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_class: Option<&'static str>,
}

#[derive(Default)]
struct UpstreamAttemptState {
    attempts: Vec<UpstreamAttempt>,
    remaining_body_bytes: usize,
}

pub(crate) struct UpstreamAttemptRecorder {
    state: RefCell<UpstreamAttemptState>,
}

impl UpstreamAttemptRecorder {
    pub(crate) fn new() -> Self {
        Self {
            state: RefCell::new(UpstreamAttemptState {
                attempts: Vec::new(),
                remaining_body_bytes: MAX_UPSTREAM_ATTEMPT_TOTAL_BODY_BYTES,
            }),
        }
    }

    fn record_text(
        &self,
        phase: &'static str,
        method: &str,
        endpoint: &str,
        status: u16,
        duration: Duration,
        body: &str,
        redact_tokens: bool,
    ) {
        let safe_body = if redact_tokens {
            redact_refresh_response(body)
        } else {
            body.to_string()
        };
        let mut state = self.state.borrow_mut();
        let limit = MAX_UPSTREAM_ATTEMPT_BODY_BYTES.min(state.remaining_body_bytes);
        let captured = utf8_prefix(&safe_body, limit);
        state.remaining_body_bytes = state.remaining_body_bytes.saturating_sub(captured.len());
        let sequence = state.attempts.len() + 1;
        state.attempts.push(UpstreamAttempt {
            sequence,
            phase,
            method: method.to_string(),
            endpoint: trace_endpoint(endpoint),
            status: Some(status),
            duration_ms: duration.as_millis().min(u64::MAX as u128) as u64,
            response_body: Some(captured.to_string()),
            body_kind: "text",
            body_truncated: captured.len() < safe_body.len(),
            response_bytes: Some(body.len()),
            error_class: (!(200..300).contains(&status)).then_some("http_status"),
        });
    }

    fn record_failure(
        &self,
        phase: &'static str,
        method: &str,
        endpoint: &str,
        duration: Duration,
        error_class: &'static str,
    ) {
        let mut state = self.state.borrow_mut();
        let sequence = state.attempts.len() + 1;
        state.attempts.push(UpstreamAttempt {
            sequence,
            phase,
            method: method.to_string(),
            endpoint: trace_endpoint(endpoint),
            status: None,
            duration_ms: duration.as_millis().min(u64::MAX as u128) as u64,
            response_body: None,
            body_kind: "none",
            body_truncated: false,
            response_bytes: None,
            error_class: Some(error_class),
        });
    }

    fn record_binary(
        &self,
        phase: &'static str,
        method: &str,
        endpoint: &str,
        status: u16,
        duration: Duration,
        response_bytes: usize,
        error_class: Option<&'static str>,
    ) {
        let mut state = self.state.borrow_mut();
        let sequence = state.attempts.len() + 1;
        state.attempts.push(UpstreamAttempt {
            sequence,
            phase,
            method: method.to_string(),
            endpoint: trace_endpoint(endpoint),
            status: Some(status),
            duration_ms: duration.as_millis().min(u64::MAX as u128) as u64,
            response_body: None,
            body_kind: "binary",
            body_truncated: false,
            response_bytes: Some(response_bytes),
            error_class,
        });
    }

    pub(crate) fn into_attempts(self) -> Vec<UpstreamAttempt> {
        self.state.into_inner().attempts
    }

    pub(crate) fn mark_last_error_class(&self, error_class: &'static str) {
        if let Some(attempt) = self.state.borrow_mut().attempts.last_mut() {
            attempt.error_class = Some(error_class);
        }
    }
}

fn utf8_prefix(value: &str, limit: usize) -> &str {
    let mut end = value.len().min(limit);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn dreamtonics_error_class(error: &'static str) -> &'static str {
    match error {
        "upstream request failed" => "transport",
        "upstream response could not be read" => "response_read",
        "upstream response exceeds the size limit" => "body_limit",
        "request method is unsupported" => "invalid_request",
        _ => "request_failed",
    }
}

fn trace_endpoint(endpoint: &str) -> String {
    let Ok(mut url) = Url::parse(endpoint) else {
        return "[invalid endpoint]".to_string();
    };
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

fn redact_refresh_response(body: &str) -> String {
    fn strip_tokens(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.retain(|key, _| {
                    !matches!(
                        key.to_ascii_lowercase().replace('_', "").as_str(),
                        "accesstoken" | "refreshtoken" | "idtoken"
                    )
                });
                for value in fields.values_mut() {
                    strip_tokens(value);
                }
            }
            serde_json::Value::Array(values) => values.iter_mut().for_each(strip_tokens),
            _ => {}
        }
    }

    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(body) else {
        return "[refresh response omitted: invalid JSON]".to_string();
    };
    strip_tokens(&mut value);
    serde_json::to_string(&value).unwrap_or_else(|_| "[refresh response omitted]".to_string())
}

pub(crate) fn recorded_dreamtonics_http<F>(
    recorder: &UpstreamAttemptRecorder,
    phase: &'static str,
    method: &str,
    endpoint: &str,
    redact_tokens: bool,
    request: F,
) -> Result<(u16, String), &'static str>
where
    F: FnOnce() -> Result<(u16, String), &'static str>,
{
    let started = Instant::now();
    let result = request();
    match &result {
        Ok((status, body)) => recorder.record_text(
            phase,
            method,
            endpoint,
            *status,
            started.elapsed(),
            body,
            redact_tokens,
        ),
        Err(error) => recorder.record_failure(
            phase,
            method,
            endpoint,
            started.elapsed(),
            dreamtonics_error_class(error),
        ),
    }
    result
}

pub(crate) fn recorded_binary_get<F>(
    recorder: &UpstreamAttemptRecorder,
    phase: &'static str,
    endpoint: &str,
    max_bytes: usize,
    request: F,
) -> Option<Vec<u8>>
where
    F: FnOnce() -> Result<(u16, Vec<u8>), &'static str>,
{
    let started = Instant::now();
    match request() {
        Ok((status, body)) => {
            let error = if !(200..300).contains(&status) {
                Some("http_status")
            } else if body.len() > max_bytes {
                Some("body_limit")
            } else {
                None
            };
            recorder.record_binary(
                phase,
                "GET",
                endpoint,
                status,
                started.elapsed(),
                body.len(),
                error,
            );
            error.is_none().then_some(body)
        }
        Err(error) => {
            recorder.record_failure(
                phase,
                "GET",
                endpoint,
                started.elapsed(),
                dreamtonics_error_class(error),
            );
            None
        }
    }
}
