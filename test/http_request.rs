use super::*;
use std::{
    fmt::Write as FmtWrite,
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn spec(method: &str, url: String) -> HttpRequestSpec {
    HttpRequestSpec {
        method: method.into(),
        url,
        headers: BTreeMap::new(),
        body: None,
        response_encoding: "utf8".into(),
        max_response_bytes: 1024,
        timeout_ms: 2_000,
    }
}

fn fake_server(status: &str, body: &[u8]) -> (String, thread::JoinHandle<(String, Vec<u8>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let location = format!("http://{address}/followed");
    let body = body.to_vec();
    let status = status.to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 2048];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let text = String::from_utf8_lossy(&request);
            if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(str::to_string)
                    })
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if body.len() >= length {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&request).into_owned();
        let (headers, body_part) = text.split_once("\r\n\r\n").unwrap();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(str::to_string)
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let sent_body = body_part.as_bytes()[..body_part.len().min(content_length)].to_vec();
        let mut duplicate_headers = String::new();
        if body == b"many duplicate headers" {
            for index in 0..70 {
                write!(duplicate_headers, "Set-Cookie: cookie-{index}=1\r\n").unwrap();
            }
        } else {
            duplicate_headers.push_str("Set-Cookie: first=1\r\nSet-Cookie: second=2\r\n");
        }
        write!(stream, "HTTP/1.1 {status}\r\n{duplicate_headers}Content-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\nLocation: {location}\r\nX-Test-Response: observed\r\n\r\n", body.len()).unwrap();
        stream.write_all(&body).unwrap();
        (headers.to_string(), sent_body)
    });
    (format!("http://{address}"), server)
}

#[test]
fn executes_server_selected_method_url_headers_and_body_without_rewriting() {
    let (base, server) = fake_server("200 OK", b"done");
    let mut request = spec("PATCH", format!("{base}/v9/items?region=cn&limit=2"));
    request
        .headers
        .insert("X-Workflow".into(), "value-7".into());
    request
        .headers
        .insert("Content-Type".into(), "application/json".into());
    request.body = Some(r#"{"selection":"server-owned"}"#.into());
    let response = execute(&request);
    let (headers, body) = server.join().unwrap();
    let first_line = headers.lines().next().unwrap();
    assert_eq!(first_line, "PATCH /v9/items?region=cn&limit=2 HTTP/1.1");
    assert!(headers.to_ascii_lowercase().contains("x-workflow: value-7"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("content-type: application/json"));
    assert_eq!(body, br#"{"selection":"server-owned"}"#);
    assert_eq!(response.status, 200);
    assert_eq!(response.body.as_slice(), b"done");
    let attempt = response.attempt.as_ref().unwrap();
    assert_eq!(attempt.endpoint, request.url);
    assert_eq!(attempt.method, "PATCH");
    assert_eq!(attempt.sequence, 1);
}

#[test]
fn single_unauthorized_response_is_returned_without_refresh_or_retry() {
    let (base, server) = fake_server("401 Unauthorized", b"still unauthorized");
    let request = spec("GET", format!("{base}/one-attempt"));
    let response = execute(&request);
    let (headers, _) = server.join().unwrap();
    assert_eq!(headers.lines().next().unwrap(), "GET /one-attempt HTTP/1.1");
    assert_eq!(response.status, 401);
    assert_eq!(response.body.as_slice(), b"still unauthorized");
    let attempt = response.attempt.as_ref().unwrap();
    assert_eq!(attempt.status, Some(401));
    assert_eq!(attempt.body_kind, "binary");
    assert!(!attempt.body_truncated);
    assert!(attempt.duration_ms <= 2_000);
}

#[test]
fn returns_binary_response_as_bytes_and_keeps_raw_3xx_without_following() {
    let (base, server) = fake_server("200 OK", &[0, 159, 146, 150]);
    let mut request = spec("GET", format!("{base}/binary"));
    request.response_encoding = "base64".into();
    let response = execute(&request);
    server.join().unwrap();
    assert_eq!(response.body.as_slice(), &[0, 159, 146, 150]);
    assert_eq!(
        response.headers.get("x-test-response").unwrap(),
        &["observed"]
    );
    assert_eq!(
        response.headers.get("set-cookie").unwrap(),
        &["first=1", "second=2"]
    );

    let (base, server) = fake_server("200 OK", &[0, 255]);
    let mut request = spec("GET", format!("{base}/encoded"));
    request.response_encoding = "base64".into();
    let result = execute(&request);
    server.join().unwrap();
    assert_eq!(result.body.as_slice(), &[0, 255]);

    let (base, server) = fake_server("302 Found", b"redirect body");
    let response = execute(&spec("GET", format!("{base}/redirect")));
    let (headers, _) = server.join().unwrap();
    assert_eq!(headers.lines().next().unwrap(), "GET /redirect HTTP/1.1");
    assert_eq!(response.status, 302);
    assert_eq!(response.body.as_slice(), b"redirect body");
    assert_eq!(response.attempt.as_ref().unwrap().status, Some(302));
}

#[test]
fn duplicate_headers_do_not_consume_the_unique_header_name_limit() {
    let (base, server) = fake_server("200 OK", b"many duplicate headers");
    let response = execute(&spec("GET", format!("{base}/many-headers")));
    server.join().unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(response.headers.get("set-cookie").unwrap().len(), 70);
    assert_eq!(
        response.headers.get("content-type").unwrap(),
        &["application/octet-stream"]
    );
}

#[test]
fn reports_transport_failure_as_599_with_one_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let response = execute(&spec("GET", format!("http://{address}/unavailable")));
    assert_eq!(response.status, 599);
    assert_eq!(response.error, Some("transport"));
    let attempt = response.attempt.as_ref().unwrap();
    assert_eq!(attempt.status, None);
    assert_eq!(attempt.body_kind, "none");
    assert!(!attempt.body_truncated);
    assert_eq!(attempt.error_class, Some("transport"));
}

#[test]
fn reports_invalid_text_and_classifies_attempt_without_echoing_invalid_bytes() {
    let (base, server) = fake_server("200 OK", &[0xff, 0xfe]);
    let response = execute(&spec("GET", format!("{base}/bad-text")));
    server.join().unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.error, Some("invalid_response"));
    assert_eq!(response.body.as_slice(), &[0xff, 0xfe]);
    let attempt = response.attempt.as_ref().unwrap();
    assert_eq!(attempt.body_kind, "unknown");
    assert!(!attempt.body_truncated);
    assert_eq!(attempt.error_class, Some("invalid_response"));
}

#[test]
fn rejects_unsafe_request_fields_before_network_access() {
    let mut request = spec("TRACE", "https://example.test/".into());
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request = spec("POST", "https://user:pass@example.test/".into());
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request = spec("GET", "https://example.test/path#frag".into());
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request = spec("GET", "https://example.test/".into());
    request.headers.insert("Host".into(), "evil.test".into());
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request.headers.clear();
    request
        .headers
        .insert("X-Test".into(), "ok\r\ninjected: yes".into());
    assert_eq!(execute(&request).error, Some("invalid_request"));

    request = spec("GET", "https://example.test/".into());
    for index in 0..65 {
        request.headers.insert(format!("X-{index}"), "v".into());
    }
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request.headers.clear();
    request
        .headers
        .insert("X-Large".into(), "x".repeat(16 * 1024));
    assert_eq!(execute(&request).error, Some("invalid_request"));
}

#[test]
fn applies_request_and_response_limits_before_returning_data() {
    let (base, server) = fake_server("200 OK", b"123456");
    let mut request = spec("GET", format!("{base}/too-large"));
    request.max_response_bytes = 4;
    let response = execute(&request);
    server.join().unwrap();
    assert_eq!(response.error, Some("body_limit"));
    assert!(response.body.is_empty());
    let attempt = response.attempt.as_ref().unwrap();
    assert_eq!(attempt.status, Some(200));
    assert!(attempt.body_truncated);
    assert_eq!(attempt.body_kind, "binary");

    request = spec("GET", "https://example.test/".into());
    request.max_response_bytes = MAX_HTTP_RESPONSE_BYTES + 1;
    assert_eq!(execute(&request).error, Some("invalid_request"));
    request.max_response_bytes = 1;
    request.timeout_ms = MAX_HTTP_TIMEOUT_MS + 1;
    assert_eq!(execute(&request).error, Some("invalid_request"));
}
