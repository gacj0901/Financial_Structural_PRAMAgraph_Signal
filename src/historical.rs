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

/// Aggregates closed D1 observations into closed ISO-week observations.
///
/// A trailing week is emitted only after its end can be established from the
/// supplied observations. This keeps an in-progress week out of downstream
/// closed-bar replay without consulting wall-clock time. A later ISO week is
/// sufficient evidence that an earlier week has closed; for a final continuous
/// week, coverage through the ISO-week boundary is also sufficient. Holiday-
/// shortened weeks remain valid once a later ISO week is observed.
pub fn aggregate_weekly(
    daily: &[MarketObservation],
) -> Result<Vec<MarketObservation>, HistoricalError> {
    if daily.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    let authority = &daily[0];
    for (index, bar) in daily.iter().enumerate() {
        if bar.timeframe != Timeframe::D1 || !bar.is_closed {
            return Err(HistoricalError::InvalidObservation {
                row: index + 1,
                message: "weekly aggregation requires closed D1 bars".into(),
            });
        }
        if bar.instrument_id != authority.instrument_id || bar.source != authority.source {
            return Err(HistoricalError::InvalidObservation {
                row: index + 1,
                message: "weekly aggregation cannot mix instrument or source authority".into(),
            });
        }
        if index > 0 && daily[index - 1].open_time_ns >= bar.open_time_ns {
            return Err(HistoricalError::NonIncreasing(index + 1));
        }
    }
    let mut weeks: Vec<Vec<&MarketObservation>> = Vec::new();
    for bar in daily {
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
    let week_count = weeks.len();
    weeks
        .into_iter()
        .enumerate()
        .filter_map(|(index, bars)| {
            weekly_bucket_is_closed(&bars, index + 1 < week_count).then_some(bars)
        })
        .map(aggregate_week)
        .collect()
}

fn weekly_bucket_is_closed(bars: &[&MarketObservation], has_later_week: bool) -> bool {
    let Some(first) = bars.first() else {
        return false;
    };
    if has_later_week {
        return true;
    }

    let first_date = chrono::DateTime::<Utc>::from_timestamp_nanos(first.open_time_ns);
    let week_start = first_date.date_naive().checked_sub_days(chrono::Days::new(
        first_date.weekday().num_days_from_monday().into(),
    ));
    let week_start_ns = week_start
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .and_then(|date_time| date_time.and_utc().timestamp_nanos_opt());
    let Some(next_week_start_ns) =
        week_start_ns.and_then(|timestamp| timestamp.checked_add(7 * DAY_NS))
    else {
        return false;
    };
    // Closed-interval providers such as Binance encode a bar ending at the
    // boundary as `boundary - 1 ms`. Accept that exact timestamp convention
    // without treating an earlier partial day as a closed week.
    const INCLUSIVE_CLOSE_MILLISECOND_NS: i64 = 1_000_000;
    bars.last().is_some_and(|last| {
        last.close_time_ns
            .saturating_add(INCLUSIVE_CLOSE_MILLISECOND_NS)
            >= next_week_start_ns
    })
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

/// Aggregate M1 bars into M5 bars (5 M1 bars = 1 M5 bar)
pub fn aggregate_m5_from_m1(
    m1_bars: &[MarketObservation],
) -> Result<Vec<MarketObservation>, HistoricalError> {
    if m1_bars.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    if m1_bars
        .iter()
        .any(|bar| bar.timeframe != Timeframe::M1 || !bar.is_closed)
    {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "all input bars must be closed M1 bars".into(),
        });
    }
    let mut m5_bars = Vec::new();
    let mut current_batch = Vec::new();

    for bar in m1_bars {
        current_batch.push(bar);
        if current_batch.len() == 5 {
            m5_bars.push(aggregate_m5_batch(&current_batch)?);
            current_batch.clear();
        }
    }

    // Partial batch at the end is dropped (incomplete bar not included)
    Ok(m5_bars)
}

fn aggregate_m5_batch(bars: &[&MarketObservation]) -> Result<MarketObservation, HistoricalError> {
    if bars.len() != 5 {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "M5 batch must have exactly 5 bars".into(),
        });
    }
    let first = bars[0];
    let last = bars[4];
    let volume = if bars
        .iter()
        .all(|bar| bar.volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let quote_volume = if bars
        .iter()
        .all(|bar| bar.quote_volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.quote_volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let trade_count = if bars
        .iter()
        .all(|bar| bar.trade_count.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.trade_count.value).sum())
    } else {
        AvailableValue::unavailable()
    };

    Ok(MarketObservation {
        instrument_id: first.instrument_id.clone(),
        timeframe: Timeframe::M5,
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
        quote_volume,
        trade_count,
        best_bid: AvailableValue::unavailable(),
        best_ask: AvailableValue::unavailable(),
        bid_size: AvailableValue::unavailable(),
        ask_size: AvailableValue::unavailable(),
    })
}

/// Aggregate M1/M5 bars into H1 bars
pub fn aggregate_h1(
    intraday_bars: &[MarketObservation],
) -> Result<Vec<MarketObservation>, HistoricalError> {
    if intraday_bars.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    // Must be all same timeframe (M1 or M5) and closed
    let source_tf = intraday_bars[0].timeframe;
    if !matches!(source_tf, Timeframe::M1 | Timeframe::M5) {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "H1 aggregation requires M1 or M5 input bars".into(),
        });
    }
    if intraday_bars
        .iter()
        .any(|bar| bar.timeframe != source_tf || !bar.is_closed)
    {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "all input bars must be closed and same timeframe".into(),
        });
    }

    let bars_per_hour = match source_tf {
        Timeframe::M1 => 60,
        Timeframe::M5 => 12,
        _ => unreachable!(),
    };

    let mut h1_bars = Vec::new();
    let mut current_batch = Vec::new();

    for bar in intraday_bars {
        current_batch.push(bar);
        if current_batch.len() == bars_per_hour {
            h1_bars.push(aggregate_h1_batch(&current_batch)?);
            current_batch.clear();
        }
    }

    // Partial batch at the end is dropped (incomplete bar not included)
    Ok(h1_bars)
}

fn aggregate_h1_batch(bars: &[&MarketObservation]) -> Result<MarketObservation, HistoricalError> {
    if bars.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    let first = bars[0];
    let last = bars.last().unwrap();
    let volume = if bars
        .iter()
        .all(|bar| bar.volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let quote_volume = if bars
        .iter()
        .all(|bar| bar.quote_volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.quote_volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let trade_count = if bars
        .iter()
        .all(|bar| bar.trade_count.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.trade_count.value).sum())
    } else {
        AvailableValue::unavailable()
    };

    Ok(MarketObservation {
        instrument_id: first.instrument_id.clone(),
        timeframe: Timeframe::H1,
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
        quote_volume,
        trade_count,
        best_bid: AvailableValue::unavailable(),
        best_ask: AvailableValue::unavailable(),
        bid_size: AvailableValue::unavailable(),
        ask_size: AvailableValue::unavailable(),
    })
}

/// Aggregate H1 bars into H4 bars (4 H1 bars = 1 H4 bar)
pub fn aggregate_h4_from_h1(
    h1_bars: &[MarketObservation],
) -> Result<Vec<MarketObservation>, HistoricalError> {
    if h1_bars.is_empty() {
        return Err(HistoricalError::EmptyAggregation);
    }
    if h1_bars
        .iter()
        .any(|bar| bar.timeframe != Timeframe::H1 || !bar.is_closed)
    {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "all input bars must be closed H1 bars".into(),
        });
    }
    let mut h4_bars = Vec::new();
    let mut current_batch = Vec::new();

    for bar in h1_bars {
        current_batch.push(bar);
        if current_batch.len() == 4 {
            h4_bars.push(aggregate_h4_batch(&current_batch)?);
            current_batch.clear();
        }
    }

    // Partial batch at the end is dropped
    Ok(h4_bars)
}

fn aggregate_h4_batch(bars: &[&MarketObservation]) -> Result<MarketObservation, HistoricalError> {
    if bars.len() != 4 {
        return Err(HistoricalError::InvalidObservation {
            row: 0,
            message: "H4 batch must have exactly 4 bars".into(),
        });
    }
    let first = bars[0];
    let last = bars[3];
    let volume = if bars
        .iter()
        .all(|bar| bar.volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let quote_volume = if bars
        .iter()
        .all(|bar| bar.quote_volume.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.quote_volume.value).sum())
    } else {
        AvailableValue::unavailable()
    };
    let trade_count = if bars
        .iter()
        .all(|bar| bar.trade_count.availability == crate::AvailabilityStatus::Available)
    {
        AvailableValue::available(bars.iter().filter_map(|bar| bar.trade_count.value).sum())
    } else {
        AvailableValue::unavailable()
    };

    Ok(MarketObservation {
        instrument_id: first.instrument_id.clone(),
        timeframe: Timeframe::H4,
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
        quote_volume,
        trade_count,
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
        writeln!(file, "2026-01-07,13,15,12,14,6").unwrap();
        writeln!(file, "2026-01-08,14,16,13,15,6").unwrap();
        writeln!(file, "2026-01-09,15,16,14,15,6").unwrap();
        writeln!(file, "2026-01-10,15,16,14,15,6").unwrap();
        writeln!(file, "2026-01-11,15,16,14,15,6").unwrap();
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
            (10.0, 16.0, 9.0, 15.0)
        );
        assert_eq!(weekly[0].volume.value, Some(42.0));
        assert!(weekly[0].is_closed);
    }

    #[test]
    fn weekly_aggregation_omits_an_in_progress_trailing_week() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        for day in 5..=14 {
            writeln!(file, "2026-01-{day:02},10,12,9,11").unwrap();
        }
        let daily = load_daily_csv(file.path(), &btc(), "test").unwrap();
        let weekly = aggregate_weekly(&daily).unwrap();

        assert_eq!(weekly.len(), 1);
        assert_eq!(
            chrono::DateTime::<Utc>::from_timestamp_nanos(weekly[0].open_time_ns).date_naive(),
            NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()
        );
        assert!(weekly.iter().all(|bar| bar.is_closed));
    }

    #[test]
    fn weekly_aggregation_does_not_close_a_partial_only_week() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        writeln!(file, "2026-01-05,10,12,9,11").unwrap();
        writeln!(file, "2026-01-06,11,14,10,13").unwrap();
        let daily = load_daily_csv(file.path(), &btc(), "test").unwrap();

        assert!(aggregate_weekly(&daily).unwrap().is_empty());
    }

    #[test]
    fn weekly_aggregation_keeps_a_closed_holiday_shortened_week() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        for day in 6..=9 {
            writeln!(file, "2026-01-{day:02},10,12,9,11").unwrap();
        }
        // Observing the following ISO week is causal evidence that the prior
        // shortened trading week has closed, even though Monday was absent.
        writeln!(file, "2026-01-12,11,13,10,12").unwrap();
        let daily = load_daily_csv(file.path(), &btc(), "test").unwrap();
        let weekly = aggregate_weekly(&daily).unwrap();

        assert_eq!(weekly.len(), 1);
        assert_eq!(
            chrono::DateTime::<Utc>::from_timestamp_nanos(weekly[0].open_time_ns).date_naive(),
            NaiveDate::from_ymd_opt(2026, 1, 6).unwrap()
        );
    }

    #[test]
    fn weekly_aggregation_accepts_binance_inclusive_close_timestamp() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        for day in 5..=11 {
            writeln!(file, "2026-01-{day:02},10,12,9,11").unwrap();
        }
        let mut daily = load_daily_csv(file.path(), &btc(), "binance_spot").unwrap();
        for bar in &mut daily {
            bar.close_time_ns = bar.close_time_ns.saturating_sub(1_000_000);
        }

        let weekly = aggregate_weekly(&daily).unwrap();

        assert_eq!(weekly.len(), 1);
        let next_week_start = NaiveDate::from_ymd_opt(2026, 1, 12)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_nanos_opt()
            .unwrap();
        assert_eq!(weekly[0].close_time_ns, next_week_start - 1_000_000);
        assert!(weekly[0].is_closed);
    }

    #[test]
    fn weekly_aggregation_rejects_mixed_authority() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "Date,Open,High,Low,Close").unwrap();
        writeln!(file, "2026-01-05,10,12,9,11").unwrap();
        writeln!(file, "2026-01-06,11,13,10,12").unwrap();
        let mut daily = load_daily_csv(file.path(), &btc(), "source-a").unwrap();
        daily[1].source = "source-b".into();

        assert!(matches!(
            aggregate_weekly(&daily),
            Err(HistoricalError::InvalidObservation { message, .. })
                if message.contains("authority")
        ));
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

    #[test]
    fn m5_aggregation_from_m1_preserves_ohlc() {
        let m1_bars = create_m1_bars(10);
        let m5_bars = aggregate_m5_from_m1(&m1_bars).unwrap();
        assert_eq!(m5_bars.len(), 2);

        // First M5 bar from first 5 M1 bars
        assert_eq!(m5_bars[0].timeframe, Timeframe::M5);
        assert_eq!(m5_bars[0].open, m1_bars[0].open);
        assert_eq!(m5_bars[0].close, m1_bars[4].close);
        let expected_high: f64 = m1_bars[0..5]
            .iter()
            .map(|b| b.high)
            .fold(f64::NEG_INFINITY, f64::max);
        let expected_low: f64 = m1_bars[0..5]
            .iter()
            .map(|b| b.low)
            .fold(f64::INFINITY, f64::min);
        assert_eq!(m5_bars[0].high, expected_high);
        assert_eq!(m5_bars[0].low, expected_low);

        // Second M5 bar from next 5 M1 bars
        assert_eq!(m5_bars[1].timeframe, Timeframe::M5);
        assert_eq!(m5_bars[1].open, m1_bars[5].open);
        assert_eq!(m5_bars[1].close, m1_bars[9].close);
    }

    #[test]
    fn m5_aggregation_drops_incomplete_batch() {
        let m1_bars = create_m1_bars(12); // 2 full M5 + 2 extra M1
        let m5_bars = aggregate_m5_from_m1(&m1_bars).unwrap();
        assert_eq!(m5_bars.len(), 2); // Only 2 complete M5 bars
    }

    #[test]
    fn m5_aggregation_rejects_non_m1_input() {
        let daily_bars = create_daily_bars(10);
        let result = aggregate_m5_from_m1(&daily_bars);
        assert!(matches!(
            result,
            Err(HistoricalError::InvalidObservation { .. })
        ));
    }

    #[test]
    fn m5_aggregation_rejects_open_bars() {
        let mut m1_bars = create_m1_bars(5);
        m1_bars[2].is_closed = false;
        let result = aggregate_m5_from_m1(&m1_bars);
        assert!(matches!(
            result,
            Err(HistoricalError::InvalidObservation { .. })
        ));
    }

    #[test]
    fn h1_aggregation_from_m1_preserves_ohlc() {
        let m1_bars = create_m1_bars(60);
        let h1_bars = aggregate_h1(&m1_bars).unwrap();
        assert_eq!(h1_bars.len(), 1);
        assert_eq!(h1_bars[0].timeframe, Timeframe::H1);
        assert_eq!(h1_bars[0].open, m1_bars[0].open);
        assert_eq!(h1_bars[0].close, m1_bars[59].close);
    }

    #[test]
    fn h1_aggregation_from_m5_preserves_ohlc() {
        let m1_bars = create_m1_bars(60);
        let m5_bars = aggregate_m5_from_m1(&m1_bars).unwrap();
        let h1_bars = aggregate_h1(&m5_bars).unwrap();
        assert_eq!(h1_bars.len(), 1);
        assert_eq!(h1_bars[0].timeframe, Timeframe::H1);
        assert_eq!(h1_bars[0].open, m5_bars[0].open);
        assert_eq!(h1_bars[0].close, m5_bars[11].close);
    }

    #[test]
    fn h1_aggregation_drops_incomplete_batch() {
        let m1_bars = create_m1_bars(65); // 1 full H1 + 5 extra M1
        let h1_bars = aggregate_h1(&m1_bars).unwrap();
        assert_eq!(h1_bars.len(), 1); // Only 1 complete H1 bar
    }

    #[test]
    fn h1_aggregation_rejects_invalid_timeframe() {
        let daily_bars = create_daily_bars(10);
        let result = aggregate_h1(&daily_bars);
        assert!(matches!(
            result,
            Err(HistoricalError::InvalidObservation { .. })
        ));
    }

    #[test]
    fn h4_aggregation_from_h1_preserves_ohlc() {
        let m1_bars = create_m1_bars(240); // 4 hours
        let h1_bars = aggregate_h1(&m1_bars).unwrap();
        let h4_bars = aggregate_h4_from_h1(&h1_bars).unwrap();
        assert_eq!(h4_bars.len(), 1);
        assert_eq!(h4_bars[0].timeframe, Timeframe::H4);
        assert_eq!(h4_bars[0].open, h1_bars[0].open);
        assert_eq!(h4_bars[0].close, h1_bars[3].close);
    }

    #[test]
    fn h4_aggregation_drops_incomplete_batch() {
        let m1_bars = create_m1_bars(250); // 4 full hours + 10 min
        let h1_bars = aggregate_h1(&m1_bars).unwrap();
        let h4_bars = aggregate_h4_from_h1(&h1_bars).unwrap();
        assert_eq!(h4_bars.len(), 1); // Only 1 complete H4 bar
    }

    #[test]
    fn h4_aggregation_rejects_non_h1_input() {
        let m5_bars = create_m1_bars(60);
        let m5_agg = aggregate_m5_from_m1(&m5_bars).unwrap();
        let result = aggregate_h4_from_h1(&m5_agg);
        assert!(matches!(
            result,
            Err(HistoricalError::InvalidObservation { .. })
        ));
    }

    fn create_m1_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let base = 100.0 + i as f64 * 0.01;
                MarketObservation {
                    instrument_id: "test:instrument".into(),
                    timeframe: Timeframe::M1,
                    open_time_ns: (i as i64) * 60_000_000_000,
                    close_time_ns: (i as i64 + 1) * 60_000_000_000 - 1,
                    open: base,
                    high: base + 0.02,
                    low: base - 0.02,
                    close: base + 0.01,
                    is_closed: true,
                    source: "test".into(),
                    volume: AvailableValue::available(100.0),
                    quote_volume: AvailableValue::available(10000.0),
                    trade_count: AvailableValue::available(50),
                    best_bid: AvailableValue::unavailable(),
                    best_ask: AvailableValue::unavailable(),
                    bid_size: AvailableValue::unavailable(),
                    ask_size: AvailableValue::unavailable(),
                }
            })
            .collect()
    }

    fn create_daily_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let base = 100.0 + i as f64 * 1.0;
                MarketObservation {
                    instrument_id: "test:instrument".into(),
                    timeframe: Timeframe::D1,
                    open_time_ns: (i as i64) * 86_400_000_000_000,
                    close_time_ns: (i as i64 + 1) * 86_400_000_000_000 - 1,
                    open: base,
                    high: base + 2.0,
                    low: base - 2.0,
                    close: base + 1.0,
                    is_closed: true,
                    source: "test".into(),
                    volume: AvailableValue::available(1000.0),
                    quote_volume: AvailableValue::available(100000.0),
                    trade_count: AvailableValue::available(500),
                    best_bid: AvailableValue::unavailable(),
                    best_ask: AvailableValue::unavailable(),
                    bid_size: AvailableValue::unavailable(),
                    ask_size: AvailableValue::unavailable(),
                }
            })
            .collect()
    }
}
