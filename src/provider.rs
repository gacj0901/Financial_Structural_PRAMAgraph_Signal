use crate::{AssetClass, AvailableValue, Instrument, MarketObservation, Timeframe};
use chrono::{DateTime, Days, NaiveDate, Utc};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use serde_json::Value;
use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

const BINANCE_KLINES_URL: &str = "https://api.binance.com/api/v3/klines";
const MASSIVE_API_ROOT: &str = "https://api.massive.com";
const DAY_NS: i64 = 86_400_000_000_000;
const MASSIVE_HISTORY_DAYS: u64 = 1_825;
const GOLD_MIN_DAYS_TO_MATURITY: i64 = 20;
const PROVIDER_TIMEOUT_SECONDS: u64 = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct ProviderBars {
    pub observations: Vec<MarketObservation>,
    pub provider_symbol: String,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider does not support instrument {0}")]
    Unsupported(String),
    #[error("provider request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("provider credential is not configured: {0}")]
    MissingCredential(&'static str),
    #[error("provider payload is malformed: {0}")]
    Payload(String),
    #[error("canonical observation is invalid: {0}")]
    Observation(String),
}

pub async fn massive_closed_daily(instrument: &Instrument) -> Result<ProviderBars, ProviderError> {
    if instrument.venue != "massive" {
        return Err(ProviderError::Unsupported(instrument.instrument_id.clone()));
    }
    let api_key = env::var("MASSIVE_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or(ProviderError::MissingCredential("MASSIVE_API_KEY"))?;
    let client = provider_client()?;
    let authorization = bearer(&api_key)?;
    let today = current_utc_date()?;

    match instrument.asset_class {
        AssetClass::Stock | AssetClass::Index | AssetClass::Forex => {
            massive_standard_daily(&client, authorization, instrument, today).await
        }
        AssetClass::Futures if instrument.symbol == "GC" => {
            massive_gold_daily(&client, authorization, instrument, today).await
        }
        _ => Err(ProviderError::Unsupported(instrument.instrument_id.clone())),
    }
}

async fn massive_standard_daily(
    client: &reqwest::Client,
    authorization: HeaderValue,
    instrument: &Instrument,
    today: NaiveDate,
) -> Result<ProviderBars, ProviderError> {
    let provider_symbol = massive_standard_symbol(instrument)?;
    let to = today
        .checked_sub_days(Days::new(1))
        .ok_or_else(|| ProviderError::Payload("date underflow".into()))?;
    let from = to
        .checked_sub_days(Days::new(MASSIVE_HISTORY_DAYS))
        .ok_or_else(|| ProviderError::Payload("date underflow".into()))?;
    let url =
        format!("{MASSIVE_API_ROOT}/v2/aggs/ticker/{provider_symbol}/range/1/day/{from}/{to}");
    let payload: Value = client
        .get(url)
        .header(AUTHORIZATION, authorization)
        .query(&[("sort", "asc"), ("limit", "50000")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let observations = parse_massive_standard_bars(instrument, &payload)?;
    if observations.is_empty() {
        return Err(ProviderError::Payload(
            "Massive returned no closed bars".into(),
        ));
    }
    Ok(ProviderBars {
        observations,
        provider_symbol,
    })
}

async fn massive_gold_daily(
    client: &reqwest::Client,
    authorization: HeaderValue,
    instrument: &Instrument,
    today: NaiveDate,
) -> Result<ProviderBars, ProviderError> {
    let contracts_url = format!("{MASSIVE_API_ROOT}/futures/v1/contracts");
    let contracts: Value = client
        .get(contracts_url)
        .header(AUTHORIZATION, authorization.clone())
        .query(&[
            ("product_code", "GC"),
            ("active", "true"),
            ("type", "single"),
            ("date", &today.to_string()),
            ("limit", "1000"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let provider_symbol = select_gold_contract(&contracts)?;
    let aggregates_url = format!("{MASSIVE_API_ROOT}/futures/v1/aggs/{provider_symbol}");
    let payload: Value = client
        .get(aggregates_url)
        .header(AUTHORIZATION, authorization)
        .query(&[
            ("resolution", "1session"),
            ("limit", "1000"),
            ("sort", "window_start.asc"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let observations = parse_massive_futures_bars(instrument, &payload, today)?;
    if observations.is_empty() {
        return Err(ProviderError::Payload(
            "Massive returned no closed bars".into(),
        ));
    }
    Ok(ProviderBars {
        observations,
        provider_symbol,
    })
}

fn massive_standard_symbol(instrument: &Instrument) -> Result<String, ProviderError> {
    match instrument.asset_class {
        AssetClass::Stock => Ok(instrument.symbol.clone()),
        AssetClass::Index => Ok(format!("I:{}", instrument.symbol)),
        AssetClass::Forex => Ok(format!("C:{}", instrument.symbol)),
        _ => Err(ProviderError::Unsupported(instrument.instrument_id.clone())),
    }
}

fn parse_massive_standard_bars(
    instrument: &Instrument,
    payload: &Value,
) -> Result<Vec<MarketObservation>, ProviderError> {
    ensure_ok(payload)?;
    let rows = results(payload)?;
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let open_time_ms = field_i64(row, "t")?;
        let open_time_ns = open_time_ms
            .checked_mul(1_000_000)
            .ok_or_else(|| ProviderError::Payload("open timestamp overflow".into()))?;
        let observation = MarketObservation {
            instrument_id: instrument.instrument_id.clone(),
            timeframe: Timeframe::D1,
            open_time_ns,
            close_time_ns: open_time_ns
                .checked_add(DAY_NS - 1)
                .ok_or_else(|| ProviderError::Payload("close timestamp overflow".into()))?,
            open: field_f64(row, "o")?,
            high: field_f64(row, "h")?,
            low: field_f64(row, "l")?,
            close: field_f64(row, "c")?,
            is_closed: true,
            source: "massive_rest".into(),
            volume: optional_f64(row, "v"),
            quote_volume: AvailableValue::unavailable(),
            trade_count: optional_u64(row, "n")?,
            best_bid: AvailableValue::unavailable(),
            best_ask: AvailableValue::unavailable(),
            bid_size: AvailableValue::unavailable(),
            ask_size: AvailableValue::unavailable(),
        };
        push_validated(&mut output, observation)?;
    }
    Ok(output)
}

fn parse_massive_futures_bars(
    instrument: &Instrument,
    payload: &Value,
    today: NaiveDate,
) -> Result<Vec<MarketObservation>, ProviderError> {
    ensure_ok(payload)?;
    let rows = results(payload)?;
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let session_end = row
            .get("session_end_date")
            .and_then(Value::as_str)
            .ok_or_else(|| ProviderError::Payload("missing session_end_date".into()))?;
        let session_end = NaiveDate::parse_from_str(session_end, "%Y-%m-%d")
            .map_err(|_| ProviderError::Payload("invalid session_end_date".into()))?;
        if session_end >= today {
            continue;
        }
        let open_time_ns = field_i64(row, "window_start")?;
        let close_time_ns = session_end
            .checked_add_days(Days::new(1))
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .and_then(|date_time| date_time.and_utc().timestamp_nanos_opt())
            .and_then(|timestamp| timestamp.checked_sub(1))
            .ok_or_else(|| ProviderError::Payload("session close timestamp overflow".into()))?;
        let observation = MarketObservation {
            instrument_id: instrument.instrument_id.clone(),
            timeframe: Timeframe::D1,
            open_time_ns,
            close_time_ns,
            open: field_f64(row, "open")?,
            high: field_f64(row, "high")?,
            low: field_f64(row, "low")?,
            close: field_f64(row, "close")?,
            is_closed: true,
            source: "massive_futures_rest".into(),
            volume: optional_f64(row, "volume"),
            quote_volume: optional_f64(row, "dollar_volume"),
            trade_count: optional_u64(row, "transactions")?,
            best_bid: AvailableValue::unavailable(),
            best_ask: AvailableValue::unavailable(),
            bid_size: AvailableValue::unavailable(),
            ask_size: AvailableValue::unavailable(),
        };
        push_validated(&mut output, observation)?;
    }
    Ok(output)
}

fn select_gold_contract(payload: &Value) -> Result<String, ProviderError> {
    ensure_ok(payload)?;
    let contracts = results(payload)?;
    let selected = contracts
        .iter()
        .filter_map(|contract| {
            let ticker = contract.get("ticker")?.as_str()?;
            let days = contract.get("days_to_maturity")?.as_i64()?;
            (days >= GOLD_MIN_DAYS_TO_MATURITY).then_some((days, ticker))
        })
        .min_by(|left, right| left.cmp(right))
        .or_else(|| {
            contracts
                .iter()
                .filter_map(|contract| {
                    let ticker = contract.get("ticker")?.as_str()?;
                    let days = contract.get("days_to_maturity")?.as_i64()?;
                    (days >= 0).then_some((days, ticker))
                })
                .min_by(|left, right| left.cmp(right))
        })
        .ok_or_else(|| ProviderError::Payload("no active GC futures contract".into()))?;
    Ok(selected.1.to_owned())
}

fn bearer(api_key: &str) -> Result<HeaderValue, ProviderError> {
    HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_| ProviderError::Payload("MASSIVE_API_KEY contains invalid header bytes".into()))
}

fn provider_client() -> Result<reqwest::Client, ProviderError> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(PROVIDER_TIMEOUT_SECONDS))
        .build()?)
}

fn current_utc_date() -> Result<NaiveDate, ProviderError> {
    DateTime::<Utc>::from_timestamp_millis(now_ms())
        .map(|value| value.date_naive())
        .ok_or_else(|| ProviderError::Payload("system time is outside chrono range".into()))
}

fn ensure_ok(payload: &Value) -> Result<(), ProviderError> {
    match payload.get("status").and_then(Value::as_str) {
        Some("OK") => Ok(()),
        Some(status) => Err(ProviderError::Payload(format!(
            "Massive response status is {status}"
        ))),
        None => Err(ProviderError::Payload("missing Massive status".into())),
    }
}

fn results(payload: &Value) -> Result<&Vec<Value>, ProviderError> {
    payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or_else(|| ProviderError::Payload("missing Massive results array".into()))
}

fn field_i64(row: &Value, field: &str) -> Result<i64, ProviderError> {
    row.get(field)
        .and_then(Value::as_i64)
        .ok_or_else(|| ProviderError::Payload(format!("invalid {field}")))
}

fn field_f64(row: &Value, field: &str) -> Result<f64, ProviderError> {
    row.get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| ProviderError::Payload(format!("invalid {field}")))
}

fn optional_f64(row: &Value, field: &str) -> AvailableValue<f64> {
    row.get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map_or_else(AvailableValue::unavailable, AvailableValue::available)
}

fn optional_u64(row: &Value, field: &str) -> Result<AvailableValue<u64>, ProviderError> {
    match row.get(field) {
        None | Some(Value::Null) => Ok(AvailableValue::unavailable()),
        Some(value) => value
            .as_u64()
            .map(AvailableValue::available)
            .ok_or_else(|| ProviderError::Payload(format!("invalid {field}"))),
    }
}

fn push_validated(
    output: &mut Vec<MarketObservation>,
    observation: MarketObservation,
) -> Result<(), ProviderError> {
    observation
        .validate()
        .map_err(|error| ProviderError::Observation(error.to_string()))?;
    if output
        .last()
        .is_some_and(|previous| previous.open_time_ns >= observation.open_time_ns)
    {
        return Err(ProviderError::Payload(
            "timestamps are not strictly increasing".into(),
        ));
    }
    output.push(observation);
    Ok(())
}

pub async fn binance_closed_daily(
    instrument: &Instrument,
    limit: u16,
) -> Result<Vec<MarketObservation>, ProviderError> {
    if instrument.venue != "binance" || limit == 0 || limit > 1_000 {
        return Err(ProviderError::Unsupported(instrument.instrument_id.clone()));
    }
    let limit_string = limit.to_string();
    let payload: Value = reqwest::Client::new()
        .get(BINANCE_KLINES_URL)
        .query(&[
            ("symbol", instrument.symbol.as_str()),
            ("interval", "1d"),
            ("limit", limit_string.as_str()),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    parse_binance_klines(instrument, &payload, now_ms())
}

fn parse_binance_klines(
    instrument: &Instrument,
    payload: &Value,
    current_time_ms: i64,
) -> Result<Vec<MarketObservation>, ProviderError> {
    let rows = payload
        .as_array()
        .ok_or_else(|| ProviderError::Payload("expected an array".into()))?;
    let mut output = Vec::with_capacity(rows.len());
    for row in rows {
        let fields = row
            .as_array()
            .ok_or_else(|| ProviderError::Payload("kline must be an array".into()))?;
        if fields.len() < 9 {
            return Err(ProviderError::Payload(
                "kline has fewer than 9 fields".into(),
            ));
        }
        let open_time_ms = integer(&fields[0], "open_time")?;
        let close_time_ms = integer(&fields[6], "close_time")?;
        if close_time_ms >= current_time_ms {
            continue;
        }
        let observation = MarketObservation {
            instrument_id: instrument.instrument_id.clone(),
            timeframe: Timeframe::D1,
            open_time_ns: open_time_ms
                .checked_mul(1_000_000)
                .ok_or_else(|| ProviderError::Payload("open timestamp overflow".into()))?,
            close_time_ns: close_time_ms
                .checked_mul(1_000_000)
                .ok_or_else(|| ProviderError::Payload("close timestamp overflow".into()))?,
            open: decimal(&fields[1], "open")?,
            high: decimal(&fields[2], "high")?,
            low: decimal(&fields[3], "low")?,
            close: decimal(&fields[4], "close")?,
            is_closed: true,
            source: "binance_spot".into(),
            volume: AvailableValue::available(decimal(&fields[5], "volume")?),
            quote_volume: AvailableValue::available(decimal(&fields[7], "quote_volume")?),
            trade_count: AvailableValue::available(
                integer(&fields[8], "trade_count")?
                    .try_into()
                    .map_err(|_| ProviderError::Payload("negative trade count".into()))?,
            ),
            best_bid: AvailableValue::unavailable(),
            best_ask: AvailableValue::unavailable(),
            bid_size: AvailableValue::unavailable(),
            ask_size: AvailableValue::unavailable(),
        };
        observation
            .validate()
            .map_err(|error| ProviderError::Observation(error.to_string()))?;
        if output.last().is_some_and(|previous: &MarketObservation| {
            previous.open_time_ns >= observation.open_time_ns
        }) {
            return Err(ProviderError::Payload(
                "timestamps are not strictly increasing".into(),
            ));
        }
        output.push(observation);
    }
    Ok(output)
}

fn integer(value: &Value, field: &str) -> Result<i64, ProviderError> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .ok_or_else(|| ProviderError::Payload(format!("invalid {field}")))
}

fn decimal(value: &Value, field: &str) -> Result<f64, ProviderError> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        .filter(|value: &f64| value.is_finite())
        .ok_or_else(|| ProviderError::Payload(format!("invalid {field}")))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after the Unix epoch")
        .as_millis()
        .try_into()
        .expect("current timestamp fits i64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::{AssetResolver, Resolution};
    use serde_json::json;

    fn btc() -> Instrument {
        match AssetResolver::default().resolve("BTC") {
            Resolution::Found { instrument } => instrument,
            _ => panic!("BTC must resolve"),
        }
    }

    fn instrument(query: &str) -> Instrument {
        match AssetResolver::default().resolve(query) {
            Resolution::Found { instrument } => instrument,
            _ => panic!("instrument must resolve"),
        }
    }

    #[test]
    fn incomplete_binance_bar_is_excluded() {
        let payload = json!([
            [1000, "10", "12", "9", "11", "5", 1999, "55", 7],
            [2000, "11", "13", "10", "12", "6", 2999, "68", 8]
        ]);
        let bars = parse_binance_klines(&btc(), &payload, 2500).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, 11.0);
    }

    #[test]
    fn massive_index_bars_preserve_unavailable_volume() {
        let payload = json!({
            "status": "OK",
            "results": [{"o": 10.0, "h": 12.0, "l": 9.0, "c": 11.0, "t": 1_700_000_000_000_i64}]
        });
        let bars = parse_massive_standard_bars(&instrument("SP500"), &payload).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].volume, AvailableValue::unavailable());
        assert_eq!(bars[0].source, "massive_rest");
    }

    #[test]
    fn massive_futures_excludes_unclosed_session() {
        let payload = json!({
            "status": "OK",
            "results": [
                {"open": 10.0, "high": 12.0, "low": 9.0, "close": 11.0,
                 "window_start": 1_700_000_000_000_000_000_i64,
                 "session_end_date": "2025-02-04", "volume": 5, "transactions": 2,
                 "dollar_volume": 55.0},
                {"open": 11.0, "high": 13.0, "low": 10.0, "close": 12.0,
                 "window_start": 1_700_086_400_000_000_000_i64,
                 "session_end_date": "2025-02-05", "volume": 6, "transactions": 3,
                 "dollar_volume": 68.0}
            ]
        });
        let today = NaiveDate::from_ymd_opt(2025, 2, 5).unwrap();
        let bars = parse_massive_futures_bars(&instrument("GOLD"), &payload, today).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, 11.0);
    }

    #[test]
    fn gold_contract_avoids_imminent_expiry() {
        let payload = json!({
            "status": "OK",
            "results": [
                {"ticker": "GCU6", "days_to_maturity": 5},
                {"ticker": "GCV6", "days_to_maturity": 35},
                {"ticker": "GCZ6", "days_to_maturity": 70}
            ]
        });
        assert_eq!(select_gold_contract(&payload).unwrap(), "GCV6");
    }
}
