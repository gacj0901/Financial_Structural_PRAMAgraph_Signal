//! Runtime request/response logging
//!
//! Structured JSONL logging for observability. Does not affect financial response hashes.

use crate::{RuntimeStatus, Timeframe};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

static LOG_FILE: Mutex<Option<BufWriter<std::fs::File>>> = Mutex::new(None);
static START_TIME: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Initialize logging to a file
pub fn init_logging(log_path: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let writer = BufWriter::new(file);
    *LOG_FILE.lock().unwrap() = Some(writer);
    START_TIME.get_or_init(Instant::now);
    Ok(())
}

/// Request received event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestReceived {
    pub event: &'static str,
    pub timestamp: String,
    pub request_id: String,
    pub endpoint: String,
    pub asset: Option<String>,
    pub requested_timeframes: Option<Vec<String>>,
    pub user_agent: Option<String>,
    pub request_body_sha256: String,
}

/// Request served event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestServed {
    pub event: &'static str,
    pub timestamp: String,
    pub request_id: String,
    pub http_status: u16,
    pub pramagraph_status: String,
    pub asset: Option<String>,
    pub returned_timeframes: Option<Vec<String>>,
    pub elapsed_ms: u64,
    pub response_sha256: Option<String>,
}

/// Request failed event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestFailed {
    pub event: &'static str,
    pub timestamp: String,
    pub request_id: String,
    pub http_status: u16,
    pub error: String,
    pub elapsed_ms: u64,
}

impl RequestReceived {
    pub fn new(
        request_id: String,
        endpoint: String,
        asset: Option<String>,
        requested_timeframes: Option<Vec<Timeframe>>,
        user_agent: Option<String>,
        request_body: &str,
    ) -> Self {
        let body_hash = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(request_body.as_bytes()))
        );
        let tfs =
            requested_timeframes.map(|tfs| tfs.iter().map(|tf| format!("{:?}", tf)).collect());
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
        Self {
            event: "REQUEST_RECEIVED",
            timestamp,
            request_id,
            endpoint,
            asset,
            requested_timeframes: tfs,
            user_agent,
            request_body_sha256: body_hash,
        }
    }

    pub fn log(&self) {
        if let Some(writer) = LOG_FILE.lock().unwrap().as_mut() {
            let _ = writeln!(
                writer,
                "{}",
                serde_json::to_string(self).unwrap_or_default()
            );
            let _ = writer.flush();
        }
        // Console log
        eprintln!(
            "[{}] {} {} asset={:?} timeframes={:?} body_hash={}",
            self.timestamp,
            self.event,
            self.endpoint,
            self.asset,
            self.requested_timeframes,
            self.request_body_sha256
        );
    }
}

impl RequestServed {
    pub fn new(
        request_id: String,
        http_status: u16,
        pramagraph_status: RuntimeStatus,
        asset: Option<String>,
        returned_timeframes: Option<Vec<Timeframe>>,
        elapsed: Duration,
        response_sha256: Option<String>,
    ) -> Self {
        let tfs = returned_timeframes.map(|tfs| tfs.iter().map(|tf| format!("{:?}", tf)).collect());
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
        Self {
            event: "REQUEST_SERVED",
            timestamp,
            request_id,
            http_status,
            pramagraph_status: format!("{:?}", pramagraph_status),
            asset,
            returned_timeframes: tfs,
            elapsed_ms: elapsed.as_millis() as u64,
            response_sha256,
        }
    }

    pub fn log(&self) {
        if let Some(writer) = LOG_FILE.lock().unwrap().as_mut() {
            let _ = writeln!(
                writer,
                "{}",
                serde_json::to_string(self).unwrap_or_default()
            );
            let _ = writer.flush();
        }
        eprintln!(
            "[{}] {} {} status={} http={} asset={:?} timeframes={:?} elapsed_ms={} resp_hash={:?}",
            self.timestamp,
            self.event,
            self.request_id,
            self.pramagraph_status,
            self.http_status,
            self.asset,
            self.returned_timeframes,
            self.elapsed_ms,
            self.response_sha256
        );
    }
}

impl RequestFailed {
    pub fn new(request_id: String, http_status: u16, error: String, elapsed: Duration) -> Self {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_else(|| "1970-01-01T00:00:00Z".into());
        Self {
            event: "REQUEST_FAILED",
            timestamp,
            request_id,
            http_status,
            error,
            elapsed_ms: elapsed.as_millis() as u64,
        }
    }

    pub fn log(&self) {
        if let Some(writer) = LOG_FILE.lock().unwrap().as_mut() {
            let _ = writeln!(
                writer,
                "{}",
                serde_json::to_string(self).unwrap_or_default()
            );
            let _ = writer.flush();
        }
        eprintln!(
            "[{}] {} {} http={} error={} elapsed_ms={}",
            self.timestamp,
            self.event,
            self.request_id,
            self.http_status,
            self.error,
            self.elapsed_ms
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn logging_creates_valid_jsonl() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        init_logging(&path).unwrap();

        let req = RequestReceived::new(
            "req-1".into(),
            "/v1/financial/signal".into(),
            Some("BTC".into()),
            Some(vec![Timeframe::D1, Timeframe::W1]),
            Some("test-agent".into()),
            r#"{"asset":"BTC"}"#,
        );
        req.log();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("REQUEST_RECEIVED"));
        assert!(content.contains("req-1"));
        assert!(content.contains("BTC"));
    }

    #[test]
    fn request_served_logs_correctly() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        init_logging(&path).unwrap();

        let served = RequestServed::new(
            "req-1".into(),
            200,
            RuntimeStatus::Ok,
            Some("BTC".into()),
            Some(vec![Timeframe::D1]),
            Duration::from_millis(42),
            Some("sha256:abc".into()),
        );
        served.log();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("REQUEST_SERVED"));
        assert!(content.contains("200"));
    }

    #[test]
    fn request_failed_logs_correctly() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        init_logging(&path).unwrap();

        let failed = RequestFailed::new(
            "req-1".into(),
            404,
            "asset not found".into(),
            Duration::from_millis(10),
        );
        failed.log();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("REQUEST_FAILED"));
        assert!(content.contains("404"));
        assert!(content.contains("asset not found"));
    }
}
