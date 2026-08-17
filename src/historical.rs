use crate::{
    AvailableValue, ContractError, Instrument, MarketObservation, SessionCalendar, Timeframe,
};
use chrono::{Datelike, NaiveDate, Utc};
use csv::StringRecord;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;
use thiserror::Error;

const DAY_NS: i64 = 86_400_000_000_000;

#[derive(Debug, Error)]
pub enum HistoricalError {
    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("missing required CSV column: {0}")]
    MissingColumn(&'static str),
    #[error("row {row}: invalid {field} value `{value}`")]
    InvalidValue {
        row: usize,
        field: &'static str,
        value: String,
    },
    #[error("row {row}: {message}")]
    InvalidObservation { row: usize, message: String },
    #[error("timestamps must be strictly increasing; failure at row {0}")]
    NonIncreasing(usize),
    #[error("weekly aggregation requires at least one D1 bar")]
    EmptyAggregation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CadenceAnomaly {
    pub previous_open_time_ns: i64,
    pub current_open_time_ns: i64,
    pub gap_ns: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct CadencePolicy {
    pub minimum_gap_ns: i64,
    pub maximum_gap_ns: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HistoricalLoadPolicy {
    pub zero_volume_is_unavailable: bool,
    pub exclude_malformed_ohlc: bool,
}

impl CadencePolicy {
    pub fn daily(calendar: SessionCalendar) -> Self {
        match calendar {
            SessionCalendar::ContinuousUtc => Self {
                minimum_gap_ns: DAY_NS,
                maximum_gap_ns: DAY_NS,
            },
            // Weekend/holiday tolerance is explicit here and reported, never resampled.
            SessionCalendar::ExchangeSession => Self {
                minimum_gap_ns: DAY_NS,
                maximum_gap_ns: DAY_NS * 4,
            },
        }
    }
}

pub fn load_daily_csv(
    path: impl AsRef<Path>,
    instrument: &Instrument,
    source: &str,
) -> Result<Vec<MarketObservation>, HistoricalError> {
    load_daily_csv_with_policy(path, instrument, source, HistoricalLoadPolicy::default())
}

pub fn load_daily_csv_with_policy(
    path: impl AsRef<Path>,
    instrument: &Instrument,
    source: &str,
    policy: HistoricalLoadPolicy,
) -> Result<Vec<MarketObservation>, HistoricalError> {
    let file = File::open(path)?;
    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(file);
    let headers = reader.headers()?.clone();
    let date = column(&headers, &["date", "timestamp", "time"])?;
    let open = column(&headers, &["open"])?;
    let high = column(&headers, &["high"])?;
    let low = column(&headers, &["low"])?;
    let close = column(&headers, &["close"])?;
    let volume = optional_column(&headers, &["volume", "vol"]);

    let mut observations = Vec::new();
    for (offset, record) in reader.records().enumerate() {
        let row = offset + 2;
        let record = record?;
        let date = parse_date(field(&record, date, row, "date")?, row)?;
        let open_time_ns = date
            .and_hms_opt(0, 0, 0)
            .expect("midnight is valid")
            .and_utc()
            .timestamp_nanos_opt()
            .expect("supported timestamp");
        let observation = MarketObservation {
            instrument_id: instrument.instrument_id.clone(),
            timeframe: Timeframe::D1,
            open_time_ns,
            close_time_ns: open_time_ns + DAY_NS,
            open: parse_f64(field(&record, open, row, "open")?, row, "open")?,
            high: parse_f64(field(&record, high, row, "high")?, row, "high")?,
            low: parse_f64(field(&record, low, row, "low")?, row, "low")?,
            close: parse_f64(field(&record, close, row, "close")?, row, "close")?,
            is_closed: true,
            source: source.to_owned(),
            volume: match volume.and_then(|index| record.get(index)).map(str::trim) {
                Some(value) if !value.is_empty() => {
                    let parsed = parse_f64(value, row, "volume")?;
                    if policy.zero_volume_is_unavailable && parsed == 0.0 {
                        AvailableValue::unavailable()
                    } else {
                        AvailableValue::available(parsed)
                    }
                }
                _ => AvailableValue::unavailable(),
            },
            quote_volume: AvailableValue::unavailable(),
            trade_count: AvailableValue::unavailable(),
            best_bid: AvailableValue::unavailable(),
            best_ask: AvailableValue::unavailable(),
            bid_size: AvailableValue::unavailable(),
            ask_size: AvailableValue::unavailable(),
        };
        if let Err(error) = observation.validate() {
            if policy.exclude_malformed_ohlc && error == ContractError::MalformedOhlc {
                continue;
            }
            return Err(HistoricalError::InvalidObservation {
                row,
                message: error.to_string(),
            });
        }
        if observations
            .last()
            .is_some_and(|prior: &MarketObservation| prior.open_time_ns >= observation.open_time_ns)
        {
            return Err(HistoricalError::NonIncreasing(row));
        }
        observations.push(observation);
    }
    Ok(observations)
}

pub fn cadence_anomalies(
    observations: &[MarketObservation],
    policy: CadencePolicy,
) -> Vec<CadenceAnomaly> {
    observations
        .windows(2)
        .filter_map(|pair| {
            let gap = pair[1].open_time_ns - pair[0].open_time_ns;
            (gap < policy.minimum_gap_ns || gap > policy.maximum_gap_ns).then_some(CadenceAnomaly {
                previous_open_time_ns: pair[0].open_time_ns,
                current_open_time_ns: pair[1].open_time_ns,
                gap_ns: gap,
            })
        })
        .collect()
}

pub fn aggregate_weekly(
    daily: &[MarketObservation],
) -> Result<Vec<MarketObservation>, HistoricalError> {
    if daily.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    let mut weeks: Vec<Vec<&MarketObservation>> = Vec::new();
    for bar in daily {
        if bar.timeframe != Timeframe::D1 || !bar.is_closed {
            continue;
        }
        let date = chrono::DateTime::<Utc>::from_timestamp_nanos(bar.open_time_ns);
        let key = (date.iso_week().year(), date.iso_week().week());
        let starts_new = weeks
            .last()
            .and_then(|week| week.first())
            .is_none_or(|first| {
                let first_date = chrono::DateTime::<Utc>::from_timestamp_nanos(first.open_time_ns);
                (first_date.iso_week().year(), first_date.iso_week().week()) != key
            });
        if starts_new {
            weeks.push(Vec::new());
        }
        weeks.last_mut().expect("week exists").push(bar);
    }
    weeks.into_iter().map(aggregate_week).collect()
}

fn aggregate_week(bars: Vec<&MarketObservation>) -> Result<MarketObservation, HistoricalError> {
    let first = bars.first().ok_or(HistoricalError::EmptyAggregation)?;
    let last = bars.last().ok_or(HistoricalError::EmptyAggregation)?;
    let volume = if bars
        .iter()
        .all(|bar| bar.volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    Ok(MarketObservation {
        instrument_id: first.instrument_id.clone(),
        timeframe: Timeframe::W1,
        open_time_ns: first.open_time_ns,
        close_time_ns: last.close_time_ns,
        open: first.open,
        high: bars
            .iter()
            .map(|bar| bar.high)
            .fold(f64::NEG_INFINITY, f64::max),
        low: bars.iter().map(|bar| bar.low).fold(f64::INFINITY, f64::min),
        close: last.close,
        is_closed: true,
        source: first.source.clone(),
        volume,
        quote_volume: AvailableValue::unavailable(),
        trade_count: AvailableValue::unavailable(),
        best_bid: AvailableValue::unavailable(),
        best_ask: AvailableValue::unavailable(),
        bid_size: AvailableValue::unavailable(),
        ask_size: AvailableValue::unavailable(),
    })
}

fn optional_column(headers: &StringRecord, candidates: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        candidates
            .iter()
            .any(|candidate| header.trim().eq_ignore_ascii_case(candidate))
    })
}

fn column(headers: &StringRecord, candidates: &[&'static str]) -> Result<usize, HistoricalError> {
    optional_column(headers, candidates).ok_or(HistoricalError::MissingColumn(candidates[0]))
}

fn field<'a>(
    record: &'a StringRecord,
    index: usize,
    row: usize,
    name: &'static str,
) -> Result<&'a str, HistoricalError> {
    record
        .get(index)
        .ok_or_else(|| HistoricalError::InvalidValue {
            row,
            field: name,
            value: String::new(),
        })
}

fn parse_f64(value: &str, row: usize, field: &'static str) -> Result<f64, HistoricalError> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| HistoricalError::InvalidValue {
            row,
            field,
            value: value.to_owned(),
        })
}

fn parse_date(value: &str, row: usize) -> Result<NaiveDate, HistoricalError> {
    ["%Y-%m-%d", "%Y/%m/%d", "%d/%m/%Y"]
        .iter()
        .find_map(|format| NaiveDate::parse_from_str(value.trim(), format).ok())
        .ok_or_else(|| HistoricalError::InvalidValue {
            row,
            field: "date",
            value: value.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{AssetResolver, Resolution};
    use std::io::Write;

    fn btc() -> Instrument {
        match AssetResolver::default().resolve("BTC") {
            Resolution::Found { instrument } => instrument,
            _ => panic!("BTC must resolve"),
        }
    }

    #[test]
    fn preserves_missing_volume() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        writeln!(file, "2026-01-01,10,12,9,11").unwrap();
        let bars = load_daily_csv(file.path(), &btc(), "test").unwrap();
        assert_eq!(bars[0].volume, AvailableValue::unavailable());
    }

    #[test]
    fn rejects_duplicate_dates() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        writeln!(file, "2026-01-01,10,12,9,11").unwrap();
        writeln!(file, "2026-01-01,11,13,10,12").unwrap();
        assert!(matches!(
            load_daily_csv(file.path(), &btc(), "test"),
            Err(HistoricalError::NonIncreasing(3))
        ));
    }

    #[test]
    fn weekly_aggregation_is_causal_and_preserves_ohlc() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close,Volume").unwrap();
        writeln!(file, "2026-01-05,10,12,9,11,5").unwrap();
        writeln!(file, "2026-01-06,11,14,10,13,7").unwrap();
        let daily = load_daily_csv(file.path(), &btc(), "test").unwrap();
        let weekly = aggregate_weekly(&daily).unwrap();
        assert_eq!(weekly.len(), 1);
        assert_eq!(
            (
                weekly[0].open,
                weekly[0].high,
                weekly[0].low,
                weekly[0].close
            ),
            (10.0, 14.0, 9.0, 13.0)
        );
        assert_eq!(weekly[0].volume.value, Some(12.0));
    }

    #[test]
    fn continuous_calendar_reports_gaps() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        writeln!(file, "2026-01-01,10,12,9,11").unwrap();
        writeln!(file, "2026-01-03,11,13,10,12").unwrap();
        let bars = load_daily_csv(file.path(), &btc(), "test").unwrap();
        assert_eq!(
            cadence_anomalies(&bars, CadencePolicy::daily(SessionCalendar::ContinuousUtc)).len(),
            1
        );
    }

    #[test]
    fn configured_zero_volume_is_unavailable() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close,Volume").unwrap();
        writeln!(file, "2026-01-01,10,12,9,11,0").unwrap();
        let bars = load_daily_csv_with_policy(
            file.path(),
            &btc(),
            "test",
            HistoricalLoadPolicy {
                zero_volume_is_unavailable: true,
                exclude_malformed_ohlc: false,
            },
        )
        .unwrap();
        assert_eq!(bars[0].volume, AvailableValue::unavailable());
    }
}
