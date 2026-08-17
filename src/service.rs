use crate::calibration::{
    resolve_direction, validate_profile_for_engine, DirectionalResolution,
    ResolutionCalibrationProfile,
};
use crate::canonical;
use crate::engine::{StructuralEngineAdapter, ENGINE_VERSION};
use crate::historical::{aggregate_weekly, load_daily_csv};
use crate::observation::{adapt_closed_bars, OBSERVATION_INTERFACE_VERSION};
use crate::provider::{binance_closed_daily, massive_closed_daily};
use crate::resolver::{AssetResolver, Resolution};
use crate::{AvailableValue, Instrument, RuntimeStatus, StructuralSnapshot, Timeframe};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FinancialDataRequest {
    pub asset: String,
    #[serde(default = "default_timeframe")]
    pub timeframe: Timeframe,
    #[serde(default)]
    pub source: DataSourcePreference,
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
    pub directional: Option<DirectionalResolution>,
    pub provenance: ServiceProvenance,
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
    if !matches!(request.timeframe, Timeframe::D1 | Timeframe::W1) {
        return Err(ServiceError::Timeframe);
    }
    let (daily, primary_provider, provider_instrument, corpus_file, input_sha256, live) =
        if request.source == DataSourcePreference::Auto && instrument.venue == "binance" {
            let observations = binance_closed_daily(&instrument, 1_000)
                .await
                .map_err(pipeline)?;
            let hash = canonical::sha256(&observations).map_err(pipeline)?;
            (
                observations,
                "binance_spot".to_owned(),
                Some(instrument.symbol.clone()),
                None,
                hash,
                true,
            )
        } else if request.source == DataSourcePreference::Auto && instrument.venue == "massive" {
            let provider_bars = massive_closed_daily(&instrument).await.map_err(pipeline)?;
            let hash = canonical::sha256(&provider_bars.observations).map_err(pipeline)?;
            (
                provider_bars.observations,
                "massive_rest".to_owned(),
                Some(provider_bars.provider_symbol),
                None,
                hash,
                true,
            )
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
    let bars = match request.timeframe {
        Timeframe::D1 => daily,
        Timeframe::W1 => aggregate_weekly(&daily).map_err(pipeline)?,
        _ => return Err(ServiceError::Timeframe),
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
    let directional = match calibration_directory {
        Some(directory) => {
            let path = directory.join(profile_file_name(
                &instrument.instrument_id,
                request.timeframe,
            ));
            if path.is_file() {
                let profile: ResolutionCalibrationProfile =
                    serde_json::from_slice(&fs::read(path).map_err(pipeline)?).map_err(pipeline)?;
                validate_profile_for_engine(&profile, ENGINE_VERSION).map_err(pipeline)?;
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
    let label = structural.structural_state.clone();
    let mut response = FinancialDataResponse {
        schema: "pramagraph.telegraph.financial_data.v1".into(),
        intent: "FINANCIAL_DATA".into(),
        status: if live {
            RuntimeStatus::Ok
        } else {
            RuntimeStatus::StaleData
        },
        label,
        reason: if live {
            format!(
                "deterministic structural snapshot from the latest closed {primary_provider} D1 bars"
            )
        } else {
            "deterministic structural snapshot from the supplied closed-bar corpus".into()
        },
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
        directional,
        provenance: ServiceProvenance {
            primary_provider,
            provider_instrument,
            corpus_file,
            input_sha256,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_intraday_corpus_request_fails_explicitly() {
        let request = FinancialDataRequest {
            asset: "BTC".into(),
            timeframe: Timeframe::M1,
            source: DataSourcePreference::SuppliedCorpus,
        };
        assert!(matches!(
            build_financial_data_response("missing", &request).await,
            Err(ServiceError::Timeframe)
        ));
    }
}
