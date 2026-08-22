use crate::canonical;
use crate::historical::{
    cadence_anomalies, load_daily_csv_with_policy, CadencePolicy, HistoricalError,
    HistoricalLoadPolicy,
};
use crate::resolver::{AssetResolver, Resolution};
use crate::{AvailabilityStatus, Instrument};
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CorpusRole {
    Calibration,
    Historical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CorpusFileStatus {
    Accepted,
    AcceptedWithExclusions,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CorpusFileReport {
    pub file: String,
    pub role: CorpusRole,
    pub instrument_id: String,
    pub expected_rows: usize,
    pub actual_rows: usize,
    pub valid_rows: usize,
    pub malformed_rows_excluded: usize,
    pub expected_start: String,
    pub actual_start: String,
    pub expected_end: String,
    pub actual_end: String,
    pub cadence_anomalies: usize,
    pub volume_available_rows: usize,
    pub volume_unavailable_rows: usize,
    pub input_sha256: String,
    pub status: CorpusFileStatus,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CorpusAuditReport {
    pub schema: String,
    pub files: Vec<CorpusFileReport>,
    pub all_files_accepted: bool,
    pub report_sha256: Option<String>,
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error(transparent)]
    Historical(#[from] HistoricalError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("catalog instrument `{0}` cannot be resolved uniquely")]
    Resolution(String),
    #[error("corpus file `{0}` is empty")]
    Empty(String),
    #[error("canonical hashing failed: {0}")]
    Hash(#[from] canonical::CanonicalError),
}

#[derive(Debug, Clone, Copy)]
struct CorpusDefinition {
    file: &'static str,
    query: &'static str,
    role: CorpusRole,
    rows: usize,
    start: &'static str,
    end: &'static str,
    expected_sha256: &'static str,
    zero_volume_is_unavailable: bool,
}

const CORPUS: [CorpusDefinition; 10] = [
    CorpusDefinition {
        file: "btc_calib.csv",
        query: "BTC",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2023-01-18",
        end: "2026-02-20",
        expected_sha256: "sha256:832703c05ed61404a5e4e84c72335fdd7afaef2f0600eee5b0eba9e8684f1a6e",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "btc_stooq.csv",
        query: "BTC",
        role: CorpusRole::Historical,
        rows: 4_036,
        start: "2010-07-19",
        end: "2026-02-20",
        expected_sha256: "sha256:a39974c14c7d96eecfec4cab83188ed7558f28e653964c36a34c508b0a498173",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "gold_calib.csv",
        query: "GOLD",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2023-01-17",
        end: "2026-02-20",
        expected_sha256: "sha256:b9574afdbff404ead9187b3d689b99e609103694c5b58c0176c6318f4cb83704",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "gold_stooq.csv",
        query: "GOLD",
        role: CorpusRole::Historical,
        rows: 15_186,
        start: "1793-03-01",
        end: "2026-02-20",
        expected_sha256: "sha256:431090e147380451039359a3eaf862f3b1e90aadbd986e3ccd2296aef6836ed5",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "nasdaq_calib.csv",
        query: "NASDAQ",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2022-12-12",
        end: "2026-02-20",
        expected_sha256: "sha256:c94aa633e297906da426d940bdfda9ac5b37a234ea7e64ccba01bb8b59c226e4",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "nasdaq_stooq.csv",
        query: "NASDAQ",
        role: CorpusRole::Historical,
        rows: 10_176,
        start: "1985-10-01",
        end: "2026-02-20",
        expected_sha256: "sha256:0048a6ecf2638e3e05f28273ee52b5040001606205546e45f77937ebb5e21954",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "sp500_calib.csv",
        query: "SP500",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2022-12-12",
        end: "2026-02-20",
        expected_sha256: "sha256:d41340eb68958937ba1e98d25109559a033207cad0dd27d90a6dfdef6d509156",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "sp500_stooq.csv",
        query: "SP500",
        role: CorpusRole::Historical,
        rows: 39_639,
        start: "1789-05-01",
        end: "2026-02-20",
        expected_sha256: "sha256:0949a29543a0b6aae2e9be66652850959f9342d04293b30b69e3870ac1bbe681",
        zero_volume_is_unavailable: true,
    },
    CorpusDefinition {
        file: "xrp_calib.csv",
        query: "XRP",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2023-12-16",
        end: "2026-02-22",
        expected_sha256: "sha256:ff988dc105665896601a6c99fddcaa553b241123f8142d1aa2d61d2cb71ea1f5",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "xrp_stooq.csv",
        query: "XRP",
        role: CorpusRole::Historical,
        rows: 4_048,
        start: "2015-01-21",
        end: "2026-02-22",
        expected_sha256: "sha256:668024a9dbc834540bb4067a7050ad1c1cd07d92219fca8b64e43f881525a8b3",
        zero_volume_is_unavailable: true,
    },
];

pub fn audit_corpus(directory: impl AsRef<Path>) -> Result<CorpusAuditReport, CorpusError> {
    audit_definitions(directory.as_ref(), CORPUS)
}

/// Audit only corpus inputs that the runtime can actually serve.
///
/// Long historical research files are deliberately excluded from readiness:
/// their absence must not take the online API down when the serving path uses
/// only the pinned calibration corpora.
pub fn audit_runtime_corpus(directory: impl AsRef<Path>) -> Result<CorpusAuditReport, CorpusError> {
    audit_definitions(
        directory.as_ref(),
        CORPUS
            .into_iter()
            .filter(|definition| definition.role == CorpusRole::Calibration),
    )
}

fn audit_definitions(
    directory: &Path,
    definitions: impl IntoIterator<Item = CorpusDefinition>,
) -> Result<CorpusAuditReport, CorpusError> {
    let resolver = AssetResolver::default();
    let mut files = Vec::new();
    for definition in definitions {
        let instrument = resolve(&resolver, definition.query)?;
        files.push(audit_file(
            directory.join(definition.file),
            &instrument,
            definition,
        )?);
    }
    let all_files_accepted = files.iter().all(|file| file.accepted);
    let mut report = CorpusAuditReport {
        schema: "pramagraph.financial_corpus_audit.v1".to_owned(),
        files,
        all_files_accepted,
        report_sha256: None,
    };
    report.report_sha256 = Some(canonical::sha256(&report)?);
    Ok(report)
}

fn audit_file(
    path: PathBuf,
    instrument: &Instrument,
    definition: CorpusDefinition,
) -> Result<CorpusFileReport, CorpusError> {
    let bytes = fs::read(&path)?;
    let raw_rows = csv::Reader::from_reader(bytes.as_slice())
        .records()
        .collect::<Result<Vec<_>, _>>()?
        .len();
    let observations = load_daily_csv_with_policy(
        &path,
        instrument,
        "supplied_corpus",
        HistoricalLoadPolicy {
            zero_volume_is_unavailable: definition.zero_volume_is_unavailable,
            exclude_malformed_ohlc: true,
        },
    )?;
    let first = observations
        .first()
        .ok_or_else(|| CorpusError::Empty(definition.file.into()))?;
    let last = observations
        .last()
        .ok_or_else(|| CorpusError::Empty(definition.file.into()))?;
    let actual_start = date_string(first.open_time_ns);
    let actual_end = date_string(last.open_time_ns);
    let volume_available_rows = observations
        .iter()
        .filter(|bar| bar.volume.availability == AvailabilityStatus::Available)
        .count();
    let volume_unavailable_rows = observations.len() - volume_available_rows;
    let malformed_rows_excluded = raw_rows - observations.len();
    let input_sha256 = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
    let accepted = raw_rows == definition.rows
        && actual_start == definition.start
        && actual_end == definition.end
        && input_sha256 == definition.expected_sha256;
    let status = if !accepted {
        CorpusFileStatus::Rejected
    } else if malformed_rows_excluded > 0 {
        CorpusFileStatus::AcceptedWithExclusions
    } else {
        CorpusFileStatus::Accepted
    };
    Ok(CorpusFileReport {
        file: definition.file.to_owned(),
        role: definition.role,
        instrument_id: instrument.instrument_id.clone(),
        expected_rows: definition.rows,
        actual_rows: raw_rows,
        valid_rows: observations.len(),
        malformed_rows_excluded,
        expected_start: definition.start.to_owned(),
        actual_start,
        expected_end: definition.end.to_owned(),
        actual_end,
        cadence_anomalies: cadence_anomalies(
            &observations,
            CadencePolicy::daily(instrument.session_calendar),
        )
        .len(),
        volume_available_rows,
        volume_unavailable_rows,
        input_sha256,
        status,
        accepted,
    })
}

fn resolve(resolver: &AssetResolver, query: &str) -> Result<Instrument, CorpusError> {
    match resolver.resolve(query) {
        Resolution::Found { instrument } => Ok(instrument),
        _ => Err(CorpusError::Resolution(query.to_owned())),
    }
}

fn date_string(timestamp_ns: i64) -> String {
    chrono::DateTime::<Utc>::from_timestamp_nanos(timestamp_ns)
        .format("%Y-%m-%d")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUNTIME_FILES: [&str; 5] = [
        "btc_calib.csv",
        "gold_calib.csv",
        "nasdaq_calib.csv",
        "sp500_calib.csv",
        "xrp_calib.csv",
    ];

    fn copy_runtime_corpus(destination: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/corpus");
        for file in RUNTIME_FILES {
            fs::copy(source.join(file), destination.join(file)).unwrap();
        }
    }

    #[test]
    fn runtime_audit_ignores_unused_historical_files() {
        let directory = tempfile::tempdir().unwrap();
        copy_runtime_corpus(directory.path());

        let report = audit_runtime_corpus(directory.path()).unwrap();
        assert!(report.all_files_accepted);
        assert_eq!(report.files.len(), RUNTIME_FILES.len());
        assert!(report
            .files
            .iter()
            .all(|file| file.role == CorpusRole::Calibration));
    }

    #[test]
    fn runtime_audit_rejects_byte_drift_even_when_csv_still_parses() {
        let directory = tempfile::tempdir().unwrap();
        copy_runtime_corpus(directory.path());
        let path = directory.path().join("btc_calib.csv");
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b'\n');
        fs::write(path, bytes).unwrap();

        let report = audit_runtime_corpus(directory.path()).unwrap();
        assert!(!report.all_files_accepted);
        let btc = report
            .files
            .iter()
            .find(|file| file.file == "btc_calib.csv")
            .unwrap();
        assert_eq!(btc.status, CorpusFileStatus::Rejected);
    }
}
