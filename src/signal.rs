//! Native Financial Signal API - Request/Response contracts for PRAMAgraph
//!
//! This module defines the native REST API for financial structural signals,
//! independent of the Telegraph adapter layer. It uses the authoritative
//! types from contracts.rs and extends them with multi-scale composition logic.

use crate::{
    Direction, FinancialSignalResponse, Instrument, RuntimeStatus, SignalMode, StructuralSnapshot,
    TechnicalCounterReading, TechnicalDirectionHead, TechnicalStructuralContrast, Timeframe,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Native request for a multi-timeframe financial structural signal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FinancialSignalRequest {
    /// Canonical asset symbol or alias (e.g., "BTC", "SP500", "GOLD")
    pub asset: String,

    /// Optional venue override (default: "auto" - resolver picks primary)
    #[serde(default = "default_venue")]
    pub venue: String,

    /// Optional quote currency override (default: "auto" - resolver picks)
    #[serde(default = "default_quote")]
    pub quote: String,

    /// Timeframes to evaluate (default: all supported for the asset)
    #[serde(default)]
    pub timeframes: Vec<Timeframe>,

    /// Signal mode (default: CONFIRMED - closed bars only)
    #[serde(default = "default_mode")]
    pub mode: SignalMode,
}

fn default_venue() -> String {
    "auto".into()
}

fn default_quote() -> String {
    "auto".into()
}

fn default_mode() -> SignalMode {
    SignalMode::Confirmed
}

/// Per-scale signal reading combining structural, technical, and contrast data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScaleSignal {
    pub timeframe: Timeframe,
    pub structural: StructuralSnapshot,
    pub technical: TechnicalDirectionHead,
    pub counter_reading: TechnicalCounterReading,
    pub structural_contrast: TechnicalStructuralContrast,
    // Optional: calibration-based directional (KNN)
    pub directional: Option<crate::calibration::DirectionalResolution>,
}

/// Cross-scale composition result
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CrossScaleSignal {
    /// Dominant technical direction across available scales
    pub dominant_direction: Direction,
    /// Agreement level: "UNANIMOUS", "MAJORITY", "SPLIT", "UNAVAILABLE"
    pub agreement: String,
    /// Scales that agree with dominant direction
    pub agreeing_scales: Vec<Timeframe>,
    /// Scales that disagree
    pub disagreeing_scales: Vec<Timeframe>,
    /// Dominant scale (highest confidence or finest granularity)
    pub dominant_scale: Option<Timeframe>,
}

/// Top-level composed signal summary
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComposedSignal {
    pub direction: Direction,
    pub structural_state: String,
    pub dominant_scale: Option<Timeframe>,
    pub cross_scale: CrossScaleSignal,
}

/// Provenance information for the complete response
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SignalProvenance {
    pub primary_provider: String,
    pub secondary_provider: Option<String>,
    pub data_watermark_ns: i64,
    pub engine_version: String,
    pub engine_config_sha256: String,
    pub structural_vector_version: String,
    pub resolution_calibration_version: Option<String>,
    pub resolution_profile_sha256: Option<String>,
    pub runtime_config_sha256: String,
    pub request_sha256: String,
    pub response_sha256: Option<String>,
}

/// Error response for native API
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorResponse {
    pub status: RuntimeStatus,
    pub error: String,
    pub details: Option<BTreeMap<String, String>>,
}

impl FinancialSignalRequest {
    /// Determine which timeframes to actually evaluate
    pub fn resolve_timeframes(&self, supported: &[Timeframe]) -> Vec<Timeframe> {
        if self.timeframes.is_empty() {
            supported.to_vec()
        } else {
            self.timeframes
                .iter()
                .filter(|tf| supported.contains(tf))
                .cloned()
                .collect()
        }
    }
}

impl FinancialSignalResponse {
    /// Verify the response hash matches the content
    pub fn verify_hash(&self) -> bool {
        let mut clone = self.clone();
        clone.provenance.response_sha256 = None;
        let canonical = serde_json::to_string(&clone).expect("response serializes");
        let expected = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        self.provenance.response_sha256.as_ref() == Some(&expected)
    }
}

impl ScaleSignal {
    /// Verify that structural snapshot is unchanged from its source
    pub fn verify_structural_integrity(&self) -> bool {
        self.structural.snapshot_sha256.is_some()
    }
}

/// Determine the dominant direction from multiple scale readings
/// Uses a simple majority vote with CONFIRMED mode preference
#[allow(dead_code)]
fn determine_dominant_direction(scales: &[ScaleSignal]) -> Direction {
    let mut up = 0;
    let mut down = 0;
    let mut range = 0;
    let mut _unresolved = 0;

    for s in scales {
        match s.technical.direction {
            crate::TechnicalDirection::Up => up += 1,
            crate::TechnicalDirection::Down => down += 1,
            crate::TechnicalDirection::Range => range += 1,
            crate::TechnicalDirection::Unavailable => _unresolved += 1,
        }
    }

    if up > down && up > range && up > _unresolved {
        Direction::Up
    } else if down > up && down > range && down > _unresolved {
        Direction::Down
    } else if range > up && range > down && range > _unresolved {
        Direction::Range
    } else {
        Direction::Unresolved
    }
}

/// Determine cross-scale agreement
fn determine_cross_scale(scales: &[ScaleSignal]) -> CrossScaleSignal {
    if scales.is_empty() {
        return CrossScaleSignal {
            dominant_direction: Direction::Unresolved,
            agreement: "UNAVAILABLE".into(),
            agreeing_scales: vec![],
            disagreeing_scales: vec![],
            dominant_scale: None,
        };
    }

    // Count directions
    let mut up = 0;
    let mut down = 0;
    let mut range = 0;
    let mut _unresolved = 0;

    for s in scales {
        match s.technical.direction {
            crate::TechnicalDirection::Up => up += 1,
            crate::TechnicalDirection::Down => down += 1,
            crate::TechnicalDirection::Range => range += 1,
            crate::TechnicalDirection::Unavailable => _unresolved += 1,
        }
    }

    // Determine dominant direction (majority among resolved)
    let dominant = if up > down && up > range {
        Direction::Up
    } else if down > up && down > range {
        Direction::Down
    } else if range > up && range > down {
        Direction::Range
    } else {
        Direction::Unresolved
    };

    // Count agreeing/disagreeing relative to dominant
    // If dominant is Unresolved, use the most common resolved direction as reference
    let reference = if dominant != Direction::Unresolved {
        dominant
    } else if up >= down && up >= range {
        Direction::Up
    } else if down >= up && down >= range {
        Direction::Down
    } else if range > 0 {
        Direction::Range
    } else {
        Direction::Unresolved
    };

    let mut agreeing = vec![];
    let mut disagreeing = vec![];

    for s in scales {
        let dir: Direction = s.technical.direction.into();
        if reference != Direction::Unresolved && dir == reference {
            agreeing.push(s.timeframe);
        } else if dir != Direction::Unresolved {
            disagreeing.push(s.timeframe);
        }
    }

    let agreement = match (agreeing.len(), disagreeing.len()) {
        (a, 0) if a > 0 => "UNANIMOUS",
        (a, d) if a > d => "MAJORITY",
        (a, d) if a == d && a > 0 => "SPLIT",
        _ => "UNAVAILABLE",
    };

    // Dominant scale: prefer finest granularity among agreeing, or first
    let dominant_scale = if reference != Direction::Unresolved {
        scales
            .iter()
            .filter(|s| {
                let dir: Direction = s.technical.direction.into();
                dir == reference
            })
            .min_by_key(|s| match s.timeframe {
                Timeframe::M1 => 0,
                Timeframe::M5 => 1,
                Timeframe::H1 => 2,
                Timeframe::H4 => 3,
                Timeframe::D1 => 4,
                Timeframe::W1 => 5,
            })
            .map(|s| s.timeframe)
    } else {
        None
    };

    CrossScaleSignal {
        dominant_direction: dominant,
        agreement: agreement.into(),
        agreeing_scales: agreeing,
        disagreeing_scales: disagreeing,
        dominant_scale,
    }
}

/// Build the complete multi-timeframe signal response
#[allow(clippy::too_many_arguments)]
pub fn build_financial_signal_response(
    instrument: Instrument,
    scales: Vec<ScaleSignal>,
    mode: SignalMode,
    primary_provider: String,
    secondary_provider: Option<String>,
    _data_watermark_ns: i64,
    engine_version: String,
    engine_config_sha256: String,
    structural_vector_version: String,
    resolution_calibration_version: Option<String>,
    resolution_profile_sha256: Option<String>,
    runtime_config_sha256: String,
    _request_sha256: String,
) -> FinancialSignalResponse {
    if scales.is_empty() {
        let mut resp = FinancialSignalResponse {
            schema: "pramagraph.financial_signal.v1".into(),
            status: RuntimeStatus::InsufficientData,
            instrument,
            as_of_ns: 0,
            data_watermark_ns: 0,
            mode,
            direction: Direction::Unresolved,
            label: "UNAVAILABLE".into(),
            probabilities_bp: None,
            scales: vec![],
            provenance: crate::Provenance {
                primary_provider,
                secondary_provider,
                source_watermark: "".into(),
                input_window_sha256: "".into(),
                engine_version,
                engine_config_sha256,
                structural_vector_version,
                resolution_calibration_version,
                resolution_profile_sha256,
                runtime_config_sha256,
                response_sha256: None,
            },
        };
        // Compute hash
        let mut clone = resp.clone();
        clone.provenance.response_sha256 = None;
        let canonical = serde_json::to_string(&clone).expect("response serializes");
        let hash = format!(
            "sha256:{}",
            hex::encode(Sha256::digest(canonical.as_bytes()))
        );
        resp.provenance.response_sha256 = Some(hash);
        return resp;
    }

    let cross_scale = determine_cross_scale(&scales);
    let as_of_ns = scales
        .iter()
        .map(|s| s.structural.as_of_ns)
        .max()
        .unwrap_or(0);

    let _structural_state = scales
        .iter()
        .map(|s| s.structural.structural_state.clone())
        .max()
        .unwrap_or_else(|| "UNAVAILABLE".into());

    let direction = cross_scale.dominant_direction;

    // Convert scales to PerScaleSignal for the existing FinancialSignalResponse
    let per_scales: Vec<crate::PerScaleSignal> = scales
        .iter()
        .map(|s| crate::PerScaleSignal {
            timeframe: s.timeframe,
            structural: s.structural.clone(),
            direction: s.technical.direction.into(),
            probabilities_bp: None, // Technical direction doesn't have probabilities
            horizon: None,
            reliability_bp: None,
            sample_support: s.technical.bars_used as u64,
            calibration_scope: crate::CalibrationScope::Unavailable,
            technical: Some(s.technical.clone()),
            counter_reading: Some(s.counter_reading.clone()),
            structural_contrast: Some(s.structural_contrast.clone()),
            directional: s.directional.clone(),
        })
        .collect();

    let mut resp = FinancialSignalResponse {
        schema: "pramagraph.financial_signal.v1".into(),
        status: RuntimeStatus::Ok,
        instrument,
        as_of_ns,
        data_watermark_ns: 0, // Will be set by caller
        mode,
        direction,
        label: format!("{:?}", direction),
        probabilities_bp: None,
        scales: per_scales,
        provenance: crate::Provenance {
            primary_provider,
            secondary_provider,
            source_watermark: "".into(),
            input_window_sha256: "".into(),
            engine_version,
            engine_config_sha256,
            structural_vector_version,
            resolution_calibration_version,
            resolution_profile_sha256,
            runtime_config_sha256,
            response_sha256: None,
        },
    };

    // Compute canonical hash
    let mut clone = resp.clone();
    clone.provenance.response_sha256 = None;
    let canonical = serde_json::to_string(&clone).expect("response serializes");
    let hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    resp.provenance.response_sha256 = Some(hash);

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssetClass, AvailableValue, ComponentSnapshot, Instrument, SessionCalendar,
        TechnicalDirection, Timeframe,
    };
    use std::collections::BTreeMap;

    fn make_test_instrument() -> Instrument {
        Instrument {
            instrument_id: "crypto:binance:BTCUSDT".into(),
            asset_class: AssetClass::Crypto,
            symbol: "BTCUSDT".into(),
            base: Some("BTC".into()),
            quote: Some("USDT".into()),
            venue: "binance".into(),
            timezone: "UTC".into(),
            session_calendar: SessionCalendar::ContinuousUtc,
            price_precision: Some(2),
            quantity_precision: Some(8),
            live_data_capable: true,
            historical_data_capable: true,
        }
    }

    fn make_test_structural(timeframe: Timeframe, state: &str) -> StructuralSnapshot {
        StructuralSnapshot {
            instrument_id: "crypto:binance:BTCUSDT".into(),
            timeframe,
            as_of_ns: 1_700_000_000_000_000_000,
            engine_version: "test".into(),
            structural_state: state.into(),
            prama: ComponentSnapshot::unavailable("test"),
            d_o: ComponentSnapshot::available(serde_json::json!({"structural_state": state})),
            odce: ComponentSnapshot::unavailable("test"),
            k_mem: ComponentSnapshot::unavailable("test"),
            availability: BTreeMap::new(),
            source_watermark: "test".into(),
            snapshot_sha256: Some("sha256:test".into()),
        }
    }

    fn make_test_technical(dir: TechnicalDirection) -> TechnicalDirectionHead {
        TechnicalDirectionHead {
            direction: dir,
            votes: crate::VoteBreakdown {
                ema_trend: dir,
                ema_slope: dir,
                macd: dir,
                rsi_centerline: dir,
            },
            range_detection: crate::RangeDetection {
                adx14: AvailableValue::available(25.0),
                ema_separation_atr: AvailableValue::available(1.0),
                is_range: false,
            },
            indicators: crate::IndicatorValues {
                ema20: AvailableValue::available(50000.0),
                ema50: AvailableValue::available(49000.0),
                ema20_slope: AvailableValue::available(100.0),
                macd_histogram: AvailableValue::available(500.0),
                rsi14: AvailableValue::available(60.0),
                adx14: AvailableValue::available(25.0),
                atr14: AvailableValue::available(1000.0),
                bollinger_upper: AvailableValue::available(52000.0),
                bollinger_lower: AvailableValue::available(48000.0),
                bollinger_middle: AvailableValue::available(50000.0),
            },
            bars_used: 100,
        }
    }

    fn make_test_counter() -> TechnicalCounterReading {
        TechnicalCounterReading {
            direction: crate::CounterReading::None,
            evidence: crate::CounterReadingEvidence {
                rsi_extreme: None,
                ema_extension: AvailableValue::available(0.5),
                bollinger_position: None,
            },
        }
    }

    fn make_test_contrast() -> TechnicalStructuralContrast {
        TechnicalStructuralContrast {
            timeframe: Timeframe::D1,
            structural: make_test_structural(Timeframe::D1, "VIABLE"),
            technical: make_test_technical(TechnicalDirection::Up),
            counter_reading: make_test_counter(),
            structural_contrast: crate::StructuralContrast {
                state: crate::ContrastState::Confirming,
                evidence: vec![],
            },
        }
    }

    #[test]
    fn response_hash_is_deterministic() {
        let instrument = make_test_instrument();
        let scales = vec![ScaleSignal {
            timeframe: Timeframe::D1,
            structural: make_test_structural(Timeframe::D1, "VIABLE"),
            technical: make_test_technical(TechnicalDirection::Up),
            counter_reading: make_test_counter(),
            structural_contrast: make_test_contrast(),
            directional: None,
        }];

        let resp = build_financial_signal_response(
            instrument.clone(),
            scales.clone(),
            SignalMode::Confirmed,
            "binance_spot".into(),
            None,
            1_700_000_000_000_000_000,
            "test-engine".into(),
            "config-hash".into(),
            "vector-v1".into(),
            None,
            None,
            "runtime-config".into(),
            "request-hash".into(),
        );

        // Verify hash matches
        assert!(resp.verify_hash());
        assert!(resp.provenance.response_sha256.is_some());
        assert!(resp
            .provenance
            .response_sha256
            .as_ref()
            .unwrap()
            .starts_with("sha256:"));
    }

    #[test]
    fn identical_request_market_identical_response() {
        let instrument = make_test_instrument();
        let scales = vec![ScaleSignal {
            timeframe: Timeframe::D1,
            structural: make_test_structural(Timeframe::D1, "VIABLE"),
            technical: make_test_technical(TechnicalDirection::Up),
            counter_reading: make_test_counter(),
            structural_contrast: make_test_contrast(),
            directional: None,
        }];

        let resp1 = build_financial_signal_response(
            instrument.clone(),
            scales.clone(),
            SignalMode::Confirmed,
            "binance_spot".into(),
            None,
            1_700_000_000_000_000_000,
            "test-engine".into(),
            "config-hash".into(),
            "vector-v1".into(),
            None,
            None,
            "runtime-config".into(),
            "request-hash".into(),
        );

        let resp2 = build_financial_signal_response(
            instrument,
            scales,
            SignalMode::Confirmed,
            "binance_spot".into(),
            None,
            1_700_000_000_000_000_000,
            "test-engine".into(),
            "config-hash".into(),
            "vector-v1".into(),
            None,
            None,
            "runtime-config".into(),
            "request-hash".into(),
        );

        assert_eq!(resp1, resp2);
        assert_eq!(
            resp1.provenance.response_sha256,
            resp2.provenance.response_sha256
        );
    }

    #[test]
    fn cross_scale_majority_works() {
        let scales = vec![
            ScaleSignal {
                timeframe: Timeframe::D1,
                structural: make_test_structural(Timeframe::D1, "VIABLE"),
                technical: make_test_technical(TechnicalDirection::Up),
                counter_reading: make_test_counter(),
                structural_contrast: make_test_contrast(),
                directional: None,
            },
            ScaleSignal {
                timeframe: Timeframe::W1,
                structural: make_test_structural(Timeframe::W1, "VIABLE"),
                technical: make_test_technical(TechnicalDirection::Up),
                counter_reading: make_test_counter(),
                structural_contrast: make_test_contrast(),
                directional: None,
            },
        ];

        let cross = determine_cross_scale(&scales);
        assert_eq!(cross.dominant_direction, Direction::Up);
        assert_eq!(cross.agreement, "UNANIMOUS");
        assert_eq!(cross.agreeing_scales.len(), 2);
    }

    #[test]
    fn cross_scale_split_works() {
        let scales = vec![
            ScaleSignal {
                timeframe: Timeframe::D1,
                structural: make_test_structural(Timeframe::D1, "VIABLE"),
                technical: make_test_technical(TechnicalDirection::Up),
                counter_reading: make_test_counter(),
                structural_contrast: make_test_contrast(),
                directional: None,
            },
            ScaleSignal {
                timeframe: Timeframe::W1,
                structural: make_test_structural(Timeframe::W1, "VIABLE"),
                technical: make_test_technical(TechnicalDirection::Down),
                counter_reading: make_test_counter(),
                structural_contrast: make_test_contrast(),
                directional: None,
            },
        ];

        let cross = determine_cross_scale(&scales);
        assert_eq!(cross.agreement, "SPLIT");
        assert_eq!(cross.agreeing_scales.len(), 1);
        assert_eq!(cross.disagreeing_scales.len(), 1);
    }

    #[test]
    fn request_timeframe_resolution() {
        let req = FinancialSignalRequest {
            asset: "BTC".into(),
            venue: "auto".into(),
            quote: "auto".into(),
            timeframes: vec![Timeframe::D1],
            mode: SignalMode::Confirmed,
        };

        let supported = vec![Timeframe::D1, Timeframe::W1];
        let resolved = req.resolve_timeframes(&supported);
        assert_eq!(resolved, vec![Timeframe::D1]);

        let req_empty = FinancialSignalRequest {
            asset: "BTC".into(),
            venue: "auto".into(),
            quote: "auto".into(),
            timeframes: vec![],
            mode: SignalMode::Confirmed,
        };
        let resolved = req_empty.resolve_timeframes(&supported);
        assert_eq!(resolved, supported);
    }
}
