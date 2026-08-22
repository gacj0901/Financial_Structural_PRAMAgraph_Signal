use crate::calibration::{
    resolve_direction, validate_profile_authority, DirectionalResolution,
    ResolutionCalibrationProfile,
};
use crate::canonical;
use crate::engine::{StructuralEngineAdapter, ENGINE_VERSION};
use crate::historical::{aggregate_weekly, load_daily_csv};
use crate::observation::{adapt_closed_bars, OBSERVATION_INTERFACE_VERSION};
use crate::provider::{
    binance_closed_daily, binance_closed_h1, binance_closed_h4, binance_closed_m1,
    binance_closed_m5, massive_closed_daily, ProviderError,
};
use crate::resolver::{AssetResolver, Resolution};
use crate::technical::{
    TechnicalCounterReading, TechnicalDirectionHead, TechnicalStructuralContrast,
};
use crate::{AvailableValue, Instrument, RuntimeStatus, StructuralSnapshot, Timeframe};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinancialDataRequest {
    pub asset: String,
    #[serde(default = "default_timeframe")]
    #[schemars(with = "RegisteredFinancialTimeframe")]
    pub timeframe: Timeframe,
    #[serde(default)]
    pub source: DataSourcePreference,
}

#[derive(JsonSchema)]
#[allow(dead_code)]
enum RegisteredFinancialTimeframe {
    D1,
    W1,
}

fn default_timeframe() -> Timeframe {
    Timeframe::D1
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataSourcePreference {
    #[default]
    Auto,
    SuppliedCorpus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketBarResponse {
    pub open_time_ns: i64,
    pub close_time_ns: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: AvailableValue<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServiceProvenance {
    pub primary_provider: String,
    pub provider_instrument: Option<String>,
    pub corpus_file: Option<String>,
    pub input_sha256: String,
    pub engine_version: String,
    pub observation_interface_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FinancialDataResponse {
    pub schema: String,
    pub intent: String,
    pub status: RuntimeStatus,
    pub label: String,
    pub reason: String,
    pub instrument: Instrument,
    pub timeframe: Timeframe,
    pub as_of_ns: i64,
    pub market: MarketBarResponse,
    pub structural: StructuralSnapshot,
    #[schemars(with = "TechnicalDirectionHead", required)]
    pub technical: Option<TechnicalDirectionHead>,
    #[schemars(with = "TechnicalCounterReading", required)]
    pub counter_reading: Option<TechnicalCounterReading>,
    #[schemars(with = "TechnicalStructuralContrast", required)]
    pub structural_contrast: Option<TechnicalStructuralContrast>,
    pub directional: Option<DirectionalResolution>,
    pub provenance: ServiceProvenance,
    #[schemars(with = "String", required)]
    pub response_sha256: Option<String>,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("unsupported or ambiguous asset: {0}")]
    Asset(String),
    #[error("only D1 and W1 are available from the supplied corpus")]
    Timeframe,
    #[error("no corpus mapping for instrument {0}")]
    CorpusMapping(String),
    #[error("service pipeline failed: {0}")]
    Pipeline(String),
}

pub async fn build_financial_data_response(
    corpus_directory: impl AsRef<Path>,
    request: &FinancialDataRequest,
) -> Result<FinancialDataResponse, ServiceError> {
    build_financial_data_response_internal(corpus_directory, None, request).await
}

pub async fn build_financial_data_response_with_profiles(
    corpus_directory: impl AsRef<Path>,
    calibration_directory: impl AsRef<Path>,
    request: &FinancialDataRequest,
) -> Result<FinancialDataResponse, ServiceError> {
    build_financial_data_response_internal(
        corpus_directory,
        Some(calibration_directory.as_ref()),
        request,
    )
    .await
}

async fn build_financial_data_response_internal(
    corpus_directory: impl AsRef<Path>,
    calibration_directory: Option<&Path>,
    request: &FinancialDataRequest,
) -> Result<FinancialDataResponse, ServiceError> {
    let instrument = match AssetResolver::default().resolve(&request.asset) {
        Resolution::Found { instrument } => instrument,
        _ => return Err(ServiceError::Asset(request.asset.clone())),
    };

    // The registered Telegraph contract exposes D1 and W1 only. Keep any
    // experimental intraday provider helpers outside this public service path.
    if !matches!(request.timeframe, Timeframe::D1 | Timeframe::W1) {
        return Err(ServiceError::Timeframe);
    }

    let (bars, primary_provider, provider_instrument, corpus_file, input_sha256, live) =
        if request.source == DataSourcePreference::Auto && instrument.venue == "binance" {
            match request.timeframe {
                Timeframe::M1 => {
                    match binance_closed_m1(&instrument, 1_000).await {
                        Ok(observations) => {
                            let hash = canonical::sha256(&observations).map_err(pipeline)?;
                            (
                                observations,
                                "binance_spot".to_owned(),
                                Some(instrument.symbol.clone()),
                                None,
                                hash,
                                true,
                            )
                        }
                        Err(ProviderError::Unsupported(_)) => {
                            return Err(ServiceError::Asset(instrument.symbol.clone()));
                        }
                        Err(_e) => {
                            // Live Binance failed - fallback to corpus with explicit STALE status
                            let file = calibration_file(&instrument)?;
                            let path = corpus_directory.as_ref().join(file);
                            let bytes = fs::read(&path).map_err(pipeline)?;
                            let observations =
                                load_daily_csv(&path, &instrument, "supplied_corpus")
                                    .map_err(pipeline)?;
                            (
                                observations,
                                "supplied_corpus".to_owned(),
                                None,
                                Some(file.to_owned()),
                                format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                                false,
                            )
                        }
                    }
                }
                Timeframe::M5 => {
                    match binance_closed_m5(&instrument, 1_000).await {
                        Ok(observations) => {
                            let hash = canonical::sha256(&observations).map_err(pipeline)?;
                            (
                                observations,
                                "binance_spot".to_owned(),
                                Some(instrument.symbol.clone()),
                                None,
                                hash,
                                true,
                            )
                        }
                        Err(ProviderError::Unsupported(_)) => {
                            return Err(ServiceError::Asset(instrument.symbol.clone()));
                        }
                        Err(_e) => {
                            // Live Binance failed - fallback to corpus
                            let file = calibration_file(&instrument)?;
                            let path = corpus_directory.as_ref().join(file);
                            let bytes = fs::read(&path).map_err(pipeline)?;
                            let _daily = load_daily_csv(&path, &instrument, "supplied_corpus")
                                .map_err(pipeline)?;
                            let _m1_obs = load_daily_csv(&path, &instrument, "supplied_corpus")
                                .map_err(pipeline)?;
                            // Note: M5 from corpus requires M1 data which we don't have
                            // Fall back to empty for now - will return insufficient data
                            (
                                vec![],
                                "supplied_corpus".to_owned(),
                                None,
                                Some(file.to_owned()),
                                format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                                false,
                            )
                        }
                    }
                }
                Timeframe::H1 => {
                    match binance_closed_h1(&instrument, 1_000).await {
                        Ok(observations) => {
                            let hash = canonical::sha256(&observations).map_err(pipeline)?;
                            (
                                observations,
                                "binance_spot".to_owned(),
                                Some(instrument.symbol.clone()),
                                None,
                                hash,
                                true,
                            )
                        }
                        Err(ProviderError::Unsupported(_)) => {
                            return Err(ServiceError::Asset(instrument.symbol.clone()));
                        }
                        Err(_e) => {
                            let file = calibration_file(&instrument)?;
                            let path = corpus_directory.as_ref().join(file);
                            let bytes = fs::read(&path).map_err(pipeline)?;
                            let _daily = load_daily_csv(&path, &instrument, "supplied_corpus")
                                .map_err(pipeline)?;
                            // Note: H1 from corpus requires M1/M5 data which we don't have
                            (
                                vec![],
                                "supplied_corpus".to_owned(),
                                None,
                                Some(file.to_owned()),
                                format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                                false,
                            )
                        }
                    }
                }
                Timeframe::H4 => match binance_closed_h4(&instrument, 1_000).await {
                    Ok(observations) => {
                        let hash = canonical::sha256(&observations).map_err(pipeline)?;
                        (
                            observations,
                            "binance_spot".to_owned(),
                            Some(instrument.symbol.clone()),
                            None,
                            hash,
                            true,
                        )
                    }
                    Err(ProviderError::Unsupported(_)) => {
                        return Err(ServiceError::Asset(instrument.symbol.clone()));
                    }
                    Err(_e) => {
                        let file = calibration_file(&instrument)?;
                        let path = corpus_directory.as_ref().join(file);
                        let bytes = fs::read(&path).map_err(pipeline)?;
                        let _daily = load_daily_csv(&path, &instrument, "supplied_corpus")
                            .map_err(pipeline)?;
                        (
                            vec![],
                            "supplied_corpus".to_owned(),
                            None,
                            Some(file.to_owned()),
                            format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                            false,
                        )
                    }
                },
                Timeframe::D1 => match binance_closed_daily(&instrument, 1_000).await {
                    Ok(observations) => {
                        let hash = canonical::sha256(&observations).map_err(pipeline)?;
                        (
                            observations,
                            "binance_spot".to_owned(),
                            Some(instrument.symbol.clone()),
                            None,
                            hash,
                            true,
                        )
                    }
                    Err(ProviderError::Unsupported(_)) => {
                        return Err(ServiceError::Asset(instrument.symbol.clone()));
                    }
                    Err(_e) => {
                        let file = calibration_file(&instrument)?;
                        let path = corpus_directory.as_ref().join(file);
                        let bytes = fs::read(&path).map_err(pipeline)?;
                        let observations = load_daily_csv(&path, &instrument, "supplied_corpus")
                            .map_err(pipeline)?;
                        (
                            observations,
                            "supplied_corpus".to_owned(),
                            None,
                            Some(file.to_owned()),
                            format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                            false,
                        )
                    }
                },
                Timeframe::W1 => match binance_closed_daily(&instrument, 1_000).await {
                    Ok(daily_obs) => {
                        let observations = aggregate_weekly(&daily_obs).map_err(pipeline)?;
                        let hash = canonical::sha256(&observations).map_err(pipeline)?;
                        (
                            observations,
                            "binance_spot".to_owned(),
                            Some(instrument.symbol.clone()),
                            None,
                            hash,
                            true,
                        )
                    }
                    Err(ProviderError::Unsupported(_)) => {
                        return Err(ServiceError::Asset(instrument.symbol.clone()));
                    }
                    Err(_e) => {
                        let file = calibration_file(&instrument)?;
                        let path = corpus_directory.as_ref().join(file);
                        let bytes = fs::read(&path).map_err(pipeline)?;
                        let daily = load_daily_csv(&path, &instrument, "supplied_corpus")
                            .map_err(pipeline)?;
                        let observations = aggregate_weekly(&daily).map_err(pipeline)?;
                        (
                            observations,
                            "supplied_corpus".to_owned(),
                            None,
                            Some(file.to_owned()),
                            format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                            false,
                        )
                    }
                },
            }
        } else if request.source == DataSourcePreference::Auto && instrument.venue == "massive" {
            match massive_closed_daily(&instrument).await {
                Ok(provider_bars) => {
                    let hash = canonical::sha256(&provider_bars.observations).map_err(pipeline)?;
                    (
                        provider_bars.observations,
                        "massive_rest".to_owned(),
                        Some(provider_bars.provider_symbol),
                        None,
                        hash,
                        true,
                    )
                }
                Err(ProviderError::MissingCredential(_)) => {
                    // Massive credential missing - fallback to corpus
                    let file = calibration_file(&instrument)?;
                    let path = corpus_directory.as_ref().join(file);
                    let bytes = fs::read(&path).map_err(pipeline)?;
                    let observations =
                        load_daily_csv(&path, &instrument, "supplied_corpus").map_err(pipeline)?;
                    (
                        observations,
                        "supplied_corpus".to_owned(),
                        None,
                        Some(file.to_owned()),
                        format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                        false,
                    )
                }
                Err(_e) => {
                    // Other Massive error - fallback to corpus
                    let file = calibration_file(&instrument)?;
                    let path = corpus_directory.as_ref().join(file);
                    let bytes = fs::read(&path).map_err(pipeline)?;
                    let observations =
                        load_daily_csv(&path, &instrument, "supplied_corpus").map_err(pipeline)?;
                    (
                        observations,
                        "supplied_corpus".to_owned(),
                        None,
                        Some(file.to_owned()),
                        format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                        false,
                    )
                }
            }
        } else {
            let file = calibration_file(&instrument)?;
            let path = corpus_directory.as_ref().join(file);
            let bytes = fs::read(&path).map_err(pipeline)?;
            let observations =
                load_daily_csv(&path, &instrument, "supplied_corpus").map_err(pipeline)?;
            (
                observations,
                "supplied_corpus".to_owned(),
                None,
                Some(file.to_owned()),
                format!("sha256:{}", hex::encode(Sha256::digest(bytes))),
                false,
            )
        };

    // For intraday timeframes, if we got daily bars from corpus fallback, we can't construct intraday
    // Return early with appropriate error for corpus-based intraday requests
    let bars = match request.timeframe {
        Timeframe::D1 => bars,
        Timeframe::W1 => closed_weekly_once(bars)?,
        Timeframe::M1 | Timeframe::M5 | Timeframe::H1 | Timeframe::H4 => {
            if bars.is_empty() || bars[0].timeframe == Timeframe::D1 {
                // Corpus fallback doesn't have intraday data
                return Err(ServiceError::Timeframe);
            }
            match request.timeframe {
                Timeframe::M1 => bars,
                Timeframe::M5 => {
                    // If we have M1, aggregate to M5
                    if bars[0].timeframe == Timeframe::M1 {
                        crate::historical::aggregate_m5_from_m1(&bars).map_err(pipeline)?
                    } else {
                        bars // Already M5 from provider
                    }
                }
                Timeframe::H1 => {
                    // If we have M1 or M5, aggregate to H1
                    if matches!(bars[0].timeframe, Timeframe::M1 | Timeframe::M5) {
                        crate::historical::aggregate_h1(&bars).map_err(pipeline)?
                    } else {
                        bars // Already H1 from provider
                    }
                }
                Timeframe::H4 => {
                    // If we have H1, aggregate to H4
                    if bars[0].timeframe == Timeframe::H1 {
                        crate::historical::aggregate_h4_from_h1(&bars).map_err(pipeline)?
                    } else {
                        bars // Already H4 from provider
                    }
                }
                _ => bars,
            }
        }
    };
    let kernel_input = adapt_closed_bars(&bars).map_err(pipeline)?;
    let last = bars
        .last()
        .ok_or_else(|| ServiceError::Pipeline("corpus contains no bars".into()))?;
    let watermark = last.close_time_ns.to_string();
    let engine = StructuralEngineAdapter::default();
    let frames = engine.replay_frames(&kernel_input).map_err(pipeline)?;
    let structural = engine
        .snapshot_from_frames(
            &instrument.instrument_id,
            request.timeframe,
            &watermark,
            &frames,
        )
        .map_err(pipeline)?;

    // Compute the registered technical channel once, together with its
    // independent structural contrast.
    let technical_structural_contrast =
        crate::technical::compute_technical_structural_contrast(&bars, &structural)
            .map_err(pipeline)?;
    let technical = technical_structural_contrast.technical.clone();
    let counter_reading = technical_structural_contrast.counter_reading.clone();

    // Optional: calibration-based directional (KNN) for backward compatibility / provenance
    // Optional: calibration-based directional (KNN) for backward compatibility / provenance
    let directional = match calibration_directory {
        Some(directory) => {
            let path = directory.join(profile_file_name(
                &instrument.instrument_id,
                request.timeframe,
            ));
            if path.is_file() {
                let profile: ResolutionCalibrationProfile =
                    serde_json::from_slice(&fs::read(path).map_err(pipeline)?).map_err(pipeline)?;
                validate_profile_authority(
                    &profile,
                    ENGINE_VERSION,
                    &instrument.instrument_id,
                    request.timeframe,
                )
                .map_err(pipeline)?;
                let vector = &frames
                    .last()
                    .ok_or_else(|| {
                        ServiceError::Pipeline("structural replay contains no frames".into())
                    })?
                    .vector;
                Some(resolve_direction(vector, &profile).map_err(pipeline)?)
            } else {
                None
            }
        }
        None => None,
    };

    // Top-level label: use technical direction as authoritative financial signal
    let label = match technical.direction {
        crate::TechnicalDirection::Up => "UP",
        crate::TechnicalDirection::Down => "DOWN",
        crate::TechnicalDirection::Range => "RANGE",
        crate::TechnicalDirection::Unavailable => "UNAVAILABLE",
    }
    .to_string();

    // Reason reflects actual timeframe
    let tf_str = format!("{:?}", request.timeframe);
    let reason = if live {
        format!("deterministic structural snapshot from the latest closed {primary_provider} {tf_str} bars")
    } else {
        format!("deterministic structural snapshot from the supplied closed-bar corpus ({tf_str})")
    };

    let mut response = FinancialDataResponse {
        schema: "pramagraph.telegraph.financial_data.v1".into(),
        intent: "FINANCIAL_DATA".into(),
        status: if live {
            RuntimeStatus::Ok
        } else {
            RuntimeStatus::StaleData
        },
        label,
        reason,
        instrument,
        timeframe: request.timeframe,
        as_of_ns: last.close_time_ns,
        market: MarketBarResponse {
            open_time_ns: last.open_time_ns,
            close_time_ns: last.close_time_ns,
            open: last.open,
            high: last.high,
            low: last.low,
            close: last.close,
            volume: last.volume.clone(),
        },
        structural,
        technical: Some(technical),
        counter_reading: Some(counter_reading),
        structural_contrast: Some(technical_structural_contrast),
        directional,
        provenance: ServiceProvenance {
            primary_provider,
            provider_instrument,
            corpus_file,
            input_sha256: if input_sha256.is_empty() {
                canonical::sha256(&bars).map_err(pipeline)?
            } else {
                input_sha256
            },
            engine_version: ENGINE_VERSION.into(),
            observation_interface_version: OBSERVATION_INTERFACE_VERSION.into(),
        },
        response_sha256: None,
    };
    response.response_sha256 = Some(canonical::sha256(&response).map_err(pipeline)?);
    Ok(response)
}

pub fn profile_file_name(instrument_id: &str, timeframe: Timeframe) -> String {
    let safe: String = instrument_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!("{safe}_{timeframe:?}.resolution.json")
}

fn calibration_file(instrument: &Instrument) -> Result<&'static str, ServiceError> {
    match instrument.symbol.as_str() {
        "BTCUSDT" => Ok("btc_calib.csv"),
        "XRPUSDT" => Ok("xrp_calib.csv"),
        "GC" => Ok("gold_calib.csv"),
        "NDX" => Ok("nasdaq_calib.csv"),
        "SPX" => Ok("sp500_calib.csv"),
        other => Err(ServiceError::CorpusMapping(other.into())),
    }
}

fn pipeline(error: impl std::fmt::Display) -> ServiceError {
    ServiceError::Pipeline(error.to_string())
}

fn closed_weekly_once(
    bars: Vec<crate::MarketObservation>,
) -> Result<Vec<crate::MarketObservation>, ServiceError> {
    let Some(first) = bars.first() else {
        return Ok(bars);
    };
    if bars.iter().any(|bar| bar.timeframe != first.timeframe) {
        return Err(ServiceError::Pipeline(
            "weekly input contains mixed timeframes".into(),
        ));
    }
    match first.timeframe {
        Timeframe::D1 => aggregate_weekly(&bars).map_err(pipeline),
        Timeframe::W1 => Ok(bars),
        timeframe => Err(ServiceError::Pipeline(format!(
            "weekly input must contain D1 or W1 bars, found {timeframe:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Days, NaiveDate};

    fn closed_daily_week() -> Vec<crate::MarketObservation> {
        let first = NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        (0..7)
            .map(|offset| {
                let date = first.checked_add_days(Days::new(offset)).unwrap();
                let open_time_ns = date
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp_nanos_opt()
                    .unwrap();
                crate::MarketObservation {
                    instrument_id: "crypto:binance:BTCUSDT".into(),
                    timeframe: Timeframe::D1,
                    open_time_ns,
                    close_time_ns: open_time_ns + 86_400_000_000_000,
                    open: 100.0 + offset as f64,
                    high: 102.0 + offset as f64,
                    low: 99.0 + offset as f64,
                    close: 101.0 + offset as f64,
                    is_closed: true,
                    source: "test".into(),
                    volume: AvailableValue::available(1.0),
                    quote_volume: AvailableValue::unavailable(),
                    trade_count: AvailableValue::unavailable(),
                    best_bid: AvailableValue::unavailable(),
                    best_ask: AvailableValue::unavailable(),
                    bid_size: AvailableValue::unavailable(),
                    ask_size: AvailableValue::unavailable(),
                }
            })
            .collect()
    }

    #[test]
    fn weekly_finalization_aggregates_daily_input_exactly_once() {
        let expected = aggregate_weekly(&closed_daily_week()).unwrap();
        let actual = closed_weekly_once(closed_daily_week()).unwrap();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 1);
        assert_eq!(actual[0].timeframe, Timeframe::W1);
    }

    #[test]
    fn weekly_finalization_preserves_preaggregated_input() {
        let weekly = aggregate_weekly(&closed_daily_week()).unwrap();

        assert_eq!(closed_weekly_once(weekly.clone()).unwrap(), weekly);
    }

    #[test]
    fn registered_request_rejects_unknown_fields() {
        let json = r#"{"asset":"BTC","timeframe":"D1","source":"AUTO","extra":true}"#;
        assert!(serde_json::from_str::<FinancialDataRequest>(json).is_err());
    }

    #[test]
    fn generated_response_schema_requires_registered_fields() {
        let schema = serde_json::to_value(schemars::schema_for!(FinancialDataResponse)).unwrap();
        let required = schema["required"].as_array().expect("required fields");
        for field in [
            "technical",
            "counter_reading",
            "structural_contrast",
            "response_sha256",
        ] {
            assert!(required.iter().any(|value| value == field), "{field}");
        }
    }

    #[tokio::test]
    async fn registered_endpoint_rejects_intraday_timeframe() {
        let request = FinancialDataRequest {
            asset: "BTC".into(),
            timeframe: Timeframe::M1,
            source: DataSourcePreference::Auto,
        };
        assert!(matches!(
            build_financial_data_response("missing", &request).await,
            Err(ServiceError::Timeframe)
        ));
    }
}
