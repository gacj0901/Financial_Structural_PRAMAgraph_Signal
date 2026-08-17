//! Frozen financial Observation Interface O_D v1.
//!
//! The mapping is deliberately minimal and parameter-free:
//! `omega[t] = (high[t] - low[t]) / close[t-1]` and
//! `expected[t] = omega[t-1]`. It is dimensionless, scale invariant and
//! strictly causal. The first derived observation is kernel warm-up.

use crate::engine::KernelObservation;
use crate::{AvailableValue, MarketObservation};
use thiserror::Error;

pub const OBSERVATION_INTERFACE_VERSION: &str = "financial_observation_interface_v1";

#[derive(Debug, Error, PartialEq)]
pub enum ObservationAdapterError {
    #[error("at least three closed bars are required")]
    InsufficientBars,
    #[error("all input bars must be closed")]
    OpenBar,
    #[error("bars must have one instrument and one timeframe")]
    MixedSeries,
    #[error("bar timestamps must be strictly increasing")]
    NonIncreasingTimestamps,
    #[error("prices used by the observation interface must be positive and finite")]
    InvalidPrice,
}

pub fn adapt_closed_bars(
    bars: &[MarketObservation],
) -> Result<Vec<KernelObservation>, ObservationAdapterError> {
    if bars.len() < 3 {
        return Err(ObservationAdapterError::InsufficientBars);
    }
    if bars.iter().any(|bar| !bar.is_closed) {
        return Err(ObservationAdapterError::OpenBar);
    }
    let first = &bars[0];
    if bars
        .iter()
        .any(|bar| bar.instrument_id != first.instrument_id || bar.timeframe != first.timeframe)
    {
        return Err(ObservationAdapterError::MixedSeries);
    }
    if bars
        .windows(2)
        .any(|pair| pair[0].close_time_ns >= pair[1].close_time_ns)
    {
        return Err(ObservationAdapterError::NonIncreasingTimestamps);
    }
    if bars.iter().any(|bar| {
        !bar.high.is_finite()
            || !bar.low.is_finite()
            || !bar.close.is_finite()
            || bar.high <= 0.0
            || bar.low <= 0.0
            || bar.close <= 0.0
    }) {
        return Err(ObservationAdapterError::InvalidPrice);
    }

    let mut output = Vec::with_capacity(bars.len() - 1);
    let mut previous_omega: Option<f64> = None;
    for pair in bars.windows(2) {
        let current = &pair[1];
        let omega = (current.high - current.low) / pair[0].close;
        let expected = previous_omega.unwrap_or(f64::NAN);
        output.push(KernelObservation {
            timestamp_ns: current.close_time_ns,
            omega,
            expected,
            u_lambda: AvailableValue::not_applicable(),
            sigma_op: AvailableValue::not_applicable(),
        });
        previous_omega = Some(omega);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AvailabilityStatus, Timeframe};

    fn bar(index: i64, scale: f64) -> MarketObservation {
        MarketObservation {
            instrument_id: "test".into(),
            timeframe: Timeframe::D1,
            open_time_ns: index * 10,
            close_time_ns: index * 10 + 9,
            open: (100.0 + index as f64) * scale,
            high: (103.0 + index as f64) * scale,
            low: (98.0 + index as f64) * scale,
            close: (101.0 + index as f64) * scale,
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

    #[test]
    fn mapping_is_unit_invariant() {
        let base = vec![bar(0, 1.0), bar(1, 1.0), bar(2, 1.0), bar(3, 1.0)];
        let scaled = vec![bar(0, 100.0), bar(1, 100.0), bar(2, 100.0), bar(3, 100.0)];
        let left = adapt_closed_bars(&base).unwrap();
        let right = adapt_closed_bars(&scaled).unwrap();
        for (left, right) in left.iter().zip(right.iter()) {
            assert!((left.omega - right.omega).abs() < 1e-15);
            if left.expected.is_finite() {
                assert!((left.expected - right.expected).abs() < 1e-15);
            }
        }
    }

    #[test]
    fn mapping_has_no_lookahead() {
        let prefix = vec![bar(0, 1.0), bar(1, 1.0), bar(2, 1.0)];
        let mut extended = prefix.clone();
        let mut future = bar(3, 1.0);
        future.high = 1_000.0;
        extended.push(future);
        let before = adapt_closed_bars(&prefix).unwrap();
        let after = adapt_closed_bars(&extended).unwrap();
        for (before, after) in before.iter().zip(after.iter()) {
            assert_eq!(before.timestamp_ns, after.timestamp_ns);
            assert_eq!(before.omega, after.omega);
            assert!(
                (before.expected.is_nan() && after.expected.is_nan())
                    || before.expected == after.expected
            );
            assert_eq!(before.u_lambda, after.u_lambda);
            assert_eq!(before.sigma_op, after.sigma_op);
        }
    }

    #[test]
    fn open_bar_fails_closed() {
        let mut bars = vec![bar(0, 1.0), bar(1, 1.0), bar(2, 1.0)];
        bars[2].is_closed = false;
        assert_eq!(
            adapt_closed_bars(&bars),
            Err(ObservationAdapterError::OpenBar)
        );
    }

    #[test]
    fn absent_controls_remain_not_applicable() {
        let bars = vec![bar(0, 1.0), bar(1, 1.0), bar(2, 1.0)];
        let adapted = adapt_closed_bars(&bars).unwrap();
        assert_eq!(
            adapted[0].u_lambda.availability,
            AvailabilityStatus::NotApplicable
        );
        assert_eq!(adapted[0].sigma_op.value, None);
    }
}
