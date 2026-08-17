//! Experimental causal dynamics derived from authoritative structural frames.
//!
//! This module does not alter `financial_structural_vector_v2` or emit a
//! direction.  It only constructs nested vectors for controlled A/B/C/D
//! ablation experiments.

use crate::structural::{StructuralFrame, StructuralVector};
use chrono::{Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const DYNAMICS_EXPERIMENT_ID: &str = "financial_dynamics_ablation_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DynamicsVariant {
    A,
    B,
    C,
    D,
}

impl DynamicsVariant {
    pub const ALL: [Self; 4] = [Self::A, Self::B, Self::C, Self::D];

    pub fn label(self) -> &'static str {
        match self {
            Self::A => "A_SNAPSHOT",
            Self::B => "B_TEMPORAL_DERIVATIVES",
            Self::C => "C_STATISTICAL_DYNAMICS",
            Self::D => "D_MULTISCALE_DYNAMICS",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentalVectorFrame {
    pub timestamp_ns: i64,
    pub variant: DynamicsVariant,
    pub names: Vec<String>,
    pub values: Vec<Option<f64>>,
    pub availability_mask: Vec<bool>,
}

#[derive(Debug, Error)]
pub enum DynamicsError {
    #[error("dynamics experiment requires non-empty daily and weekly structural trajectories")]
    Empty,
    #[error("structural frame timestamps must be strictly increasing")]
    NonIncreasingTimestamps,
    #[error("structural vector dimensions are inconsistent")]
    DimensionMismatch,
    #[error("derived dynamics contain a non-finite value")]
    NonFinite,
}

#[derive(Debug, Clone, Copy, Default)]
struct RunningMoments {
    count: usize,
    mean: f64,
    m2: f64,
}

impl RunningMoments {
    fn prior_z(&self, value: f64) -> Option<f64> {
        if self.count < 2 {
            return None;
        }
        let variance = self.m2 / self.count as f64;
        Some(if variance > 0.0 {
            (value - self.mean) / variance.sqrt()
        } else {
            // A constant causal history carries no standardized deviation.
            0.0
        })
    }

    fn update(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        self.m2 += delta * (value - self.mean);
    }
}

/// Build all four nested experimental vector families in one causal pass.
///
/// Weekly comparison always uses the latest W1 frame from a strictly earlier
/// ISO week.  A partially observed current week is therefore never treated as
/// a closed W1 state.
pub fn build_dynamics_variants(
    daily: &[StructuralFrame],
    weekly: &[StructuralFrame],
) -> Result<BTreeMap<DynamicsVariant, Vec<ExperimentalVectorFrame>>, DynamicsError> {
    validate_frames(daily)?;
    validate_frames(weekly)?;
    let base_names = &daily[0].vector.names;
    if daily
        .iter()
        .chain(weekly)
        .any(|frame| frame.vector.names != *base_names)
    {
        return Err(DynamicsError::DimensionMismatch);
    }

    let mut output: BTreeMap<DynamicsVariant, Vec<ExperimentalVectorFrame>> = DynamicsVariant::ALL
        .into_iter()
        .map(|variant| (variant, Vec::with_capacity(daily.len())))
        .collect();
    let mut pressure_moments = RunningMoments::default();
    let mut momentum_moments = RunningMoments::default();
    let mut transition_moments = RunningMoments::default();
    let mut previous_delta: Option<f64> = None;
    let mut previous_velocity: Option<f64> = None;
    let mut previous_pressure: Option<f64> = None;
    let mut previous_snapshot: Option<&StructuralVector> = None;
    let mut previous_regime: Option<&str> = None;
    let mut regime_age = 0_usize;
    let mut previous_velocity_sign = None;
    let mut momentum_sign_streak = 0_usize;
    let mut previous_cross_scale_dispersion = None;

    for frame in daily {
        let pressure = 1.0 - frame.prama.lambda;
        let velocity = previous_delta.map(|prior| frame.prama.delta - prior);
        let acceleration = velocity
            .zip(previous_velocity)
            .map(|(now, prior)| now - prior);
        let reversal = velocity.zip(previous_velocity).map(|(now, prior)| {
            if now.signum() != prior.signum() && now != 0.0 && prior != 0.0 {
                now.abs().min(prior.abs())
            } else {
                0.0
            }
        });
        let transition = previous_snapshot.and_then(|prior| pairwise_rms(&frame.vector, prior));

        regime_age = if previous_regime == Some(frame.d_o.structural_state.as_str()) {
            regime_age + 1
        } else {
            1
        };
        let velocity_sign = velocity.map(sign_class);
        momentum_sign_streak = match (velocity_sign, previous_velocity_sign) {
            (Some(now), Some(prior)) if now == prior => momentum_sign_streak + 1,
            (Some(_), _) => 1,
            (None, _) => 0,
        };

        let pressure_z = pressure_moments.prior_z(pressure);
        let momentum_z = velocity.and_then(|value| momentum_moments.prior_z(value));
        let transition_z = transition.and_then(|value| transition_moments.prior_z(value));
        let recovery = previous_pressure.map(|prior| prior - pressure);

        let prior_week = latest_strictly_prior_week(frame.timestamp_ns, weekly);
        let (dispersion, coherence) = prior_week
            .map(|week| multiscale_geometry(&frame.vector, &week.vector))
            .unwrap_or((None, None));
        let cross_scale_change = dispersion
            .zip(previous_cross_scale_dispersion)
            .map(|(now, prior)| now - prior);

        let base = (
            frame.vector.names.as_slice(),
            frame.vector.values.as_slice(),
        );
        let derivatives = [
            ("dynamics.delta_velocity", velocity),
            ("dynamics.delta_acceleration", acceleration),
            ("dynamics.delta_reversal_magnitude", reversal),
        ];
        let statistical = [
            ("dynamics.pressure_z_prior", pressure_z),
            ("dynamics.momentum_z_prior", momentum_z),
            ("dynamics.transition_intensity_z_prior", transition_z),
            ("dynamics.regime_age_bars", Some(regime_age as f64)),
            (
                "dynamics.momentum_sign_streak_bars",
                velocity.map(|_| momentum_sign_streak as f64),
            ),
            ("dynamics.pressure_recovery", recovery),
        ];
        let multiscale = [
            ("multiscale.d1_w1_dispersion", dispersion),
            ("multiscale.d1_w1_coherence", coherence),
            (
                "multiscale.cross_scale_dispersion_change",
                cross_scale_change,
            ),
        ];

        push_variant(
            &mut output,
            frame.timestamp_ns,
            DynamicsVariant::A,
            base,
            &[],
        )?;
        push_variant(
            &mut output,
            frame.timestamp_ns,
            DynamicsVariant::B,
            base,
            &derivatives,
        )?;
        let mut c_features = derivatives.to_vec();
        c_features.extend(statistical);
        push_variant(
            &mut output,
            frame.timestamp_ns,
            DynamicsVariant::C,
            base,
            &c_features,
        )?;
        c_features.extend(multiscale);
        push_variant(
            &mut output,
            frame.timestamp_ns,
            DynamicsVariant::D,
            base,
            &c_features,
        )?;

        pressure_moments.update(pressure);
        if let Some(value) = velocity {
            momentum_moments.update(value);
        }
        if let Some(value) = transition {
            transition_moments.update(value);
        }
        previous_delta = Some(frame.prama.delta);
        previous_velocity = velocity;
        previous_pressure = Some(pressure);
        previous_snapshot = Some(&frame.vector);
        previous_regime = Some(&frame.d_o.structural_state);
        previous_velocity_sign = velocity_sign;
        if let Some(value) = dispersion {
            previous_cross_scale_dispersion = Some(value);
        }
    }
    Ok(output)
}

fn validate_frames(frames: &[StructuralFrame]) -> Result<(), DynamicsError> {
    if frames.is_empty() {
        return Err(DynamicsError::Empty);
    }
    if frames
        .windows(2)
        .any(|pair| pair[0].timestamp_ns >= pair[1].timestamp_ns)
    {
        return Err(DynamicsError::NonIncreasingTimestamps);
    }
    let dimension = frames[0].vector.names.len();
    if frames.iter().any(|frame| {
        frame.vector.names.len() != dimension
            || frame.vector.values.len() != dimension
            || frame.vector.availability_mask.len() != dimension
    }) {
        return Err(DynamicsError::DimensionMismatch);
    }
    Ok(())
}

fn push_variant(
    output: &mut BTreeMap<DynamicsVariant, Vec<ExperimentalVectorFrame>>,
    timestamp_ns: i64,
    variant: DynamicsVariant,
    base: (&[String], &[Option<f64>]),
    extra: &[(&str, Option<f64>)],
) -> Result<(), DynamicsError> {
    let mut names = base.0.to_vec();
    names.extend(extra.iter().map(|(name, _)| (*name).to_owned()));
    let mut values = base.1.to_vec();
    values.extend(extra.iter().map(|(_, value)| *value));
    if values.iter().flatten().any(|value| !value.is_finite()) {
        return Err(DynamicsError::NonFinite);
    }
    let availability_mask = values.iter().map(Option::is_some).collect();
    output
        .get_mut(&variant)
        .expect("all variants initialized")
        .push(ExperimentalVectorFrame {
            timestamp_ns,
            variant,
            names,
            values,
            availability_mask,
        });
    Ok(())
}

fn pairwise_rms(left: &StructuralVector, right: &StructuralVector) -> Option<f64> {
    let mut squared_sum = 0.0;
    let mut active = 0_usize;
    for (left, right) in left.values.iter().zip(&right.values) {
        if let (Some(left), Some(right)) = (left, right) {
            squared_sum += (left - right).powi(2);
            active += 1;
        }
    }
    (active > 0).then(|| (squared_sum / active as f64).sqrt())
}

fn multiscale_geometry(
    daily: &StructuralVector,
    weekly: &StructuralVector,
) -> (Option<f64>, Option<f64>) {
    // The first six v2 coordinates are the bounded PRAMA financial state.
    let pairs: Vec<(f64, f64)> = daily
        .values
        .iter()
        .zip(&weekly.values)
        .take(6)
        .filter_map(|(daily, weekly)| Some((daily.as_ref()?, weekly.as_ref()?)))
        .map(|(daily, weekly)| (*daily, *weekly))
        .collect();
    if pairs.is_empty() {
        return (None, None);
    }
    let dispersion = (pairs
        .iter()
        .map(|(daily, weekly)| (daily - weekly).powi(2))
        .sum::<f64>()
        / pairs.len() as f64)
        .sqrt();
    let daily_norm = pairs.iter().map(|(value, _)| value.powi(2)).sum::<f64>();
    let weekly_norm = pairs.iter().map(|(_, value)| value.powi(2)).sum::<f64>();
    let coherence = if daily_norm > 0.0 && weekly_norm > 0.0 {
        Some(
            pairs
                .iter()
                .map(|(daily, weekly)| daily * weekly)
                .sum::<f64>()
                / (daily_norm * weekly_norm).sqrt(),
        )
    } else {
        None
    };
    (Some(dispersion), coherence)
}

fn latest_strictly_prior_week(
    daily_timestamp_ns: i64,
    weekly: &[StructuralFrame],
) -> Option<&StructuralFrame> {
    let daily_date = chrono::DateTime::<Utc>::from_timestamp_nanos(daily_timestamp_ns);
    let daily_week = (daily_date.iso_week().year(), daily_date.iso_week().week());
    weekly.iter().rev().find(|frame| {
        let date = chrono::DateTime::<Utc>::from_timestamp_nanos(frame.timestamp_ns);
        let week = (date.iso_week().year(), date.iso_week().week());
        week < daily_week
    })
}

fn sign_class(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StructuralEngineAdapter;
    use crate::historical::{aggregate_weekly, load_daily_csv};
    use crate::observation::adapt_closed_bars;
    use crate::resolver::{AssetResolver, Resolution};
    use std::path::PathBuf;

    fn trajectories() -> (Vec<StructuralFrame>, Vec<StructuralFrame>) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let daily =
            load_daily_csv(root.join("data/corpus/btc_calib.csv"), &instrument, "test").unwrap();
        let weekly = aggregate_weekly(&daily).unwrap();
        let adapter = StructuralEngineAdapter::default();
        let daily_frames = adapter
            .replay_frames(&adapt_closed_bars(&daily).unwrap())
            .unwrap();
        let weekly_frames = adapter
            .replay_frames(&adapt_closed_bars(&weekly).unwrap())
            .unwrap();
        (daily_frames, weekly_frames)
    }

    #[test]
    fn variant_a_is_exact_v2_snapshot() {
        let (daily, weekly) = trajectories();
        let variants = build_dynamics_variants(&daily, &weekly).unwrap();
        for (source, derived) in daily.iter().zip(&variants[&DynamicsVariant::A]) {
            assert_eq!(derived.names, source.vector.names);
            assert_eq!(derived.values, source.vector.values);
            assert_eq!(derived.availability_mask, source.vector.availability_mask);
        }
    }

    #[test]
    fn dynamic_features_are_prefix_invariant() {
        let (daily, weekly) = trajectories();
        let full = build_dynamics_variants(&daily, &weekly).unwrap();
        let cutoff = daily.len() - 17;
        let prefix = build_dynamics_variants(&daily[..cutoff], &weekly).unwrap();
        for variant in DynamicsVariant::ALL {
            assert_eq!(prefix[&variant], full[&variant][..cutoff]);
        }
    }
}
