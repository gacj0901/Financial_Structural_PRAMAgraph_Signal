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
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "btc_stooq.csv",
        query: "BTC",
        role: CorpusRole::Historical,
        rows: 4_036,
        start: "2010-07-19",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "gold_calib.csv",
        query: "GOLD",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2023-01-17",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "gold_stooq.csv",
        query: "GOLD",
        role: CorpusRole::Historical,
        rows: 15_186,
        start: "1793-03-01",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "nasdaq_calib.csv",
        query: "NASDAQ",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2022-12-12",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "nasdaq_stooq.csv",
        query: "NASDAQ",
        role: CorpusRole::Historical,
        rows: 10_176,
        start: "1985-10-01",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "sp500_calib.csv",
        query: "SP500",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2022-12-12",
        end: "2026-02-20",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "sp500_stooq.csv",
        query: "SP500",
        role: CorpusRole::Historical,
        rows: 39_639,
        start: "1789-05-01",
        end: "2026-02-20",
        zero_volume_is_unavailable: true,
    },
    CorpusDefinition {
        file: "xrp_calib.csv",
        query: "XRP",
        role: CorpusRole::Calibration,
        rows: 800,
        start: "2023-12-16",
        end: "2026-02-22",
        zero_volume_is_unavailable: false,
    },
    CorpusDefinition {
        file: "xrp_stooq.csv",
        query: "XRP",
        role: CorpusRole::Historical,
        rows: 4_048,
        start: "2015-01-21",
        end: "2026-02-22",
        zero_volume_is_unavailable: true,
    },
];

pub fn audit_corpus(directory: impl AsRef<Path>) -> Result<CorpusAuditReport, CorpusError> {
    let directory = directory.as_ref();
    let resolver = AssetResolver::default();
    let mut files = Vec::with_capacity(CORPUS.len());
    for definition in CORPUS {
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
    let accepted = raw_rows == definition.rows
        && actual_start == definition.start
        && actual_end == definition.end;
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
        input_sha256: format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
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
