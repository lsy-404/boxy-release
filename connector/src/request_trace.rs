use serde::Serialize;
use zeroize::Zeroize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpstreamAttempt {
    pub(crate) sequence: usize,
    pub(crate) phase: &'static str,
    pub(crate) method: String,
    pub(crate) endpoint: String,
    pub(crate) status: Option<u16>,
    pub(crate) duration_ms: u64,
    pub(crate) body_kind: &'static str,
    pub(crate) body_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error_class: Option<&'static str>,
}

impl UpstreamAttempt {
    pub(crate) fn new(
        method: &str,
        endpoint: &str,
        status: Option<u16>,
        duration_ms: u64,
        body_kind: &'static str,
        body_truncated: bool,
        error_class: Option<&'static str>,
    ) -> Self {
        Self {
            sequence: 1,
            phase: "request",
            method: method.to_string(),
            endpoint: endpoint.to_string(),
            status,
            duration_ms,
            body_kind,
            body_truncated,
            error_class,
        }
    }
}

impl Drop for UpstreamAttempt {
    fn drop(&mut self) {
        self.method.zeroize();
        self.endpoint.zeroize();
    }
}
