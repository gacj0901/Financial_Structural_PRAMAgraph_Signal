use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AvailabilityStatus {
    Available,
    Unavailable,
    NotApplicable,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AvailableValue<T> {
    pub value: Option<T>,
    pub availability: AvailabilityStatus,
}

impl<T> AvailableValue<T> {
    pub fn available(value: T) -> Self {
        Self {
            value: Some(value),
            availability: AvailabilityStatus::Available,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            value: None,
            availability: AvailabilityStatus::Unavailable,
        }
    }

    pub fn not_applicable() -> Self {
        Self {
            value: None,
            availability: AvailabilityStatus::NotApplicable,
        }
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        match (self.availability, self.value.is_some()) {
            (AvailabilityStatus::Available, false) => Err(ContractError::AvailabilityMismatch),
            (AvailabilityStatus::Available, true) => Ok(()),
            (_, true) => Err(ContractError::AvailabilityMismatch),
            (_, false) => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    Crypto,
    Stock,
    Index,
    Forex,
    Futures,
    Commodity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Timeframe {
    M1,
    M5,
    H1,
    H4,
    D1,
    W1,
}

impl Timeframe {
    pub const fn nominal_seconds(self) -> u64 {
        match self {
            Self::M1 => 60,
            Self::M5 => 300,
            Self::H1 => 3_600,
            Self::H4 => 14_400,
            Self::D1 => 86_400,
            Self::W1 => 604_800,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionCalendar {
    ContinuousUtc,
    ExchangeSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Instrument {
    pub instrument_id: String,
    pub asset_class: AssetClass,
    pub symbol: String,
    pub base: Option<String>,
    pub quote: Option<String>,
    pub venue: String,
    pub timezone: String,
    pub session_calendar: SessionCalendar,
    pub price_precision: Option<u8>,
    pub quantity_precision: Option<u8>,
    pub live_data_capable: bool,
    pub historical_data_capable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketObservation {
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub open_time_ns: i64,
    pub close_time_ns: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub is_closed: bool,
    pub source: String,
    pub volume: AvailableValue<f64>,
    pub quote_volume: AvailableValue<f64>,
    pub trade_count: AvailableValue<u64>,
    pub best_bid: AvailableValue<f64>,
    pub best_ask: AvailableValue<f64>,
    pub bid_size: AvailableValue<f64>,
    pub ask_size: AvailableValue<f64>,
}

impl MarketObservation {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.instrument_id.trim().is_empty() || self.source.trim().is_empty() {
            return Err(ContractError::MissingIdentity);
        }
        if self.close_time_ns <= self.open_time_ns {
            return Err(ContractError::InvalidTimeRange);
        }
        if [self.open, self.high, self.low, self.close]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err(ContractError::NonFinitePrice);
        }
        if self.low > self.high
            || self.high < self.open.max(self.close)
            || self.low > self.open.min(self.close)
        {
            return Err(ContractError::MalformedOhlc);
        }
        self.volume.validate()?;
        self.quote_volume.validate()?;
        self.trade_count.validate()?;
        self.best_bid.validate()?;
        self.best_ask.validate()?;
        self.bid_size.validate()?;
        self.ask_size.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Direction {
    Up,
    Down,
    Range,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuntimeStatus {
    Ok,
    Bootstrapping,
    Unresolved,
    InsufficientData,
    InsufficientCalibration,
    StaleData,
    ProviderDivergence,
    AmbiguousAsset,
    UnsupportedAsset,
    EngineError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignalMode {
    Confirmed,
    LivePreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CalibrationScope {
    Instrument,
    AssetClass,
    Global,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProbabilitiesBp {
    pub up: u16,
    pub range: u16,
    pub down: u16,
}

impl ProbabilitiesBp {
    pub fn validate(&self) -> Result<(), ContractError> {
        if u32::from(self.up) + u32::from(self.range) + u32::from(self.down) == 10_000 {
            Ok(())
        } else {
            Err(ContractError::ProbabilitySum)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComponentSnapshot {
    pub availability: AvailabilityStatus,
    pub value: Option<serde_json::Value>,
    pub reason: Option<String>,
}

impl ComponentSnapshot {
    pub fn available(value: serde_json::Value) -> Self {
        Self {
            availability: AvailabilityStatus::Available,
            value: Some(value),
            reason: None,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            availability: AvailabilityStatus::Unavailable,
            value: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructuralSnapshot {
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub as_of_ns: i64,
    pub engine_version: String,
    pub structural_state: String,
    pub prama: ComponentSnapshot,
    pub d_o: ComponentSnapshot,
    pub odce: ComponentSnapshot,
    pub k_mem: ComponentSnapshot,
    pub availability: BTreeMap<String, AvailabilityStatus>,
    pub source_watermark: String,
    pub snapshot_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Horizon {
    pub p25_bars: Option<u32>,
    pub median_bars: Option<u32>,
    pub p75_bars: Option<u32>,
    pub p25_seconds: Option<u64>,
    pub median_seconds: Option<u64>,
    pub p75_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PerScaleSignal {
    pub timeframe: Timeframe,
    pub structural: StructuralSnapshot,
    pub direction: Direction,
    pub probabilities_bp: Option<ProbabilitiesBp>,
    pub horizon: Option<Horizon>,
    pub reliability_bp: Option<u16>,
    pub sample_support: u64,
    pub calibration_scope: CalibrationScope,
    // Step 1: Technical Direction Head, Counter Reading, Structural Contrast
    pub technical: Option<crate::technical::TechnicalDirectionHead>,
    pub counter_reading: Option<crate::technical::TechnicalCounterReading>,
    pub structural_contrast: Option<crate::technical::TechnicalStructuralContrast>,
    // Optional: calibration-based directional (KNN)
    pub directional: Option<crate::calibration::DirectionalResolution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub primary_provider: String,
    pub secondary_provider: Option<String>,
    pub source_watermark: String,
    pub input_window_sha256: String,
    pub engine_version: String,
    pub engine_config_sha256: String,
    pub structural_vector_version: String,
    pub resolution_calibration_version: Option<String>,
    pub resolution_profile_sha256: Option<String>,
    pub runtime_config_sha256: String,
    pub response_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FinancialSignalResponse {
    pub schema: String,
    pub status: RuntimeStatus,
    pub instrument: Instrument,
    pub as_of_ns: i64,
    pub data_watermark_ns: i64,
    pub mode: SignalMode,
    pub direction: Direction,
    pub label: String,
    pub probabilities_bp: Option<ProbabilitiesBp>,
    pub scales: Vec<PerScaleSignal>,
    pub provenance: Provenance,
}

impl FinancialSignalResponse {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != "pramagraph.financial_signal.v1" {
            return Err(ContractError::SchemaVersion);
        }
        if let Some(probabilities) = self.probabilities_bp {
            probabilities.validate()?;
        }
        for scale in &self.scales {
            if let Some(probabilities) = scale.probabilities_bp {
                probabilities.validate()?;
            }
            if scale.reliability_bp.is_some_and(|v| v > 10_000) {
                return Err(ContractError::BasisPointsRange);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("availability and value do not agree")]
    AvailabilityMismatch,
    #[error("instrument/source identity is missing")]
    MissingIdentity,
    #[error("observation close must be after open")]
    InvalidTimeRange,
    #[error("OHLC contains a non-finite value")]
    NonFinitePrice,
    #[error("OHLC ordering is malformed")]
    MalformedOhlc,
    #[error("direction probabilities must sum to exactly 10000 basis points")]
    ProbabilitySum,
    #[error("basis-point value is outside 0..=10000")]
    BasisPointsRange,
    #[error("unsupported response schema version")]
    SchemaVersion,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_is_not_zero() {
        let missing = AvailableValue::<f64>::unavailable();
        assert_eq!(missing.value, None);
        assert!(missing.validate().is_ok());
    }

    #[test]
    fn availability_mismatch_fails_closed() {
        let invalid = AvailableValue {
            value: Some(0.0),
            availability: AvailabilityStatus::Unavailable,
        };
        assert_eq!(invalid.validate(), Err(ContractError::AvailabilityMismatch));
    }

    #[test]
    fn probabilities_are_exact_basis_points() {
        assert!(ProbabilitiesBp {
            up: 2500,
            range: 5000,
            down: 2500
        }
        .validate()
        .is_ok());
        assert_eq!(
            ProbabilitiesBp {
                up: 2500,
                range: 4999,
                down: 2500
            }
            .validate(),
            Err(ContractError::ProbabilitySum)
        );
    }
}
