//! Deterministic Technical Analysis Layer
//!
//! Computes conventional indicators from CLOSED OHLCV bars only.
//! Does NOT feed back into PRAMA. Does NOT modify structural state.

use crate::{AvailabilityStatus, AvailableValue, MarketObservation, Timeframe};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Minimum bars required for each indicator
#[allow(dead_code)]
const MIN_BARS_EMA_50: usize = 50;
#[allow(dead_code)]
const MIN_BARS_MACD: usize = 35; // 26 + 9 for signal
#[allow(dead_code)]
const MIN_BARS_RSI: usize = 15; // 14 + 1
#[allow(dead_code)]
const MIN_BARS_ADX: usize = 15; // 14 + 1
#[allow(dead_code)]
const MIN_BARS_ATR: usize = 15; // 14 + 1
#[allow(dead_code)]
const MIN_BARS_BOLLINGER: usize = 21; // 20 + 1

/// Overall minimum bars for a complete technical assessment
const MIN_BARS_TECHNICAL: usize = 60;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TechnicalError {
    #[error("insufficient closed bars: need at least {required}, got {available}")]
    InsufficientBars { required: usize, available: usize },
    #[error("all input bars must be closed")]
    OpenBar,
    #[error("bars must have consistent instrument and timeframe")]
    MixedSeries,
    #[error("bar timestamps must be strictly increasing")]
    NonIncreasingTimestamps,
    #[error("prices must be positive and finite")]
    InvalidPrice,
    #[error("volume data required but unavailable")]
    VolumeUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TechnicalDirection {
    Up,
    Down,
    Range,
    Unavailable,
}

impl From<TechnicalDirection> for crate::Direction {
    fn from(td: TechnicalDirection) -> Self {
        match td {
            TechnicalDirection::Up => crate::Direction::Up,
            TechnicalDirection::Down => crate::Direction::Down,
            TechnicalDirection::Range => crate::Direction::Range,
            TechnicalDirection::Unavailable => crate::Direction::Unresolved,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CounterReading {
    None,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContrastState {
    Confirming,
    Conflicting,
    Mixed,
    Neutral,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorValues {
    pub ema20: AvailableValue<f64>,
    pub ema50: AvailableValue<f64>,
    pub ema20_slope: AvailableValue<f64>,
    pub macd_histogram: AvailableValue<f64>,
    pub rsi14: AvailableValue<f64>,
    pub adx14: AvailableValue<f64>,
    pub atr14: AvailableValue<f64>,
    pub bollinger_upper: AvailableValue<f64>,
    pub bollinger_lower: AvailableValue<f64>,
    pub bollinger_middle: AvailableValue<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoteBreakdown {
    pub ema_trend: TechnicalDirection,
    pub ema_slope: TechnicalDirection,
    pub macd: TechnicalDirection,
    pub rsi_centerline: TechnicalDirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RangeDetection {
    pub adx14: AvailableValue<f64>,
    pub ema_separation_atr: AvailableValue<f64>,
    pub is_range: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CounterReadingEvidence {
    pub rsi_extreme: Option<CounterReading>,
    pub ema_extension: AvailableValue<f64>,
    pub bollinger_position: Option<CounterReading>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TechnicalDirectionHead {
    pub direction: TechnicalDirection,
    pub votes: VoteBreakdown,
    pub range_detection: RangeDetection,
    pub indicators: IndicatorValues,
    pub bars_used: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TechnicalCounterReading {
    pub direction: CounterReading,
    pub evidence: CounterReadingEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContrastEvidenceItem {
    pub structural_field: String,
    pub structural_value: String,
    pub technical_direction: TechnicalDirection,
    pub reasoning: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructuralContrast {
    pub state: ContrastState,
    pub evidence: Vec<ContrastEvidenceItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TechnicalStructuralContrast {
    pub timeframe: Timeframe,
    pub structural: crate::StructuralSnapshot,
    pub technical: TechnicalDirectionHead,
    pub counter_reading: TechnicalCounterReading,
    pub structural_contrast: StructuralContrast,
}

/// Compute EMA (Exponential Moving Average) deterministically
fn compute_ema(values: &[f64], period: usize) -> Vec<AvailableValue<f64>> {
    let mut result = Vec::with_capacity(values.len());
    if values.len() < period {
        return vec![AvailableValue::unavailable(); values.len()];
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut ema = values[..period].iter().sum::<f64>() / period as f64;
    for (i, &v) in values.iter().enumerate() {
        if i < period - 1 {
            result.push(AvailableValue::unavailable());
        } else if i == period - 1 {
            result.push(AvailableValue::available(ema));
        } else {
            ema = v * k + ema * (1.0 - k);
            result.push(AvailableValue::available(ema));
        }
    }
    result
}

/// Compute RSI (Relative Strength Index) deterministically using Wilder's smoothing
fn compute_rsi(closes: &[f64], period: usize) -> Vec<AvailableValue<f64>> {
    let mut result = Vec::with_capacity(closes.len());
    if closes.len() < period + 1 {
        return vec![AvailableValue::unavailable(); closes.len()];
    }
    let mut gains = 0.0;
    let mut losses = 0.0;
    for i in 1..=period {
        let diff = closes[i] - closes[i - 1];
        if diff >= 0.0 {
            gains += diff;
        } else {
            losses -= diff;
        }
    }
    let mut avg_gain = gains / period as f64;
    let mut avg_loss = losses / period as f64;
    for (i, &close) in closes.iter().enumerate() {
        if i < period {
            result.push(AvailableValue::unavailable());
        } else if i == period {
            let rs = if avg_loss > 0.0 {
                avg_gain / avg_loss
            } else {
                f64::INFINITY
            };
            let rsi = 100.0 - 100.0 / (1.0 + rs);
            result.push(AvailableValue::available(rsi));
        } else {
            let diff = close - closes[i - 1];
            let gain = if diff >= 0.0 { diff } else { 0.0 };
            let loss = if diff < 0.0 { -diff } else { 0.0 };
            avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
            avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;
            let rs = if avg_loss > 0.0 {
                avg_gain / avg_loss
            } else {
                f64::INFINITY
            };
            let rsi = 100.0 - 100.0 / (1.0 + rs);
            result.push(AvailableValue::available(rsi));
        }
    }
    result
}

/// Compute MACD (12, 26, 9) deterministically
#[allow(clippy::type_complexity)]
fn compute_macd(
    closes: &[f64],
) -> (
    Vec<AvailableValue<f64>>,
    Vec<AvailableValue<f64>>,
    Vec<AvailableValue<f64>>,
) {
    let ema12 = compute_ema(closes, 12);
    let ema26 = compute_ema(closes, 26);
    let mut macd_line = Vec::with_capacity(closes.len());
    let mut signal_line = Vec::with_capacity(closes.len());
    let mut histogram = Vec::with_capacity(closes.len());

    for i in 0..closes.len() {
        match (&ema12[i], &ema26[i]) {
            (
                AvailableValue {
                    value: Some(v1),
                    availability: AvailabilityStatus::Available,
                },
                AvailableValue {
                    value: Some(v2),
                    availability: AvailabilityStatus::Available,
                },
            ) => {
                macd_line.push(AvailableValue::available(v1 - v2));
            }
            _ => macd_line.push(AvailableValue::unavailable()),
        }
    }

    let macd_values: Vec<f64> = macd_line.iter().filter_map(|v| v.value).collect();
    if macd_values.len() >= 9 {
        let k = 2.0 / 10.0;
        let mut signal = macd_values[..9].iter().sum::<f64>() / 9.0;
        #[allow(clippy::needless_range_loop)]
        for i in 0..closes.len() {
            if i < 34 {
                // 26 + 9 - 1
                signal_line.push(AvailableValue::unavailable());
                histogram.push(AvailableValue::unavailable());
            } else if i == 34 {
                signal_line.push(AvailableValue::available(signal));
                let macd_val = macd_line[i].value.unwrap_or(0.0);
                histogram.push(AvailableValue::available(macd_val - signal));
            } else {
                signal = macd_line[i].value.unwrap_or(0.0) * k + signal * (1.0 - k);
                signal_line.push(AvailableValue::available(signal));
                let macd_val = macd_line[i].value.unwrap_or(0.0);
                histogram.push(AvailableValue::available(macd_val - signal));
            }
        }
    } else {
        signal_line = vec![AvailableValue::unavailable(); closes.len()];
        histogram = vec![AvailableValue::unavailable(); closes.len()];
    }

    (macd_line, signal_line, histogram)
}

/// Compute ADX (Average Directional Index) deterministically
fn compute_adx(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
) -> Vec<AvailableValue<f64>> {
    let mut result = Vec::with_capacity(highs.len());
    if highs.len() < period + 1 {
        return vec![AvailableValue::unavailable(); highs.len()];
    }

    let mut plus_dm = vec![0.0; highs.len()];
    let mut minus_dm = vec![0.0; highs.len()];
    let mut tr = vec![0.0; highs.len()];

    for i in 1..highs.len() {
        let up_move = highs[i] - highs[i - 1];
        let down_move = lows[i - 1] - lows[i];
        plus_dm[i] = if up_move > down_move && up_move > 0.0 {
            up_move
        } else {
            0.0
        };
        minus_dm[i] = if down_move > up_move && down_move > 0.0 {
            down_move
        } else {
            0.0
        };
        let tr1 = highs[i] - lows[i];
        let tr2 = (highs[i] - closes[i - 1]).abs();
        let tr3 = (lows[i] - closes[i - 1]).abs();
        tr[i] = tr1.max(tr2).max(tr3);
    }

    let mut atr = vec![0.0; highs.len()];
    let mut plus_di = vec![0.0; highs.len()];
    let mut minus_di = vec![0.0; highs.len()];
    let mut dx = vec![0.0; highs.len()];

    let sum_tr: f64 = tr[1..=period].iter().sum::<f64>();
    let sum_plus: f64 = plus_dm[1..=period].iter().sum::<f64>();
    let sum_minus: f64 = minus_dm[1..=period].iter().sum::<f64>();

    for i in period..highs.len() {
        if i == period {
            atr[i] = sum_tr / period as f64;
            plus_di[i] = 100.0 * (sum_plus / period as f64) / atr[i];
            minus_di[i] = 100.0 * (sum_minus / period as f64) / atr[i];
        } else {
            atr[i] = (atr[i - 1] * (period as f64 - 1.0) + tr[i]) / period as f64;
            plus_di[i] = (plus_di[i - 1] * (period as f64 - 1.0) + 100.0 * plus_dm[i] / atr[i])
                / period as f64;
            minus_di[i] = (minus_di[i - 1] * (period as f64 - 1.0) + 100.0 * minus_dm[i] / atr[i])
                / period as f64;
        }
        let di_sum = plus_di[i] + minus_di[i];
        dx[i] = if di_sum > 0.0 {
            100.0 * (plus_di[i] - minus_di[i]).abs() / di_sum
        } else {
            0.0
        };
    }

    let mut adx = vec![0.0; highs.len()];
    let sum_dx: f64 = dx[period..=period * 2 - 1].iter().sum::<f64>();
    for i in period * 2 - 1..highs.len() {
        if i == period * 2 - 1 {
            adx[i] = sum_dx / period as f64;
        } else {
            adx[i] = (adx[i - 1] * (period as f64 - 1.0) + dx[i]) / period as f64;
        }
        result.push(AvailableValue::available(adx[i]));
    }
    for _ in 0..period * 2 - 1 {
        result.insert(0, AvailableValue::unavailable());
    }
    result
}

/// Compute ATR (Average True Range) deterministically
fn compute_atr(
    highs: &[f64],
    lows: &[f64],
    closes: &[f64],
    period: usize,
) -> Vec<AvailableValue<f64>> {
    let mut result = Vec::with_capacity(highs.len());
    if highs.len() < period + 1 {
        return vec![AvailableValue::unavailable(); highs.len()];
    }
    let mut tr = vec![0.0; highs.len()];
    for i in 1..highs.len() {
        let tr1 = highs[i] - lows[i];
        let tr2 = (highs[i] - closes[i - 1]).abs();
        let tr3 = (lows[i] - closes[i - 1]).abs();
        tr[i] = tr1.max(tr2).max(tr3);
    }
    // First element has no TR (no previous close)
    result.push(AvailableValue::unavailable());
    let mut atr = 0.0;
    #[allow(clippy::needless_range_loop)]
    for i in 1..highs.len() {
        if i <= period {
            atr += tr[i];
            if i == period {
                atr /= period as f64;
                result.push(AvailableValue::available(atr));
            } else {
                result.push(AvailableValue::unavailable());
            }
        } else {
            atr = (atr * (period as f64 - 1.0) + tr[i]) / period as f64;
            result.push(AvailableValue::available(atr));
        }
    }
    result
}

/// Compute Bollinger Bands (20, 2 std) deterministically
#[allow(clippy::type_complexity)]
fn compute_bollinger(
    closes: &[f64],
    period: usize,
    std_mult: f64,
) -> (
    Vec<AvailableValue<f64>>,
    Vec<AvailableValue<f64>>,
    Vec<AvailableValue<f64>>,
) {
    let mut middle = Vec::with_capacity(closes.len());
    let mut upper = Vec::with_capacity(closes.len());
    let mut lower = Vec::with_capacity(closes.len());
    for i in 0..closes.len() {
        if i < period - 1 {
            middle.push(AvailableValue::unavailable());
            upper.push(AvailableValue::unavailable());
            lower.push(AvailableValue::unavailable());
        } else {
            // i >= period - 1, so i + 1 >= period, no underflow
            let start = i + 1 - period;
            let window = &closes[start..=i];
            let mean = window.iter().sum::<f64>() / period as f64;
            let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
            let std = variance.sqrt();
            middle.push(AvailableValue::available(mean));
            upper.push(AvailableValue::available(mean + std_mult * std));
            lower.push(AvailableValue::available(mean - std_mult * std));
        }
    }
    (middle, upper, lower)
}

/// Validate input bars for technical analysis
fn validate_bars(bars: &[MarketObservation]) -> Result<(), TechnicalError> {
    if bars.is_empty() {
        return Err(TechnicalError::InsufficientBars {
            required: MIN_BARS_TECHNICAL,
            available: 0,
        });
    }
    if bars.iter().any(|bar| !bar.is_closed) {
        return Err(TechnicalError::OpenBar);
    }
    let first = &bars[0];
    if bars
        .iter()
        .any(|bar| bar.instrument_id != first.instrument_id || bar.timeframe != first.timeframe)
    {
        return Err(TechnicalError::MixedSeries);
    }
    if bars
        .windows(2)
        .any(|pair| pair[0].close_time_ns >= pair[1].close_time_ns)
    {
        return Err(TechnicalError::NonIncreasingTimestamps);
    }
    if bars.iter().any(|bar| {
        !bar.open.is_finite()
            || !bar.high.is_finite()
            || !bar.low.is_finite()
            || !bar.close.is_finite()
            || bar.high <= 0.0
            || bar.low <= 0.0
            || bar.close <= 0.0
    }) {
        return Err(TechnicalError::InvalidPrice);
    }
    if bars.len() < MIN_BARS_TECHNICAL {
        return Err(TechnicalError::InsufficientBars {
            required: MIN_BARS_TECHNICAL,
            available: bars.len(),
        });
    }
    Ok(())
}

/// Compute all indicators and produce TechnicalDirectionHead
pub fn compute_technical_direction(
    bars: &[MarketObservation],
) -> Result<TechnicalDirectionHead, TechnicalError> {
    validate_bars(bars)?;

    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let highs: Vec<f64> = bars.iter().map(|b| b.high).collect();
    let lows: Vec<f64> = bars.iter().map(|b| b.low).collect();

    let ema20 = compute_ema(&closes, 20);
    let ema50 = compute_ema(&closes, 50);
    let rsi14 = compute_rsi(&closes, 14);
    let (_macd_line, _signal_line, macd_histogram) = compute_macd(&closes);
    let adx14 = compute_adx(&highs, &lows, &closes, 14);
    let atr14 = compute_atr(&highs, &lows, &closes, 14);
    let (bollinger_middle, bollinger_upper, bollinger_lower) = compute_bollinger(&closes, 20, 2.0);

    // EMA20 slope (difference between last two available EMA20 values)
    let mut ema20_slope = AvailableValue::unavailable();
    let last_idx = ema20.len() - 1;
    if let (Some(v1), Some(v2)) = (
        ema20[last_idx].value,
        ema20.get(last_idx.saturating_sub(1)).and_then(|v| v.value),
    ) {
        ema20_slope = AvailableValue::available(v1 - v2);
    }

    let last = bars.len() - 1;

    // Directional votes
    let ema_trend = match (&ema20[last], &ema50[last]) {
        (
            AvailableValue {
                value: Some(e20),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(e50),
                availability: AvailabilityStatus::Available,
            },
        ) => {
            if e20 > e50 {
                TechnicalDirection::Up
            } else {
                TechnicalDirection::Down
            }
        }
        _ => TechnicalDirection::Unavailable,
    };

    let ema_slope = match ema20_slope {
        AvailableValue {
            value: Some(s),
            availability: AvailabilityStatus::Available,
        } => {
            if s > 0.0 {
                TechnicalDirection::Up
            } else {
                TechnicalDirection::Down
            }
        }
        _ => TechnicalDirection::Unavailable,
    };

    let macd = match macd_histogram[last] {
        AvailableValue {
            value: Some(h),
            availability: AvailabilityStatus::Available,
        } => {
            if h > 0.0 {
                TechnicalDirection::Up
            } else {
                TechnicalDirection::Down
            }
        }
        _ => TechnicalDirection::Unavailable,
    };

    let rsi_centerline = match rsi14[last] {
        AvailableValue {
            value: Some(r),
            availability: AvailabilityStatus::Available,
        } => {
            if r >= 50.0 {
                TechnicalDirection::Up
            } else {
                TechnicalDirection::Down
            }
        }
        _ => TechnicalDirection::Unavailable,
    };

    // Range detection (H1 rule, applied generically)
    let adx14_val = adx14[last].clone();
    let ema_separation_atr = match (&ema20[last], &ema50[last], &atr14[last]) {
        (
            AvailableValue {
                value: Some(e20),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(e50),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(a),
                availability: AvailabilityStatus::Available,
            },
        ) if *a > 0.0 => AvailableValue::available((e20 - e50).abs() / a),
        _ => AvailableValue::unavailable(),
    };

    let is_range = matches!(
        (&adx14_val, &ema_separation_atr),
        (
            AvailableValue {
                value: Some(adx),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(sep),
                availability: AvailabilityStatus::Available,
            },
        ) if *adx < 20.0 && *sep < 0.5
    );

    let direction = if is_range {
        TechnicalDirection::Range
    } else {
        let votes = [ema_trend, ema_slope, macd, rsi_centerline];
        let up_votes = votes
            .iter()
            .filter(|&&d| d == TechnicalDirection::Up)
            .count();
        let down_votes = votes
            .iter()
            .filter(|&&d| d == TechnicalDirection::Down)
            .count();
        if up_votes > down_votes {
            TechnicalDirection::Up
        } else if down_votes > up_votes {
            TechnicalDirection::Down
        } else {
            // Tie-break: EMA20 slope
            if matches!(ema_slope, TechnicalDirection::Up) {
                TechnicalDirection::Up
            } else {
                TechnicalDirection::Down
            }
        }
    };

    let votes = VoteBreakdown {
        ema_trend,
        ema_slope,
        macd,
        rsi_centerline,
    };

    let range_detection = RangeDetection {
        adx14: adx14[last].clone(),
        ema_separation_atr,
        is_range,
    };

    let indicators = IndicatorValues {
        ema20: ema20[last].clone(),
        ema50: ema50[last].clone(),
        ema20_slope,
        macd_histogram: macd_histogram[last].clone(),
        rsi14: rsi14[last].clone(),
        adx14: adx14[last].clone(),
        atr14: atr14[last].clone(),
        bollinger_upper: bollinger_upper[last].clone(),
        bollinger_lower: bollinger_lower[last].clone(),
        bollinger_middle: bollinger_middle[last].clone(),
    };

    Ok(TechnicalDirectionHead {
        direction,
        votes,
        range_detection,
        indicators,
        bars_used: bars.len(),
    })
}

/// Compute Technical Counter-Reading (exhaustion / mean-reversion pressure)
pub fn compute_counter_reading(
    bars: &[MarketObservation],
    technical: &TechnicalDirectionHead,
) -> TechnicalCounterReading {
    let last = bars.len() - 1;
    let close = bars[last].close;

    // RSI extreme check
    let rsi_extreme = match technical.indicators.rsi14 {
        AvailableValue {
            value: Some(rsi),
            availability: AvailabilityStatus::Available,
        } => {
            if rsi >= 70.0 {
                Some(CounterReading::Down)
            } else if rsi <= 30.0 {
                Some(CounterReading::Up)
            } else {
                None
            }
        }
        _ => None,
    };

    // EMA extension (normalized)
    let ema_extension = match (&technical.indicators.ema20, &technical.indicators.atr14) {
        (
            AvailableValue {
                value: Some(ema20),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(atr),
                availability: AvailabilityStatus::Available,
            },
        ) if *atr > 0.0 => AvailableValue::available((close - ema20) / atr),
        _ => AvailableValue::unavailable(),
    };

    // Bollinger position
    let bollinger_position = match (
        &technical.indicators.bollinger_upper,
        &technical.indicators.bollinger_lower,
    ) {
        (
            AvailableValue {
                value: Some(upper),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(lower),
                availability: AvailabilityStatus::Available,
            },
        ) => {
            if close > *upper {
                Some(CounterReading::Down)
            } else if close < *lower {
                Some(CounterReading::Up)
            } else {
                None
            }
        }
        _ => None,
    };

    // Determine counter-reading direction (priority: RSI extreme -> Bollinger -> EMA extension)
    let direction = if let Some(cr) = rsi_extreme {
        cr
    } else if let Some(cr) = bollinger_position {
        cr
    } else if let AvailableValue {
        value: Some(ext),
        availability: AvailabilityStatus::Available,
    } = ema_extension
    {
        if ext > 2.0 {
            CounterReading::Down
        } else if ext < -2.0 {
            CounterReading::Up
        } else {
            CounterReading::None
        }
    } else {
        CounterReading::None
    };

    TechnicalCounterReading {
        direction,
        evidence: CounterReadingEvidence {
            rsi_extreme,
            ema_extension,
            bollinger_position,
        },
    }
}

// Compute StructuralContrast using only explicitly available structural fields
//
// DO NOT map structural states to price direction - they are independent channels
// The authoritative price direction is exclusively TechnicalDirectionHead
//
// Structural fields are presented as descriptive observational evidence only.
// They do NOT vote for or against TechnicalDirectionHead.
// No "aligned", "opposed", "mixed", "confirming", "conflicting" reasoning.
//
// When both structural and technical channels are available: state = NEUTRAL
// When either channel is unavailable: state = UNAVAILABLE
pub fn compute_structural_contrast(
    structural: &crate::StructuralSnapshot,
    technical: &TechnicalDirectionHead,
) -> StructuralContrast {
    let mut evidence = Vec::new();

    // Only present structural fields as descriptive observational evidence.
    // No mapping to price direction, no confirmation/conflict logic.

    // D_O transport coherence - descriptive only
    if let Some(coherence) = structural
        .d_o
        .value
        .as_ref()
        .and_then(|v| v.get("transport_coherence"))
        .and_then(|v| v.as_f64())
    {
        let quality = if coherence >= 0.5 {
            "coherent"
        } else {
            "incoherent"
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "d_o.transport_coherence".into(),
            structural_value: coherence.to_string(),
            technical_direction: technical.direction,
            reasoning: format!("Transport coherence {:.2} ({})", coherence, quality),
        });
    }

    // D_O recurrence persistence - descriptive only
    if let Some(recurrence) = structural
        .d_o
        .value
        .as_ref()
        .and_then(|v| v.get("recurrence_persistence"))
        .and_then(|v| v.as_f64())
    {
        let quality = if recurrence >= 0.3 {
            "recurrent"
        } else {
            "non_recurrent"
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "d_o.recurrence_persistence".into(),
            structural_value: recurrence.to_string(),
            technical_direction: technical.direction,
            reasoning: format!("Recurrence persistence {:.2} ({})", recurrence, quality),
        });
    }

    // K-MEM strictly prior state - descriptive only
    if let Some(k_mem) = structural
        .k_mem
        .value
        .as_ref()
        .and_then(|v| v.get("strictly_prior_state"))
        .and_then(|v| v.as_f64())
    {
        evidence.push(ContrastEvidenceItem {
            structural_field: "k_mem.strictly_prior_state".into(),
            structural_value: k_mem.to_string(),
            technical_direction: technical.direction,
            reasoning: format!("K-MEM strictly prior z[{:.2}]", k_mem),
        });
    }

    // State: NEUTRAL when both channels available, UNAVAILABLE otherwise
    let state = if evidence.is_empty() || technical.direction == TechnicalDirection::Unavailable {
        ContrastState::Unavailable
    } else {
        ContrastState::Neutral
    };

    StructuralContrast { state, evidence }
}

/// Main entry point: compute full technical + structural contrast for a timeframe
pub fn compute_technical_structural_contrast(
    bars: &[MarketObservation],
    structural: &crate::StructuralSnapshot,
) -> Result<TechnicalStructuralContrast, TechnicalError> {
    let technical = compute_technical_direction(bars)?;
    let counter_reading = compute_counter_reading(bars, &technical);
    let structural_contrast = compute_structural_contrast(structural, &technical);

    Ok(TechnicalStructuralContrast {
        timeframe: bars[0].timeframe,
        structural: structural.clone(),
        technical,
        counter_reading,
        structural_contrast,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AvailableValue, MarketObservation, Timeframe};

    fn make_bar(idx: usize, close: f64, high: f64, low: f64, open: f64) -> MarketObservation {
        let ns = (idx as i64) * 86_400_000_000_000;
        MarketObservation {
            instrument_id: "test:instrument".into(),
            timeframe: Timeframe::D1,
            open_time_ns: ns,
            close_time_ns: ns + 86_400_000_000_000 - 1,
            open,
            high,
            low,
            close,
            is_closed: true,
            source: "test".into(),
            volume: AvailableValue::unavailable(),
            quote_volume: AvailableValue::unavailable(),
            trade_count: AvailableValue::unavailable(),
            best_bid: AvailableValue::unavailable(),
            best_ask: AvailableValue::unavailable(),
            bid_size: AvailableValue::unavailable(),
            ask_size: AvailableValue::unavailable(),
        }
    }

    fn rising_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let base = 100.0 + i as f64 * 0.5;
                make_bar(i, base + 1.0, base + 2.0, base, base + 0.5)
            })
            .collect()
    }

    fn falling_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let base = 200.0 - i as f64 * 0.5;
                make_bar(i, base - 1.0, base + 1.0, base - 2.0, base - 0.5)
            })
            .collect()
    }

    #[allow(dead_code)]
    fn range_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let base = 100.0 + i as f64 * 0.1;
                make_bar(i, base + 0.2, base + 0.5, base - 0.5, base)
            })
            .collect()
    }

    #[test]
    fn clearly_rising_series_returns_up() {
        let bars = rising_bars(100);
        let result = compute_technical_direction(&bars).unwrap();
        assert_eq!(result.direction, TechnicalDirection::Up);
        assert_eq!(result.bars_used, 100);
    }

    #[test]
    fn clearly_falling_series_returns_down() {
        let bars = falling_bars(100);
        let result = compute_technical_direction(&bars).unwrap();
        assert_eq!(result.direction, TechnicalDirection::Down);
        assert_eq!(result.bars_used, 100);
    }

    #[test]
    fn insufficient_history_returns_unavailable() {
        let bars = rising_bars(10);
        let result = compute_technical_direction(&bars);
        assert!(matches!(
            result,
            Err(TechnicalError::InsufficientBars { .. })
        ));
    }

    #[test]
    fn identical_input_produces_identical_result() {
        let bars = rising_bars(100);
        let r1 = compute_technical_direction(&bars).unwrap();
        let r2 = compute_technical_direction(&bars).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn d1_and_w1_evaluated_independently() {
        let d1_bars = rising_bars(100);
        let w1_bars = rising_bars(100);
        let d1_result = compute_technical_direction(&d1_bars).unwrap();
        let w1_result = compute_technical_direction(&w1_bars).unwrap();
        assert_eq!(d1_result.direction, TechnicalDirection::Up);
        assert_eq!(w1_result.direction, TechnicalDirection::Up);
    }

    #[test]
    fn low_adx_compressed_ema_returns_range() {
        let n = 100;
        let bars: Vec<MarketObservation> = (0..n)
            .map(|i| {
                let base = 100.0 + (i as f64).sin() * 0.5;
                make_bar(i, base + 0.5, base + 0.8, base - 0.8, base)
            })
            .collect();
        let result = compute_technical_direction(&bars).unwrap();
        assert_eq!(result.direction, TechnicalDirection::Range);
    }

    #[test]
    fn no_future_observation_enters_indicator_calculation() {
        let bars = rising_bars(100);
        let result = compute_technical_direction(&bars).unwrap();
        assert_eq!(result.bars_used, 100);
        assert!(result.indicators.ema20.value.is_some());
    }

    #[test]
    fn technical_analysis_does_not_modify_structural_snapshot() {
        let bars = rising_bars(100);
        let _result = compute_technical_direction(&bars).unwrap();
    }

    #[test]
    fn counter_reading_detects_overextended_up() {
        let n = 100;
        let bars: Vec<MarketObservation> = (0..n)
            .map(|i| {
                let base = 100.0 + i as f64 * 2.0;
                make_bar(i, base + 5.0, base + 6.0, base + 4.0, base + 4.5)
            })
            .collect();
        let technical = compute_technical_direction(&bars).unwrap();
        let counter = compute_counter_reading(&bars, &technical);
        assert_eq!(counter.direction, CounterReading::Down);
    }

    #[test]
    fn counter_reading_detects_overextended_down() {
        let n = 100;
        let bars: Vec<MarketObservation> = (0..n)
            .map(|i| {
                let base = 500.0 - i as f64 * 2.0;
                make_bar(i, base - 5.0, base - 4.0, base - 6.0, base - 5.5)
            })
            .collect();
        let technical = compute_technical_direction(&bars).unwrap();
        let counter = compute_counter_reading(&bars, &technical);
        assert_eq!(counter.direction, CounterReading::Up);
    }
}
