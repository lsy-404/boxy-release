use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

use base64::Engine as _;
use serde_json::Value;

#[test]
fn signed_progress_reports_file_changes_and_install_stage_without_failing_the_caller() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let verifying_key = key.verifying_key();
    let server = std::thread::spawn(move || {
        for (expected_file, expected_stage, expected_bytes) in [
            ("voice.dnni", "downloading", 128),
            ("voice-2.dnni", "downloading", 128),
            ("voice-2.dnni", "installing", 128),
            ("voice-2.dnni", "installing", 512),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let request = read_request(&mut stream);
            let (headers, body) = request.split_once("\r\n\r\n").unwrap();
            let first_line = headers.lines().next().unwrap();
            assert_eq!(
                first_line,
                "POST /api/v2/connector/operations/op/directory-tasks/task/progress HTTP/1.1"
            );
            let nonce = header(headers, "x-connector-nonce");
            let signature = header(headers, "x-connector-signature");
            let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(signature)
                .unwrap();
            let signed = format!(
                "POST\n/api/v2/connector/operations/op/directory-tasks/task/progress\n{nonce}"
            );
            verifying_key
                .verify_strict(
                    signed.as_bytes(),
                    &ed25519_dalek::Signature::from_slice(&signature).unwrap(),
                )
                .unwrap();
            let body: Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["stage"], expected_stage);
            assert_eq!(body["filename"], expected_file);
            assert_eq!(body["productId"], "product-1");
            assert_eq!(body["downloadedBytes"], expected_bytes);
            assert_eq!(body["totalBytes"], 1024);
            assert_eq!(body["completedItems"], 1);
            assert_eq!(body["totalItems"], 3);
            assert_eq!(body["fileIndex"], 2);
            assert_eq!(body["fileCount"], 4);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();
        }
    });

    let task_base = "/api/v2/connector/operations/op/directory-tasks/task";
    let server_url = format!("http://{address}");
    let mut reporter =
        crate::progress::ProductProgressReporter::new(&server_url, &key, task_base, 3);
    reporter.report(
        "downloading",
        "product-1",
        r"C:\models\voice.dnni",
        128,
        Some(1024),
        1,
        2,
        4,
    );
    reporter.report(
        "downloading",
        "product-1",
        r"C:\models\voice.dnni",
        256,
        Some(1024),
        1,
        2,
        4,
    );
    reporter.report(
        "downloading",
        "product-1",
        r"C:\models\voice-2.dnni",
        128,
        Some(1024),
        1,
        2,
        4,
    );
    reporter.report(
        "installing",
        "product-1",
        r"C:\models\voice-2.dnni",
        128,
        Some(1024),
        1,
        2,
        4,
    );
    std::thread::sleep(Duration::from_millis(1_050));
    reporter.report(
        "installing",
        "product-1",
        r"C:\models\voice-2.dnni",
        512,
        Some(1024),
        1,
        2,
        4,
    );
    server.join().unwrap();

    let unreachable = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let unused = unreachable.local_addr().unwrap();
    drop(unreachable);
    let unreachable_url = format!("http://{unused}");
    let mut best_effort =
        crate::progress::ProductProgressReporter::new(&unreachable_url, &key, task_base, 1);
    best_effort.report("downloading", "product-1", "voice.dnni", 1, None, 0, 1, 1);
}

fn read_request(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let count = stream.read(&mut chunk).unwrap();
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some((headers, body)) = String::from_utf8_lossy(&bytes).split_once("\r\n\r\n") {
            let length = header(headers, "content-length").parse::<usize>().unwrap();
            if body.len() >= length {
                break;
            }
        }
    }
    String::from_utf8(bytes).unwrap()
}

fn header<'a>(headers: &'a str, name: &str) -> &'a str {
    headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name).then(|| value.trim())
        })
        .unwrap()
}
