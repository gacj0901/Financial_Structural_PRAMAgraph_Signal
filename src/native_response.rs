//! Native Financial Response Structure
//!
//! This module defines the authoritative native response shape for financial
//! structural signals. It is independent of the Telegraph adapter layer and
//! uses the calibration machinery exactly as implemented.

use crate::{
    AssetClass, CalibrationScope, Direction, Horizon, Instrument, ProbabilitiesBp, RuntimeStatus,
    TechnicalCounterReading, TechnicalDirectionHead, TechnicalStructuralContrast, Timeframe,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[allow(unused_imports)]
use std::collections::BTreeMap;

/// Direction basis distinguishing technical from calibrated resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DirectionBasis {
    Technical,
    CalibratedResolution,
}

/// Calibration status in the native response
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CalibrationStatus {
    Available,
    Unavailable,
    InsufficientMatchedSupport,
    HeldOutProbabilitySkillNotPositive,
    HeldOutReliabilityNotMet,
    CalibratedDirectionEdgeNotMet,
    CalibrationEvidenceNotPreregistered,
    MissingProfile,
    MissingCorpus,
    IncompatibleProfile,
}

impl From<&str> for CalibrationStatus {
    fn from(reason: &str) -> Self {
        match reason {
            "CALIBRATED_EVIDENCE_SATISFIED" => CalibrationStatus::Available,
            "INSUFFICIENT_MATCHED_SUPPORT" => CalibrationStatus::InsufficientMatchedSupport,
            "HELD_OUT_PROBABILITY_SKILL_NOT_POSITIVE" => {
                CalibrationStatus::HeldOutProbabilitySkillNotPositive
            }
            "HELD_OUT_RELIABILITY_NOT_MET" => CalibrationStatus::HeldOutReliabilityNotMet,
            "CALIBRATED_DIRECTION_EDGE_NOT_MET" => CalibrationStatus::CalibratedDirectionEdgeNotMet,
            "CALIBRATION_EVIDENCE_NOT_PREREGISTERED" => {
                CalibrationStatus::CalibrationEvidenceNotPreregistered
            }
            _ => CalibrationStatus::Unavailable,
        }
    }
}

/// Native calibration section in the response
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeCalibration {
    pub status: CalibrationStatus,
    pub calibrated: bool,
    pub scope: CalibrationScope,
    pub probabilities_bp: Option<ProbabilitiesBp>,
    pub reliability_bp: Option<u16>,
    pub sample_support: u64,
    pub profile_sha256: Option<String>,
    pub publication_reason: Option<String>,
}

impl NativeCalibration {
    /// Create from a DirectionalResolution (available calibration)
    pub fn from_resolution(resolution: &crate::calibration::DirectionalResolution) -> Self {
        Self {
            status: CalibrationStatus::Available,
            calibrated: true,
            scope: resolution.calibration_scope,
            probabilities_bp: resolution.probabilities_bp,
            reliability_bp: resolution.reliability_bp,
            sample_support: resolution.sample_support,
            profile_sha256: Some(resolution.profile_sha256.clone()),
            publication_reason: Some(resolution.publication_reason.clone()),
        }
    }

    /// Create unavailable calibration with explicit reason
    pub fn unavailable(
        scope: CalibrationScope,
        reason: CalibrationStatus,
        profile_sha256: Option<String>,
    ) -> Self {
        Self {
            status: reason,
            calibrated: false,
            scope,
            probabilities_bp: None,
            reliability_bp: None,
            sample_support: 0,
            profile_sha256,
            publication_reason: None,
        }
    }

    /// Create from unresolved DirectionalResolution
    pub fn from_unresolved(resolution: &crate::calibration::DirectionalResolution) -> Self {
        let reason = CalibrationStatus::from(resolution.publication_reason.as_str());
        Self {
            status: reason,
            calibrated: false,
            scope: resolution.calibration_scope,
            probabilities_bp: None,
            reliability_bp: None,
            sample_support: resolution.sample_support,
            profile_sha256: Some(resolution.profile_sha256.clone()),
            publication_reason: Some(resolution.publication_reason.clone()),
        }
    }
}

/// Native horizon section
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeHorizon {
    pub status: HorizonStatus,
    pub p25_bars: Option<u32>,
    pub median_bars: Option<u32>,
    pub p75_bars: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HorizonStatus {
    Available,
    Unavailable,
}

impl NativeHorizon {
    pub fn from_horizon(horizon: &Horizon) -> Self {
        Self {
            status: HorizonStatus::Available,
            p25_bars: horizon.p25_bars,
            median_bars: horizon.median_bars,
            p75_bars: horizon.p75_bars,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            status: HorizonStatus::Unavailable,
            p25_bars: None,
            median_bars: None,
            p75_bars: None,
        }
    }
}

/// Quality assessment
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeQuality {
    pub structural_contrast: String,
    pub counter_pressure: String,
    pub data_freshness: String,
}

/// Instrument identification
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeInstrument {
    pub id: String,
    pub symbol: String,
    pub asset: String,
    pub asset_class: AssetClass,
    pub venue: String,
}

impl NativeInstrument {
    pub fn from_instrument(inst: &Instrument) -> Self {
        Self {
            id: inst.instrument_id.clone(),
            symbol: inst.symbol.clone(),
            asset: inst.base.clone().unwrap_or_default(),
            asset_class: inst.asset_class,
            venue: inst.venue.clone(),
        }
    }
}

/// Detail sections
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeDetail {
    pub technical: TechnicalDirectionHead,
    pub structural: crate::StructuralSnapshot,
    pub counter_reading: TechnicalCounterReading,
    pub structural_contrast: TechnicalStructuralContrast,
    pub structural_contrast_evidence: Vec<StructuralContrastEvidence>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructuralContrastEvidence {
    pub component: String,
    pub evidence: String,
    pub alignment: String,
}

/// Provenance
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeProvenance {
    pub engine_version: String,
    pub structural_vector_version: String,
    pub observation_interface_version: String,
    pub input_window_sha256: String,
    pub resolution_profile_sha256: Option<String>,
    pub primary_provider: String,
    pub provider_instrument: Option<String>,
    pub response_sha256: Option<String>,
}

/// Signal section
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeSignal {
    pub direction: Direction,
    pub direction_basis: DirectionBasis,
    pub strength: String, // Always "UNAVAILABLE" per spec
    pub reason_code: String,
    pub reason: String,
    pub calibration: NativeCalibration,
    pub horizon: NativeHorizon,
}

/// Cross-asset relation evidence
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CrossAssetRelation {
    /// Reference instrument identifier
    pub reference_instrument_id: String,
    /// Temporal overlap start (earliest common observation)
    pub overlap_start_ns: i64,
    /// Temporal overlap end (latest common observation)
    pub overlap_end_ns: i64,
    /// Number of temporally aligned observation pairs
    pub aligned_observation_count: usize,
    /// Structural vector cosine similarity (computed on common available dimensions)
    pub structural_vector_cosine_similarity: Option<f64>,
    /// PRAMA structural state agreement
    pub prama_state_agreement: String, // "ALIGNED", "DIVERGING", "UNAVAILABLE"
    /// D_O structural state agreement
    pub do_state_agreement: String, // "ALIGNED", "DIVERGING", "UNAVAILABLE"
    /// ODCE structural state agreement
    pub odce_state_agreement: String, // "ALIGNED", "DIVERGING", "UNAVAILABLE"
    /// K-MEM structural state agreement
    pub k_mem_state_agreement: String, // "ALIGNED", "DIVERGING", "UNAVAILABLE"
    /// Technical direction agreement
    pub technical_direction_agreement: String, // "SAME", "OPPOSITE", "UNAVAILABLE"
    /// Counter reading agreement
    pub counter_reading_agreement: String, // "SAME", "OPPOSITE", "UNAVAILABLE"
    /// Overall relation classification
    pub relation_classification: String, // "STRONG_ALIGNMENT", "WEAK_ALIGNMENT", "DIVERGENCE", "UNAVAILABLE"
    /// Provenance for cross-asset computation
    pub provenance: CrossAssetProvenance,
}

/// Cross-asset computation provenance
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CrossAssetProvenance {
    /// Primary instrument vector SHA-256
    pub primary_vector_sha256: String,
    /// Reference instrument vector SHA-256
    pub reference_vector_sha256: String,
    /// Structural engine version used
    pub structural_engine_version: String,
    /// Structural vector version used
    pub structural_vector_version: String,
    /// Computation timestamp
    pub computed_at_ns: i64,
}

/// Main native response
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NativeFinancialResponse {
    pub schema: String,
    pub intent: String,
    pub status: RuntimeStatus,
    pub signal: NativeSignal,
    pub quality: NativeQuality,
    pub instrument: NativeInstrument,
    pub timeframe: Timeframe,
    pub as_of_ns: i64,
    pub data_watermark_ns: i64,
    pub detail: NativeDetail,
    pub provenance: NativeProvenance,
    pub response_sha256: Option<String>,
    /// Optional cross-asset relation evidence (present when reference_asset provided and computable)
    pub cross_asset: Option<CrossAssetRelation>,
}

impl NativeFinancialResponse {
    pub const SCHEMA_VERSION: &'static str = "pramagraph.telegraph.financial_data";

    /// Verify the response hash matches the content
    pub fn verify_hash(&self) -> bool {
        let mut clone = self.clone();
        clone.response_sha256 = None;
        let canonical = serde_json::to_string(&clone).expect("response serializes");
        let expected = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        self.response_sha256.as_ref() == Some(&expected)
    }
}

/// Adapter: NativeFinancialResponse → Telegraph FinancialDataResponse
pub fn adapt_to_telegraph(
    native: &NativeFinancialResponse,
) -> crate::service::FinancialDataResponse {
    let mut telegraph = crate::service::FinancialDataResponse {
        schema: "pramagraph.telegraph.financial_data.v1".into(),
        intent: "FINANCIAL_DATA".into(),
        status: native.status,
        label: format!("{:?}", native.signal.direction),
        reason: native.signal.reason.clone(),
        instrument: crate::Instrument {
            instrument_id: native.instrument.id.clone(),
            symbol: native.instrument.symbol.clone(),
            base: Some(native.instrument.asset.clone()),
            quote: None,
            venue: native.instrument.venue.clone(),
            timezone: "UTC".into(),
            session_calendar: crate::SessionCalendar::ContinuousUtc,
            price_precision: None,
            quantity_precision: None,
            live_data_capable: true,
            historical_data_capable: true,
            asset_class: native.instrument.asset_class,
        },
        timeframe: native.timeframe,
        as_of_ns: native.as_of_ns,
        market: crate::service::MarketBarResponse {
            open_time_ns: 0, // Not in native, would need to be passed through
            close_time_ns: native.as_of_ns,
            open: 0.0,
            high: 0.0,
            low: 0.0,
            close: 0.0,
            volume: crate::AvailableValue::unavailable(),
        },
        structural: native.detail.structural.clone(),
        technical: Some(native.detail.technical.clone()),
        counter_reading: Some(native.detail.counter_reading.clone()),
        structural_contrast: Some(native.detail.structural_contrast.clone()),
        directional: native.signal.calibration.calibrated.then(|| {
            // Convert native calibration to DirectionalResolution
            crate::calibration::DirectionalResolution {
                direction: native.signal.direction,
                probabilities_bp: native.signal.calibration.probabilities_bp,
                horizon: native.signal.horizon.p25_bars.map(|_| crate::Horizon {
                    p25_bars: native.signal.horizon.p25_bars,
                    median_bars: native.signal.horizon.median_bars,
                    p75_bars: native.signal.horizon.p75_bars,
                    p25_seconds: None,
                    median_seconds: None,
                    p75_seconds: None,
                }),
                reliability_bp: native.signal.calibration.reliability_bp,
                sample_support: native.signal.calibration.sample_support,
                calibration_scope: native.signal.calibration.scope,
                profile_sha256: native
                    .signal
                    .calibration
                    .profile_sha256
                    .clone()
                    .unwrap_or_default(),
                publication_reason: native
                    .signal
                    .calibration
                    .publication_reason
                    .clone()
                    .unwrap_or_default(),
            }
        }),
        provenance: crate::service::ServiceProvenance {
            primary_provider: native.provenance.primary_provider.clone(),
            provider_instrument: native.provenance.provider_instrument.clone(),
            corpus_file: None,
            input_sha256: native.provenance.input_window_sha256.clone(),
            engine_version: native.provenance.engine_version.clone(),
            observation_interface_version: native.provenance.observation_interface_version.clone(),
        },
        response_sha256: native.response_sha256.clone(),
    };

    // Update the market bar if we have better data (from structural)
    // The structural snapshot has as_of_ns which is the close time
    telegraph.market.close_time_ns = native.as_of_ns;

    telegraph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Direction, RuntimeStatus, Timeframe};

    #[test]
    fn test_native_calibration_from_resolution() {
        let resolution = crate::calibration::DirectionalResolution {
            direction: Direction::Up,
            probabilities_bp: Some(ProbabilitiesBp {
                up: 4000,
                range: 3000,
                down: 3000,
            }),
            horizon: Some(Horizon {
                p25_bars: Some(5),
                median_bars: Some(10),
                p75_bars: Some(20),
                p25_seconds: None,
                median_seconds: None,
                p75_seconds: None,
            }),
            reliability_bp: Some(8000),
            sample_support: 15,
            calibration_scope: CalibrationScope::Instrument,
            profile_sha256: "sha256:abc123".into(),
            publication_reason: "CALIBRATED_EVIDENCE_SATISFIED".into(),
        };

        let cal = NativeCalibration::from_resolution(&resolution);
        assert!(cal.calibrated);
        assert_eq!(cal.status, CalibrationStatus::Available);
        assert_eq!(cal.probabilities_bp.unwrap().up, 4000);
        assert_eq!(cal.reliability_bp.unwrap(), 8000);
        assert_eq!(cal.sample_support, 15);
    }

    #[test]
    fn test_native_calibration_unavailable() {
        let cal = NativeCalibration::unavailable(
            CalibrationScope::Instrument,
            CalibrationStatus::InsufficientMatchedSupport,
            Some("sha256:xyz".into()),
        );
        assert!(!cal.calibrated);
        assert_eq!(cal.status, CalibrationStatus::InsufficientMatchedSupport);
        assert!(cal.probabilities_bp.is_none());
        assert!(cal.reliability_bp.is_none());
        assert_eq!(cal.sample_support, 0);
    }

    #[test]
    fn test_native_horizon_available() {
        let horizon = Horizon {
            p25_bars: Some(5),
            median_bars: Some(10),
            p75_bars: Some(20),
            p25_seconds: Some(432000),
            median_seconds: Some(864000),
            p75_seconds: Some(1728000),
        };
        let native = NativeHorizon::from_horizon(&horizon);
        assert_eq!(native.status, HorizonStatus::Available);
        assert_eq!(native.p25_bars, Some(5));
        assert_eq!(native.median_bars, Some(10));
        assert_eq!(native.p75_bars, Some(20));
    }

    #[test]
    fn test_native_horizon_unavailable() {
        let native = NativeHorizon::unavailable();
        assert_eq!(native.status, HorizonStatus::Unavailable);
        assert!(native.p25_bars.is_none());
        assert!(native.median_bars.is_none());
        assert!(native.p75_bars.is_none());
    }

    #[test]
    fn test_probabilities_bp_validation() {
        let prob = ProbabilitiesBp {
            up: 4000,
            range: 3000,
            down: 3000,
        };
        assert!(prob.validate().is_ok());

        let prob = ProbabilitiesBp {
            up: 4000,
            range: 3000,
            down: 2999,
        };
        assert!(prob.validate().is_err());
    }

    #[test]
    fn test_native_response_serialization() {
        let native = NativeFinancialResponse {
            schema: NativeFinancialResponse::SCHEMA_VERSION.into(),
            intent: "FINANCIAL_DATA".into(),
            status: RuntimeStatus::Ok,
            signal: NativeSignal {
                direction: Direction::Up,
                direction_basis: DirectionBasis::Technical,
                strength: "UNAVAILABLE".into(),
                reason_code: "TECHNICAL_UP".into(),
                reason: "technical up".into(),
                calibration: NativeCalibration::unavailable(
                    CalibrationScope::Instrument,
                    CalibrationStatus::Unavailable,
                    None,
                ),
                horizon: NativeHorizon::unavailable(),
            },
            quality: NativeQuality {
                structural_contrast: "test".into(),
                counter_pressure: "test".into(),
                data_freshness: "test".into(),
            },
            instrument: NativeInstrument {
                id: "BTCUSDT_D1".into(),
                symbol: "BTCUSDT".into(),
                asset: "BTC".into(),
                asset_class: AssetClass::Crypto,
                venue: "binance".into(),
            },
            timeframe: Timeframe::D1,
            as_of_ns: 1_700_000_000_000_000_000,
            data_watermark_ns: 1_700_000_000_000_000_000,
            detail: NativeDetail {
                technical: TechnicalDirectionHead {
                    direction: crate::TechnicalDirection::Up,
                    votes: crate::VoteBreakdown {
                        ema_trend: crate::TechnicalDirection::Up,
                        ema_slope: crate::TechnicalDirection::Up,
                        macd: crate::TechnicalDirection::Up,
                        rsi_centerline: crate::TechnicalDirection::Up,
                    },
                    range_detection: crate::RangeDetection {
                        adx14: crate::AvailableValue::available(25.0),
                        ema_separation_atr: crate::AvailableValue::available(0.0),
                        is_range: false,
                    },
                    indicators: crate::IndicatorValues {
                        ema20: crate::AvailableValue::available(0.0),
                        ema50: crate::AvailableValue::available(0.0),
                        ema20_slope: crate::AvailableValue::available(0.0),
                        macd_histogram: crate::AvailableValue::available(0.0),
                        rsi14: crate::AvailableValue::available(50.0),
                        adx14: crate::AvailableValue::available(25.0),
                        atr14: crate::AvailableValue::available(0.0),
                        bollinger_upper: crate::AvailableValue::available(0.0),
                        bollinger_lower: crate::AvailableValue::available(0.0),
                        bollinger_middle: crate::AvailableValue::available(0.0),
                    },
                    bars_used: 100,
                },
                structural: crate::StructuralSnapshot {
                    instrument_id: "BTCUSDT_D1".into(),
                    timeframe: Timeframe::D1,
                    as_of_ns: 1_700_000_000_000_000_000,
                    engine_version: "test".into(),
                    structural_state: "UP".into(),
                    prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                    d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                    odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                    k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                    availability: BTreeMap::new(),
                    source_watermark: "test".into(),
                    snapshot_sha256: None,
                },
                counter_reading: TechnicalCounterReading {
                    direction: crate::CounterReading::None,
                    evidence: crate::CounterReadingEvidence {
                        rsi_extreme: None,
                        ema_extension: crate::AvailableValue::unavailable(),
                        bollinger_position: None,
                    },
                },
                structural_contrast: TechnicalStructuralContrast {
                    timeframe: Timeframe::D1,
                    structural: crate::StructuralSnapshot {
                        instrument_id: "BTCUSDT_D1".into(),
                        timeframe: Timeframe::D1,
                        as_of_ns: 1_700_000_000_000_000_000,
                        engine_version: "test".into(),
                        structural_state: "UP".into(),
                        prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                        d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                        odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                        k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                        availability: BTreeMap::new(),
                        source_watermark: "test".into(),
                        snapshot_sha256: None,
                    },
                    technical: TechnicalDirectionHead {
                        direction: crate::TechnicalDirection::Up,
                        votes: crate::VoteBreakdown {
                            ema_trend: crate::TechnicalDirection::Up,
                            ema_slope: crate::TechnicalDirection::Up,
                            macd: crate::TechnicalDirection::Up,
                            rsi_centerline: crate::TechnicalDirection::Up,
                        },
                        range_detection: crate::RangeDetection {
                            adx14: crate::AvailableValue::available(25.0),
                            ema_separation_atr: crate::AvailableValue::available(0.0),
                            is_range: false,
                        },
                        indicators: crate::IndicatorValues {
                            ema20: crate::AvailableValue::available(0.0),
                            ema50: crate::AvailableValue::available(0.0),
                            ema20_slope: crate::AvailableValue::available(0.0),
                            macd_histogram: crate::AvailableValue::available(0.0),
                            rsi14: crate::AvailableValue::available(50.0),
                            adx14: crate::AvailableValue::available(25.0),
                            atr14: crate::AvailableValue::available(0.0),
                            bollinger_upper: crate::AvailableValue::available(0.0),
                            bollinger_lower: crate::AvailableValue::available(0.0),
                            bollinger_middle: crate::AvailableValue::available(0.0),
                        },
                        bars_used: 100,
                    },
                    counter_reading: TechnicalCounterReading {
                        direction: crate::CounterReading::None,
                        evidence: crate::CounterReadingEvidence {
                            rsi_extreme: None,
                            ema_extension: crate::AvailableValue::unavailable(),
                            bollinger_position: None,
                        },
                    },
                    structural_contrast: crate::StructuralContrast {
                        state: crate::ContrastState::Confirming,
                        evidence: vec![],
                    },
                },
                structural_contrast_evidence: vec![],
            },
            provenance: NativeProvenance {
                engine_version: "test".into(),
                structural_vector_version: "test".into(),
                observation_interface_version: "test".into(),
                input_window_sha256: "sha256:test".into(),
                resolution_profile_sha256: None,
                primary_provider: "test".into(),
                provider_instrument: None,
                response_sha256: None,
            },
            response_sha256: None,
            cross_asset: None,
        };

        let json = serde_json::to_string(&native).unwrap();
        assert!(json.contains("pramagraph.telegraph.financial_data"));
        assert!(json.contains("direction_basis"));
        assert!(json.contains("TECHNICAL"));
        assert!(json.contains("UNAVAILABLE"));
    }

    #[test]
    fn test_native_response_hash() {
        let mut native = NativeFinancialResponse {
            schema: NativeFinancialResponse::SCHEMA_VERSION.into(),
            intent: "FINANCIAL_DATA".into(),
            status: RuntimeStatus::Ok,
            signal: NativeSignal {
                direction: Direction::Up,
                direction_basis: DirectionBasis::Technical,
                strength: "UNAVAILABLE".into(),
                reason_code: "TECHNICAL_UP".into(),
                reason: "test".into(),
                calibration: NativeCalibration::unavailable(
                    CalibrationScope::Instrument,
                    CalibrationStatus::Unavailable,
                    None,
                ),
                horizon: NativeHorizon::unavailable(),
            },
            quality: NativeQuality {
                structural_contrast: "test".into(),
                counter_pressure: "test".into(),
                data_freshness: "test".into(),
            },
            instrument: NativeInstrument {
                id: "BTCUSDT_D1".into(),
                symbol: "BTCUSDT".into(),
                asset: "BTC".into(),
                asset_class: AssetClass::Crypto,
                venue: "binance".into(),
            },
            timeframe: Timeframe::D1,
            as_of_ns: 1_700_000_000_000_000_000,
            data_watermark_ns: 1_700_000_000_000_000_000,
            detail: NativeDetail {
                technical: TechnicalDirectionHead {
                    direction: crate::TechnicalDirection::Up,
                    votes: crate::VoteBreakdown {
                        ema_trend: crate::TechnicalDirection::Up,
                        ema_slope: crate::TechnicalDirection::Up,
                        macd: crate::TechnicalDirection::Up,
                        rsi_centerline: crate::TechnicalDirection::Up,
                    },
                    range_detection: crate::RangeDetection {
                        adx14: crate::AvailableValue::available(25.0),
                        ema_separation_atr: crate::AvailableValue::available(0.0),
                        is_range: false,
                    },
                    indicators: crate::IndicatorValues {
                        ema20: crate::AvailableValue::available(0.0),
                        ema50: crate::AvailableValue::available(0.0),
                        ema20_slope: crate::AvailableValue::available(0.0),
                        macd_histogram: crate::AvailableValue::available(0.0),
                        rsi14: crate::AvailableValue::available(50.0),
                        adx14: crate::AvailableValue::available(25.0),
                        atr14: crate::AvailableValue::available(0.0),
                        bollinger_upper: crate::AvailableValue::available(0.0),
                        bollinger_lower: crate::AvailableValue::available(0.0),
                        bollinger_middle: crate::AvailableValue::available(0.0),
                    },
                    bars_used: 100,
                },
                structural: crate::StructuralSnapshot {
                    instrument_id: "BTCUSDT_D1".into(),
                    timeframe: Timeframe::D1,
                    as_of_ns: 1_700_000_000_000_000_000,
                    engine_version: "test".into(),
                    structural_state: "UP".into(),
                    prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                    d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                    odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                    k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                    availability: BTreeMap::new(),
                    source_watermark: "test".into(),
                    snapshot_sha256: None,
                },
                counter_reading: TechnicalCounterReading {
                    direction: crate::CounterReading::None,
                    evidence: crate::CounterReadingEvidence {
                        rsi_extreme: None,
                        ema_extension: crate::AvailableValue::unavailable(),
                        bollinger_position: None,
                    },
                },
                structural_contrast: TechnicalStructuralContrast {
                    timeframe: Timeframe::D1,
                    structural: crate::StructuralSnapshot {
                        instrument_id: "BTCUSDT_D1".into(),
                        timeframe: Timeframe::D1,
                        as_of_ns: 1_700_000_000_000_000_000,
                        engine_version: "test".into(),
                        structural_state: "UP".into(),
                        prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                        d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                        odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                        k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                        availability: BTreeMap::new(),
                        source_watermark: "test".into(),
                        snapshot_sha256: None,
                    },
                    technical: TechnicalDirectionHead {
                        direction: crate::TechnicalDirection::Up,
                        votes: crate::VoteBreakdown {
                            ema_trend: crate::TechnicalDirection::Up,
                            ema_slope: crate::TechnicalDirection::Up,
                            macd: crate::TechnicalDirection::Up,
                            rsi_centerline: crate::TechnicalDirection::Up,
                        },
                        range_detection: crate::RangeDetection {
                            adx14: crate::AvailableValue::available(25.0),
                            ema_separation_atr: crate::AvailableValue::available(0.0),
                            is_range: false,
                        },
                        indicators: crate::IndicatorValues {
                            ema20: crate::AvailableValue::available(0.0),
                            ema50: crate::AvailableValue::available(0.0),
                            ema20_slope: crate::AvailableValue::available(0.0),
                            macd_histogram: crate::AvailableValue::available(0.0),
                            rsi14: crate::AvailableValue::available(50.0),
                            adx14: crate::AvailableValue::available(25.0),
                            atr14: crate::AvailableValue::available(0.0),
                            bollinger_upper: crate::AvailableValue::available(0.0),
                            bollinger_lower: crate::AvailableValue::available(0.0),
                            bollinger_middle: crate::AvailableValue::available(0.0),
                        },
                        bars_used: 100,
                    },
                    counter_reading: TechnicalCounterReading {
                        direction: crate::CounterReading::None,
                        evidence: crate::CounterReadingEvidence {
                            rsi_extreme: None,
                            ema_extension: crate::AvailableValue::unavailable(),
                            bollinger_position: None,
                        },
                    },
                    structural_contrast: crate::StructuralContrast {
                        state: crate::ContrastState::Confirming,
                        evidence: vec![],
                    },
                },
                structural_contrast_evidence: vec![],
            },
            provenance: NativeProvenance {
                engine_version: "test".into(),
                structural_vector_version: "test".into(),
                observation_interface_version: "test".into(),
                input_window_sha256: "sha256:test".into(),
                resolution_profile_sha256: None,
                primary_provider: "test".into(),
                provider_instrument: None,
                response_sha256: None,
            },
            response_sha256: None,
            cross_asset: None,
        };

        let mut clone = native.clone();
        clone.response_sha256 = None;
        let canonical = serde_json::to_string(&clone).expect("response serializes");
        let expected = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        native.response_sha256 = Some(expected.clone());

        assert!(native.verify_hash());
    }

    #[test]
    fn test_adapter_to_telegraph() {
        let native = NativeFinancialResponse {
            schema: NativeFinancialResponse::SCHEMA_VERSION.into(),
            intent: "FINANCIAL_DATA".into(),
            status: RuntimeStatus::Ok,
            signal: NativeSignal {
                direction: Direction::Up,
                direction_basis: DirectionBasis::Technical,
                strength: "UNAVAILABLE".into(),
                reason_code: "TECHNICAL_UP".into(),
                reason: "test".into(),
                calibration: NativeCalibration::unavailable(
                    CalibrationScope::Instrument,
                    CalibrationStatus::Unavailable,
                    None,
                ),
                horizon: NativeHorizon::unavailable(),
            },
            quality: NativeQuality {
                structural_contrast: "test".into(),
                counter_pressure: "test".into(),
                data_freshness: "test".into(),
            },
            instrument: NativeInstrument {
                id: "BTCUSDT_D1".into(),
                symbol: "BTCUSDT".into(),
                asset: "BTC".into(),
                asset_class: AssetClass::Crypto,
                venue: "binance".into(),
            },
            timeframe: Timeframe::D1,
            as_of_ns: 1_700_000_000_000_000_000,
            data_watermark_ns: 1_700_000_000_000_000_000,
            detail: NativeDetail {
                technical: TechnicalDirectionHead {
                    direction: crate::TechnicalDirection::Up,
                    votes: crate::VoteBreakdown {
                        ema_trend: crate::TechnicalDirection::Up,
                        ema_slope: crate::TechnicalDirection::Up,
                        macd: crate::TechnicalDirection::Up,
                        rsi_centerline: crate::TechnicalDirection::Up,
                    },
                    range_detection: crate::RangeDetection {
                        adx14: crate::AvailableValue::available(25.0),
                        ema_separation_atr: crate::AvailableValue::available(0.0),
                        is_range: false,
                    },
                    indicators: crate::IndicatorValues {
                        ema20: crate::AvailableValue::available(0.0),
                        ema50: crate::AvailableValue::available(0.0),
                        ema20_slope: crate::AvailableValue::available(0.0),
                        macd_histogram: crate::AvailableValue::available(0.0),
                        rsi14: crate::AvailableValue::available(50.0),
                        adx14: crate::AvailableValue::available(25.0),
                        atr14: crate::AvailableValue::available(0.0),
                        bollinger_upper: crate::AvailableValue::available(0.0),
                        bollinger_lower: crate::AvailableValue::available(0.0),
                        bollinger_middle: crate::AvailableValue::available(0.0),
                    },
                    bars_used: 100,
                },
                structural: crate::StructuralSnapshot {
                    instrument_id: "BTCUSDT_D1".into(),
                    timeframe: Timeframe::D1,
                    as_of_ns: 1_700_000_000_000_000_000,
                    engine_version: "test".into(),
                    structural_state: "UP".into(),
                    prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                    d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                    odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                    k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                    availability: BTreeMap::new(),
                    source_watermark: "test".into(),
                    snapshot_sha256: None,
                },
                counter_reading: TechnicalCounterReading {
                    direction: crate::CounterReading::None,
                    evidence: crate::CounterReadingEvidence {
                        rsi_extreme: None,
                        ema_extension: crate::AvailableValue::unavailable(),
                        bollinger_position: None,
                    },
                },
                structural_contrast: TechnicalStructuralContrast {
                    timeframe: Timeframe::D1,
                    structural: crate::StructuralSnapshot {
                        instrument_id: "BTCUSDT_D1".into(),
                        timeframe: Timeframe::D1,
                        as_of_ns: 1_700_000_000_000_000_000,
                        engine_version: "test".into(),
                        structural_state: "UP".into(),
                        prama: crate::ComponentSnapshot::available(serde_json::json!({})),
                        d_o: crate::ComponentSnapshot::available(serde_json::json!({})),
                        odce: crate::ComponentSnapshot::available(serde_json::json!({})),
                        k_mem: crate::ComponentSnapshot::available(serde_json::json!({})),
                        availability: BTreeMap::new(),
                        source_watermark: "test".into(),
                        snapshot_sha256: None,
                    },
                    technical: TechnicalDirectionHead {
                        direction: crate::TechnicalDirection::Up,
                        votes: crate::VoteBreakdown {
                            ema_trend: crate::TechnicalDirection::Up,
                            ema_slope: crate::TechnicalDirection::Up,
                            macd: crate::TechnicalDirection::Up,
                            rsi_centerline: crate::TechnicalDirection::Up,
                        },
                        range_detection: crate::RangeDetection {
                            adx14: crate::AvailableValue::available(25.0),
                            ema_separation_atr: crate::AvailableValue::available(0.0),
                            is_range: false,
                        },
                        indicators: crate::IndicatorValues {
                            ema20: crate::AvailableValue::available(0.0),
                            ema50: crate::AvailableValue::available(0.0),
                            ema20_slope: crate::AvailableValue::available(0.0),
                            macd_histogram: crate::AvailableValue::available(0.0),
                            rsi14: crate::AvailableValue::available(50.0),
                            adx14: crate::AvailableValue::available(25.0),
                            atr14: crate::AvailableValue::available(0.0),
                            bollinger_upper: crate::AvailableValue::available(0.0),
                            bollinger_lower: crate::AvailableValue::available(0.0),
                            bollinger_middle: crate::AvailableValue::available(0.0),
                        },
                        bars_used: 100,
                    },
                    counter_reading: TechnicalCounterReading {
                        direction: crate::CounterReading::None,
                        evidence: crate::CounterReadingEvidence {
                            rsi_extreme: None,
                            ema_extension: crate::AvailableValue::unavailable(),
                            bollinger_position: None,
                        },
                    },
                    structural_contrast: crate::StructuralContrast {
                        state: crate::ContrastState::Confirming,
                        evidence: vec![],
                    },
                },
                structural_contrast_evidence: vec![],
            },
            provenance: NativeProvenance {
                engine_version: "test".into(),
                structural_vector_version: "test".into(),
                observation_interface_version: "test".into(),
                input_window_sha256: "sha256:test".into(),
                resolution_profile_sha256: None,
                primary_provider: "test".into(),
                provider_instrument: None,
                response_sha256: None,
            },
            response_sha256: None,
            cross_asset: None,
        };

        let telegraph = adapt_to_telegraph(&native);
        assert_eq!(telegraph.schema, "pramagraph.telegraph.financial_data.v1");
        assert_eq!(telegraph.intent, "FINANCIAL_DATA");
        assert_eq!(telegraph.label, "Up");
        assert!(telegraph.technical.is_some());
        assert!(telegraph.counter_reading.is_some());
        assert!(telegraph.structural_contrast.is_some());
        assert!(telegraph.directional.is_none()); // calibration unavailable
    }
}
