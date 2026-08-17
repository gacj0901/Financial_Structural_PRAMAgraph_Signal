use crate::canonical;
use crate::{AvailabilityStatus, AvailableValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const DO_VERSION: &str = "D_O_v9";
pub const DO_FINANCIAL_ADAPTER_VERSION: &str =
    "financial_gamma_state_adapter_v2_prior_robust_geometry";
pub const ODCE_VERSION: &str = "ODCE-v0.1.0+financial-causal-normalization-v1";
pub const K_MEM_VERSION: &str = "K-MEM-reference-0.1/K1-tau32";
pub const STRUCTURAL_VECTOR_VERSION: &str = "financial_structural_vector_v2";

const GEOMETRY_WINDOW: usize = 16;
const MINIMUM_GEOMETRY_POINTS: usize = 8;
const RECURRENCE_LAG_EXCLUSION: usize = 2;
const ACTIVITY_PATH_LENGTH_THRESHOLD: f64 = 0.5;
const OPERATOR_WINDOW: usize = 32;
const MINIMUM_OPERATOR_TRANSITIONS: usize = 8;
const RIDGE_ALPHA: f64 = 0.001;
const RESIDUAL_SCALE_FLOOR: f64 = 0.05;
const RECURRENCE_WINDOW: usize = 16;
const RECURRENCE_RELATIVE_RADIUS: f64 = 0.5;
const RECURRENCE_THRESHOLD: f64 = 0.3;
const COHERENCE_THRESHOLD: f64 = 0.5;
const VARIATION_REFERENCE_WINDOW: usize = 32;
const VARIATION_CONTRACTION_THRESHOLD: f64 = 0.25;
const TAU_WINDOWS: usize = 16;
const HYSTERESIS_GRACE_WINDOWS: usize = 4;
const SCALE_EPSILON: f64 = 1e-12;

const ODCE_WINDOW: usize = 32;
const ODCE_FRICTION_DECAY: f64 = 0.939_413_062_813_475_8;
const ODCE_MINIMUM_ORGANIZATION_SUPPORT: usize = 8;
const K_MEM_TAU: f64 = 32.0;

#[allow(non_snake_case)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PramaState {
    pub delta: f64,
    pub delta_tilde: f64,
    pub e: f64,
    pub xi: f64,
    pub A: f64,
    pub lambda: f64,
    pub theta: f64,
    pub M: f64,
    pub G: f64,
    pub u_lambda: f64,
    pub sigma_op: bool,
    pub valid: bool,
    pub input_index: usize,
    pub state_index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoObservation {
    pub observer: String,
    pub observer_version: String,
    pub financial_adapter_version: String,
    pub index: usize,
    pub geometry_ready: bool,
    pub transport_status: String,
    pub recurrence_status: String,
    pub contraction_status: String,
    pub mobility_status: Option<String>,
    pub structural_state: String,
    pub movement: f64,
    pub transport_coherence: Option<f64>,
    pub operator_prediction_residual: Option<f64>,
    pub operator_training_support: usize,
    pub recurrence_persistence: f64,
    pub variation_capacity: Option<f64>,
    pub variation_contraction: Option<f64>,
    pub alert_eligible: bool,
    pub transport_deficit: Option<f64>,
    pub cumulative_transport_deficit: f64,
    pub diagnostics: Vec<String>,
    pub causal: bool,
    pub external_outcome_used: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OdceState {
    pub operator: String,
    pub operator_version: String,
    pub index: usize,
    pub window_start: usize,
    pub window_end: usize,
    pub raw_cost_vector: BTreeMap<String, AvailableValue<f64>>,
    pub raw_benefit_vector: BTreeMap<String, AvailableValue<f64>>,
    pub cost_vector: BTreeMap<String, AvailableValue<f64>>,
    pub benefit_vector: BTreeMap<String, AvailableValue<f64>>,
    pub normalization_reference: BTreeMap<String, AvailableValue<f64>>,
    pub differential_vector: BTreeMap<String, AvailableValue<f64>>,
    pub differential_trend: BTreeMap<String, AvailableValue<f64>>,
    pub cumulative_conversion_deficit_exposure: BTreeMap<String, f64>,
    pub positive_persistence: BTreeMap<String, AvailableValue<f64>>,
    pub normalization_status: String,
    pub causal: bool,
    pub predictive_model_used: bool,
    pub future_outcome_used: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KMemState {
    pub schema: String,
    pub runtime_version: String,
    pub mode: String,
    pub topology: String,
    pub index: usize,
    pub timescale: f64,
    pub source_channel: String,
    pub source_status: AvailabilityStatus,
    pub strictly_prior_state: AvailableValue<f64>,
    pub state_after_update: AvailableValue<f64>,
    pub update_applied: bool,
    pub causal: bool,
    pub state_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructuralVector {
    pub version: String,
    pub names: Vec<String>,
    pub values: Vec<Option<f64>>,
    pub availability_mask: Vec<bool>,
    pub vector_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StructuralFrame {
    pub timestamp_ns: i64,
    pub prama: PramaState,
    pub d_o: DoObservation,
    pub odce: OdceState,
    pub k_mem: KMemState,
    pub vector: StructuralVector,
}

#[derive(Debug, Error)]
pub enum StructuralError {
    #[error("structural stack requires at least one PRAMA state")]
    Empty,
    #[error("structural state is non-finite")]
    NonFinite,
    #[error("ridge system is singular")]
    Singular,
    #[error("structural hashing failed: {0}")]
    Hash(#[from] canonical::CanonicalError),
}

#[derive(Debug, Clone)]
struct GeometryRow {
    state: Vec<f64>,
    ready: bool,
    path_length: f64,
}

pub fn execute_structural_stack(
    rows: &[(i64, PramaState)],
) -> Result<Vec<StructuralFrame>, StructuralError> {
    execute_structural_stack_with_lambda_bounds(rows, 0.1, 1.0)
}

pub fn execute_structural_stack_with_lambda_bounds(
    rows: &[(i64, PramaState)],
    lambda_min: f64,
    lambda_max: f64,
) -> Result<Vec<StructuralFrame>, StructuralError> {
    if rows.is_empty() {
        return Err(StructuralError::Empty);
    }
    if !lambda_min.is_finite() || !lambda_max.is_finite() || lambda_max <= lambda_min {
        return Err(StructuralError::NonFinite);
    }
    let geometry = geometry_rows(rows, lambda_min, lambda_max)?;
    let do_rows = do_trajectory(&geometry)?;
    let odce_rows = odce_trajectory(rows, &do_rows);
    let k_mem_rows = k_mem_trajectory(&do_rows)?;
    rows.iter()
        .zip(do_rows)
        .zip(odce_rows)
        .zip(k_mem_rows)
        .map(|((((timestamp_ns, prama), d_o), odce), k_mem)| {
            let vector = structural_vector(prama, &d_o, &odce, &k_mem, lambda_min, lambda_max)?;
            Ok(StructuralFrame {
                timestamp_ns: *timestamp_ns,
                prama: prama.clone(),
                d_o,
                odce,
                k_mem,
                vector,
            })
        })
        .collect()
}

fn geometry_rows(
    rows: &[(i64, PramaState)],
    lambda_min: f64,
    lambda_max: f64,
) -> Result<Vec<GeometryRow>, StructuralError> {
    let raw_states: Vec<Vec<f64>> = rows
        .iter()
        .map(|(_, row)| financial_state(row, lambda_min, lambda_max))
        .collect::<Result<_, _>>()?;
    let states = prior_robust_geometry(&raw_states);
    Ok(states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let start = index.saturating_sub(GEOMETRY_WINDOW - 1);
            let local = &states[start..=index];
            let path_length = local
                .windows(2)
                .map(|pair| distance(&pair[0], &pair[1]))
                .sum();
            GeometryRow {
                state: state.clone(),
                ready: local.len() >= MINIMUM_GEOMETRY_POINTS,
                path_length,
            }
        })
        .collect())
}

/// Map bounded PRAMA coordinates to a locally comparable financial geometry.
/// Center and scale at `t` are fitted exclusively on rows `< t`; this adapts
/// domain amplitude without changing D_O's structural state machine.
fn prior_robust_geometry(raw: &[Vec<f64>]) -> Vec<Vec<f64>> {
    raw.iter()
        .enumerate()
        .map(|(index, current)| {
            let start = index.saturating_sub(OPERATOR_WINDOW);
            (0..current.len())
                .map(|axis| {
                    let mut prior: Vec<f64> =
                        raw[start..index].iter().map(|state| state[axis]).collect();
                    let Some(center) = median(&mut prior) else {
                        return 0.5;
                    };
                    let mut deviations: Vec<f64> =
                        prior.iter().map(|value| (value - center).abs()).collect();
                    let mad = median(&mut deviations).unwrap_or(0.0);
                    let range = prior.iter().copied().reduce(f64::max).unwrap_or(center)
                        - prior.iter().copied().reduce(f64::min).unwrap_or(center);
                    let scale = if mad > 0.0 { mad } else { range / 2.0 };
                    if scale > 0.0 {
                        0.5 + 0.5 * ((current[axis] - center) / scale).tanh()
                    } else {
                        0.5
                    }
                })
                .collect()
        })
        .collect()
}

fn financial_state(
    row: &PramaState,
    lambda_min: f64,
    lambda_max: f64,
) -> Result<Vec<f64>, StructuralError> {
    let lambda_span = lambda_max - lambda_min;
    let theta = row.theta.max(SCALE_EPSILON);
    let state = vec![
        squash_positive(row.delta_tilde),
        (row.xi / theta).clamp(0.0, 1.0),
        ((row.lambda - lambda_min) / lambda_span).clamp(0.0, 1.0),
        0.5 + 0.5 * (row.M / theta).tanh(),
        0.5 + 0.5 * row.G.tanh(),
        squash_positive(row.A),
    ];
    if state.iter().all(|value| value.is_finite()) {
        Ok(state)
    } else {
        Err(StructuralError::NonFinite)
    }
}

fn squash_positive(value: f64) -> f64 {
    if value <= 0.0 {
        0.0
    } else {
        value / (1.0 + value)
    }
}

fn do_trajectory(geometry: &[GeometryRow]) -> Result<Vec<DoObservation>, StructuralError> {
    let states: Vec<Vec<f64>> = geometry.iter().map(|row| row.state.clone()).collect();
    let mut intensities = Vec::with_capacity(states.len());
    let mut capacities = Vec::with_capacity(states.len());
    let mut output = Vec::with_capacity(states.len());
    let mut last_coherent_regime: Option<String> = None;
    let mut discontinuity_run = 0usize;
    let mut crystallizing_run = 0usize;
    let mut first_transport_evaluable = None;
    let mut cumulative_deficit = 0.0;

    for index in 0..states.len() {
        let start = index.saturating_sub(GEOMETRY_WINDOW - 1);
        let intensity = recurrence_intensity(&states[start..=index]);
        intensities.push(intensity);
        let recurrence = mean(&intensities[intensities.len().saturating_sub(RECURRENCE_WINDOW)..]);
        let ready = geometry[index].ready;
        let movement = geometry[index].path_length;
        let active = ready && movement > ACTIVITY_PATH_LENGTH_THRESHOLD;
        let recurrence = if active { recurrence } else { 0.0 };
        let (coherence, residual, support) = if active {
            ridge_prediction(&states, index)?
        } else {
            (None, None, 0)
        };
        let capacity = if ready {
            variation_capacity(&states, start, index)
        } else {
            None
        };
        capacities.push(capacity);
        let peak = capacities[capacities.len().saturating_sub(VARIATION_REFERENCE_WINDOW)..]
            .iter()
            .flatten()
            .copied()
            .reduce(f64::max);
        let contraction = match (capacity, peak) {
            (Some(current), Some(reference)) if reference > SCALE_EPSILON => {
                Some((1.0 - current / reference).max(0.0))
            }
            (Some(_), _) => Some(0.0),
            _ => None,
        };
        let coherent = coherence.is_some_and(|value| value >= COHERENCE_THRESHOLD);
        let recurrent = recurrence >= RECURRENCE_THRESHOLD;
        let contracting = contraction.is_some_and(|value| value >= VARIATION_CONTRACTION_THRESHOLD);
        let local_discontinuity = ready && active && coherence.is_some() && !coherent;
        if ready && active && coherence.is_some() && first_transport_evaluable.is_none() {
            first_transport_evaluable = Some(index);
        }
        let alert_eligible =
            first_transport_evaluable.is_some_and(|first| index >= first + TAU_WINDOWS);

        let (transport, mobility, structural_state, inherited) = if !ready {
            discontinuity_run = 0;
            crystallizing_run = 0;
            ("UNRESOLVED", None, "TRANSPORT_UNRESOLVED", false)
        } else if !active {
            discontinuity_run = 0;
            crystallizing_run = 0;
            last_coherent_regime = None;
            ("INACTIVE", Some("STAGNANT"), "STAGNANT", false)
        } else if coherence.is_none() {
            discontinuity_run = 0;
            crystallizing_run = 0;
            ("UNRESOLVED", None, "TRANSPORT_UNRESOLVED", false)
        } else if coherent {
            discontinuity_run = 0;
            let predicate = recurrent && contracting;
            crystallizing_run = if predicate { crystallizing_run + 1 } else { 0 };
            let regime = if predicate && crystallizing_run >= TAU_WINDOWS {
                "CRYSTALLIZED"
            } else if predicate {
                "CRYSTALLIZING"
            } else if recurrent {
                "RECURRENT"
            } else {
                "VIABLE"
            };
            last_coherent_regime = Some(regime.to_owned());
            ("COHERENT", Some(regime), regime, false)
        } else {
            discontinuity_run += 1;
            crystallizing_run = 0;
            if last_coherent_regime.as_deref() == Some("VIABLE")
                && discontinuity_run <= HYSTERESIS_GRACE_WINDOWS
            {
                ("PROVISIONAL", Some("VIABLE"), "VIABLE", true)
            } else {
                ("DISRUPTED", None, "TRANSPORT_DISRUPTED", false)
            }
        };
        let deficit =
            coherence.map(|value| ((COHERENCE_THRESHOLD - value) / COHERENCE_THRESHOLD).max(0.0));
        if alert_eligible {
            cumulative_deficit += deficit.unwrap_or(0.0);
        }
        let mut diagnostics = Vec::new();
        if !ready {
            diagnostics.push("INSUFFICIENT_GEOMETRY".to_owned());
        } else if active && coherence.is_none() {
            diagnostics.push("INSUFFICIENT_TRANSPORT_SUPPORT".to_owned());
        }
        if local_discontinuity {
            diagnostics.push("LOCAL_TRANSPORT_DISCONTINUITY".to_owned());
            diagnostics.push(if inherited {
                "HYSTERESIS_INHERITANCE".to_owned()
            } else {
                "TRANSPORT_DISRUPTION".to_owned()
            });
        }
        if transport == "DISRUPTED" && discontinuity_run > HYSTERESIS_GRACE_WINDOWS {
            diagnostics.push("PERSISTENT_TRANSPORT_DISRUPTION".to_owned());
        }
        if local_discontinuity && recurrent {
            diagnostics.push("IMITATIVE_ECHO".to_owned());
        }
        output.push(DoObservation {
            observer: DO_VERSION.to_owned(),
            observer_version: DO_VERSION.to_owned(),
            financial_adapter_version: DO_FINANCIAL_ADAPTER_VERSION.to_owned(),
            index,
            geometry_ready: ready,
            transport_status: transport.to_owned(),
            recurrence_status: if !ready {
                "UNRESOLVED"
            } else if !active {
                "INACTIVE"
            } else if recurrent {
                "RECURRENT"
            } else {
                "NON_RECURRENT"
            }
            .to_owned(),
            contraction_status: if !ready || contraction.is_none() {
                "UNRESOLVED"
            } else if contracting {
                "CONTRACTING"
            } else {
                "NOT_CONTRACTING"
            }
            .to_owned(),
            mobility_status: mobility.map(str::to_owned),
            structural_state: structural_state.to_owned(),
            movement,
            transport_coherence: coherence,
            operator_prediction_residual: residual,
            operator_training_support: support,
            recurrence_persistence: recurrence,
            variation_capacity: capacity,
            variation_contraction: contraction,
            alert_eligible,
            transport_deficit: deficit,
            cumulative_transport_deficit: cumulative_deficit,
            diagnostics,
            causal: true,
            external_outcome_used: false,
        });
    }
    Ok(output)
}

fn ridge_prediction(
    states: &[Vec<f64>],
    index: usize,
) -> Result<(Option<f64>, Option<f64>, usize), StructuralError> {
    if index < 2 {
        return Ok((None, None, 0));
    }
    let end_source = index - 1;
    let first_source = end_source.saturating_sub(OPERATOR_WINDOW);
    let support = end_source.saturating_sub(first_source);
    if support < MINIMUM_OPERATOR_TRANSITIONS {
        return Ok((None, None, support));
    }
    let dimension = states[0].len();
    let mut gram = vec![vec![0.0; dimension]; dimension];
    let mut xty = vec![vec![0.0; dimension]; dimension];
    for source in first_source..end_source {
        for left in 0..dimension {
            for right in 0..dimension {
                gram[left][right] += states[source][left] * states[source][right];
                xty[left][right] += states[source][left] * states[source + 1][right];
            }
        }
    }
    for (axis, row) in gram.iter_mut().enumerate() {
        row[axis] += RIDGE_ALPHA;
    }
    let coefficients = solve_matrix(gram, xty)?;
    let mut predicted = vec![0.0; dimension];
    for (target, value) in predicted.iter_mut().enumerate() {
        *value = (0..dimension)
            .map(|source| states[index - 1][source] * coefficients[source][target])
            .sum();
    }
    let residual = distance(&predicted, &states[index]);
    let mut movements: Vec<f64> = (first_source..end_source)
        .map(|source| distance(&states[source], &states[source + 1]))
        .filter(|value| *value > SCALE_EPSILON)
        .collect();
    let movement_scale = median(&mut movements).unwrap_or(SCALE_EPSILON);
    let scale = RESIDUAL_SCALE_FLOOR.max(movement_scale).max(SCALE_EPSILON);
    let coherence = (-residual / scale).exp().clamp(0.0, 1.0);
    Ok((Some(coherence), Some(residual), support))
}

fn solve_matrix(
    mut left: Vec<Vec<f64>>,
    mut right: Vec<Vec<f64>>,
) -> Result<Vec<Vec<f64>>, StructuralError> {
    let n = left.len();
    for pivot in 0..n {
        let best = (pivot..n)
            .max_by(|a, b| left[*a][pivot].abs().total_cmp(&left[*b][pivot].abs()))
            .ok_or(StructuralError::Singular)?;
        if left[best][pivot].abs() <= SCALE_EPSILON {
            return Err(StructuralError::Singular);
        }
        left.swap(pivot, best);
        right.swap(pivot, best);
        let divisor = left[pivot][pivot];
        for column in 0..n {
            left[pivot][column] /= divisor;
            right[pivot][column] /= divisor;
        }
        for row in 0..n {
            if row == pivot {
                continue;
            }
            let factor = left[row][pivot];
            for column in 0..n {
                left[row][column] -= factor * left[pivot][column];
                right[row][column] -= factor * right[pivot][column];
            }
        }
    }
    Ok(right)
}

fn recurrence_intensity(states: &[Vec<f64>]) -> f64 {
    let mut admissible = Vec::new();
    for left in 0..states.len() {
        for right in (left + 1)..states.len() {
            if right - left > RECURRENCE_LAG_EXCLUSION {
                admissible.push(distance(&states[left], &states[right]));
            }
        }
    }
    let mut positive: Vec<f64> = admissible
        .into_iter()
        .filter(|value| *value > SCALE_EPSILON)
        .collect();
    let Some(local_scale) = median(&mut positive) else {
        return 0.0;
    };
    let current_limit = states.len().saturating_sub(RECURRENCE_LAG_EXCLUSION + 1);
    if current_limit == 0 {
        return 0.0;
    }
    let radius = (RECURRENCE_RELATIVE_RADIUS * local_scale).max(SCALE_EPSILON);
    (0..current_limit)
        .map(|prior| {
            (1.0 - distance(states.last().expect("nonempty"), &states[prior]) / radius).max(0.0)
        })
        .reduce(f64::max)
        .unwrap_or(0.0)
}

fn variation_capacity(states: &[Vec<f64>], start: usize, index: usize) -> Option<f64> {
    let dimension = states[0].len();
    let active: Vec<Vec<f64>> = ((start + 1).max(1)..=index)
        .map(|current| {
            (0..dimension)
                .map(|axis| states[current][axis] - states[current - 1][axis])
                .collect::<Vec<_>>()
        })
        .filter(|step| rms_norm(step) > SCALE_EPSILON)
        .collect();
    if active.len() < 3 {
        return None;
    }
    let means: Vec<f64> = (0..dimension)
        .map(|axis| active.iter().map(|row| row[axis]).sum::<f64>() / active.len() as f64)
        .collect();
    let mut covariance = vec![vec![0.0; dimension]; dimension];
    for row in &active {
        for left in 0..dimension {
            for right in 0..dimension {
                covariance[left][right] += (row[left] - means[left]) * (row[right] - means[right]);
            }
        }
    }
    let eigenvalues = symmetric_eigenvalues(covariance);
    let total: f64 = eigenvalues
        .iter()
        .filter(|value| **value > SCALE_EPSILON)
        .sum();
    if total <= SCALE_EPSILON {
        return Some(0.0);
    }
    let entropy: f64 = eigenvalues
        .iter()
        .filter(|value| **value > SCALE_EPSILON)
        .map(|value| {
            let probability = value / total;
            -probability * probability.ln()
        })
        .sum();
    let effective_rank = entropy.exp();
    Some(((effective_rank - 1.0) / (dimension - 1) as f64).clamp(0.0, 1.0))
}

#[allow(clippy::needless_range_loop)] // Jacobi rotations require paired matrix indexing.
fn symmetric_eigenvalues(mut matrix: Vec<Vec<f64>>) -> Vec<f64> {
    let n = matrix.len();
    for _ in 0..(n * n * 12) {
        let mut p = 0;
        let mut q = 1.min(n.saturating_sub(1));
        let mut largest = 0.0;
        for row in 0..n {
            for column in (row + 1)..n {
                if matrix[row][column].abs() > largest {
                    largest = matrix[row][column].abs();
                    p = row;
                    q = column;
                }
            }
        }
        if largest <= 1e-14 || p == q {
            break;
        }
        let angle = 0.5 * (2.0 * matrix[p][q]).atan2(matrix[q][q] - matrix[p][p]);
        let (sine, cosine) = angle.sin_cos();
        for index in 0..n {
            if index != p && index != q {
                let ip = matrix[index][p];
                let iq = matrix[index][q];
                matrix[index][p] = cosine * ip - sine * iq;
                matrix[p][index] = matrix[index][p];
                matrix[index][q] = sine * ip + cosine * iq;
                matrix[q][index] = matrix[index][q];
            }
        }
        let pp = matrix[p][p];
        let qq = matrix[q][q];
        let pq = matrix[p][q];
        matrix[p][p] = cosine * cosine * pp - 2.0 * sine * cosine * pq + sine * sine * qq;
        matrix[q][q] = sine * sine * pp + 2.0 * sine * cosine * pq + cosine * cosine * qq;
        matrix[p][q] = 0.0;
        matrix[q][p] = 0.0;
    }
    (0..n).map(|index| matrix[index][index].max(0.0)).collect()
}

fn odce_trajectory(rows: &[(i64, PramaState)], d_o: &[DoObservation]) -> Vec<OdceState> {
    let mut output = Vec::with_capacity(rows.len());
    let mut exposure: BTreeMap<String, f64> = BTreeMap::new();
    let mut histories: BTreeMap<String, Vec<Option<f64>>> = BTreeMap::new();
    let initial_capacity = rows[0].1.lambda;
    for index in 0..rows.len() {
        let start = index.saturating_sub(ODCE_WINDOW - 1);
        let window: Vec<&PramaState> = rows[start..=index].iter().map(|(_, row)| row).collect();
        let do_window = &d_o[start..=index];
        let retained = weighted_mean(
            &window.iter().map(|row| row.xi).collect::<Vec<_>>(),
            ODCE_FRICTION_DECAY,
        );
        let debt = (window.last().expect("nonempty").A - window[0].A).max(0.0);
        let capacity = (initial_capacity - window.last().expect("nonempty").lambda).max(0.0);
        let excess =
            window.iter().filter(|row| row.xi > row.theta).count() as f64 / window.len() as f64;
        let adverse = weighted_mean(
            &window
                .iter()
                .map(|row| (-row.G).max(0.0))
                .collect::<Vec<_>>(),
            ODCE_FRICTION_DECAY,
        );
        let min_capacity = window
            .iter()
            .map(|row| row.lambda)
            .reduce(f64::min)
            .expect("nonempty");
        let recovery = (window.last().expect("nonempty").lambda - min_capacity).max(0.0);
        let organization_values: Vec<f64> = do_window
            .iter()
            .filter_map(|row| Some(row.transport_coherence? * (1.0 - row.variation_contraction?)))
            .collect();
        let organization = (organization_values.len() >= ODCE_MINIMUM_ORGANIZATION_SUPPORT)
            .then(|| mean(&organization_values));

        let raw_cost_vector = BTreeMap::from([
            (
                "retained_friction".to_owned(),
                AvailableValue::available(retained),
            ),
            (
                "accumulated_debt".to_owned(),
                AvailableValue::available(debt),
            ),
            (
                "capacity_consumption".to_owned(),
                AvailableValue::available(capacity),
            ),
            (
                "excess_persistence".to_owned(),
                AvailableValue::available(excess),
            ),
            (
                "adverse_trend".to_owned(),
                AvailableValue::available(adverse),
            ),
        ]);
        let raw_benefit_vector = BTreeMap::from([
            (
                "structural_recovery".to_owned(),
                AvailableValue::available(recovery),
            ),
            (
                "adaptive_organization_level".to_owned(),
                available_option(organization),
            ),
            ("functional_gain".to_owned(), AvailableValue::unavailable()),
            (
                "external_integration".to_owned(),
                AvailableValue::unavailable(),
            ),
            ("verified_outcome".to_owned(), AvailableValue::unavailable()),
        ]);
        let mut cost_vector = BTreeMap::new();
        let mut benefit_vector = BTreeMap::new();
        let mut normalization_reference = BTreeMap::new();
        for (name, raw) in &raw_cost_vector {
            let (normalized, reference) = causal_relative_magnitude(
                raw.value,
                histories.get(name).map(Vec::as_slice).unwrap_or(&[]),
            );
            cost_vector.insert(name.clone(), normalized);
            normalization_reference.insert(name.clone(), reference);
        }
        for (name, raw) in &raw_benefit_vector {
            let (normalized, reference) = causal_relative_magnitude(
                raw.value,
                histories.get(name).map(Vec::as_slice).unwrap_or(&[]),
            );
            benefit_vector.insert(name.clone(), normalized);
            normalization_reference.insert(name.clone(), reference);
        }
        let mut differential_vector = BTreeMap::new();
        differential_vector.insert(
            "retained_friction_vs_structural_recovery".to_owned(),
            subtract_available(
                cost_vector.get("retained_friction").expect("declared"),
                benefit_vector.get("structural_recovery").expect("declared"),
            ),
        );
        differential_vector.insert(
            "retained_friction_vs_adaptive_organization_level".to_owned(),
            subtract_available(
                cost_vector.get("retained_friction").expect("declared"),
                benefit_vector
                    .get("adaptive_organization_level")
                    .expect("declared"),
            ),
        );
        differential_vector.insert(
            "capacity_consumption_vs_functional_gain".to_owned(),
            AvailableValue::unavailable(),
        );
        let mut differential_trend = BTreeMap::new();
        let mut positive_persistence = BTreeMap::new();
        for (name, value) in &differential_vector {
            let previous = output
                .last()
                .and_then(|row: &OdceState| row.differential_vector.get(name)?.value);
            differential_trend.insert(
                name.clone(),
                match (value.value, previous) {
                    (Some(current), Some(previous)) => {
                        AvailableValue::available(current - previous)
                    }
                    _ => AvailableValue::unavailable(),
                },
            );
            if let Some(current) = value.value {
                *exposure.entry(name.clone()).or_insert(0.0) += current.max(0.0);
            } else {
                exposure.entry(name.clone()).or_insert(0.0);
            }
            positive_persistence.insert(name.clone(), AvailableValue::unavailable());
        }
        for (name, raw) in raw_cost_vector.iter().chain(&raw_benefit_vector) {
            histories.entry(name.clone()).or_default().push(raw.value);
        }
        output.push(OdceState {
            operator: "ODCE_v0".to_owned(),
            operator_version: ODCE_VERSION.to_owned(),
            index,
            window_start: start,
            window_end: index,
            raw_cost_vector,
            raw_benefit_vector,
            cost_vector,
            benefit_vector,
            normalization_reference,
            differential_vector,
            differential_trend,
            cumulative_conversion_deficit_exposure: exposure.clone(),
            positive_persistence,
            normalization_status: "FINANCIAL_CAUSAL_RELATIVE_MAGNITUDE_V1_STRICTLY_PRIOR_REFERENCE"
                .to_owned(),
            causal: true,
            predictive_model_used: false,
            future_outcome_used: false,
        });
    }
    output
}

fn causal_relative_magnitude(
    current: Option<f64>,
    history: &[Option<f64>],
) -> (AvailableValue<f64>, AvailableValue<f64>) {
    let Some(current) = current else {
        return (AvailableValue::unavailable(), AvailableValue::unavailable());
    };
    let mut reference_values: Vec<f64> = history
        .iter()
        .rev()
        .take(ODCE_WINDOW)
        .flatten()
        .map(|value| value.abs())
        .collect();
    if reference_values.len() < ODCE_MINIMUM_ORGANIZATION_SUPPORT {
        return (AvailableValue::unavailable(), AvailableValue::unavailable());
    }
    let reference = median(&mut reference_values).unwrap_or(0.0);
    if reference > 0.0 {
        (
            AvailableValue::available(current.abs() / (current.abs() + reference)),
            AvailableValue::available(reference),
        )
    } else if current == 0.0 {
        (
            AvailableValue::available(0.0),
            AvailableValue::available(0.0),
        )
    } else {
        (AvailableValue::unavailable(), AvailableValue::unavailable())
    }
}

fn subtract_available(
    cost: &AvailableValue<f64>,
    benefit: &AvailableValue<f64>,
) -> AvailableValue<f64> {
    match (cost.value, benefit.value) {
        (Some(cost), Some(benefit)) => AvailableValue::available(clean_zero(cost - benefit)),
        _ => AvailableValue::unavailable(),
    }
}

fn k_mem_trajectory(d_o: &[DoObservation]) -> Result<Vec<KMemState>, StructuralError> {
    let decay = (-1.0 / K_MEM_TAU).exp();
    let mut state = 0.0;
    let mut initialized = false;
    let mut output = Vec::with_capacity(d_o.len());
    for row in d_o {
        let prior = initialized.then_some(state);
        let update_applied = row.transport_deficit.is_some();
        if let Some(value) = row.transport_deficit {
            state = decay * state + (1.0 - decay) * value;
            initialized = true;
        }
        let mut artifact = KMemState {
            schema: "K-MEM-state/0.1".to_owned(),
            runtime_version: K_MEM_VERSION.to_owned(),
            mode: "K1".to_owned(),
            topology: "POST_OBSERVER".to_owned(),
            index: row.index,
            timescale: K_MEM_TAU,
            source_channel: "D_O_v9.transport_deficit".to_owned(),
            source_status: if row.transport_deficit.is_some() {
                AvailabilityStatus::Available
            } else {
                AvailabilityStatus::Unavailable
            },
            strictly_prior_state: available_option(prior),
            state_after_update: available_option(initialized.then_some(state)),
            update_applied,
            causal: true,
            state_sha256: String::new(),
        };
        artifact.state_sha256 = canonical::sha256(&artifact)?;
        output.push(artifact);
    }
    Ok(output)
}

fn structural_vector(
    prama: &PramaState,
    d_o: &DoObservation,
    odce: &OdceState,
    k_mem: &KMemState,
    lambda_min: f64,
    lambda_max: f64,
) -> Result<StructuralVector, StructuralError> {
    let financial = financial_state(prama, lambda_min, lambda_max)?;
    let mut dimensions: Vec<(&str, Option<f64>)> = vec![
        ("prama.delta", Some(financial[0])),
        ("prama.xi_occupancy", Some(financial[1])),
        ("prama.capacity", Some(financial[2])),
        ("prama.margin", Some(financial[3])),
        ("prama.trend", Some(financial[4])),
        ("prama.accumulated_excess", Some(financial[5])),
        ("d_o.movement", Some(d_o.movement)),
        ("d_o.transport_coherence", d_o.transport_coherence),
        (
            "d_o.recurrence_persistence",
            Some(d_o.recurrence_persistence),
        ),
        ("d_o.variation_contraction", d_o.variation_contraction),
        ("d_o.transport_deficit", d_o.transport_deficit),
        (
            "odce.retained_friction_vs_structural_recovery",
            odce.differential_vector
                .get("retained_friction_vs_structural_recovery")
                .and_then(|value| value.value),
        ),
        (
            "odce.retained_friction_vs_adaptive_organization_level",
            odce.differential_vector
                .get("retained_friction_vs_adaptive_organization_level")
                .and_then(|value| value.value),
        ),
        ("k_mem.strictly_prior_z32", k_mem.strictly_prior_state.value),
    ];
    for (_, value) in &mut dimensions {
        if value.is_some_and(|number| !number.is_finite()) {
            return Err(StructuralError::NonFinite);
        }
    }
    let names = dimensions
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    let values: Vec<Option<f64>> = dimensions.iter().map(|(_, value)| *value).collect();
    let availability_mask = values.iter().map(Option::is_some).collect();
    let mut vector = StructuralVector {
        version: STRUCTURAL_VECTOR_VERSION.to_owned(),
        names,
        values,
        availability_mask,
        vector_sha256: String::new(),
    };
    vector.vector_sha256 = canonical::sha256(&vector)?;
    Ok(vector)
}

fn available_option(value: Option<f64>) -> AvailableValue<f64> {
    value.map_or_else(AvailableValue::unavailable, AvailableValue::available)
}

fn weighted_mean(values: &[f64], decay: f64) -> f64 {
    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (index, value) in values.iter().enumerate() {
        let weight = decay.powi((values.len() - index - 1) as i32);
        numerator += value * weight;
        denominator += weight;
    }
    numerator / denominator
}

fn clean_zero(value: f64) -> f64 {
    if value.abs() < 1e-12 {
        0.0
    } else {
        value
    }
}

fn distance(left: &[f64], right: &[f64]) -> f64 {
    debug_assert_eq!(left.len(), right.len());
    ((0..left.len())
        .map(|index| (left[index] - right[index]).powi(2))
        .sum::<f64>()
        / left.len() as f64)
        .sqrt()
}

fn rms_norm(values: &[f64]) -> f64 {
    (values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt()
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(index: usize, delta: f64) -> PramaState {
        PramaState {
            delta,
            delta_tilde: delta,
            e: 0.0,
            xi: delta * 0.2,
            A: delta * index as f64,
            lambda: 1.0,
            theta: 2.0,
            M: 2.0 - delta * 0.2,
            G: delta * 0.01,
            u_lambda: 0.0,
            sigma_op: true,
            valid: true,
            input_index: index,
            state_index: index + 1,
        }
    }

    #[test]
    fn stack_is_deterministic_and_fully_populated() {
        let rows: Vec<_> = (0..80)
            .map(|index| (index as i64, state(index, 0.1 + (index % 9) as f64 * 0.03)))
            .collect();
        let left = execute_structural_stack(&rows).unwrap();
        let right = execute_structural_stack(&rows).unwrap();
        assert_eq!(left, right);
        let last = left.last().unwrap();
        assert_eq!(last.d_o.observer, DO_VERSION);
        assert_eq!(last.odce.operator, "ODCE_v0");
        assert_eq!(last.k_mem.mode, "K1");
        assert_eq!(last.vector.version, STRUCTURAL_VECTOR_VERSION);
    }

    #[test]
    fn prefix_is_unchanged_when_future_rows_are_appended() {
        let rows: Vec<_> = (0..90)
            .map(|index| (index as i64, state(index, 0.1 + (index % 7) as f64 * 0.02)))
            .collect();
        let prefix = execute_structural_stack(&rows[..70]).unwrap();
        let full = execute_structural_stack(&rows).unwrap();
        assert_eq!(prefix, full[..70]);
    }

    #[test]
    fn k_mem_feature_is_strictly_prior() {
        let rows: Vec<_> = (0..80)
            .map(|index| (index as i64, state(index, 0.1 + (index % 5) as f64 * 0.04)))
            .collect();
        let frames = execute_structural_stack(&rows).unwrap();
        for pair in frames.windows(2) {
            assert_eq!(
                pair[1].k_mem.strictly_prior_state.value,
                pair[0].k_mem.state_after_update.value
            );
        }
    }
}
