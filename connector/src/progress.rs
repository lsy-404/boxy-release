use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use serde::Serialize;

use crate::request_signature;

const REPORT_INTERVAL: Duration = Duration::from_secs(1);
const REPORT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressBody<'a> {
    stage: &'a str,
    product_id: &'a str,
    filename: &'a str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    completed_items: usize,
    total_items: usize,
    file_index: usize,
    file_count: usize,
}

pub(crate) struct ProductProgressReporter<'a> {
    server: &'a str,
    key: &'a SigningKey,
    path: String,
    total_items: usize,
    last_report: Option<Instant>,
    last_event: Option<(String, String, String, usize)>,
}

impl<'a> ProductProgressReporter<'a> {
    pub(crate) fn new(
        server: &'a str,
        key: &'a SigningKey,
        task_base: &str,
        total_items: usize,
    ) -> Self {
        Self {
            server,
            key,
            path: format!("{task_base}/progress"),
            total_items,
            last_report: None,
            last_event: None,
        }
    }

    pub(crate) fn report(
        &mut self,
        stage: &str,
        product_id: &str,
        filename: &str,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        completed_items: usize,
        file_index: usize,
        file_count: usize,
    ) {
        let filename = filename.rsplit(['/', '\\']).next().unwrap_or_default();
        let event = (
            stage.to_string(),
            product_id.to_string(),
            filename.to_string(),
            file_index,
        );
        let now = Instant::now();
        let changed = self.last_event.as_ref() != Some(&event);
        if !changed
            && self
                .last_report
                .is_some_and(|last| now.duration_since(last) < REPORT_INTERVAL)
        {
            return;
        }

        self.last_event = Some(event);
        self.last_report = Some(now);
        let (nonce, signature) = request_signature(self.key, "POST", &self.path);
        let body = ProgressBody {
            stage,
            product_id,
            filename,
            downloaded_bytes,
            total_bytes,
            completed_items,
            total_items: self.total_items,
            file_index,
            file_count,
        };
        let _ = ureq::post(&format!("{}{}", self.server, self.path))
            .timeout(REPORT_TIMEOUT)
            .set("Content-Type", "application/json")
            .set("X-Connector-Nonce", &nonce)
            .set("X-Connector-Signature", &signature)
            .send_json(body);
    }
}
