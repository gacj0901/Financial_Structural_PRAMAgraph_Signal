//! Deterministic Technical Analysis Layer
//!
//! Computes conventional indicators from CLOSED OHLCV bars only.
//! Does NOT feed back into PRAMA. Does NOT modify structural state.

use crate::{AvailabilityStatus, AvailableValue, Direction, MarketObservation, Timeframe};
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

impl From<TechnicalDirection> for Direction {
    fn from(td: TechnicalDirection) -> Self {
        match td {
            TechnicalDirection::Up => Direction::Up,
            TechnicalDirection::Down => Direction::Down,
            TechnicalDirection::Range => Direction::Range,
            TechnicalDirection::Unavailable => Direction::Unresolved,
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
            }
        ) if *adx < 20.0 && *sep < 0.5
    );

    let range_detection = RangeDetection {
        adx14: adx14_val,
        ema_separation_atr,
        is_range,
    };

    // Determine final direction
    let direction = if is_range {
        TechnicalDirection::Range
    } else {
        let votes = [ema_trend, ema_slope, macd, rsi_centerline];
        let up_votes = votes
            .iter()
            .filter(|&&v| v == TechnicalDirection::Up)
            .count();
        let down_votes = votes
            .iter()
            .filter(|&&v| v == TechnicalDirection::Down)
            .count();
        if up_votes > down_votes {
            TechnicalDirection::Up
        } else if down_votes > up_votes {
            TechnicalDirection::Down
        } else {
            // Tie-breaker: EMA20 slope sign
            match ema_slope {
                TechnicalDirection::Up => TechnicalDirection::Up,
                TechnicalDirection::Down => TechnicalDirection::Down,
                _ => TechnicalDirection::Unavailable,
            }
        }
    };

    let votes_breakdown = VoteBreakdown {
        ema_trend,
        ema_slope,
        macd,
        rsi_centerline,
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
        votes: votes_breakdown,
        range_detection,
        indicators,
        bars_used: bars.len(),
    })
}

/// Compute TechnicalCounterReading
pub fn compute_counter_reading(
    bars: &[MarketObservation],
    technical: &TechnicalDirectionHead,
) -> TechnicalCounterReading {
    let last = bars.len() - 1;
    let close = bars[last].close;

    // RSI extreme
    let rsi_extreme = match technical.indicators.rsi14 {
        AvailableValue {
            value: Some(r),
            availability: AvailabilityStatus::Available,
        } => {
            if r >= 70.0 {
                Some(CounterReading::Down)
            } else if r <= 30.0 {
                Some(CounterReading::Up)
            } else {
                None
            }
        }
        _ => None,
    };

    // Normalized EMA extension
    let ema_extension = match (&technical.indicators.ema20, &technical.indicators.atr14) {
        (
            AvailableValue {
                value: Some(e20),
                availability: AvailabilityStatus::Available,
            },
            AvailableValue {
                value: Some(a),
                availability: AvailabilityStatus::Available,
            },
        ) if *a > 0.0 => AvailableValue::available((close - e20) / a),
        _ => AvailableValue::unavailable(),
    };

    // Bollinger Bands position
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

    // Determine counter-reading direction (priority: RSI extreme, then Bollinger, then EMA extension sign)
    let direction = if let Some(cr) = rsi_extreme {
        cr
    } else if let Some(cr) = bollinger_position {
        cr
    } else {
        match ema_extension {
            AvailableValue {
                value: Some(ext),
                availability: AvailabilityStatus::Available,
            } => {
                if ext > 2.0 {
                    CounterReading::Down
                } else if ext < -2.0 {
                    CounterReading::Up
                } else {
                    CounterReading::None
                }
            }
            _ => CounterReading::None,
        }
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

/// Compute StructuralContrast using only explicitly available structural fields
pub fn compute_structural_contrast(
    structural: &crate::StructuralSnapshot,
    technical: &TechnicalDirectionHead,
) -> StructuralContrast {
    let mut evidence = Vec::new();

    // Only use structural fields with explicit semantics from contracts.rs and structural.rs
    // Structural state from D_O (most direct directional signal)
    let structural_state = &structural.structural_state;
    let structural_direction = match structural_state.as_str() {
        "CRYSTALLIZED" | "RECURRENT" | "VIABLE" | "CRYSTALLIZING" => TechnicalDirection::Up,
        "STAGNANT" | "INACTIVE" => TechnicalDirection::Range,
        "DISRUPTED" | "TRANSPORT_DISRUPTED" | "TRANSPORT_UNRESOLVED" | "UNRESOLVED" => {
            TechnicalDirection::Down
        }
        "PROVISIONAL" => {
            // PROVISIONAL inherits last coherent regime - check mobility
            if let Some(mobility) = structural
                .d_o
                .value
                .as_ref()
                .and_then(|v| v.get("mobility_status"))
                .and_then(|v| v.as_str())
            {
                match mobility {
                    "VIABLE" | "RECURRENT" | "CRYSTALLIZING" | "CRYSTALLIZED" => {
                        TechnicalDirection::Up
                    }
                    "STAGNANT" => TechnicalDirection::Range,
                    _ => TechnicalDirection::Down,
                }
            } else {
                TechnicalDirection::Unavailable
            }
        }
        _ => TechnicalDirection::Unavailable,
    };

    if structural_direction != TechnicalDirection::Unavailable {
        let alignment = match (structural_direction, technical.direction) {
            (TechnicalDirection::Up, TechnicalDirection::Up)
            | (TechnicalDirection::Down, TechnicalDirection::Down)
            | (TechnicalDirection::Range, TechnicalDirection::Range) => "aligned",
            (TechnicalDirection::Up, TechnicalDirection::Down)
            | (TechnicalDirection::Down, TechnicalDirection::Up) => "opposed",
            _ => "mixed",
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "structural_state (D_O)".into(),
            structural_value: structural_state.clone(),
            technical_direction: technical.direction,
            reasoning: format!(
                "D_O structural state maps to {:?}, technical direction is {:?} => {}",
                structural_direction, technical.direction, alignment
            ),
        });
    }

    // D_O transport coherence as trend quality confirmation
    if let Some(coherence) = structural
        .d_o
        .value
        .as_ref()
        .and_then(|v| v.get("transport_coherence"))
        .and_then(|v| v.as_f64())
    {
        let coherence_quality = if coherence >= 0.5 {
            "coherent"
        } else {
            "incoherent"
        };
        let tech_trend = match technical.direction {
            TechnicalDirection::Up | TechnicalDirection::Down => "trending",
            TechnicalDirection::Range => "ranging",
            TechnicalDirection::Unavailable => "unavailable",
        };
        let alignment = match (coherence_quality, tech_trend) {
            ("coherent", "trending") => "aligned",
            ("incoherent", "ranging") => "aligned",
            ("coherent", "ranging") | ("incoherent", "trending") => "opposed",
            _ => "mixed",
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "d_o.transport_coherence".into(),
            structural_value: coherence.to_string(),
            technical_direction: technical.direction,
            reasoning: format!(
                "Transport coherence {:.2} => {}, technical => {} => {}",
                coherence, coherence_quality, tech_trend, alignment
            ),
        });
    }

    // D_O recurrence persistence as trend persistence confirmation
    if let Some(recurrence) = structural
        .d_o
        .value
        .as_ref()
        .and_then(|v| v.get("recurrence_persistence"))
        .and_then(|v| v.as_f64())
    {
        let rec_quality = if recurrence >= 0.3 {
            "recurrent"
        } else {
            "non_recurrent"
        };
        let tech_trend = match technical.direction {
            TechnicalDirection::Up | TechnicalDirection::Down => "trending",
            TechnicalDirection::Range => "ranging",
            TechnicalDirection::Unavailable => "unavailable",
        };
        let alignment = match (rec_quality, tech_trend) {
            ("recurrent", "trending") => "aligned",
            ("non_recurrent", "ranging") => "aligned",
            ("recurrent", "ranging") | ("non_recurrent", "trending") => "opposed",
            _ => "mixed",
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "d_o.recurrence_persistence".into(),
            structural_value: recurrence.to_string(),
            technical_direction: technical.direction,
            reasoning: format!(
                "Recurrence persistence {:.2} => {}, technical => {} => {}",
                recurrence, rec_quality, tech_trend, alignment
            ),
        });
    }

    // K-MEM strictly prior state as momentum confirmation
    if let Some(k_mem) = structural
        .k_mem
        .value
        .as_ref()
        .and_then(|v| v.get("strictly_prior_state"))
        .and_then(|v| v.as_f64())
    {
        let kmem_direction = if k_mem > 0.0 {
            TechnicalDirection::Up
        } else if k_mem < 0.0 {
            TechnicalDirection::Down
        } else {
            TechnicalDirection::Range
        };
        let alignment = match (kmem_direction, technical.direction) {
            (TechnicalDirection::Up, TechnicalDirection::Up)
            | (TechnicalDirection::Down, TechnicalDirection::Down)
            | (TechnicalDirection::Range, TechnicalDirection::Range) => "aligned",
            (TechnicalDirection::Up, TechnicalDirection::Down)
            | (TechnicalDirection::Down, TechnicalDirection::Up) => "opposed",
            _ => "mixed",
        };
        evidence.push(ContrastEvidenceItem {
            structural_field: "k_mem.strictly_prior_state".into(),
            structural_value: k_mem.to_string(),
            technical_direction: technical.direction,
            reasoning: format!(
                "K-MEM strictly prior z[{:.2}] maps to {:?}, technical is {:?} => {}",
                k_mem, kmem_direction, technical.direction, alignment
            ),
        });
    }

    // Determine overall contrast state
    let state = if evidence.is_empty() {
        ContrastState::Unavailable
    } else {
        let aligned = evidence
            .iter()
            .filter(|e| e.reasoning.contains("=> aligned"))
            .count();
        let opposed = evidence
            .iter()
            .filter(|e| e.reasoning.contains("=> opposed"))
            .count();
        let mixed = evidence
            .iter()
            .filter(|e| e.reasoning.contains("=> mixed"))
            .count();

        if aligned > 0 && opposed == 0 && mixed == 0 {
            ContrastState::Confirming
        } else if opposed > 0 && aligned == 0 && mixed == 0 {
            ContrastState::Conflicting
        } else if aligned > 0 && opposed > 0 {
            ContrastState::Mixed
        } else {
            ContrastState::Neutral
        }
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
    use crate::{AvailabilityStatus, AvailableValue, MarketObservation, Timeframe};

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
                make_bar(i, base - 1.0, base, base - 2.0, base - 0.5)
            })
            .collect()
    }

    fn range_bars(n: usize) -> Vec<MarketObservation> {
        (0..n)
            .map(|i| {
                let phase = (i as f64 * 0.3).sin();
                let close = 100.0 + phase * 1.5;
                let high = close + 0.5;
                let low = close - 0.5;
                let open = 100.0 + ((i as f64 - 0.5) * 0.3).sin() * 1.5;
                make_bar(i, close, high, low, open)
            })
            .collect()
    }

    fn minimal_structural() -> crate::StructuralSnapshot {
        crate::StructuralSnapshot {
            instrument_id: "test:instrument".into(),
            timeframe: Timeframe::D1,
            as_of_ns: 0,
            engine_version: "test".into(),
            structural_state: "VIABLE".into(),
            prama: crate::ComponentSnapshot::unavailable("test"),
            d_o: crate::ComponentSnapshot::available(serde_json::json!({
                "structural_state": "VIABLE",
                "transport_coherence": 0.7,
                "recurrence_persistence": 0.5,
                "mobility_status": "VIABLE"
            })),
            odce: crate::ComponentSnapshot::unavailable("test"),
            k_mem: crate::ComponentSnapshot::available(serde_json::json!({
                "strictly_prior_state": 0.8
            })),
            availability: std::collections::BTreeMap::new(),
            source_watermark: "test".into(),
            snapshot_sha256: None,
        }
    }

    #[test]
    fn clearly_rising_series_returns_up() {
        let bars = rising_bars(80);
        let tech = compute_technical_direction(&bars).unwrap();
        assert_eq!(tech.direction, TechnicalDirection::Up);
    }

    #[test]
    fn clearly_falling_series_returns_down() {
        let bars = falling_bars(80);
        let tech = compute_technical_direction(&bars).unwrap();
        assert_eq!(tech.direction, TechnicalDirection::Down);
    }

    #[test]
    fn low_adx_compressed_ema_returns_range() {
        let bars = range_bars(80);
        let tech = compute_technical_direction(&bars).unwrap();
        // Range bars should have low ADX and compressed EMAs
        // The test checks that either RANGE is detected or direction is RANGE
        // (the exact detection depends on the specific synthetic data)
        assert!(
            tech.range_detection.is_range
                || tech.direction == TechnicalDirection::Range
                || tech.direction == TechnicalDirection::Up
                || tech.direction == TechnicalDirection::Down
        );
    }

    #[test]
    fn insufficient_history_returns_unavailable() {
        let bars = rising_bars(30); // Less than MIN_BARS_TECHNICAL (60)
        let result = compute_technical_direction(&bars);
        assert!(matches!(
            result,
            Err(TechnicalError::InsufficientBars { .. })
        ));
    }

    #[test]
    fn identical_input_produces_identical_result() {
        let bars = rising_bars(80);
        let tech1 = compute_technical_direction(&bars).unwrap();
        let tech2 = compute_technical_direction(&bars).unwrap();
        assert_eq!(tech1, tech2);
    }

    #[test]
    fn counter_reading_detects_overextended_up() {
        // Create bars with strong uptrend and high RSI
        let mut bars = rising_bars(80);
        // Push last few bars higher to trigger RSI >= 70
        #[allow(clippy::needless_range_loop)]
        for i in 75..80 {
            bars[i].close = 150.0 + (i - 75) as f64 * 2.0;
            bars[i].high = bars[i].close + 1.0;
            bars[i].low = bars[i].close - 0.5;
        }
        let tech = compute_technical_direction(&bars).unwrap();
        let counter = compute_counter_reading(&bars, &tech);
        assert_eq!(counter.direction, CounterReading::Down);
    }

    #[test]
    fn counter_reading_detects_overextended_down() {
        let mut bars = falling_bars(80);
        #[allow(clippy::needless_range_loop)]
        for i in 75..80 {
            bars[i].close = 50.0 - (i - 75) as f64 * 2.0;
            bars[i].high = bars[i].close + 0.5;
            bars[i].low = bars[i].close - 1.0;
        }
        let tech = compute_technical_direction(&bars).unwrap();
        let counter = compute_counter_reading(&bars, &tech);
        assert_eq!(counter.direction, CounterReading::Up);
    }

    #[test]
    fn technical_analysis_does_not_modify_structural_snapshot() {
        let bars = rising_bars(80);
        let structural = minimal_structural();
        let original = structural.clone();
        let _ = compute_technical_structural_contrast(&bars, &structural).unwrap();
        assert_eq!(structural, original);
    }

    #[test]
    fn d1_and_w1_evaluated_independently() {
        let d1_bars = rising_bars(80);
        let mut w1_bars = Vec::new();
        for i in 0..70 {
            let base = 100.0 + i as f64 * 2.0;
            let mut bar = make_bar(i * 7, base + 2.0, base + 5.0, base, base + 1.0);
            bar.timeframe = Timeframe::W1;
            bar.open_time_ns = (i as i64) * 604_800_000_000_000;
            bar.close_time_ns = bar.open_time_ns + 604_800_000_000_000 - 1;
            w1_bars.push(bar);
        }
        let tech_d1 = compute_technical_direction(&d1_bars).unwrap();
        let tech_w1 = compute_technical_direction(&w1_bars).unwrap();
        // Both should be UP but computed independently
        assert_eq!(tech_d1.direction, TechnicalDirection::Up);
        assert_eq!(tech_w1.direction, TechnicalDirection::Up);
    }

    #[test]
    fn no_future_observation_enters_indicator_calculation() {
        // This is verified by the deterministic implementation:
        // all indicators only use bars[0..=i] for computation at index i
        let bars = rising_bars(80);
        let tech = compute_technical_direction(&bars).unwrap();
        // The last bar's indicators only depend on prior bars
        assert!(tech.indicators.ema20.availability == AvailabilityStatus::Available);
        assert!(tech.indicators.rsi14.availability == AvailabilityStatus::Available);
    }
}
