use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant},
};

use serde::Deserialize;
use url::Url;
use zeroize::{Zeroize, Zeroizing};

use crate::request_trace::UpstreamAttempt;

pub(crate) const MAX_HTTP_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_HTTP_REQUEST_BODY_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_HTTP_TIMEOUT_MS: u64 = 120_000;
const MAX_REQUEST_HEADERS: usize = 64;
const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_HEADERS: usize = 64;
const MAX_RESPONSE_HEADER_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HttpRequestSpec {
    pub(crate) method: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Option<String>,
    pub(crate) response_encoding: String,
    pub(crate) max_response_bytes: usize,
    pub(crate) timeout_ms: u64,
}

impl Drop for HttpRequestSpec {
    fn drop(&mut self) {
        self.url.zeroize();
        if let Some(body) = &mut self.body {
            zeroize::Zeroize::zeroize(body);
        }
        for value in self.headers.values_mut() {
            zeroize::Zeroize::zeroize(value);
        }
    }
}

pub(crate) struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) headers: BTreeMap<String, Vec<String>>,
    pub(crate) body: Zeroizing<Vec<u8>>,
    pub(crate) attempt: Option<UpstreamAttempt>,
    pub(crate) error: Option<&'static str>,
}

impl Drop for HttpResponse {
    fn drop(&mut self) {
        self.body.zeroize();
        for values in self.headers.values_mut() {
            for value in values {
                value.zeroize();
            }
        }
        self.attempt.take();
    }
}

pub(crate) fn execute(request: &HttpRequestSpec) -> HttpResponse {
    let invalid = |error| HttpResponse {
        status: 599,
        headers: BTreeMap::new(),
        body: Zeroizing::new(Vec::new()),
        attempt: None,
        error: Some(error),
    };
    if let Err(error) = validate(&request) {
        return invalid(error);
    }

    let started = Instant::now();
    let response = match open(&request) {
        Ok(response) => response,
        Err((status, error)) => {
            return HttpResponse {
                status: status.unwrap_or(599),
                headers: BTreeMap::new(),
                body: Zeroizing::new(Vec::new()),
                attempt: Some(UpstreamAttempt::new(
                    &request.method,
                    &request.url,
                    status,
                    elapsed_ms(started),
                    "none",
                    false,
                    Some(error),
                )),
                error: Some(error),
            };
        }
    };
    let status = response.status();
    let headers = response_headers(&response);
    let mut bytes = Zeroizing::new(Vec::new());
    let read_result = response
        .into_reader()
        .take(request.max_response_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes);
    let error = if read_result.is_err() {
        Some("response_read")
    } else if bytes.len() > request.max_response_bytes {
        bytes.zeroize();
        Some("body_limit")
    } else if request.response_encoding == "utf8" && std::str::from_utf8(&bytes).is_err() {
        Some("invalid_response")
    } else if !(200..300).contains(&status) {
        None
    } else {
        None
    };
    let body_truncated = matches!(error, Some("body_limit" | "response_read"));
    let body_kind = classify_body_kind(
        request.response_encoding.as_str(),
        &headers,
        bytes.len(),
        error,
    );
    let attempt = Some(UpstreamAttempt::new(
        &request.method,
        &request.url,
        Some(status),
        elapsed_ms(started),
        body_kind,
        body_truncated,
        error,
    ));
    HttpResponse {
        status,
        headers,
        body: bytes,
        attempt,
        error,
    }
}

fn validate(request: &HttpRequestSpec) -> Result<(), &'static str> {
    validate_download_request(request)?;
    if request
        .body
        .as_ref()
        .is_some_and(|body| body.len() > MAX_HTTP_REQUEST_BODY_BYTES)
    {
        return Err("body_limit");
    }
    if request.url.len() > 16 * 1024 || request.headers.len() > MAX_REQUEST_HEADERS {
        return Err("invalid_request");
    }
    let header_bytes = request.headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.len()).saturating_add(value.len())
    });
    if header_bytes > MAX_REQUEST_HEADER_BYTES {
        return Err("invalid_request");
    }
    if request.max_response_bytes == 0 || request.max_response_bytes > MAX_HTTP_RESPONSE_BYTES {
        return Err("invalid_request");
    }
    if request.response_encoding != "utf8" && request.response_encoding != "base64" {
        return Err("invalid_request");
    }
    Ok(())
}

fn validate_download_request(request: &HttpRequestSpec) -> Result<(), &'static str> {
    if !matches!(
        request.method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return Err("invalid_request");
    }
    if !(1..=MAX_HTTP_TIMEOUT_MS).contains(&request.timeout_ms) {
        return Err("invalid_request");
    }
    let url = Url::parse(&request.url).map_err(|_| "invalid_request")?;
    let loopback_http = cfg!(test)
        && url.scheme() == "http"
        && url
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "localhost" || host == "[::1]");
    if (url.scheme() != "https" && !loopback_http)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("invalid_request");
    }
    for (name, value) in &request.headers {
        if name.is_empty()
            || !name.bytes().all(is_header_name_byte)
            || value.chars().any(|ch| ch.is_control() && ch != '\t')
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "host"
                    | "content-length"
                    | "connection"
                    | "transfer-encoding"
                    | "te"
                    | "trailer"
                    | "upgrade"
                    | "keep-alive"
                    | "proxy-connection"
                    | "proxy-authorization"
                    | "proxy-authenticate"
                    | "expect"
            )
        {
            return Err("invalid_request");
        }
    }
    Ok(())
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn classify_body_kind(
    encoding: &str,
    headers: &BTreeMap<String, Vec<String>>,
    body_length: usize,
    error: Option<&'static str>,
) -> &'static str {
    if body_length == 0 && error.is_none() {
        return "empty";
    }
    if error == Some("invalid_response") {
        return "unknown";
    }
    let content_type = headers
        .get("content-type")
        .and_then(|values| values.first())
        .map(|value| value.split(';').next().unwrap_or_default().trim());
    if let Some(media_type) = content_type {
        if media_type.eq_ignore_ascii_case("application/json")
            || media_type.to_ascii_lowercase().ends_with("+json")
        {
            return "json";
        }
        if media_type.to_ascii_lowercase().starts_with("text/")
            || [
                "application/xml",
                "application/javascript",
                "application/x-www-form-urlencoded",
            ]
            .iter()
            .any(|known| media_type.eq_ignore_ascii_case(known))
        {
            return "text";
        }
        if !media_type.is_empty() {
            return "binary";
        }
    }
    if encoding == "base64" {
        "binary"
    } else {
        "text"
    }
}

fn open(request: &HttpRequestSpec) -> Result<ureq::Response, (Option<u16>, &'static str)> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_millis(request.timeout_ms))
        .build();
    let mut outgoing = agent.request(&request.method, &request.url);
    for (name, value) in &request.headers {
        outgoing = outgoing.set(name, value);
    }
    let result = match request.body.as_deref() {
        Some(body) => outgoing.send_string(body),
        None => outgoing.call(),
    };
    match result {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(_, response)) => Ok(response),
        Err(ureq::Error::Transport(_)) => Err((None, "transport")),
    }
}

fn response_headers(response: &ureq::Response) -> BTreeMap<String, Vec<String>> {
    let mut headers = BTreeMap::new();
    let mut total = 0usize;
    let mut seen = std::collections::HashSet::new();
    for name in response.headers_names() {
        let normalized_name = name.to_ascii_lowercase();
        if seen.contains(&normalized_name) {
            continue;
        }
        if seen.len() >= MAX_RESPONSE_HEADERS {
            break;
        }
        seen.insert(normalized_name.clone());
        let mut values = Vec::new();
        for value in response.all(&name) {
            let size = name.len().saturating_add(value.len());
            if total.saturating_add(size) > MAX_RESPONSE_HEADER_BYTES {
                break;
            }
            total += size;
            values.push(value.to_string());
        }
        if !values.is_empty() {
            headers.insert(normalized_name, values);
        }
    }
    headers
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "../../test/http_request.rs"]
mod tests;
