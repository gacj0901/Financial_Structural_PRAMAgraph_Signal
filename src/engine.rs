use crate::canonical;
use crate::structural::{
    execute_structural_stack_with_lambda_bounds, PramaState, StructuralError, StructuralFrame,
};
use crate::{AvailabilityStatus, AvailableValue, ComponentSnapshot, StructuralSnapshot, Timeframe};
use prama_protokol::v3::{GammaRowV3, KernelConfigV3, KernelV3, V3Error};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const ENGINE_VERSION: &str =
    "prama-protokol-rs/0.3.0@ddb91cad+D_O_v9-financial-adapter-v2+ODCE-v0.1-financial-normalization-v1+K-MEM-K1-tau32";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KernelObservation {
    pub timestamp_ns: i64,
    pub omega: f64,
    pub expected: f64,
    pub u_lambda: AvailableValue<f64>,
    pub sigma_op: AvailableValue<bool>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("kernel observation stream is empty")]
    Empty,
    #[error("kernel input {field} is unavailable at timestamp {timestamp_ns}")]
    UnavailableInput {
        field: &'static str,
        timestamp_ns: i64,
    },
    #[error("timestamps must be strictly increasing")]
    NonIncreasingTimestamps,
    #[error("PRAMA kernel failed: {0}")]
    Kernel(#[from] V3Error),
    #[error("structural stack failed: {0}")]
    Structural(#[from] StructuralError),
    #[error("snapshot hashing failed: {0}")]
    Hash(#[from] canonical::CanonicalError),
}

pub struct StructuralEngineAdapter {
    config: KernelConfigV3,
}

impl StructuralEngineAdapter {
    pub fn new(config: KernelConfigV3) -> Result<Self, EngineError> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn replay(
        &self,
        instrument_id: &str,
        timeframe: Timeframe,
        source_watermark: &str,
        observations: &[KernelObservation],
    ) -> Result<StructuralSnapshot, EngineError> {
        let frames = self.replay_frames(observations)?;
        self.snapshot_from_frames(instrument_id, timeframe, source_watermark, &frames)
    }

    pub fn snapshot_from_frames(
        &self,
        instrument_id: &str,
        timeframe: Timeframe,
        source_watermark: &str,
        frames: &[StructuralFrame],
    ) -> Result<StructuralSnapshot, EngineError> {
        let last = frames.last().ok_or(EngineError::Empty)?;
        let as_of_ns = last.timestamp_ns;
        let row = &last.prama;
        let prama_value = serde_json::to_value(row).expect("typed PRAMA state serializes");
        let d_o_value = serde_json::to_value(&last.d_o).expect("typed D_O state serializes");
        let odce_value = serde_json::to_value(&last.odce).expect("typed ODCE state serializes");
        let k_mem_value = serde_json::to_value(&last.k_mem).expect("typed K-MEM state serializes");
        let mut availability = BTreeMap::new();
        availability.insert("prama".to_owned(), AvailabilityStatus::Available);
        availability.insert("d_o".to_owned(), AvailabilityStatus::Available);
        availability.insert("odce".to_owned(), AvailabilityStatus::Available);
        availability.insert("k_mem".to_owned(), AvailabilityStatus::Available);
        let mut snapshot = StructuralSnapshot {
            instrument_id: instrument_id.to_owned(),
            timeframe,
            as_of_ns,
            engine_version: ENGINE_VERSION.to_owned(),
            structural_state: last.d_o.structural_state.clone(),
            prama: ComponentSnapshot::available(prama_value),
            d_o: ComponentSnapshot::available(d_o_value),
            odce: ComponentSnapshot::available(odce_value),
            k_mem: ComponentSnapshot::available(k_mem_value),
            availability,
            source_watermark: source_watermark.to_owned(),
            snapshot_sha256: None,
        };
        snapshot.snapshot_sha256 = Some(canonical::sha256(&snapshot)?);
        Ok(snapshot)
    }

    /// Replay the complete causal trajectory. Directional calibration consumes
    /// these frames offline; live inference only reads the final frame.
    pub fn replay_frames(
        &self,
        observations: &[KernelObservation],
    ) -> Result<Vec<StructuralFrame>, EngineError> {
        if observations.is_empty() {
            return Err(EngineError::Empty);
        }
        if observations
            .windows(2)
            .any(|pair| pair[0].timestamp_ns >= pair[1].timestamp_ns)
        {
            return Err(EngineError::NonIncreasingTimestamps);
        }
        let mut kernel = KernelV3::new(self.config)?;
        let mut rows = Vec::new();
        for observation in observations {
            observation
                .u_lambda
                .validate()
                .map_err(|_| EngineError::UnavailableInput {
                    field: "u_lambda",
                    timestamp_ns: observation.timestamp_ns,
                })?;
            observation
                .sigma_op
                .validate()
                .map_err(|_| EngineError::UnavailableInput {
                    field: "sigma_op",
                    timestamp_ns: observation.timestamp_ns,
                })?;
            let u_lambda = match observation.u_lambda.availability {
                AvailabilityStatus::Available => observation.u_lambda.value.expect("validated"),
                AvailabilityStatus::NotApplicable => 0.0,
                _ => {
                    return Err(EngineError::UnavailableInput {
                        field: "u_lambda",
                        timestamp_ns: observation.timestamp_ns,
                    })
                }
            };
            let sigma_op = match observation.sigma_op.availability {
                AvailabilityStatus::Available => observation.sigma_op.value,
                AvailabilityStatus::NotApplicable => None,
                _ => {
                    return Err(EngineError::UnavailableInput {
                        field: "sigma_op",
                        timestamp_ns: observation.timestamp_ns,
                    })
                }
            };
            if let Some(row) =
                kernel.step(observation.omega, observation.expected, u_lambda, sigma_op)?
            {
                rows.push((observation.timestamp_ns, prama_state(row)));
            }
        }
        execute_structural_stack_with_lambda_bounds(
            &rows,
            self.config.lambda_min,
            self.config.lambda_max,
        )
        .map_err(EngineError::from)
    }
}

#[allow(non_snake_case)]
fn prama_state(row: GammaRowV3) -> PramaState {
    PramaState {
        delta: row.delta,
        delta_tilde: row.delta_tilde,
        e: row.e,
        xi: row.xi,
        A: row.A,
        lambda: row.lambda,
        theta: row.theta,
        M: row.M,
        G: row.G,
        u_lambda: row.u_lambda,
        sigma_op: row.sigma_op,
        valid: row.valid,
        input_index: row.input_index,
        state_index: row.state_index,
    }
}

impl Default for StructuralEngineAdapter {
    fn default() -> Self {
        Self::new(KernelConfigV3::default()).expect("certified default config is valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(timestamp_ns: i64, omega: f64, expected: f64) -> KernelObservation {
        KernelObservation {
            timestamp_ns,
            omega,
            expected,
            u_lambda: AvailableValue::not_applicable(),
            sigma_op: AvailableValue::not_applicable(),
        }
    }

    #[test]
    fn replay_is_deterministic() {
        let inputs = vec![input(1, 1.0, 0.9), input(2, 1.1, 1.0), input(3, 0.8, 1.0)];
        let engine = StructuralEngineAdapter::default();
        let left = engine
            .replay("test", Timeframe::D1, "watermark", &inputs)
            .unwrap();
        let right = engine
            .replay("test", Timeframe::D1, "watermark", &inputs)
            .unwrap();
        assert_eq!(left, right);
        assert_eq!(left.snapshot_sha256, right.snapshot_sha256);
    }

    #[test]
    fn unavailable_control_fails_closed() {
        let mut observation = input(1, 1.0, 0.9);
        observation.u_lambda = AvailableValue::unavailable();
        assert!(matches!(
            StructuralEngineAdapter::default().replay(
                "test",
                Timeframe::D1,
                "watermark",
                &[observation]
            ),
            Err(EngineError::UnavailableInput {
                field: "u_lambda",
                ..
            })
        ));
    }

    #[test]
    fn structural_components_are_populated() {
        let snapshot = StructuralEngineAdapter::default()
            .replay("test", Timeframe::D1, "watermark", &[input(1, 1.0, 0.9)])
            .unwrap();
        assert_eq!(snapshot.d_o.availability, AvailabilityStatus::Available);
        assert!(snapshot.odce.value.is_some());
        assert!(snapshot.k_mem.value.is_some());
    }
}
