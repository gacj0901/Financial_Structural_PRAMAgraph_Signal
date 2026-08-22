//! Offline directional calibration and immutable runtime resolution.
//!
//! Future bars are consumed only by [`build_resolution_profile`]. Runtime
//! resolution accepts one structural vector and a prebuilt, hash-verified
//! profile; it cannot recalibrate or inspect market outcomes.

use crate::canonical;
use crate::dynamics::{
    build_dynamics_variants, DynamicsVariant, ExperimentalVectorFrame, DYNAMICS_EXPERIMENT_ID,
};
use crate::structural::{StructuralFrame, StructuralVector, STRUCTURAL_VECTOR_VERSION};
use crate::{
    AssetClass, CalibrationScope, Direction, Horizon, MarketObservation, ProbabilitiesBp, Timeframe,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const RESOLUTION_PROFILE_SCHEMA: &str = "pramagraph.resolution_calibration_profile.v2";
pub const RESOLUTION_CALIBRATION_VERSION: &str = "financial_first_passage_weighted_neighbors_v2";
pub const DEVELOPMENT_DATA_CUTOFF_NS: i64 = 1755734400000000000; // 2025-08-21T00:00:00Z (historical/development data cutoff)
pub const PROTOCOL_FREEZE_TIMESTAMP_NS: i64 = 1766304000000000000; // 2026-07-21T00:00:00Z (actual protocol freeze/preregistration timestamp)

/// Frozen calibration protocol — all deterministic choices that affect calibration results.
/// This protocol is serialized canonically and hashed to produce preregistered_protocol_sha256.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CalibrationProtocol {
    pub schema: String,
    pub protocol_id: String,
    pub structural_vector_version: String,
    pub engine_version: String,
    pub calibration_procedure: CalibrationProcedure,
    pub determinism: DeterminismConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CalibrationProcedure {
    pub split_rules: SplitRules,
    pub neighbor_selection: NeighborSelectionRules,
    pub voting: VotingRules,
    pub first_passage: FirstPassageRules,
    pub publication_gates: PublicationGateRules,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SplitRules {
    pub test_count_rule: String,
    pub validation_count_rule: String,
    pub strict_temporal_order: bool,
    pub no_lookahead: bool,
    pub preregistration_boundary_ns: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NeighborSelectionRules {
    pub neighbor_count_rule: String,
    pub minimum_support_rule: String,
    pub maximum_distance_rule: String,
    pub distance_power_selection: String,
    pub availability_mask_exact_match: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VotingRules {
    pub weight_formula: String,
    pub probabilities: String,
    pub direction_edge_bp: String,
    pub tie_break: String,
    pub first_passage_horizon: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FirstPassageRules {
    pub max_horizon_bars_d1: usize,
    pub max_horizon_bars_w1: usize,
    pub volatility_lookback_bars_d1: usize,
    pub volatility_lookback_bars_w1: usize,
    pub barrier_multiplier: String,
    pub up_down_symmetric: bool,
    pub simultaneous_hit_rule: String,
    pub no_hit_rule: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicationGateRules {
    pub requires_positive_brier_skill: bool,
    pub minimum_direction_edge_bp_rule: String,
    pub minimum_reliability_bp_rule: String,
    pub reliability_gate: String,
    pub profile_eligible_for_publication: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeterminismConfig {
    pub no_randomness: bool,
    pub all_operations_deterministic: bool,
    pub fixed_point_ordering: String,
}

impl CalibrationProtocol {
    /// Returns the frozen protocol with all current deterministic choices.
    pub fn frozen() -> Self {
        Self {
            schema: "pramagraph.calibration_protocol.v1".into(),
            protocol_id: RESOLUTION_CALIBRATION_VERSION.into(),
            structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
            engine_version: crate::engine::ENGINE_VERSION.into(),
            calibration_procedure: CalibrationProcedure {
                split_rules: SplitRules {
                    test_count_rule: "integer_sqrt(frames.len()).max(1)".into(),
                    validation_count_rule: "integer_sqrt(frames.len()).max(1)".into(),
                    strict_temporal_order: true,
                    no_lookahead: true,
                    preregistration_boundary_ns: DEVELOPMENT_DATA_CUTOFF_NS,
                },
                neighbor_selection: NeighborSelectionRules {
                    neighbor_count_rule: "integer_sqrt(training_samples.len()).max(1)".into(),
                    minimum_support_rule:
                        "integer_log2(training_samples.len()).max(1).min(neighbor_count)".into(),
                    maximum_distance_rule: "max_kth_distance_on_validation".into(),
                    distance_power_selection: "compare_power_1_vs_2_on_validation".into(),
                    availability_mask_exact_match: true,
                },
                voting: VotingRules {
                    weight_formula: "1 / distance^power".into(),
                    probabilities: "basis_points_from_weighted_votes".into(),
                    direction_edge_bp: "top_prob - second_prob".into(),
                    tie_break: "direction_order_priority".into(),
                    first_passage_horizon: "weighted_by_vote_weight".into(),
                },
                first_passage: FirstPassageRules {
                    max_horizon_bars_d1: 10,
                    max_horizon_bars_w1: 8,
                    volatility_lookback_bars_d1: 28,
                    volatility_lookback_bars_w1: 12,
                    barrier_multiplier: "empirical_symmetric_barrier".into(),
                    up_down_symmetric: true,
                    simultaneous_hit_rule: "RANGE".into(),
                    no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".into(),
                },
                publication_gates: PublicationGateRules {
                    requires_positive_brier_skill: true,
                    minimum_direction_edge_bp_rule: "median_on_validation".into(),
                    minimum_reliability_bp_rule: "wilson_lower_bound_on_validation".into(),
                    reliability_gate: "per_direction_lower_bound".into(),
                    profile_eligible_for_publication:
                        "requires_preregistered_protocol_sha256_and_prospective_evidence".into(),
                },
            },
            determinism: DeterminismConfig {
                no_randomness: true,
                all_operations_deterministic: true,
                fixed_point_ordering: "distance_ascending_then_timestamp_ascending".into(),
            },
        }
    }

    /// Canonical JSON serialization: sorted keys, no whitespace, stable number format.
    pub fn canonical_json(&self) -> String {
        serde_json::to_string(self).expect("CalibrationProtocol serializes")
    }

    /// SHA-256 of canonical JSON.
    pub fn sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        let json = self.canonical_json();
        let hash = Sha256::digest(json.as_bytes());
        format!("sha256:{}", hex::encode(hash))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureNormalization {
    pub names: Vec<String>,
    pub median: Vec<f64>,
    pub scale: Vec<f64>,
    pub effective_dimension_mask: Vec<bool>,
    pub effective_dimension_count: usize,
    pub fitted_sample_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OutcomeLabelParameters {
    pub volatility_lookback_bars: usize,
    pub upper_barrier_volatility_multiple: f64,
    pub lower_barrier_volatility_multiple: f64,
    pub maximum_horizon_bars: usize,
    pub up_down_symmetric: bool,
    pub simultaneous_hit_rule: String,
    pub no_hit_rule: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NeighborParameters {
    pub neighbor_count: usize,
    pub minimum_support: usize,
    pub maximum_distance: f64,
    pub distance_power: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicationPolicy {
    pub minimum_direction_edge_bp: u16,
    pub minimum_reliability_bp: u16,
    pub parameters_selected_on: String,
    pub reliability_evaluated_on: String,
    pub test_outcomes_used_for_parameter_selection: bool,
    pub requires_positive_brier_skill: bool,
    pub profile_eligible_for_publication: bool,
    pub preregistered_protocol_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CalibratedSample {
    pub timestamp_ns: i64,
    pub vector: Vec<f64>,
    pub availability_mask: Vec<bool>,
    pub direction: Direction,
    pub first_passage_bars: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HeldOutReliability {
    pub correct: usize,
    pub evaluated: usize,
    pub reliability_bp: u16,
    pub reliability_lower_bound_bp: u16,
    pub confidence_level_bp: u16,
    pub balanced_accuracy_bp: u16,
    pub multiclass_brier_score: f64,
    pub climatology_brier_score: f64,
    pub brier_skill_score: f64,
    pub by_direction_bp: BTreeMap<String, u16>,
    pub by_direction_lower_bound_bp: BTreeMap<String, u16>,
    pub by_actual_direction_bp: BTreeMap<String, u16>,
    pub actual_support: BTreeMap<String, usize>,
    pub predicted_support: BTreeMap<String, usize>,
    pub confusion_matrix: BTreeMap<String, BTreeMap<String, usize>>,
    pub temporal_split_timestamp_ns: i64,
    pub untouched_test: bool,
    pub evidence_status: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeatureDiagnostic {
    pub name: String,
    pub available_samples: usize,
    pub availability_bp: u16,
    pub empirically_variable: bool,
    pub included_in_distance: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CalibrationDiagnostics {
    pub train_samples: usize,
    pub validation_samples: usize,
    pub evaluation_tail_samples: usize,
    pub train_label_counts: BTreeMap<String, usize>,
    pub validation_label_counts: BTreeMap<String, usize>,
    pub test_label_counts: BTreeMap<String, usize>,
    pub total_vector_dimensions: usize,
    pub effective_vector_dimensions: usize,
    pub features: Vec<FeatureDiagnostic>,
    pub upper_lower_barrier_ratio: f64,
    pub d_o_transport_status_counts: BTreeMap<String, usize>,
    pub d_o_transport_evaluable_bp: u16,
    pub odce_adaptive_organization_available_bp: u16,
    pub k_mem_strictly_prior_available_bp: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolutionCalibrationProfile {
    pub schema: String,
    pub calibration_version: String,
    pub profile_id: String,
    pub instrument_id: String,
    pub asset_class: AssetClass,
    pub timeframe: Timeframe,
    pub scope: CalibrationScope,
    pub engine_version: String,
    pub structural_vector_version: String,
    pub calibration_start_ns: i64,
    pub calibration_end_ns: i64,
    pub normalization: FeatureNormalization,
    pub outcome_label: OutcomeLabelParameters,
    pub estimator: NeighborParameters,
    pub publication: PublicationPolicy,
    pub reliability: HeldOutReliability,
    pub diagnostics: CalibrationDiagnostics,
    pub samples: Vec<CalibratedSample>,
    pub prefix_causality_verified: bool,
    pub runtime_recalibration: bool,
    pub profile_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DirectionalResolution {
    pub direction: Direction,
    pub probabilities_bp: Option<ProbabilitiesBp>,
    pub horizon: Option<Horizon>,
    pub reliability_bp: Option<u16>,
    pub sample_support: u64,
    pub calibration_scope: CalibrationScope,
    pub profile_sha256: String,
    pub publication_reason: String,
}

#[derive(Debug, Error)]
pub enum CalibrationError {
    #[error("calibration needs aligned closed bars and structural frames")]
    Alignment,
    #[error("calibration corpus is too short for a temporal train/held-out split")]
    InsufficientData,
    #[error("calibration contains a non-finite value")]
    NonFinite,
    #[error("resolution profile contract is invalid: {0}")]
    InvalidProfile(String),
    #[error("profile is not compatible with the structural vector: {0}")]
    Incompatible(String),
    #[error("neighbor anatomy diagnostic failed: {0}")]
    Diagnostic(String),
    #[error("canonical hashing failed: {0}")]
    Hash(#[from] canonical::CanonicalError),
}

#[derive(Debug, Clone)]
struct Candidate {
    timestamp_ns: i64,
    raw: Vec<Option<f64>>,
    mask: Vec<bool>,
    direction: Direction,
    first_passage_bars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeLabelState {
    Resolved(Direction, usize),
    RightCensored { observed_future_bars: usize },
}

#[derive(Debug, Clone, PartialEq)]
struct VoteResult {
    probabilities: ProbabilitiesBp,
    support: usize,
    winning: Direction,
    edge_bp: u16,
    passages: Vec<(f64, usize)>,
}

#[derive(Debug, Clone)]
struct DistanceBreakdown {
    distance: f64,
    dimension_mask: Vec<bool>,
    active_dimension_count: usize,
    normalized_abs_delta: Vec<f64>,
    dimension_contribution: Vec<f64>,
    zero_distance: bool,
}

#[derive(Debug, Clone)]
struct EvaluatedNeighbor<'a> {
    sample: &'a CalibratedSample,
    breakdown: DistanceBreakdown,
    weight: f64,
}

#[derive(Debug, Clone)]
struct VoteEvaluation<'a> {
    result: VoteResult,
    neighbors: Vec<EvaluatedNeighbor<'a>>,
    weighted_mass: [f64; 3],
    unweighted_count: [usize; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticClassMass {
    #[serde(rename = "UP")]
    pub up: f64,
    #[serde(rename = "DOWN")]
    pub down: f64,
    #[serde(rename = "RANGE")]
    pub range: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticClassCount {
    #[serde(rename = "UP")]
    pub up: usize,
    #[serde(rename = "DOWN")]
    pub down: usize,
    #[serde(rename = "RANGE")]
    pub range: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeighborAnatomyRecord {
    pub rank: usize,
    pub neighbor_timestamp_ns: i64,
    pub distance: f64,
    pub direction: Direction,
    pub weight: f64,
    pub neighbor_availability_mask: Vec<bool>,
    pub distance_dimension_mask: Vec<bool>,
    pub active_dimension_count: usize,
    pub normalized_abs_delta: Vec<f64>,
    pub dimension_contribution: Vec<f64>,
    pub contribution_basis: String,
    pub zero_distance: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeighborAnatomyQuery {
    pub query_timestamp_ns: i64,
    pub query_vector: Vec<f64>,
    pub query_availability_mask: Vec<bool>,
    pub actual_direction: Direction,
    pub requested_neighbor_count: usize,
    pub selected_neighbor_count: usize,
    pub selection_note: String,
    pub neighbors: Vec<NeighborAnatomyRecord>,
    pub weighted_mass: DiagnosticClassMass,
    pub normalized_weighted_mass: DiagnosticClassMass,
    pub total_weighted_mass: f64,
    pub zero_total_weighted_mass: bool,
    pub unweighted_count: DiagnosticClassCount,
    pub nearest_range_rank: Option<usize>,
    pub predicted_direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopDistanceDimension {
    pub dimension: String,
    pub mean_contribution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActualRangeDiagnostic {
    pub query_timestamp_ns: i64,
    pub predicted_direction: Direction,
    pub neighbor_counts: DiagnosticClassCount,
    pub normalized_weighted_mass: DiagnosticClassMass,
    pub nearest_range_rank: Option<usize>,
    pub top_distance_dimensions: Vec<TopDistanceDimension>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NeighborAnatomySummary {
    pub diagnostic: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub number_of_neighbors: usize,
    pub audit_minimum_support: usize,
    pub profile_runtime_minimum_support: usize,
    pub effective_dimension_count: usize,
    pub audit_points: usize,
    pub actual_direction_counts: DiagnosticClassCount,
    pub predicted_direction_counts: DiagnosticClassCount,
    pub actual_range_points: usize,
    pub actual_range_diagnostics: Vec<ActualRangeDiagnostic>,
    pub mean_normalized_class_mass: DiagnosticClassMass,
    pub evaluated_query_neighbor_pairs: usize,
    pub zero_distance_neighbor_pairs: usize,
    pub top_dimension_frequency_eligible_pairs: usize,
    pub top_dimension_tie_break: String,
    pub mean_dimension_contribution: BTreeMap<String, f64>,
    pub median_dimension_contribution: BTreeMap<String, f64>,
    pub top_dimension_frequency: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NeighborAnatomyArtifacts {
    pub audit_tail: Vec<NeighborAnatomyQuery>,
    pub summary: NeighborAnatomySummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDistanceStats {
    pub count: usize,
    pub minimum: Option<f64>,
    pub p10: Option<f64>,
    pub p25: Option<f64>,
    pub median: Option<f64>,
    pub p75: Option<f64>,
    pub maximum: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDistanceGeometry {
    #[serde(rename = "UP")]
    pub up: ClassDistanceStats,
    #[serde(rename = "DOWN")]
    pub down: ClassDistanceStats,
    #[serde(rename = "RANGE")]
    pub range: ClassDistanceStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopKClassComposition {
    pub actual_k_used: usize,
    #[serde(rename = "UP")]
    pub up: usize,
    #[serde(rename = "DOWN")]
    pub down: usize,
    #[serde(rename = "RANGE")]
    pub range: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearestClassCandidate {
    pub rank_among_all_admissible: usize,
    pub distance: f64,
    pub timestamp_ns: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NearestByClass {
    #[serde(rename = "UP")]
    pub up: Option<NearestClassCandidate>,
    #[serde(rename = "DOWN")]
    pub down: Option<NearestClassCandidate>,
    #[serde(rename = "RANGE")]
    pub range: Option<NearestClassCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeDistanceGeometryQuery {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub candidates_after_mask: usize,
    pub candidates_within_maximum_distance: usize,
    pub selected_neighbor_count: usize,
    pub runtime_minimum_support: usize,
    pub runtime_resolvable: bool,
    pub class_distance_stats: ClassDistanceGeometry,
    pub top_k_composition: BTreeMap<String, TopKClassComposition>,
    pub nearest_by_class: NearestByClass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedAuditObservation {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub selected_neighbor_count: usize,
    pub runtime_minimum_support: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DevelopmentAuditRuntimeParity {
    pub audit_observations: usize,
    pub resolved_observations: usize,
    pub unresolved_observations: usize,
    pub resolved_actual_direction_counts: DiagnosticClassCount,
    pub resolved_predicted_direction_counts: DiagnosticClassCount,
    pub unresolved_actual_direction_counts: DiagnosticClassCount,
    pub resolved_only_confusion_matrix: BTreeMap<String, BTreeMap<String, usize>>,
    pub unresolved_queries: Vec<UnresolvedAuditObservation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionalClassDistance {
    #[serde(rename = "UP")]
    pub up: Option<f64>,
    #[serde(rename = "DOWN")]
    pub down: Option<f64>,
    #[serde(rename = "RANGE")]
    pub range: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeDistanceGeometryAggregate {
    pub actual_range_queries: usize,
    pub runtime_resolvable_range_queries: usize,
    pub runtime_unresolved_range_queries: usize,
    pub mean_range_share_by_k: BTreeMap<String, f64>,
    pub median_range_rank: Option<f64>,
    pub mean_class_median_distance: OptionalClassDistance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeDistanceGeometryAudit {
    pub diagnostic: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub neighbor_count: usize,
    pub runtime_minimum_support: usize,
    pub maximum_distance: f64,
    pub distance: String,
    pub availability_rule: String,
    pub candidate_order: String,
    pub percentile_method: String,
    pub development_audit_runtime_parity: DevelopmentAuditRuntimeParity,
    pub queries: Vec<RangeDistanceGeometryQuery>,
    pub aggregate: RangeDistanceGeometryAggregate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SameClassFractionStats {
    pub requested_k: usize,
    pub evaluable_samples: usize,
    pub samples_with_full_k: usize,
    pub mean_actual_k_used: Option<f64>,
    pub minimum: Option<f64>,
    pub median: Option<f64>,
    pub mean: Option<f64>,
    pub maximum: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassCompactnessSummary {
    pub sample_count: usize,
    pub nearest_same_class_distance: ClassDistanceStats,
    pub nearest_other_class_distance: ClassDistanceStats,
    pub nearest_same_within_maximum_distance_fraction: Option<f64>,
    pub nearest_other_within_maximum_distance_fraction: Option<f64>,
    pub runtime_admissible_same_class_fraction_by_k: BTreeMap<String, SameClassFractionStats>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactnessByClass {
    #[serde(rename = "UP")]
    pub up: ClassCompactnessSummary,
    #[serde(rename = "DOWN")]
    pub down: ClassCompactnessSummary,
    #[serde(rename = "RANGE")]
    pub range: ClassCompactnessSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactnessView {
    pub candidate_time_rule: String,
    pub query_samples: usize,
    pub class_compactness: CompactnessByClass,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeIntraclassCompactnessAudit {
    pub diagnostic: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub labeled_sample_count: usize,
    pub labeled_class_counts: DiagnosticClassCount,
    pub self_neighbor_rule: String,
    pub availability_rule: String,
    pub distance: String,
    pub nearest_neighbor_cutoff_rule: String,
    pub top_k_cutoff_rule: String,
    pub maximum_distance: f64,
    pub k_values: Vec<usize>,
    pub leave_one_out_all_time: CompactnessView,
    pub causal_prefix: CompactnessView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeTrajectoryRecord {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub label_mechanism: String,
    pub configured_horizon_bars: usize,
    pub observed_label_path_bars: usize,
    pub first_passage_bars: usize,
    pub origin_close: f64,
    pub causal_label_volatility: f64,
    pub upper_barrier_return: f64,
    pub lower_barrier_return: f64,
    pub maximum_up_excursion: f64,
    pub maximum_down_excursion: f64,
    pub upper_excursion_ratio: f64,
    pub lower_excursion_ratio: f64,
    pub terminal_displacement: f64,
    pub realized_volatility: f64,
    pub direction_reversals: usize,
    pub time_of_maximum_up_excursion_bars: usize,
    pub time_of_maximum_down_excursion_bars: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeTrajectoryMetricSummary {
    pub upper_excursion_ratio: ClassDistanceStats,
    pub lower_excursion_ratio: ClassDistanceStats,
    pub maximum_up_excursion: ClassDistanceStats,
    pub maximum_down_excursion: ClassDistanceStats,
    pub terminal_displacement: ClassDistanceStats,
    pub realized_volatility: ClassDistanceStats,
    pub direction_reversals: ClassDistanceStats,
    pub time_of_maximum_up_excursion_bars: ClassDistanceStats,
    pub time_of_maximum_down_excursion_bars: ClassDistanceStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeTrajectoryAggregate {
    pub actual_range_samples: usize,
    pub label_mechanism_counts: BTreeMap<String, usize>,
    pub full_configured_horizon_samples: usize,
    pub truncated_horizon_samples: usize,
    pub upper_ratio_at_or_above_one: usize,
    pub lower_ratio_at_or_above_one: usize,
    pub both_ratios_at_or_above_one: usize,
    pub upper_lower_excursion_ratio_pearson_correlation: Option<f64>,
    pub all_range: RangeTrajectoryMetricSummary,
    pub by_label_mechanism: BTreeMap<String, RangeTrajectoryMetricSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeTrajectoryAnatomyAudit {
    pub diagnostic: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub configured_horizon_bars: usize,
    pub upper_barrier_volatility_multiple: f64,
    pub lower_barrier_volatility_multiple: f64,
    pub realized_volatility_definition: String,
    pub direction_reversal_definition: String,
    pub records: Vec<RangeTrajectoryRecord>,
    pub aggregate: RangeTrajectoryAggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RightCensoredAuditObservation {
    pub query_timestamp_ns: i64,
    pub observed_future_bars: usize,
    pub required_future_bars: usize,
    pub label_source_end_timestamp_ns: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelableAuditObservation {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub first_passage_bars: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RightCensoringAudit {
    pub diagnostic: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub query_cutoff_timestamp_ns: i64,
    pub label_source_end_timestamp_ns: i64,
    pub label_source_extends_query_cutoff: bool,
    pub label_source_bars_after_query_cutoff: usize,
    pub maximum_horizon_bars: usize,
    pub target_rule: String,
    pub audit_candidate_observations: usize,
    pub labelable_observations: usize,
    pub labelable_queries: Vec<LabelableAuditObservation>,
    pub right_censored_observations: usize,
    pub right_censored_queries: Vec<RightCensoredAuditObservation>,
    pub labelable_actual_direction_counts: DiagnosticClassCount,
    pub runtime_minimum_support: usize,
    pub runtime_resolved_observations: usize,
    pub runtime_support_unresolved_observations: usize,
    pub runtime_support_unresolved_queries: Vec<UnresolvedAuditObservation>,
    pub resolved_actual_direction_counts: DiagnosticClassCount,
    pub resolved_predicted_direction_counts: DiagnosticClassCount,
    pub resolved_only_confusion_matrix: BTreeMap<String, BTreeMap<String, usize>>,
    pub resolved_correct: usize,
    pub resolved_accuracy_bp: u16,
    pub resolved_balanced_accuracy_bp: u16,
    pub resolved_multiclass_brier_score: f64,
    pub resolved_climatology_brier_score: f64,
    pub resolved_brier_skill_score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsExperimentMetrics {
    pub observations: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub coverage: f64,
    pub correct: usize,
    pub accuracy: f64,
    pub balanced_accuracy: f64,
    pub multiclass_brier_score: f64,
    pub causal_climatology_brier_score: f64,
    pub brier_skill_score: f64,
    pub classwise_calibration_error: f64,
    pub actual_direction_counts: BTreeMap<String, usize>,
    pub predicted_direction_counts: BTreeMap<String, usize>,
    pub confusion_matrix: BTreeMap<String, BTreeMap<String, usize>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsExperimentEstimator {
    pub neighbor_count: usize,
    pub minimum_support: usize,
    pub maximum_distance: f64,
    pub distance_power: f64,
    pub selected_on: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsExperimentPrediction {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub predicted_direction: Direction,
    pub probabilities_bp: Option<ProbabilitiesBp>,
    pub selected_neighbor_count: usize,
    pub causal_library_size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsVariantAudit {
    pub variant: String,
    pub feature_families: Vec<String>,
    pub feature_names: Vec<String>,
    pub effective_feature_names: Vec<String>,
    pub ineffective_feature_names: Vec<String>,
    pub total_dimensions: usize,
    pub effective_dimensions: usize,
    pub normalization_fitted_samples: usize,
    pub estimator: DynamicsExperimentEstimator,
    pub validation: DynamicsExperimentMetrics,
    pub validation_predictions: Vec<DynamicsExperimentPrediction>,
    pub walk_forward_evaluation: DynamicsExperimentMetrics,
    pub predictions: Vec<DynamicsExperimentPrediction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsAblationAudit {
    pub diagnostic: String,
    pub experiment_id: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub base_structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub evidence_status: String,
    pub legacy_artifact_sources: Vec<String>,
    pub feature_definitions: BTreeMap<String, String>,
    pub label_source_end_timestamp_ns: i64,
    pub label_rule: String,
    pub temporal_method: String,
    pub normalization_rule: String,
    pub weekly_alignment_rule: String,
    pub common_eligible_timestamps: usize,
    pub common_training_observations: usize,
    pub common_validation_observations: usize,
    pub common_evaluation_observations: usize,
    pub variant_ranking: Vec<String>,
    pub ranking_rule: Vec<String>,
    pub variants: Vec<DynamicsVariantAudit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalResidualAssociation {
    pub feature_vs_up_residual: Option<f64>,
    pub feature_vs_down_residual: Option<f64>,
    pub feature_vs_range_residual: Option<f64>,
    pub feature_vs_brier_loss: Option<f64>,
    pub feature_vs_error_indicator: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalCalibrationFit {
    pub feature_mean_on_validation: f64,
    pub feature_std_on_validation: f64,
    pub intercept_only_residual: BTreeMap<String, f64>,
    pub feature_residual_slope: BTreeMap<String, f64>,
    pub probability_projection: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalCounterfactualMetrics {
    pub observations: usize,
    pub multiclass_brier_score: f64,
    pub brier_delta_vs_raw_a: f64,
    pub brier_delta_vs_intercept_only: Option<f64>,
    pub accuracy: f64,
    pub predicted_direction_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalClassFeatureStats {
    pub count: usize,
    pub mean: Option<f64>,
    pub median: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalFeatureSupportGeometry {
    pub validation_standardized_minimum: f64,
    pub validation_standardized_maximum: f64,
    pub evaluation_standardized_minimum: f64,
    pub evaluation_standardized_maximum: f64,
    pub evaluation_below_validation_minimum: usize,
    pub evaluation_above_validation_maximum: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalQueryRecord {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub baseline_direction: Direction,
    pub baseline_probabilities_bp: ProbabilitiesBp,
    pub baseline_top_two_edge_bp: u16,
    pub baseline_brier_loss: f64,
    pub baseline_correct: bool,
    pub feature_value: f64,
    pub standardized_feature_value: f64,
    pub feature_only_probabilities: [f64; 3],
    pub feature_only_direction: Direction,
    pub bounded_standardized_feature_value: f64,
    pub bounded_feature_only_probabilities: [f64; 3],
    pub bounded_feature_only_direction: Direction,
    pub intercept_only_probabilities: [f64; 3],
    pub feature_adjusted_probabilities: [f64; 3],
    pub feature_adjusted_direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionalFeatureAudit {
    pub feature: String,
    pub validation_observations: usize,
    pub evaluation_observations: usize,
    pub fit: ConditionalCalibrationFit,
    pub support_geometry: ConditionalFeatureSupportGeometry,
    pub validation_residual_association: ConditionalResidualAssociation,
    pub evaluation_residual_association: ConditionalResidualAssociation,
    pub raw_a_metrics: ConditionalCounterfactualMetrics,
    pub feature_only_metrics: ConditionalCounterfactualMetrics,
    pub bounded_feature_only_metrics: ConditionalCounterfactualMetrics,
    pub intercept_only_metrics: ConditionalCounterfactualMetrics,
    pub feature_adjusted_metrics: ConditionalCounterfactualMetrics,
    pub baseline_predicted_up_by_actual_direction: BTreeMap<String, ConditionalClassFeatureStats>,
    pub evaluation_queries: Vec<ConditionalQueryRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsConditionalInformationAudit {
    pub diagnostic: String,
    pub experiment_id: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub base_structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub evidence_status: String,
    pub baseline_resolver: String,
    pub conditional_method: String,
    pub fitting_partition: String,
    pub evaluation_partition: String,
    pub probability_projection: String,
    pub tested_features: Vec<String>,
    pub feature_ranking_by_feature_only_brier_delta_vs_raw_a: Vec<String>,
    pub feature_ranking_by_bounded_brier_delta_vs_raw_a: Vec<String>,
    pub feature_ranking_by_brier_delta_vs_intercept_only: Vec<String>,
    pub baseline_validation_metrics: DynamicsExperimentMetrics,
    pub baseline_walk_forward_metrics: DynamicsExperimentMetrics,
    pub features: Vec<ConditionalFeatureAudit>,
    pub runtime_or_profile_modified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequentialResidualQueryRecord {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub a_probabilities: [f64; 3],
    pub a_direction: Direction,
    pub velocity_standardized_raw: f64,
    pub velocity_standardized_bounded: f64,
    pub acceleration_standardized_raw: f64,
    pub acceleration_standardized_bounded: f64,
    pub a_plus_velocity_probabilities: [f64; 3],
    pub a_plus_velocity_direction: Direction,
    pub a_plus_velocity_plus_acceleration_probabilities: [f64; 3],
    pub a_plus_velocity_plus_acceleration_direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsSequentialResidualAudit {
    pub diagnostic: String,
    pub experiment_id: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub base_structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub evidence_status: String,
    pub method: String,
    pub validation_observations: usize,
    pub evaluation_observations: usize,
    pub velocity_fit: ConditionalCalibrationFit,
    pub velocity_support: ConditionalFeatureSupportGeometry,
    pub acceleration_fit_against_a: ConditionalCalibrationFit,
    pub acceleration_fit_after_velocity: ConditionalCalibrationFit,
    pub acceleration_support: ConditionalFeatureSupportGeometry,
    pub acceleration_after_velocity_validation_association: ConditionalResidualAssociation,
    pub acceleration_after_velocity_evaluation_association: ConditionalResidualAssociation,
    pub a_metrics: ConditionalCounterfactualMetrics,
    pub a_plus_bounded_velocity_metrics: ConditionalCounterfactualMetrics,
    pub a_plus_bounded_acceleration_metrics: ConditionalCounterfactualMetrics,
    pub a_plus_bounded_velocity_plus_bounded_acceleration_metrics: ConditionalCounterfactualMetrics,
    pub queries: Vec<SequentialResidualQueryRecord>,
    pub runtime_or_profile_modified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenVelocityForwardQueryRecord {
    pub query_timestamp_ns: i64,
    pub actual_direction: Direction,
    pub causal_library_size: usize,
    pub selected_neighbor_count: usize,
    pub baseline_probabilities: Option<[f64; 3]>,
    pub baseline_direction: Direction,
    pub velocity_value: Option<f64>,
    pub velocity_standardized_raw: Option<f64>,
    pub velocity_standardized_bounded: Option<f64>,
    pub corrected_probabilities: Option<[f64; 3]>,
    pub corrected_direction: Direction,
    pub decision_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicsFrozenVelocityForwardAudit {
    pub diagnostic: String,
    pub experiment_id: String,
    pub instrument_id: String,
    pub timeframe: Timeframe,
    pub base_structural_vector_version: String,
    pub calibration_profile_id: String,
    pub calibration_profile_sha256: String,
    pub diagnostic_generation_timestamp_unix_seconds: u64,
    pub evidence_status: String,
    pub method: String,
    pub frozen_feature_cutoff_timestamp_ns: i64,
    pub frozen_feature_source_end_timestamp_ns: i64,
    pub forward_feature_start_timestamp_ns: i64,
    pub forward_feature_end_timestamp_ns: i64,
    pub label_source_end_timestamp_ns: i64,
    pub required_horizon_bars: usize,
    pub frozen_estimator: DynamicsExperimentEstimator,
    pub frozen_velocity_fit: ConditionalCalibrationFit,
    pub forward_velocity_support: ConditionalFeatureSupportGeometry,
    pub source_a_metrics: ConditionalCounterfactualMetrics,
    pub source_a_plus_bounded_velocity_metrics: ConditionalCounterfactualMetrics,
    pub forward_a_metrics: DynamicsExperimentMetrics,
    pub forward_a_plus_bounded_velocity_metrics: DynamicsExperimentMetrics,
    pub queries: Vec<FrozenVelocityForwardQueryRecord>,
    pub slope_or_support_refitted_on_forward: bool,
    pub runtime_or_profile_modified: bool,
}

#[derive(Debug, Clone)]
struct DynamicsCandidate {
    row: Candidate,
    maturity_timestamp_ns: i64,
}

#[derive(Debug, Clone)]
struct DynamicsScoredCase {
    case: ScoredCase,
    climatology: [f64; 3],
}

#[derive(Debug, Clone)]
struct DynamicsScore {
    prediction: DynamicsExperimentPrediction,
    scored: Option<DynamicsScoredCase>,
}

#[derive(Debug, Clone)]
struct ConditionalRow {
    prediction: DynamicsExperimentPrediction,
    probabilities: [f64; 3],
    feature_value: f64,
    standardized_feature_value: f64,
}

#[derive(Debug, Clone)]
struct CompactnessObservation {
    direction: Direction,
    nearest_same_class_distance: Option<f64>,
    nearest_other_class_distance: Option<f64>,
    nearest_same_within_maximum_distance: Option<bool>,
    nearest_other_within_maximum_distance: Option<bool>,
    same_class_fraction_by_k: BTreeMap<String, (usize, f64)>,
}

#[derive(Debug, Clone)]
struct ScoredCase {
    actual: Direction,
    predicted: Direction,
    probabilities: ProbabilitiesBp,
    correct: bool,
}

pub fn build_resolution_profile(
    instrument_id: &str,
    asset_class: AssetClass,
    timeframe: Timeframe,
    engine_version: &str,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    preregistered_protocol_sha256: Option<&str>,
) -> Result<ResolutionCalibrationProfile, CalibrationError> {
    if instrument_id.trim().is_empty()
        || engine_version.trim().is_empty()
        || bars.len() < 16
        || frames.len() < 8
        || bars
            .iter()
            .any(|bar| !bar.is_closed || bar.timeframe != timeframe)
    {
        return Err(CalibrationError::InsufficientData);
    }
    let bar_positions: BTreeMap<i64, usize> = bars
        .iter()
        .enumerate()
        .map(|(index, bar)| (bar.close_time_ns, index))
        .collect();
    if bar_positions.len() != bars.len() {
        return Err(CalibrationError::Alignment);
    }

    // Every parameter below is generated from corpus size or empirical
    // distributions and then frozen in the artifact.
    let volatility_lookback = integer_sqrt(bars.len()).max(2);
    let maximum_horizon = integer_log2(bars.len()).max(2);
    let test_count = integer_sqrt(frames.len()).max(1);
    let validation_count = test_count;
    let test_split_frame = frames
        .len()
        .checked_sub(test_count)
        .filter(|split| *split >= 4)
        .ok_or(CalibrationError::InsufficientData)?;
    let validation_split_frame = test_split_frame
        .checked_sub(validation_count)
        .filter(|split| *split >= 4)
        .ok_or(CalibrationError::InsufficientData)?;
    let validation_split_timestamp = frames[validation_split_frame].timestamp_ns;
    let test_split_timestamp = frames[test_split_frame].timestamp_ns;
    let development_bar = *bar_positions
        .get(&validation_split_timestamp)
        .ok_or(CalibrationError::Alignment)?;

    let volatility: Vec<Option<f64>> = (0..bars.len())
        .map(|index| causal_volatility(bars, index, volatility_lookback))
        .collect();
    let symmetric_multiple =
        empirical_symmetric_barrier(bars, &volatility, development_bar, maximum_horizon)?;
    let outcome = OutcomeLabelParameters {
        volatility_lookback_bars: volatility_lookback,
        upper_barrier_volatility_multiple: symmetric_multiple,
        lower_barrier_volatility_multiple: symmetric_multiple,
        maximum_horizon_bars: maximum_horizon,
        up_down_symmetric: true,
        simultaneous_hit_rule: "RANGE".to_owned(),
        no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".to_owned(),
    };

    let mut candidates = Vec::new();
    for frame in frames {
        let Some(&bar_index) = bar_positions.get(&frame.timestamp_ns) else {
            return Err(CalibrationError::Alignment);
        };
        let Some(volatility) = volatility[bar_index] else {
            continue;
        };
        let Some((direction, first_passage_bars)) =
            label_outcome(bars, bar_index, volatility, &outcome)
        else {
            continue;
        };
        if frame.vector.values.len() != frame.vector.availability_mask.len()
            || frame
                .vector
                .values
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
        {
            return Err(CalibrationError::NonFinite);
        }
        candidates.push(Candidate {
            timestamp_ns: frame.timestamp_ns,
            raw: frame.vector.values.clone(),
            mask: frame.vector.availability_mask.clone(),
            direction,
            first_passage_bars,
        });
    }
    let validation_split =
        candidates.partition_point(|row| row.timestamp_ns < validation_split_timestamp);
    let test_split = candidates.partition_point(|row| row.timestamp_ns < test_split_timestamp);
    if validation_split < 4
        || test_split <= validation_split
        || candidates.len().saturating_sub(test_split) < 1
    {
        return Err(CalibrationError::InsufficientData);
    }
    let training = &candidates[..validation_split];
    let validation = &candidates[validation_split..test_split];
    let untouched_test = &candidates[test_split..];
    let normalization = fit_normalization(&frames[0].vector.names, training)?;
    let training_samples: Vec<CalibratedSample> = training
        .iter()
        .map(|row| normalized_sample(row, &normalization))
        .collect::<Result<_, _>>()?;

    let neighbor_count = integer_sqrt(training_samples.len()).max(1);
    let minimum_support = integer_log2(training_samples.len())
        .max(1)
        .min(neighbor_count);
    let validation_vectors: Vec<CalibratedSample> = validation
        .iter()
        .map(|row| normalized_sample(row, &normalization))
        .collect::<Result<_, _>>()?;
    let maximum_distance = validation_vectors
        .iter()
        .filter_map(|query| kth_distance(query, &training_samples, neighbor_count))
        .reduce(f64::max)
        .ok_or(CalibrationError::InsufficientData)?
        .max(f64::EPSILON);

    let power_one = score_power(
        &validation_vectors,
        &training_samples,
        neighbor_count,
        minimum_support,
        maximum_distance,
        1.0,
    );
    let power_two = score_power(
        &validation_vectors,
        &training_samples,
        neighbor_count,
        minimum_support,
        maximum_distance,
        2.0,
    );
    let distance_power = if power_two.0 > power_one.0 { 2.0 } else { 1.0 };
    let validation_scored = score_power(
        &validation_vectors,
        &training_samples,
        neighbor_count,
        minimum_support,
        maximum_distance,
        distance_power,
    );
    let validation_reliability_lower_bound_bp =
        wilson_lower_bp(validation_scored.0, validation_scored.1);
    let minimum_direction_edge_bp = median_u16(&mut validation_scored.2.clone()).unwrap_or(10_000);
    let protocol_hash = preregistered_protocol_sha256
        .map(str::to_owned)
        .filter(|value| valid_sha256(value));
    if preregistered_protocol_sha256.is_some() && protocol_hash.is_none() {
        return Err(CalibrationError::InvalidProfile(
            "invalid preregistered protocol hash".into(),
        ));
    }
    let preregistered = protocol_hash.is_some();
    let publication = PublicationPolicy {
        minimum_direction_edge_bp,
        minimum_reliability_bp: validation_reliability_lower_bound_bp,
        parameters_selected_on: "TEMPORAL_VALIDATION".into(),
        reliability_evaluated_on: if preregistered {
            "UNTOUCHED_TEMPORAL_TEST"
        } else {
            "CONSUMED_DEVELOPMENT_AUDIT"
        }
        .into(),
        test_outcomes_used_for_parameter_selection: false,
        requires_positive_brier_skill: true,
        profile_eligible_for_publication: preregistered,
        preregistered_protocol_sha256: protocol_hash,
    };

    // Once every parameter is frozen on train/validation, validation samples
    // may join the runtime library. Test labels remain outside the library.
    let mut samples = training_samples;
    samples.extend(validation_vectors);
    let test_vectors: Vec<CalibratedSample> = untouched_test
        .iter()
        .map(|row| normalized_sample(row, &normalization))
        .collect::<Result<_, _>>()?;
    let test_scored = score_power(
        &test_vectors,
        &samples,
        neighbor_count,
        minimum_support,
        maximum_distance,
        distance_power,
    );
    let reliability_bp = ratio_bp(test_scored.0, test_scored.1);
    let test_brier = multiclass_brier_score(&test_scored.3);
    let climatology_brier = climatology_brier_score(&test_scored.3, &samples);
    let reliability = HeldOutReliability {
        correct: test_scored.0,
        evaluated: test_scored.1,
        reliability_bp,
        reliability_lower_bound_bp: wilson_lower_bp(test_scored.0, test_scored.1),
        confidence_level_bp: 9_500,
        balanced_accuracy_bp: balanced_accuracy_bp(&test_scored.3),
        multiclass_brier_score: test_brier,
        climatology_brier_score: climatology_brier,
        brier_skill_score: if climatology_brier > 0.0 {
            1.0 - test_brier / climatology_brier
        } else {
            0.0
        },
        by_direction_bp: direction_reliability(&test_scored.3),
        by_direction_lower_bound_bp: direction_reliability_lower_bounds(&test_scored.3),
        by_actual_direction_bp: actual_direction_recall(&test_scored.3),
        actual_support: direction_support(&test_scored.3, true),
        predicted_support: direction_support(&test_scored.3, false),
        confusion_matrix: confusion_matrix(&test_scored.3),
        temporal_split_timestamp_ns: test_split_timestamp,
        untouched_test: preregistered,
        evidence_status: if preregistered {
            "PREREGISTERED_UNTOUCHED_TEST"
        } else {
            "DEVELOPMENT_AUDIT_CONSUMED"
        }
        .into(),
    };
    let diagnostics = calibration_diagnostics(
        training,
        validation,
        untouched_test,
        &normalization,
        &frames[..validation_split_frame],
    );

    let mut profile = ResolutionCalibrationProfile {
        schema: RESOLUTION_PROFILE_SCHEMA.to_owned(),
        calibration_version: RESOLUTION_CALIBRATION_VERSION.to_owned(),
        profile_id: format!("{instrument_id}:{timeframe:?}:{RESOLUTION_CALIBRATION_VERSION}"),
        instrument_id: instrument_id.to_owned(),
        asset_class,
        timeframe,
        scope: CalibrationScope::Instrument,
        engine_version: engine_version.to_owned(),
        structural_vector_version: STRUCTURAL_VECTOR_VERSION.to_owned(),
        calibration_start_ns: candidates.first().expect("nonempty").timestamp_ns,
        calibration_end_ns: candidates.last().expect("nonempty").timestamp_ns,
        normalization,
        outcome_label: outcome,
        estimator: NeighborParameters {
            neighbor_count,
            minimum_support,
            maximum_distance,
            distance_power,
        },
        publication,
        reliability,
        diagnostics,
        samples,
        prefix_causality_verified: true,
        runtime_recalibration: false,
        profile_sha256: None,
    };
    profile.profile_sha256 = Some(canonical::sha256(&profile)?);
    validate_profile(&profile)?;
    Ok(profile)
}

/// Compare the frozen v2 snapshot against nested causal dynamics families.
/// This is a development artifact only: it does not modify or serialize a
/// runtime calibration profile.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamics_ablation_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    label_bars: &[MarketObservation],
    daily_frames: &[StructuralFrame],
    weekly_frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<DynamicsAblationAudit, CalibrationError> {
    validate_profile(profile)?;
    if timeframe != Timeframe::D1
        || profile.instrument_id != instrument_id
        || profile.timeframe != timeframe
        || daily_frames.len() < 16
        || weekly_frames.is_empty()
    {
        return Err(CalibrationError::Incompatible(
            "dynamics ablation requires the matching D1 profile and D1/W1 trajectories".into(),
        ));
    }
    let variants = build_dynamics_variants(daily_frames, weekly_frames)
        .map_err(|error| CalibrationError::Diagnostic(error.to_string()))?;
    let base_dimension = daily_frames[0].vector.names.len();
    let common_timestamps: Vec<i64> = variants[&DynamicsVariant::D]
        .iter()
        .filter(|frame| {
            frame
                .values
                .iter()
                .skip(base_dimension)
                .all(Option::is_some)
        })
        .map(|frame| frame.timestamp_ns)
        .filter(|timestamp| *timestamp <= profile.calibration_end_ns)
        .collect();
    if common_timestamps.len() < 16 {
        let d_frames = &variants[&DynamicsVariant::D];
        let added_availability: Vec<String> = d_frames[0]
            .names
            .iter()
            .enumerate()
            .skip(base_dimension)
            .map(|(axis, name)| {
                format!(
                    "{name}={}",
                    d_frames
                        .iter()
                        .filter(|frame| frame.values[axis].is_some())
                        .count()
                )
            })
            .collect();
        return Err(CalibrationError::Diagnostic(format!(
            "only {} timestamps have complete added dynamics; availability: {}",
            common_timestamps.len(),
            added_availability.join(", ")
        )));
    }

    let bar_positions: BTreeMap<i64, usize> = label_bars
        .iter()
        .enumerate()
        .map(|(index, bar)| (bar.close_time_ns, index))
        .collect();
    let volatility: Vec<Option<f64>> = (0..label_bars.len())
        .map(|index| {
            causal_volatility(
                label_bars,
                index,
                profile.outcome_label.volatility_lookback_bars,
            )
        })
        .collect();
    let test_split_timestamp = profile.reliability.temporal_split_timestamp_ns;
    let frozen_query_prefix_len =
        daily_frames.partition_point(|frame| frame.timestamp_ns <= profile.calibration_end_ns);
    let frozen_feature_prefix_len = (frozen_query_prefix_len
        + usize::from(frozen_query_prefix_len < daily_frames.len()))
    .min(daily_frames.len());
    let split_count = integer_sqrt(frozen_feature_prefix_len).max(1);
    let validation_split_frame = frozen_feature_prefix_len
        .checked_sub(split_count * 2)
        .ok_or(CalibrationError::InsufficientData)?;
    let validation_split_timestamp = daily_frames[validation_split_frame].timestamp_ns;

    let mut candidate_sets = BTreeMap::new();
    for variant in DynamicsVariant::ALL {
        let by_timestamp: BTreeMap<i64, &ExperimentalVectorFrame> = variants[&variant]
            .iter()
            .map(|frame| (frame.timestamp_ns, frame))
            .collect();
        let mut candidates = Vec::new();
        for timestamp in &common_timestamps {
            let frame = by_timestamp
                .get(timestamp)
                .ok_or(CalibrationError::Alignment)?;
            let bar_index = *bar_positions
                .get(timestamp)
                .ok_or(CalibrationError::Alignment)?;
            let Some(scale) = volatility[bar_index] else {
                continue;
            };
            let Some((direction, first_passage_bars)) =
                label_outcome(label_bars, bar_index, scale, &profile.outcome_label)
            else {
                continue;
            };
            let maturity_index = bar_index
                .checked_add(first_passage_bars)
                .filter(|index| *index < label_bars.len())
                .ok_or(CalibrationError::Alignment)?;
            candidates.push(DynamicsCandidate {
                row: Candidate {
                    timestamp_ns: *timestamp,
                    raw: frame.values.clone(),
                    mask: frame.availability_mask.clone(),
                    direction,
                    first_passage_bars,
                },
                maturity_timestamp_ns: label_bars[maturity_index].close_time_ns,
            });
        }
        candidate_sets.insert(variant, candidates);
    }
    let common_labeled_timestamps: Vec<i64> = candidate_sets[&DynamicsVariant::D]
        .iter()
        .map(|candidate| candidate.row.timestamp_ns)
        .collect();
    for candidates in candidate_sets.values_mut() {
        candidates.retain(|candidate| {
            common_labeled_timestamps
                .binary_search(&candidate.row.timestamp_ns)
                .is_ok()
        });
    }
    let training_count = common_labeled_timestamps
        .partition_point(|timestamp| *timestamp < validation_split_timestamp);
    let validation_end =
        common_labeled_timestamps.partition_point(|timestamp| *timestamp < test_split_timestamp);
    if training_count < 4
        || validation_end <= training_count
        || validation_end >= common_labeled_timestamps.len()
    {
        return Err(CalibrationError::Diagnostic(format!(
            "invalid common split: train={training_count}, validation_end={validation_end}, total={}",
            common_labeled_timestamps.len()
        )));
    }

    let mut variant_audits = Vec::new();
    for variant in DynamicsVariant::ALL {
        let candidates = &candidate_sets[&variant];
        let training = &candidates[..training_count];
        let validation = &candidates[training_count..validation_end];
        let evaluation = &candidates[validation_end..];
        let names = &variants[&variant][0].names;
        let training_rows: Vec<Candidate> = training.iter().map(|row| row.row.clone()).collect();
        let normalization = fit_normalization(names, &training_rows)?;
        let neighbor_count = integer_sqrt(training.len()).max(1);
        let minimum_support = integer_log2(training.len()).max(1).min(neighbor_count);
        let maximum_distance = causal_validation_maximum_distance(
            validation,
            training,
            &normalization,
            neighbor_count,
        )
        .map_err(|error| {
            CalibrationError::Diagnostic(format!(
                "{} maximum-distance calibration failed: {error}",
                variant.label()
            ))
        })?;
        let power_one = score_dynamics_walk_forward(
            validation,
            training,
            &normalization,
            &score_parameters(neighbor_count, minimum_support, maximum_distance, 1.0),
        )?;
        let power_two = score_dynamics_walk_forward(
            validation,
            training,
            &normalization,
            &score_parameters(neighbor_count, minimum_support, maximum_distance, 2.0),
        )?;
        let power_one_metrics = dynamics_metrics(validation, &power_one);
        let power_two_metrics = dynamics_metrics(validation, &power_two);
        let distance_power = if dynamics_metric_order(
            &power_two_metrics,
            normalization.effective_dimension_count,
            &power_one_metrics,
            normalization.effective_dimension_count,
        )
        .is_gt()
        {
            2.0
        } else {
            1.0
        };
        let parameters = score_parameters(
            neighbor_count,
            minimum_support,
            maximum_distance,
            distance_power,
        );
        let validation_scored =
            score_dynamics_walk_forward(validation, training, &normalization, &parameters)?;
        let prior_pool: Vec<DynamicsCandidate> = candidates[..validation_end].to_vec();
        let evaluation_scored =
            score_dynamics_walk_forward(evaluation, &prior_pool, &normalization, &parameters)?;
        variant_audits.push(DynamicsVariantAudit {
            variant: variant.label().to_owned(),
            feature_families: variant_feature_families(variant),
            feature_names: normalization.names.clone(),
            effective_feature_names: normalization
                .names
                .iter()
                .zip(&normalization.effective_dimension_mask)
                .filter(|(_, effective)| **effective)
                .map(|(name, _)| name.clone())
                .collect(),
            ineffective_feature_names: normalization
                .names
                .iter()
                .zip(&normalization.effective_dimension_mask)
                .filter(|(_, effective)| !**effective)
                .map(|(name, _)| name.clone())
                .collect(),
            total_dimensions: normalization.names.len(),
            effective_dimensions: normalization.effective_dimension_count,
            normalization_fitted_samples: normalization.fitted_sample_count,
            estimator: DynamicsExperimentEstimator {
                neighbor_count,
                minimum_support,
                maximum_distance,
                distance_power,
                selected_on: "CAUSAL_TEMPORAL_VALIDATION".into(),
            },
            validation: dynamics_metrics(validation, &validation_scored),
            validation_predictions: validation_scored
                .iter()
                .map(|scored| scored.prediction.clone())
                .collect(),
            walk_forward_evaluation: dynamics_metrics(evaluation, &evaluation_scored),
            predictions: evaluation_scored
                .iter()
                .map(|scored| scored.prediction.clone())
                .collect(),
        });
    }
    let mut ranking: Vec<&DynamicsVariantAudit> = variant_audits.iter().collect();
    ranking.sort_by(|left, right| {
        dynamics_metric_order(
            &right.walk_forward_evaluation,
            right.effective_dimensions,
            &left.walk_forward_evaluation,
            left.effective_dimensions,
        )
        .then_with(|| left.variant.cmp(&right.variant))
    });
    Ok(DynamicsAblationAudit {
        diagnostic: "PRAMAGRAPH_FINANCIAL_DYNAMICS_ABLATION".into(),
        experiment_id: DYNAMICS_EXPERIMENT_ID.into(),
        instrument_id: instrument_id.to_owned(),
        timeframe,
        base_structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().expect("validated profile"),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        evidence_status: "DEVELOPMENT_AUDIT_CONSUMED_NOT_A_PRODUCTION_CLAIM".into(),
        legacy_artifact_sources: vec![
            "PRAMA Protokol Sentinel Market/src/signals/engine.rs".into(),
            "PRAMA Protokol Sentinel Market/src/signals/types.rs".into(),
            "PRAMA Protokol Sentinel Market/src/calibration/optimizer.rs".into(),
            "PRAMA Protokol Sentinel Market/src/calibration/backtest.rs".into(),
            "PRAMA Protokol Sentinel Market/src/bin/calibrate.rs".into(),
        ],
        feature_definitions: dynamics_feature_definitions(),
        label_source_end_timestamp_ns: label_bars.last().expect("nonempty").close_time_ns,
        label_rule: "UNCHANGED_PROFILE_FIRST_PASSAGE_WITH_RIGHT_CENSORING".into(),
        temporal_method: "FROZEN_TRAIN_NORMALIZATION; CAUSAL_VALIDATION; WALK_FORWARD_EVALUATION; LABEL_ENTERS_LIBRARY_ONLY_AFTER_MATURITY_TIMESTAMP_LT_QUERY".into(),
        normalization_rule: "VARIANT_SPECIFIC_TRAIN_ONLY_MEDIAN_MAD; DYNAMIC_Z_SCORES_USE_STRICTLY_PRIOR_FEATURE_HISTORY".into(),
        weekly_alignment_rule: "LATEST_W1_STRUCTURAL_FRAME_FROM_STRICTLY_EARLIER_ISO_WEEK".into(),
        common_eligible_timestamps: common_labeled_timestamps.len(),
        common_training_observations: training_count,
        common_validation_observations: validation_end - training_count,
        common_evaluation_observations: common_labeled_timestamps.len() - validation_end,
        variant_ranking: ranking
            .iter()
            .map(|variant| variant.variant.clone())
            .collect(),
        ranking_rule: vec![
            "higher walk-forward Brier Skill".into(),
            "higher walk-forward balanced accuracy".into(),
            "lower classwise calibration error".into(),
            "higher coverage".into(),
            "lower effective dimension count".into(),
            "variant identifier for deterministic final tie-break".into(),
        ],
        variants: variant_audits,
    })
}

fn dynamics_feature_definitions() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "dynamics.delta_velocity".into(),
            "authoritative_prama_delta[t] - authoritative_prama_delta[t-1]".into(),
        ),
        (
            "dynamics.delta_acceleration".into(),
            "delta_velocity[t] - delta_velocity[t-1]".into(),
        ),
        (
            "dynamics.delta_reversal_magnitude".into(),
            "min(abs(velocity[t]), abs(velocity[t-1])) when their nonzero signs differ; otherwise 0"
                .into(),
        ),
        (
            "dynamics.pressure_z_prior".into(),
            "z-score of 1 - lambda[t] against the strictly-prior expanding distribution; 0 for a constant prior distribution"
                .into(),
        ),
        (
            "dynamics.momentum_z_prior".into(),
            "z-score of delta_velocity[t] against its strictly-prior expanding distribution"
                .into(),
        ),
        (
            "dynamics.transition_intensity_z_prior".into(),
            "z-score of pairwise-available RMS change in financial_structural_vector_v2 against its strictly-prior expanding distribution"
                .into(),
        ),
        (
            "dynamics.regime_age_bars".into(),
            "consecutive observations with the current authoritative D_O structural_state".into(),
        ),
        (
            "dynamics.momentum_sign_streak_bars".into(),
            "consecutive observations sharing the exact sign of delta_velocity".into(),
        ),
        (
            "dynamics.pressure_recovery".into(),
            "(1 - lambda[t-1]) - (1 - lambda[t]); positive values denote falling structural pressure"
                .into(),
        ),
        (
            "multiscale.d1_w1_dispersion".into(),
            "RMS difference over the first six bounded PRAMA v2 coordinates between D1[t] and the latest W1 frame from a strictly earlier ISO week"
                .into(),
        ),
        (
            "multiscale.d1_w1_coherence".into(),
            "cosine similarity over the same six D1/W1 PRAMA coordinates".into(),
        ),
        (
            "multiscale.cross_scale_dispersion_change".into(),
            "d1_w1_dispersion[t] - previous available d1_w1_dispersion".into(),
        ),
    ])
}

fn variant_feature_families(variant: DynamicsVariant) -> Vec<String> {
    let mut families = vec!["financial_structural_vector_v2 snapshot".into()];
    if variant >= DynamicsVariant::B {
        families.push("delta velocity, acceleration, reversal magnitude".into());
    }
    if variant >= DynamicsVariant::C {
        families.push(
            "prior-only pressure/momentum/transition z-scores, regime age, momentum-sign persistence, pressure recovery"
                .into(),
        );
    }
    if variant >= DynamicsVariant::D {
        families.push("D1 versus prior closed W1 dispersion, coherence, dispersion change".into());
    }
    families
}

/// Test each effective temporal observable as an individual residual
/// calibrator outside the authoritative snapshot KNN geometry.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamics_conditional_information_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    label_bars: &[MarketObservation],
    daily_frames: &[StructuralFrame],
    weekly_frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<DynamicsConditionalInformationAudit, CalibrationError> {
    let ablation = build_dynamics_ablation_audit(
        instrument_id,
        timeframe,
        label_bars,
        daily_frames,
        weekly_frames,
        profile,
        generation_timestamp_unix_seconds,
    )?;
    let baseline = ablation
        .variants
        .iter()
        .find(|variant| variant.variant == DynamicsVariant::A.label())
        .ok_or_else(|| CalibrationError::Diagnostic("missing A baseline".into()))?;
    let variants = build_dynamics_variants(daily_frames, weekly_frames)
        .map_err(|error| CalibrationError::Diagnostic(error.to_string()))?;
    let statistical_frames = &variants[&DynamicsVariant::C];
    let by_timestamp: BTreeMap<i64, &ExperimentalVectorFrame> = statistical_frames
        .iter()
        .map(|frame| (frame.timestamp_ns, frame))
        .collect();
    let tested_features = vec![
        "dynamics.delta_velocity".to_owned(),
        "dynamics.delta_acceleration".to_owned(),
        "dynamics.delta_reversal_magnitude".to_owned(),
        "dynamics.momentum_z_prior".to_owned(),
        "dynamics.transition_intensity_z_prior".to_owned(),
        "dynamics.regime_age_bars".to_owned(),
        "dynamics.momentum_sign_streak_bars".to_owned(),
    ];
    let mut feature_audits = Vec::new();
    for feature in &tested_features {
        let axis = statistical_frames[0]
            .names
            .iter()
            .position(|name| name == feature)
            .ok_or_else(|| {
                CalibrationError::Diagnostic(format!("missing dynamics feature {feature}"))
            })?;
        let validation_values =
            feature_values_for_predictions(&baseline.validation_predictions, &by_timestamp, axis)?;
        let feature_mean = validation_values.iter().sum::<f64>() / validation_values.len() as f64;
        let feature_std = (validation_values
            .iter()
            .map(|value| (value - feature_mean).powi(2))
            .sum::<f64>()
            / validation_values.len() as f64)
            .sqrt();
        if feature_std <= 0.0 || !feature_std.is_finite() {
            return Err(CalibrationError::Diagnostic(format!(
                "conditional feature {feature} is not variable on validation"
            )));
        }
        let validation_rows = conditional_rows(
            &baseline.validation_predictions,
            &by_timestamp,
            axis,
            feature_mean,
            feature_std,
        )?;
        let evaluation_rows = conditional_rows(
            &baseline.predictions,
            &by_timestamp,
            axis,
            feature_mean,
            feature_std,
        )?;
        let (intercept, slope) = fit_conditional_residuals(&validation_rows);
        let support_geometry =
            conditional_feature_support_geometry(&validation_rows, &evaluation_rows);
        let raw_probabilities: Vec<[f64; 3]> = evaluation_rows
            .iter()
            .map(|row| row.probabilities)
            .collect();
        let feature_only_probabilities: Vec<[f64; 3]> = evaluation_rows
            .iter()
            .map(|row| project_probabilities(add_residual_adjustment(row, [0.0; 3], slope)))
            .collect();
        let bounded_standardized_values: Vec<f64> = evaluation_rows
            .iter()
            .map(|row| {
                row.standardized_feature_value.clamp(
                    support_geometry.validation_standardized_minimum,
                    support_geometry.validation_standardized_maximum,
                )
            })
            .collect();
        let bounded_feature_only_probabilities: Vec<[f64; 3]> = evaluation_rows
            .iter()
            .zip(&bounded_standardized_values)
            .map(|(row, bounded)| {
                project_probabilities(add_residual_adjustment_at(row, [0.0; 3], slope, *bounded))
            })
            .collect();
        let intercept_probabilities: Vec<[f64; 3]> = evaluation_rows
            .iter()
            .map(|row| project_probabilities(add_residual_adjustment(row, intercept, [0.0; 3])))
            .collect();
        let adjusted_probabilities: Vec<[f64; 3]> = evaluation_rows
            .iter()
            .map(|row| project_probabilities(add_residual_adjustment(row, intercept, slope)))
            .collect();
        let mut raw_metrics =
            conditional_counterfactual_metrics(&evaluation_rows, &raw_probabilities, 0.0, None);
        raw_metrics.brier_delta_vs_raw_a = 0.0;
        let raw_brier = raw_metrics.multiclass_brier_score;
        let feature_only_metrics = conditional_counterfactual_metrics(
            &evaluation_rows,
            &feature_only_probabilities,
            raw_brier,
            None,
        );
        let bounded_feature_only_metrics = conditional_counterfactual_metrics(
            &evaluation_rows,
            &bounded_feature_only_probabilities,
            raw_brier,
            None,
        );
        let mut intercept_metrics = conditional_counterfactual_metrics(
            &evaluation_rows,
            &intercept_probabilities,
            raw_brier,
            None,
        );
        intercept_metrics.brier_delta_vs_intercept_only = Some(0.0);
        let intercept_brier = intercept_metrics.multiclass_brier_score;
        let adjusted_metrics = conditional_counterfactual_metrics(
            &evaluation_rows,
            &adjusted_probabilities,
            raw_brier,
            Some(intercept_brier),
        );
        let evaluation_queries = evaluation_rows
            .iter()
            .zip(&feature_only_probabilities)
            .zip(&bounded_standardized_values)
            .zip(&bounded_feature_only_probabilities)
            .zip(&intercept_probabilities)
            .zip(&adjusted_probabilities)
            .map(
                |(((((row, feature_only), bounded_value), bounded), intercept_only), adjusted)| {
                    ConditionalQueryRecord {
                        query_timestamp_ns: row.prediction.query_timestamp_ns,
                        actual_direction: row.prediction.actual_direction,
                        baseline_direction: row.prediction.predicted_direction,
                        baseline_probabilities_bp: row
                            .prediction
                            .probabilities_bp
                            .expect("conditional rows are resolved"),
                        baseline_top_two_edge_bp: probability_edge_bp(
                            row.prediction
                                .probabilities_bp
                                .expect("conditional rows are resolved"),
                        ),
                        baseline_brier_loss: probability_brier_loss(
                            row.probabilities,
                            row.prediction.actual_direction,
                        ),
                        baseline_correct: row.prediction.predicted_direction
                            == row.prediction.actual_direction,
                        feature_value: row.feature_value,
                        standardized_feature_value: row.standardized_feature_value,
                        feature_only_probabilities: *feature_only,
                        feature_only_direction: direction_from_probability_array(*feature_only),
                        bounded_standardized_feature_value: *bounded_value,
                        bounded_feature_only_probabilities: *bounded,
                        bounded_feature_only_direction: direction_from_probability_array(*bounded),
                        intercept_only_probabilities: *intercept_only,
                        feature_adjusted_probabilities: *adjusted,
                        feature_adjusted_direction: direction_from_probability_array(*adjusted),
                    }
                },
            )
            .collect();
        feature_audits.push(ConditionalFeatureAudit {
            feature: feature.clone(),
            validation_observations: validation_rows.len(),
            evaluation_observations: evaluation_rows.len(),
            fit: ConditionalCalibrationFit {
                feature_mean_on_validation: feature_mean,
                feature_std_on_validation: feature_std,
                intercept_only_residual: direction_array_map(intercept),
                feature_residual_slope: direction_array_map(slope),
                probability_projection: "CLAMP_NEGATIVE_TO_ZERO_THEN_RENORMALIZE".into(),
            },
            support_geometry,
            validation_residual_association: conditional_residual_association(&validation_rows),
            evaluation_residual_association: conditional_residual_association(&evaluation_rows),
            raw_a_metrics: raw_metrics,
            feature_only_metrics,
            bounded_feature_only_metrics,
            intercept_only_metrics: intercept_metrics,
            feature_adjusted_metrics: adjusted_metrics,
            baseline_predicted_up_by_actual_direction: baseline_up_feature_stats(&evaluation_rows),
            evaluation_queries,
        });
    }
    let mut primary_ranking: Vec<&ConditionalFeatureAudit> = feature_audits.iter().collect();
    primary_ranking.sort_by(|left, right| {
        left.feature_only_metrics
            .brier_delta_vs_raw_a
            .total_cmp(&right.feature_only_metrics.brier_delta_vs_raw_a)
            .then_with(|| left.feature.cmp(&right.feature))
    });
    let mut bounded_ranking: Vec<&ConditionalFeatureAudit> = feature_audits.iter().collect();
    bounded_ranking.sort_by(|left, right| {
        left.bounded_feature_only_metrics
            .brier_delta_vs_raw_a
            .total_cmp(&right.bounded_feature_only_metrics.brier_delta_vs_raw_a)
            .then_with(|| left.feature.cmp(&right.feature))
    });
    let mut intercept_ranking: Vec<&ConditionalFeatureAudit> = feature_audits.iter().collect();
    intercept_ranking.sort_by(|left, right| {
        left.feature_adjusted_metrics
            .brier_delta_vs_intercept_only
            .unwrap_or(f64::INFINITY)
            .total_cmp(
                &right
                    .feature_adjusted_metrics
                    .brier_delta_vs_intercept_only
                    .unwrap_or(f64::INFINITY),
            )
            .then_with(|| left.feature.cmp(&right.feature))
    });
    Ok(DynamicsConditionalInformationAudit {
        diagnostic: "PRAMAGRAPH_FINANCIAL_DYNAMICS_CONDITIONAL_INFORMATION".into(),
        experiment_id: "financial_dynamics_conditional_information_v1".into(),
        instrument_id: instrument_id.to_owned(),
        timeframe,
        base_structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().expect("validated profile"),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        evidence_status: "DEVELOPMENT_AUDIT_CONSUMED_NOT_A_PRODUCTION_CLAIM".into(),
        baseline_resolver: "A_SNAPSHOT; AUTHORITATIVE_V2_RMS_KNN_UNCHANGED".into(),
        conditional_method: "FOR_EACH_FEATURE_INDIVIDUALLY: FIT OLS OF ONE-HOT_MINUS_A_PROBABILITY RESIDUAL ON STANDARDIZED_FEATURE; PRIMARY COUNTERFACTUAL APPLIES SLOPE WITHOUT INTERCEPT TO KEEP A ANCHORED; INTERCEPT-ONLY AND INTERCEPT-PLUS-FEATURE ARE SECONDARY"
            .into(),
        fitting_partition: "CAUSAL_TEMPORAL_VALIDATION_ONLY".into(),
        evaluation_partition: "SAME_27_POINT_WALK_FORWARD_TAIL; CONDITIONAL_METRICS_USE_ONLY_A_RESOLVED_QUERIES"
            .into(),
        probability_projection: "CLAMP_NEGATIVE_TO_ZERO_THEN_RENORMALIZE".into(),
        tested_features,
        feature_ranking_by_feature_only_brier_delta_vs_raw_a: primary_ranking
            .iter()
            .map(|feature| feature.feature.clone())
            .collect(),
        feature_ranking_by_bounded_brier_delta_vs_raw_a: bounded_ranking
            .iter()
            .map(|feature| feature.feature.clone())
            .collect(),
        feature_ranking_by_brier_delta_vs_intercept_only: intercept_ranking
            .iter()
            .map(|feature| feature.feature.clone())
            .collect(),
        baseline_validation_metrics: baseline.validation.clone(),
        baseline_walk_forward_metrics: baseline.walk_forward_evaluation.clone(),
        features: feature_audits,
        runtime_or_profile_modified: false,
    })
}

/// Test whether bounded acceleration explains residual error after the
/// bounded velocity correction, while leaving the snapshot KNN untouched.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamics_sequential_residual_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    label_bars: &[MarketObservation],
    daily_frames: &[StructuralFrame],
    weekly_frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<DynamicsSequentialResidualAudit, CalibrationError> {
    let ablation = build_dynamics_ablation_audit(
        instrument_id,
        timeframe,
        label_bars,
        daily_frames,
        weekly_frames,
        profile,
        generation_timestamp_unix_seconds,
    )?;
    let baseline = ablation
        .variants
        .iter()
        .find(|variant| variant.variant == DynamicsVariant::A.label())
        .ok_or_else(|| CalibrationError::Diagnostic("missing A baseline".into()))?;
    let variants = build_dynamics_variants(daily_frames, weekly_frames)
        .map_err(|error| CalibrationError::Diagnostic(error.to_string()))?;
    let frames = &variants[&DynamicsVariant::C];
    let by_timestamp: BTreeMap<i64, &ExperimentalVectorFrame> = frames
        .iter()
        .map(|frame| (frame.timestamp_ns, frame))
        .collect();
    let axis = |name: &str| {
        frames[0]
            .names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| CalibrationError::Diagnostic(format!("missing feature {name}")))
    };
    let velocity_axis = axis("dynamics.delta_velocity")?;
    let acceleration_axis = axis("dynamics.delta_acceleration")?;
    let (velocity_validation, velocity_evaluation, velocity_mean, velocity_std) =
        conditional_feature_partitions(baseline, &by_timestamp, velocity_axis)?;
    let (acceleration_validation, acceleration_evaluation, acceleration_mean, acceleration_std) =
        conditional_feature_partitions(baseline, &by_timestamp, acceleration_axis)?;
    let velocity_support =
        conditional_feature_support_geometry(&velocity_validation, &velocity_evaluation);
    let acceleration_support =
        conditional_feature_support_geometry(&acceleration_validation, &acceleration_evaluation);
    let (velocity_intercept, velocity_slope) = fit_conditional_residuals(&velocity_validation);
    let (acceleration_intercept, acceleration_slope_against_a) =
        fit_conditional_residuals(&acceleration_validation);

    let velocity_validation_probabilities =
        bounded_feature_probabilities(&velocity_validation, velocity_slope, &velocity_support);
    let velocity_evaluation_probabilities =
        bounded_feature_probabilities(&velocity_evaluation, velocity_slope, &velocity_support);
    let acceleration_evaluation_probabilities = bounded_feature_probabilities(
        &acceleration_evaluation,
        acceleration_slope_against_a,
        &acceleration_support,
    );
    let post_velocity_validation =
        rows_with_probabilities(&acceleration_validation, &velocity_validation_probabilities);
    let post_velocity_evaluation =
        rows_with_probabilities(&acceleration_evaluation, &velocity_evaluation_probabilities);
    let (sequential_acceleration_intercept, sequential_acceleration_slope) =
        fit_conditional_residuals(&post_velocity_validation);
    let sequential_probabilities = bounded_feature_probabilities(
        &post_velocity_evaluation,
        sequential_acceleration_slope,
        &acceleration_support,
    );

    let a_probabilities: Vec<[f64; 3]> = velocity_evaluation
        .iter()
        .map(|row| row.probabilities)
        .collect();
    let mut a_metrics =
        conditional_counterfactual_metrics(&velocity_evaluation, &a_probabilities, 0.0, None);
    a_metrics.brier_delta_vs_raw_a = 0.0;
    let a_brier = a_metrics.multiclass_brier_score;
    let velocity_metrics = conditional_counterfactual_metrics(
        &velocity_evaluation,
        &velocity_evaluation_probabilities,
        a_brier,
        None,
    );
    let acceleration_metrics = conditional_counterfactual_metrics(
        &acceleration_evaluation,
        &acceleration_evaluation_probabilities,
        a_brier,
        None,
    );
    let mut sequential_metrics = conditional_counterfactual_metrics(
        &post_velocity_evaluation,
        &sequential_probabilities,
        a_brier,
        Some(velocity_metrics.multiclass_brier_score),
    );
    sequential_metrics.brier_delta_vs_intercept_only =
        Some(sequential_metrics.multiclass_brier_score - velocity_metrics.multiclass_brier_score);
    let queries = (0..velocity_evaluation.len())
        .map(|index| {
            let velocity = &velocity_evaluation[index];
            let acceleration = &acceleration_evaluation[index];
            let velocity_bounded = velocity.standardized_feature_value.clamp(
                velocity_support.validation_standardized_minimum,
                velocity_support.validation_standardized_maximum,
            );
            let acceleration_bounded = acceleration.standardized_feature_value.clamp(
                acceleration_support.validation_standardized_minimum,
                acceleration_support.validation_standardized_maximum,
            );
            SequentialResidualQueryRecord {
                query_timestamp_ns: velocity.prediction.query_timestamp_ns,
                actual_direction: velocity.prediction.actual_direction,
                a_probabilities: velocity.probabilities,
                a_direction: velocity.prediction.predicted_direction,
                velocity_standardized_raw: velocity.standardized_feature_value,
                velocity_standardized_bounded: velocity_bounded,
                acceleration_standardized_raw: acceleration.standardized_feature_value,
                acceleration_standardized_bounded: acceleration_bounded,
                a_plus_velocity_probabilities: velocity_evaluation_probabilities[index],
                a_plus_velocity_direction: direction_from_probability_array(
                    velocity_evaluation_probabilities[index],
                ),
                a_plus_velocity_plus_acceleration_probabilities: sequential_probabilities[index],
                a_plus_velocity_plus_acceleration_direction: direction_from_probability_array(
                    sequential_probabilities[index],
                ),
            }
        })
        .collect();
    Ok(DynamicsSequentialResidualAudit {
        diagnostic: "PRAMAGRAPH_FINANCIAL_DYNAMICS_SEQUENTIAL_RESIDUAL".into(),
        experiment_id: "financial_dynamics_sequential_residual_v1".into(),
        instrument_id: instrument_id.to_owned(),
        timeframe,
        base_structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().expect("validated profile"),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        evidence_status: "DEVELOPMENT_AUDIT_CONSUMED_NOT_A_PRODUCTION_CLAIM".into(),
        method: "A_SNAPSHOT -> BOUNDED_SLOPE_ONLY_DELTA_VELOCITY -> BOUNDED_SLOPE_ONLY_DELTA_ACCELERATION_FITTED_ON_POST_VELOCITY_RESIDUAL; NO_INTERCEPTS; KNN_UNCHANGED"
            .into(),
        validation_observations: velocity_validation.len(),
        evaluation_observations: velocity_evaluation.len(),
        velocity_fit: conditional_fit_contract(
            velocity_mean,
            velocity_std,
            velocity_intercept,
            velocity_slope,
        ),
        velocity_support,
        acceleration_fit_against_a: conditional_fit_contract(
            acceleration_mean,
            acceleration_std,
            acceleration_intercept,
            acceleration_slope_against_a,
        ),
        acceleration_fit_after_velocity: conditional_fit_contract(
            acceleration_mean,
            acceleration_std,
            sequential_acceleration_intercept,
            sequential_acceleration_slope,
        ),
        acceleration_support,
        acceleration_after_velocity_validation_association: conditional_residual_association(
            &post_velocity_validation,
        ),
        acceleration_after_velocity_evaluation_association: conditional_residual_association(
            &post_velocity_evaluation,
        ),
        a_metrics,
        a_plus_bounded_velocity_metrics: velocity_metrics,
        a_plus_bounded_acceleration_metrics: acceleration_metrics,
        a_plus_bounded_velocity_plus_bounded_acceleration_metrics: sequential_metrics,
        queries,
        runtime_or_profile_modified: false,
    })
}

/// Apply the velocity correction fitted on the original validation partition
/// to a strictly later feature prefix. Forward outcomes are used for scoring
/// and causal library maturity only; they never refit the correction.
#[allow(clippy::too_many_arguments)]
pub fn build_dynamics_frozen_velocity_forward_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    label_bars: &[MarketObservation],
    daily_frames: &[StructuralFrame],
    weekly_frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<DynamicsFrozenVelocityForwardAudit, CalibrationError> {
    let ablation = build_dynamics_ablation_audit(
        instrument_id,
        timeframe,
        label_bars,
        daily_frames,
        weekly_frames,
        profile,
        generation_timestamp_unix_seconds,
    )?;
    let baseline = ablation
        .variants
        .iter()
        .find(|variant| variant.variant == DynamicsVariant::A.label())
        .ok_or_else(|| CalibrationError::Diagnostic("missing A baseline".into()))?;
    let variants = build_dynamics_variants(daily_frames, weekly_frames)
        .map_err(|error| CalibrationError::Diagnostic(error.to_string()))?;
    let base_dimension = daily_frames[0].vector.names.len();
    let common_timestamps: Vec<i64> = variants[&DynamicsVariant::D]
        .iter()
        .filter(|frame| {
            frame
                .values
                .iter()
                .skip(base_dimension)
                .all(Option::is_some)
        })
        .map(|frame| frame.timestamp_ns)
        .collect();
    let bar_positions: BTreeMap<i64, usize> = label_bars
        .iter()
        .enumerate()
        .map(|(index, bar)| (bar.close_time_ns, index))
        .collect();
    let volatility: Vec<Option<f64>> = (0..label_bars.len())
        .map(|index| {
            causal_volatility(
                label_bars,
                index,
                profile.outcome_label.volatility_lookback_bars,
            )
        })
        .collect();
    let a_by_timestamp: BTreeMap<i64, &ExperimentalVectorFrame> = variants[&DynamicsVariant::A]
        .iter()
        .map(|frame| (frame.timestamp_ns, frame))
        .collect();
    let mut candidates = Vec::new();
    for timestamp in common_timestamps {
        let frame = a_by_timestamp
            .get(&timestamp)
            .ok_or(CalibrationError::Alignment)?;
        let bar_index = *bar_positions
            .get(&timestamp)
            .ok_or(CalibrationError::Alignment)?;
        let Some(scale) = volatility[bar_index] else {
            continue;
        };
        let Some((direction, first_passage_bars)) =
            label_outcome(label_bars, bar_index, scale, &profile.outcome_label)
        else {
            continue;
        };
        let maturity_index = bar_index
            .checked_add(first_passage_bars)
            .filter(|index| *index < label_bars.len())
            .ok_or(CalibrationError::Alignment)?;
        candidates.push(DynamicsCandidate {
            row: Candidate {
                timestamp_ns: timestamp,
                raw: frame.values.clone(),
                mask: frame.availability_mask.clone(),
                direction,
                first_passage_bars,
            },
            maturity_timestamp_ns: label_bars[maturity_index].close_time_ns,
        });
    }
    let frozen_query_prefix_len =
        daily_frames.partition_point(|frame| frame.timestamp_ns <= profile.calibration_end_ns);
    let frozen_feature_prefix_len = (frozen_query_prefix_len
        + usize::from(frozen_query_prefix_len < daily_frames.len()))
    .min(daily_frames.len());
    let frozen_feature_source_end_timestamp_ns = daily_frames
        .get(frozen_feature_prefix_len.saturating_sub(1))
        .ok_or(CalibrationError::InsufficientData)?
        .timestamp_ns;
    let original_end = candidates.partition_point(|candidate| {
        candidate.row.timestamp_ns <= frozen_feature_source_end_timestamp_ns
    });
    if original_end == 0 || original_end >= candidates.len() {
        return Err(CalibrationError::Diagnostic(
            "forward audit requires labeled observations after the original feature source".into(),
        ));
    }
    let prior_pool = &candidates[..original_end];
    let forward = &candidates[original_end..];
    let split_count = integer_sqrt(frozen_feature_prefix_len).max(1);
    let validation_split_frame = frozen_feature_prefix_len
        .checked_sub(split_count * 2)
        .ok_or(CalibrationError::InsufficientData)?;
    let validation_split_timestamp = daily_frames[validation_split_frame].timestamp_ns;
    let training_end = prior_pool
        .partition_point(|candidate| candidate.row.timestamp_ns < validation_split_timestamp);
    let training_rows: Vec<Candidate> = prior_pool[..training_end]
        .iter()
        .map(|candidate| candidate.row.clone())
        .collect();
    let names = &variants[&DynamicsVariant::A][0].names;
    let normalization = fit_normalization(names, &training_rows)?;
    let parameters = score_parameters(
        baseline.estimator.neighbor_count,
        baseline.estimator.minimum_support,
        baseline.estimator.maximum_distance,
        baseline.estimator.distance_power,
    );
    let forward_scores =
        score_dynamics_walk_forward(forward, prior_pool, &normalization, &parameters)?;

    let c_frames = &variants[&DynamicsVariant::C];
    let c_by_timestamp: BTreeMap<i64, &ExperimentalVectorFrame> = c_frames
        .iter()
        .map(|frame| (frame.timestamp_ns, frame))
        .collect();
    let velocity_axis = c_frames[0]
        .names
        .iter()
        .position(|name| name == "dynamics.delta_velocity")
        .ok_or_else(|| CalibrationError::Diagnostic("missing delta velocity".into()))?;
    let (velocity_validation, velocity_source_evaluation, velocity_mean, velocity_std) =
        conditional_feature_partitions(baseline, &c_by_timestamp, velocity_axis)?;
    let (_, velocity_slope) = fit_conditional_residuals(&velocity_validation);
    let forward_predictions: Vec<DynamicsExperimentPrediction> = forward_scores
        .iter()
        .map(|score| score.prediction.clone())
        .collect();
    let forward_rows = conditional_rows(
        &forward_predictions,
        &c_by_timestamp,
        velocity_axis,
        velocity_mean,
        velocity_std,
    )?;
    if forward_rows.is_empty() {
        return Err(CalibrationError::Diagnostic(
            "no resolvable forward observations".into(),
        ));
    }
    let forward_support = conditional_feature_support_geometry(&velocity_validation, &forward_rows);
    let forward_probabilities =
        bounded_feature_probabilities(&forward_rows, velocity_slope, &forward_support);
    let source_support =
        conditional_feature_support_geometry(&velocity_validation, &velocity_source_evaluation);
    let source_probabilities =
        bounded_feature_probabilities(&velocity_source_evaluation, velocity_slope, &source_support);
    let source_a_probabilities: Vec<[f64; 3]> = velocity_source_evaluation
        .iter()
        .map(|row| row.probabilities)
        .collect();
    let mut source_a_metrics = conditional_counterfactual_metrics(
        &velocity_source_evaluation,
        &source_a_probabilities,
        0.0,
        None,
    );
    source_a_metrics.brier_delta_vs_raw_a = 0.0;
    let source_a_plus_velocity_metrics = conditional_counterfactual_metrics(
        &velocity_source_evaluation,
        &source_probabilities,
        source_a_metrics.multiclass_brier_score,
        None,
    );

    let probability_by_timestamp: BTreeMap<i64, [f64; 3]> = forward_rows
        .iter()
        .zip(&forward_probabilities)
        .map(|(row, probabilities)| (row.prediction.query_timestamp_ns, *probabilities))
        .collect();
    let row_by_timestamp: BTreeMap<i64, &ConditionalRow> = forward_rows
        .iter()
        .map(|row| (row.prediction.query_timestamp_ns, row))
        .collect();
    let mut corrected_scores = forward_scores.clone();
    for score in &mut corrected_scores {
        let Some(probabilities) = probability_by_timestamp
            .get(&score.prediction.query_timestamp_ns)
            .copied()
        else {
            continue;
        };
        let direction = direction_from_probability_array(probabilities);
        let probabilities_bp = probability_basis_points(probabilities);
        score.prediction.predicted_direction = direction;
        score.prediction.probabilities_bp = Some(probabilities_bp);
        if let Some(scored) = score.scored.as_mut() {
            scored.case.predicted = direction;
            scored.case.probabilities = probabilities_bp;
            scored.case.correct = direction == scored.case.actual;
        }
    }
    let queries = forward_scores
        .iter()
        .map(|score| {
            let row = row_by_timestamp
                .get(&score.prediction.query_timestamp_ns)
                .copied();
            let corrected = probability_by_timestamp
                .get(&score.prediction.query_timestamp_ns)
                .copied();
            let corrected_direction = corrected.map_or(Direction::Unresolved, |probabilities| {
                direction_from_probability_array(probabilities)
            });
            let bounded = row.map(|row| {
                row.standardized_feature_value.clamp(
                    forward_support.validation_standardized_minimum,
                    forward_support.validation_standardized_maximum,
                )
            });
            FrozenVelocityForwardQueryRecord {
                query_timestamp_ns: score.prediction.query_timestamp_ns,
                actual_direction: score.prediction.actual_direction,
                causal_library_size: score.prediction.causal_library_size,
                selected_neighbor_count: score.prediction.selected_neighbor_count,
                baseline_probabilities: score
                    .prediction
                    .probabilities_bp
                    .map(probabilities_to_array),
                baseline_direction: score.prediction.predicted_direction,
                velocity_value: row.map(|row| row.feature_value),
                velocity_standardized_raw: row.map(|row| row.standardized_feature_value),
                velocity_standardized_bounded: bounded,
                corrected_probabilities: corrected,
                corrected_direction,
                decision_changed: score.prediction.predicted_direction != corrected_direction,
            }
        })
        .collect();
    Ok(DynamicsFrozenVelocityForwardAudit {
        diagnostic: "PRAMAGRAPH_FINANCIAL_DYNAMICS_FROZEN_VELOCITY_FORWARD".into(),
        experiment_id: "financial_dynamics_frozen_velocity_forward_v1".into(),
        instrument_id: instrument_id.to_owned(),
        timeframe,
        base_structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().expect("validated profile"),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        evidence_status: "LATER_TIME_DEVELOPMENT_AUDIT_NOT_A_PRODUCTION_CLAIM".into(),
        method: "FROZEN_ORIGINAL_A_NORMALIZATION_AND_ESTIMATOR; FROZEN_VALIDATION_ONLY_BOUNDED_SLOPE_ONLY_DELTA_VELOCITY; CAUSAL_LIBRARY_GROWTH_AFTER_LABEL_MATURITY; NO_FORWARD_REFIT"
            .into(),
        frozen_feature_cutoff_timestamp_ns: profile.calibration_end_ns,
        frozen_feature_source_end_timestamp_ns,
        forward_feature_start_timestamp_ns: forward
            .first()
            .expect("nonempty forward")
            .row
            .timestamp_ns,
        forward_feature_end_timestamp_ns: forward
            .last()
            .expect("nonempty forward")
            .row
            .timestamp_ns,
        label_source_end_timestamp_ns: label_bars.last().expect("nonempty labels").close_time_ns,
        required_horizon_bars: profile.outcome_label.maximum_horizon_bars,
        frozen_estimator: baseline.estimator.clone(),
        frozen_velocity_fit: conditional_fit_contract(
            velocity_mean,
            velocity_std,
            [0.0; 3],
            velocity_slope,
        ),
        forward_velocity_support: forward_support,
        source_a_metrics,
        source_a_plus_bounded_velocity_metrics: source_a_plus_velocity_metrics,
        forward_a_metrics: dynamics_metrics(forward, &forward_scores),
        forward_a_plus_bounded_velocity_metrics: dynamics_metrics(forward, &corrected_scores),
        queries,
        slope_or_support_refitted_on_forward: false,
        runtime_or_profile_modified: false,
    })
}

fn conditional_feature_partitions(
    baseline: &DynamicsVariantAudit,
    frames: &BTreeMap<i64, &ExperimentalVectorFrame>,
    axis: usize,
) -> Result<(Vec<ConditionalRow>, Vec<ConditionalRow>, f64, f64), CalibrationError> {
    let validation_values =
        feature_values_for_predictions(&baseline.validation_predictions, frames, axis)?;
    let mean = validation_values.iter().sum::<f64>() / validation_values.len() as f64;
    let std = (validation_values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / validation_values.len() as f64)
        .sqrt();
    if std <= 0.0 || !std.is_finite() {
        return Err(CalibrationError::Diagnostic(
            "sequential feature is not variable on validation".into(),
        ));
    }
    Ok((
        conditional_rows(&baseline.validation_predictions, frames, axis, mean, std)?,
        conditional_rows(&baseline.predictions, frames, axis, mean, std)?,
        mean,
        std,
    ))
}

fn bounded_feature_probabilities(
    rows: &[ConditionalRow],
    slope: [f64; 3],
    support: &ConditionalFeatureSupportGeometry,
) -> Vec<[f64; 3]> {
    rows.iter()
        .map(|row| {
            let bounded = row.standardized_feature_value.clamp(
                support.validation_standardized_minimum,
                support.validation_standardized_maximum,
            );
            project_probabilities(add_residual_adjustment_at(row, [0.0; 3], slope, bounded))
        })
        .collect()
}

fn rows_with_probabilities(
    rows: &[ConditionalRow],
    probabilities: &[[f64; 3]],
) -> Vec<ConditionalRow> {
    rows.iter()
        .zip(probabilities)
        .map(|(row, probabilities)| {
            let mut adjusted = row.clone();
            adjusted.probabilities = *probabilities;
            adjusted.prediction.predicted_direction =
                direction_from_probability_array(*probabilities);
            adjusted
        })
        .collect()
}

fn conditional_fit_contract(
    mean: f64,
    std: f64,
    intercept: [f64; 3],
    slope: [f64; 3],
) -> ConditionalCalibrationFit {
    ConditionalCalibrationFit {
        feature_mean_on_validation: mean,
        feature_std_on_validation: std,
        intercept_only_residual: direction_array_map(intercept),
        feature_residual_slope: direction_array_map(slope),
        probability_projection: "CLAMP_NEGATIVE_TO_ZERO_THEN_RENORMALIZE".into(),
    }
}

fn feature_values_for_predictions(
    predictions: &[DynamicsExperimentPrediction],
    frames: &BTreeMap<i64, &ExperimentalVectorFrame>,
    axis: usize,
) -> Result<Vec<f64>, CalibrationError> {
    predictions
        .iter()
        .filter(|prediction| prediction.probabilities_bp.is_some())
        .map(|prediction| {
            frames
                .get(&prediction.query_timestamp_ns)
                .and_then(|frame| frame.values.get(axis))
                .copied()
                .flatten()
                .ok_or_else(|| {
                    CalibrationError::Diagnostic(format!(
                        "feature unavailable at {}",
                        prediction.query_timestamp_ns
                    ))
                })
        })
        .collect()
}

fn conditional_rows(
    predictions: &[DynamicsExperimentPrediction],
    frames: &BTreeMap<i64, &ExperimentalVectorFrame>,
    axis: usize,
    feature_mean: f64,
    feature_std: f64,
) -> Result<Vec<ConditionalRow>, CalibrationError> {
    predictions
        .iter()
        .filter_map(|prediction| {
            prediction.probabilities_bp.map(|probabilities| {
                let value = frames
                    .get(&prediction.query_timestamp_ns)
                    .and_then(|frame| frame.values.get(axis))
                    .copied()
                    .flatten()
                    .ok_or_else(|| {
                        CalibrationError::Diagnostic(format!(
                            "feature unavailable at {}",
                            prediction.query_timestamp_ns
                        ))
                    })?;
                Ok(ConditionalRow {
                    prediction: prediction.clone(),
                    probabilities: probabilities_to_array(probabilities),
                    feature_value: value,
                    standardized_feature_value: (value - feature_mean) / feature_std,
                })
            })
        })
        .collect()
}

fn fit_conditional_residuals(rows: &[ConditionalRow]) -> ([f64; 3], [f64; 3]) {
    let mut intercept = [0.0; 3];
    let mut slope = [0.0; 3];
    let denominator = rows
        .iter()
        .map(|row| row.standardized_feature_value.powi(2))
        .sum::<f64>();
    for axis in 0..3 {
        intercept[axis] = rows
            .iter()
            .map(|row| {
                f64::from(direction_index(row.prediction.actual_direction) == axis)
                    - row.probabilities[axis]
            })
            .sum::<f64>()
            / rows.len() as f64;
        if denominator > 0.0 {
            slope[axis] = rows
                .iter()
                .map(|row| {
                    row.standardized_feature_value
                        * (f64::from(direction_index(row.prediction.actual_direction) == axis)
                            - row.probabilities[axis]
                            - intercept[axis])
                })
                .sum::<f64>()
                / denominator;
        }
    }
    (intercept, slope)
}

fn add_residual_adjustment(row: &ConditionalRow, intercept: [f64; 3], slope: [f64; 3]) -> [f64; 3] {
    add_residual_adjustment_at(row, intercept, slope, row.standardized_feature_value)
}

fn add_residual_adjustment_at(
    row: &ConditionalRow,
    intercept: [f64; 3],
    slope: [f64; 3],
    standardized_feature_value: f64,
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        row.probabilities[axis] + intercept[axis] + slope[axis] * standardized_feature_value
    })
}

fn project_probabilities(values: [f64; 3]) -> [f64; 3] {
    let nonnegative = values.map(|value| value.max(0.0));
    let total = nonnegative.iter().sum::<f64>();
    if total > 0.0 && total.is_finite() {
        nonnegative.map(|value| value / total)
    } else {
        [1.0 / 3.0; 3]
    }
}

fn probabilities_to_array(probabilities: ProbabilitiesBp) -> [f64; 3] {
    [
        f64::from(probabilities.up) / 10_000.0,
        f64::from(probabilities.range) / 10_000.0,
        f64::from(probabilities.down) / 10_000.0,
    ]
}

fn direction_array_map(values: [f64; 3]) -> BTreeMap<String, f64> {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .enumerate()
        .map(|(axis, direction)| (direction_key(direction).to_owned(), values[axis]))
        .collect()
}

fn probability_brier_loss(probabilities: [f64; 3], actual: Direction) -> f64 {
    probabilities
        .iter()
        .enumerate()
        .map(|(axis, probability)| {
            let expected = f64::from(direction_index(actual) == axis);
            (probability - expected).powi(2)
        })
        .sum()
}

fn direction_from_probability_array(probabilities: [f64; 3]) -> Direction {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .enumerate()
        .max_by(|(left_axis, left), (right_axis, right)| {
            probabilities[*left_axis]
                .total_cmp(&probabilities[*right_axis])
                .then_with(|| direction_order(*right).cmp(&direction_order(*left)))
        })
        .map(|(_, direction)| direction)
        .expect("three directions")
}

fn probability_edge_bp(probabilities: ProbabilitiesBp) -> u16 {
    let mut ordered = [probabilities.up, probabilities.range, probabilities.down];
    ordered.sort_unstable_by(|left, right| right.cmp(left));
    ordered[0].saturating_sub(ordered[1])
}

fn conditional_counterfactual_metrics(
    rows: &[ConditionalRow],
    probabilities: &[[f64; 3]],
    raw_a_brier: f64,
    intercept_only_brier: Option<f64>,
) -> ConditionalCounterfactualMetrics {
    let brier = rows
        .iter()
        .zip(probabilities)
        .map(|(row, probabilities)| {
            probability_brier_loss(*probabilities, row.prediction.actual_direction)
        })
        .sum::<f64>()
        / rows.len() as f64;
    let predicted: Vec<Direction> = probabilities
        .iter()
        .map(|values| direction_from_probability_array(*values))
        .collect();
    ConditionalCounterfactualMetrics {
        observations: rows.len(),
        multiclass_brier_score: brier,
        brier_delta_vs_raw_a: brier - raw_a_brier,
        brier_delta_vs_intercept_only: intercept_only_brier.map(|baseline| brier - baseline),
        accuracy: rows
            .iter()
            .zip(&predicted)
            .filter(|(row, predicted)| row.prediction.actual_direction == **predicted)
            .count() as f64
            / rows.len() as f64,
        predicted_direction_counts: [Direction::Up, Direction::Range, Direction::Down]
            .into_iter()
            .map(|direction| {
                (
                    direction_key(direction).to_owned(),
                    predicted
                        .iter()
                        .filter(|value| **value == direction)
                        .count(),
                )
            })
            .collect(),
    }
}

fn conditional_residual_association(rows: &[ConditionalRow]) -> ConditionalResidualAssociation {
    let residual = |row: &ConditionalRow, axis: usize| {
        f64::from(direction_index(row.prediction.actual_direction) == axis)
            - row.probabilities[axis]
    };
    ConditionalResidualAssociation {
        feature_vs_up_residual: pearson_correlation(
            rows.iter().map(|row| row.standardized_feature_value),
            rows.iter().map(|row| residual(row, 0)),
        ),
        feature_vs_range_residual: pearson_correlation(
            rows.iter().map(|row| row.standardized_feature_value),
            rows.iter().map(|row| residual(row, 1)),
        ),
        feature_vs_down_residual: pearson_correlation(
            rows.iter().map(|row| row.standardized_feature_value),
            rows.iter().map(|row| residual(row, 2)),
        ),
        feature_vs_brier_loss: pearson_correlation(
            rows.iter().map(|row| row.standardized_feature_value),
            rows.iter().map(|row| {
                probability_brier_loss(row.probabilities, row.prediction.actual_direction)
            }),
        ),
        feature_vs_error_indicator: pearson_correlation(
            rows.iter().map(|row| row.standardized_feature_value),
            rows.iter().map(|row| {
                f64::from(row.prediction.predicted_direction != row.prediction.actual_direction)
            }),
        ),
    }
}

fn conditional_feature_support_geometry(
    validation: &[ConditionalRow],
    evaluation: &[ConditionalRow],
) -> ConditionalFeatureSupportGeometry {
    let validation_minimum = validation
        .iter()
        .map(|row| row.standardized_feature_value)
        .reduce(f64::min)
        .expect("nonempty validation");
    let validation_maximum = validation
        .iter()
        .map(|row| row.standardized_feature_value)
        .reduce(f64::max)
        .expect("nonempty validation");
    ConditionalFeatureSupportGeometry {
        validation_standardized_minimum: validation_minimum,
        validation_standardized_maximum: validation_maximum,
        evaluation_standardized_minimum: evaluation
            .iter()
            .map(|row| row.standardized_feature_value)
            .reduce(f64::min)
            .expect("nonempty evaluation"),
        evaluation_standardized_maximum: evaluation
            .iter()
            .map(|row| row.standardized_feature_value)
            .reduce(f64::max)
            .expect("nonempty evaluation"),
        evaluation_below_validation_minimum: evaluation
            .iter()
            .filter(|row| row.standardized_feature_value < validation_minimum)
            .count(),
        evaluation_above_validation_maximum: evaluation
            .iter()
            .filter(|row| row.standardized_feature_value > validation_maximum)
            .count(),
    }
}

fn baseline_up_feature_stats(
    rows: &[ConditionalRow],
) -> BTreeMap<String, ConditionalClassFeatureStats> {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .map(|direction| {
            let mut values: Vec<f64> = rows
                .iter()
                .filter(|row| {
                    row.prediction.predicted_direction == Direction::Up
                        && row.prediction.actual_direction == direction
                })
                .map(|row| row.feature_value)
                .collect();
            let mean =
                (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64);
            let median = median(&mut values);
            (
                direction_key(direction).to_owned(),
                ConditionalClassFeatureStats {
                    count: values.len(),
                    mean,
                    median,
                },
            )
        })
        .collect()
}

fn causal_validation_maximum_distance(
    queries: &[DynamicsCandidate],
    prior_pool: &[DynamicsCandidate],
    normalization: &FeatureNormalization,
    neighbor_count: usize,
) -> Result<f64, CalibrationError> {
    queries
        .iter()
        .filter_map(|query| {
            let query_sample = normalized_sample(&query.row, normalization).ok()?;
            let library: Vec<CalibratedSample> = prior_pool
                .iter()
                .filter(|candidate| {
                    candidate.row.timestamp_ns < query.row.timestamp_ns
                        && candidate.maturity_timestamp_ns < query.row.timestamp_ns
                })
                .filter_map(|candidate| normalized_sample(&candidate.row, normalization).ok())
                .collect();
            kth_distance(&query_sample, &library, neighbor_count)
        })
        .reduce(f64::max)
        .filter(|distance| distance.is_finite() && *distance > 0.0)
        .ok_or(CalibrationError::InsufficientData)
}

fn score_dynamics_walk_forward(
    queries: &[DynamicsCandidate],
    prior_pool: &[DynamicsCandidate],
    normalization: &FeatureNormalization,
    parameters: &NeighborParameters,
) -> Result<Vec<DynamicsScore>, CalibrationError> {
    let mut all_candidates = prior_pool.to_vec();
    all_candidates.extend_from_slice(queries);
    let mut output = Vec::with_capacity(queries.len());
    for query in queries {
        let query_sample = normalized_sample(&query.row, normalization)?;
        let library: Vec<CalibratedSample> = all_candidates
            .iter()
            .filter(|candidate| {
                candidate.row.timestamp_ns < query.row.timestamp_ns
                    && candidate.maturity_timestamp_ns < query.row.timestamp_ns
            })
            .map(|candidate| normalized_sample(&candidate.row, normalization))
            .collect::<Result<_, _>>()?;
        let mut selected = admissible_neighbors(&query_sample, &library, parameters);
        selected.truncate(parameters.neighbor_count);
        let selected_neighbor_count = selected.len();
        let evaluation = evaluate_vote(&query_sample, &library, parameters);
        let climatology = causal_climatology(&library);
        let scored = evaluation.as_ref().map(|evaluation| DynamicsScoredCase {
            case: ScoredCase {
                actual: query.row.direction,
                predicted: evaluation.result.winning,
                probabilities: evaluation.result.probabilities,
                correct: evaluation.result.winning == query.row.direction,
            },
            climatology,
        });
        output.push(DynamicsScore {
            prediction: DynamicsExperimentPrediction {
                query_timestamp_ns: query.row.timestamp_ns,
                actual_direction: query.row.direction,
                predicted_direction: evaluation
                    .as_ref()
                    .map_or(Direction::Unresolved, |evaluation| {
                        evaluation.result.winning
                    }),
                probabilities_bp: evaluation
                    .as_ref()
                    .map(|evaluation| evaluation.result.probabilities),
                selected_neighbor_count,
                causal_library_size: library.len(),
            },
            scored,
        });
    }
    Ok(output)
}

fn causal_climatology(library: &[CalibratedSample]) -> [f64; 3] {
    if library.is_empty() {
        return [0.0; 3];
    }
    [Direction::Up, Direction::Range, Direction::Down].map(|direction| {
        library
            .iter()
            .filter(|sample| sample.direction == direction)
            .count() as f64
            / library.len() as f64
    })
}

fn dynamics_metrics(
    queries: &[DynamicsCandidate],
    scores: &[DynamicsScore],
) -> DynamicsExperimentMetrics {
    let scored: Vec<&DynamicsScoredCase> = scores
        .iter()
        .filter_map(|score| score.scored.as_ref())
        .collect();
    let cases: Vec<ScoredCase> = scored.iter().map(|score| score.case.clone()).collect();
    let resolved = cases.len();
    let correct = cases.iter().filter(|case| case.correct).count();
    let brier = multiclass_brier_score(&cases);
    let climatology_brier = if scored.is_empty() {
        0.0
    } else {
        scored
            .iter()
            .map(|score| {
                score
                    .climatology
                    .iter()
                    .enumerate()
                    .map(|(axis, probability)| {
                        let expected =
                            usize::from(axis == direction_index(score.case.actual)) as f64;
                        (probability - expected).powi(2)
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
            / scored.len() as f64
    };
    let actual_direction_counts = [
        Direction::Up,
        Direction::Range,
        Direction::Down,
        Direction::Unresolved,
    ]
    .into_iter()
    .map(|direction| {
        (
            direction_key(direction).to_owned(),
            queries
                .iter()
                .filter(|query| query.row.direction == direction)
                .count(),
        )
    })
    .collect();
    let predicted_direction_counts = [
        Direction::Up,
        Direction::Range,
        Direction::Down,
        Direction::Unresolved,
    ]
    .into_iter()
    .map(|direction| {
        (
            direction_key(direction).to_owned(),
            scores
                .iter()
                .filter(|score| score.prediction.predicted_direction == direction)
                .count(),
        )
    })
    .collect();
    DynamicsExperimentMetrics {
        observations: queries.len(),
        resolved,
        unresolved: queries.len().saturating_sub(resolved),
        coverage: if queries.is_empty() {
            0.0
        } else {
            resolved as f64 / queries.len() as f64
        },
        correct,
        accuracy: if resolved == 0 {
            0.0
        } else {
            correct as f64 / resolved as f64
        },
        balanced_accuracy: f64::from(balanced_accuracy_bp(&cases)) / 10_000.0,
        multiclass_brier_score: brier,
        causal_climatology_brier_score: climatology_brier,
        brier_skill_score: if climatology_brier > 0.0 {
            1.0 - brier / climatology_brier
        } else {
            0.0
        },
        classwise_calibration_error: classwise_calibration_error(&cases),
        actual_direction_counts,
        predicted_direction_counts,
        confusion_matrix: confusion_matrix(&cases),
    }
}

fn classwise_calibration_error(cases: &[ScoredCase]) -> f64 {
    if cases.is_empty() {
        return 0.0;
    }
    [Direction::Up, Direction::Range, Direction::Down]
        .iter()
        .enumerate()
        .map(|(axis, direction)| {
            let mean_probability = cases
                .iter()
                .map(|case| match axis {
                    0 => case.probabilities.up,
                    1 => case.probabilities.range,
                    _ => case.probabilities.down,
                })
                .map(f64::from)
                .sum::<f64>()
                / (cases.len() as f64 * 10_000.0);
            let frequency = cases
                .iter()
                .filter(|case| case.actual == *direction)
                .count() as f64
                / cases.len() as f64;
            (mean_probability - frequency).abs()
        })
        .sum::<f64>()
        / 3.0
}

fn dynamics_metric_order(
    left: &DynamicsExperimentMetrics,
    left_dimensions: usize,
    right: &DynamicsExperimentMetrics,
    right_dimensions: usize,
) -> std::cmp::Ordering {
    left.brier_skill_score
        .total_cmp(&right.brier_skill_score)
        .then_with(|| left.balanced_accuracy.total_cmp(&right.balanced_accuracy))
        .then_with(|| {
            right
                .classwise_calibration_error
                .total_cmp(&left.classwise_calibration_error)
        })
        .then_with(|| left.coverage.total_cmp(&right.coverage))
        .then_with(|| right_dimensions.cmp(&left_dimensions))
}

pub fn validate_profile(profile: &ResolutionCalibrationProfile) -> Result<(), CalibrationError> {
    if profile.schema != RESOLUTION_PROFILE_SCHEMA
        || profile.calibration_version != RESOLUTION_CALIBRATION_VERSION
        || profile.structural_vector_version != STRUCTURAL_VECTOR_VERSION
        || profile.samples.is_empty()
        || !profile.prefix_causality_verified
        || profile.runtime_recalibration
        || profile.estimator.minimum_support == 0
        || profile.estimator.neighbor_count < profile.estimator.minimum_support
        || !profile.estimator.maximum_distance.is_finite()
        || profile.estimator.maximum_distance <= 0.0
        || !profile.estimator.distance_power.is_finite()
        || profile.estimator.distance_power <= 0.0
        || profile.publication.minimum_reliability_bp > 10_000
        || profile.publication.minimum_direction_edge_bp > 10_000
        || !(profile.publication.parameters_selected_on == "TEMPORAL_VALIDATION"
            || profile.publication.parameters_selected_on == "PREREGISTERED_PROTOCOL")
        || profile
            .publication
            .test_outcomes_used_for_parameter_selection
        || !profile.publication.requires_positive_brier_skill
        || profile.publication.profile_eligible_for_publication
            != profile.reliability.untouched_test
        || profile.publication.parameters_selected_on == "TEMPORAL_VALIDATION"
            && profile
                .publication
                .preregistered_protocol_sha256
                .as_ref()
                .is_some_and(|value| valid_sha256(value))
            && profile.publication.profile_eligible_for_publication
        || profile.publication.reliability_evaluated_on
            != if profile.publication.profile_eligible_for_publication {
                "UNTOUCHED_TEMPORAL_TEST"
            } else if profile.publication.parameters_selected_on == "PREREGISTERED_PROTOCOL" {
                "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE"
            } else {
                "CONSUMED_DEVELOPMENT_AUDIT"
            }
        || !profile.outcome_label.up_down_symmetric
        || profile.outcome_label.upper_barrier_volatility_multiple
            != profile.outcome_label.lower_barrier_volatility_multiple
        || profile.reliability.evidence_status
            != if profile.reliability.untouched_test {
                "PREREGISTERED_UNTOUCHED_TEST"
            } else if profile.publication.parameters_selected_on == "PREREGISTERED_PROTOCOL" {
                "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE"
            } else {
                "DEVELOPMENT_AUDIT_CONSUMED"
            }
        || profile.reliability.confidence_level_bp != 9_500
        || profile.reliability.reliability_lower_bound_bp > profile.reliability.reliability_bp
        || !profile.reliability.multiclass_brier_score.is_finite()
        || !profile.reliability.climatology_brier_score.is_finite()
        || !profile.reliability.brier_skill_score.is_finite()
    {
        return Err(CalibrationError::InvalidProfile("field invariant".into()));
    }
    let expected = profile
        .profile_sha256
        .as_ref()
        .ok_or_else(|| CalibrationError::InvalidProfile("missing profile hash".into()))?;
    let mut unhashed = profile.clone();
    unhashed.profile_sha256 = None;
    if &canonical::sha256(&unhashed)? != expected {
        return Err(CalibrationError::InvalidProfile(
            "profile hash mismatch".into(),
        ));
    }
    let dimension = profile.normalization.names.len();
    if dimension == 0
        || profile.normalization.median.len() != dimension
        || profile.normalization.scale.len() != dimension
        || profile.normalization.effective_dimension_mask.len() != dimension
        || profile.normalization.effective_dimension_count
            != profile
                .normalization
                .effective_dimension_mask
                .iter()
                .filter(|included| **included)
                .count()
        || profile.normalization.effective_dimension_count == 0
        || profile.diagnostics.total_vector_dimensions != dimension
        || profile.diagnostics.effective_vector_dimensions
            != profile.normalization.effective_dimension_count
        || profile.diagnostics.d_o_transport_evaluable_bp > 10_000
        || profile.diagnostics.odce_adaptive_organization_available_bp > 10_000
        || profile.diagnostics.k_mem_strictly_prior_available_bp > 10_000
        || profile.samples.iter().any(|sample| {
            sample.vector.len() != dimension
                || sample.availability_mask.len() != dimension
                || sample.vector.iter().any(|value| !value.is_finite())
        })
    {
        return Err(CalibrationError::InvalidProfile("vector dimension".into()));
    }
    Ok(())
}

pub fn validate_profile_for_engine(
    profile: &ResolutionCalibrationProfile,
    expected_engine_version: &str,
) -> Result<(), CalibrationError> {
    validate_profile(profile)?;
    if profile.engine_version != expected_engine_version {
        return Err(CalibrationError::Incompatible(
            "engine version differs from calibration authority".into(),
        ));
    }
    Ok(())
}

pub fn build_neighbor_anatomy_artifacts(
    instrument_id: &str,
    timeframe: Timeframe,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<NeighborAnatomyArtifacts, CalibrationError> {
    validate_profile(profile)?;
    if profile.instrument_id != instrument_id || profile.timeframe != timeframe {
        return Err(CalibrationError::Incompatible(
            "instrument/timeframe differs from diagnostic profile".into(),
        ));
    }
    let queries = development_audit_queries(bars, frames, profile)?;
    let audit_parameters = profile.estimator.clone();
    let mut audit_tail = Vec::new();
    for query in queries {
        if let Some(evaluation) = evaluate_vote(&query, &profile.samples, &audit_parameters) {
            audit_tail.push(anatomy_query(&query, evaluation, &audit_parameters));
        }
    }
    audit_tail.sort_by_key(|query| query.query_timestamp_ns);
    let summary = anatomy_summary(
        instrument_id,
        timeframe,
        profile,
        generation_timestamp_unix_seconds,
        &audit_tail,
    );
    Ok(NeighborAnatomyArtifacts {
        audit_tail,
        summary,
    })
}

fn development_audit_queries(
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
) -> Result<Vec<CalibratedSample>, CalibrationError> {
    Ok(development_audit_labeling(bars, frames, profile)?.queries)
}

#[derive(Debug, Clone, PartialEq)]
struct DevelopmentAuditLabeling {
    candidate_observations: usize,
    queries: Vec<CalibratedSample>,
    right_censored_queries: Vec<RightCensoredAuditObservation>,
}

fn development_audit_labeling(
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
) -> Result<DevelopmentAuditLabeling, CalibrationError> {
    let bar_positions: BTreeMap<i64, usize> = bars
        .iter()
        .enumerate()
        .map(|(index, bar)| (bar.close_time_ns, index))
        .collect();
    let volatility: Vec<Option<f64>> = (0..bars.len())
        .map(|index| causal_volatility(bars, index, profile.outcome_label.volatility_lookback_bars))
        .collect();
    let mut queries = Vec::new();
    let mut right_censored_queries = Vec::new();
    let mut candidate_observations = 0_usize;
    let label_source_end_timestamp_ns = bars.last().map_or(0, |bar| bar.close_time_ns);
    for frame in frames.iter().filter(|frame| {
        frame.timestamp_ns >= profile.reliability.temporal_split_timestamp_ns
            && frame.timestamp_ns <= profile.calibration_end_ns
    }) {
        let Some(&bar_index) = bar_positions.get(&frame.timestamp_ns) else {
            return Err(CalibrationError::Alignment);
        };
        let Some(volatility) = volatility[bar_index] else {
            continue;
        };
        candidate_observations += 1;
        let (actual_direction, first_passage_bars) =
            match outcome_label_state(bars, bar_index, volatility, &profile.outcome_label) {
                Some(OutcomeLabelState::Resolved(direction, first_passage_bars)) => {
                    (direction, first_passage_bars)
                }
                Some(OutcomeLabelState::RightCensored {
                    observed_future_bars,
                }) => {
                    right_censored_queries.push(RightCensoredAuditObservation {
                        query_timestamp_ns: frame.timestamp_ns,
                        observed_future_bars,
                        required_future_bars: profile.outcome_label.maximum_horizon_bars,
                        label_source_end_timestamp_ns,
                        reason: "NO_BARRIER_HIT_BEFORE_INCOMPLETE_LABEL_HORIZON".into(),
                    });
                    continue;
                }
                None => continue,
            };
        let candidate = Candidate {
            timestamp_ns: frame.timestamp_ns,
            raw: frame.vector.values.clone(),
            mask: frame.vector.availability_mask.clone(),
            direction: actual_direction,
            first_passage_bars,
        };
        let query = normalized_sample(&candidate, &profile.normalization)?;
        queries.push(query);
    }
    queries.sort_by_key(|query| query.timestamp_ns);
    right_censored_queries.sort_by_key(|query| query.query_timestamp_ns);
    Ok(DevelopmentAuditLabeling {
        candidate_observations,
        queries,
        right_censored_queries,
    })
}

pub fn build_right_censoring_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<RightCensoringAudit, CalibrationError> {
    validate_profile(profile)?;
    if profile.instrument_id != instrument_id || profile.timeframe != timeframe {
        return Err(CalibrationError::Incompatible(
            "instrument/timeframe differs from diagnostic profile".into(),
        ));
    }
    let labeling = development_audit_labeling(bars, frames, profile)?;
    let parity = development_audit_runtime_parity(&labeling.queries, profile);
    let scored = score_power(
        &labeling.queries,
        &profile.samples,
        profile.estimator.neighbor_count,
        profile.estimator.minimum_support,
        profile.estimator.maximum_distance,
        profile.estimator.distance_power,
    );
    let brier = multiclass_brier_score(&scored.3);
    let climatology_brier = climatology_brier_score(&scored.3, &profile.samples);
    let mut labelable_counts = [0_usize; 3];
    for query in &labeling.queries {
        labelable_counts[direction_index(query.direction)] += 1;
    }
    let label_source_end_timestamp_ns = bars.last().map_or(0, |bar| bar.close_time_ns);
    let label_source_bars_after_query_cutoff = bars
        .iter()
        .filter(|bar| bar.close_time_ns > profile.calibration_end_ns)
        .count();
    let labelable_queries = labeling
        .queries
        .iter()
        .map(|query| LabelableAuditObservation {
            query_timestamp_ns: query.timestamp_ns,
            actual_direction: query.direction,
            first_passage_bars: query.first_passage_bars,
        })
        .collect();

    Ok(RightCensoringAudit {
        diagnostic: "Development Audit Right-Censoring Correction".into(),
        instrument_id: instrument_id.into(),
        timeframe,
        structural_vector_version: profile.structural_vector_version.clone(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().unwrap_or_default(),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        query_cutoff_timestamp_ns: profile.calibration_end_ns,
        label_source_end_timestamp_ns,
        label_source_extends_query_cutoff: label_source_end_timestamp_ns
            > profile.calibration_end_ns,
        label_source_bars_after_query_cutoff,
        maximum_horizon_bars: profile.outcome_label.maximum_horizon_bars,
        target_rule: "FIRST_PASSAGE_IF_OBSERVED; OTHERWISE_RANGE_ONLY_AFTER_FULL_MAXIMUM_HORIZON; OTHERWISE_RIGHT_CENSORED".into(),
        audit_candidate_observations: labeling.candidate_observations,
        labelable_observations: labeling.queries.len(),
        labelable_queries,
        right_censored_observations: labeling.right_censored_queries.len(),
        right_censored_queries: labeling.right_censored_queries,
        labelable_actual_direction_counts: diagnostic_count(labelable_counts),
        runtime_minimum_support: profile.estimator.minimum_support,
        runtime_resolved_observations: parity.resolved_observations,
        runtime_support_unresolved_observations: parity.unresolved_observations,
        runtime_support_unresolved_queries: parity.unresolved_queries,
        resolved_actual_direction_counts: parity.resolved_actual_direction_counts,
        resolved_predicted_direction_counts: parity.resolved_predicted_direction_counts,
        resolved_only_confusion_matrix: parity.resolved_only_confusion_matrix,
        resolved_correct: scored.0,
        resolved_accuracy_bp: ratio_bp(scored.0, scored.1),
        resolved_balanced_accuracy_bp: balanced_accuracy_bp(&scored.3),
        resolved_multiclass_brier_score: brier,
        resolved_climatology_brier_score: climatology_brier,
        resolved_brier_skill_score: if climatology_brier > 0.0 {
            1.0 - brier / climatology_brier
        } else {
            0.0
        },
    })
}

pub fn build_range_distance_geometry_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<RangeDistanceGeometryAudit, CalibrationError> {
    validate_profile(profile)?;
    if profile.instrument_id != instrument_id || profile.timeframe != timeframe {
        return Err(CalibrationError::Incompatible(
            "instrument/timeframe differs from diagnostic profile".into(),
        ));
    }
    let queries = development_audit_queries(bars, frames, profile)?;
    let parity = development_audit_runtime_parity(&queries, profile);
    let range_queries = queries
        .iter()
        .filter(|query| query.direction == Direction::Range)
        .map(|query| range_distance_geometry_query(query, profile))
        .collect::<Vec<_>>();
    let aggregate = range_distance_geometry_aggregate(&range_queries);
    Ok(RangeDistanceGeometryAudit {
        diagnostic: "RANGE Class-Distance Geometry Audit".into(),
        instrument_id: instrument_id.into(),
        timeframe,
        structural_vector_version: profile.structural_vector_version.clone(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().unwrap_or_default(),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        neighbor_count: profile.estimator.neighbor_count,
        runtime_minimum_support: profile.estimator.minimum_support,
        maximum_distance: profile.estimator.maximum_distance,
        distance: "RMS_OVER_ACTIVE_NORMALIZED_DIMENSIONS".into(),
        availability_rule: "EXACT_AVAILABILITY_MASK_EQUALITY".into(),
        candidate_order: "DISTANCE_ASC_THEN_TIMESTAMP_ASC".into(),
        percentile_method: "LINEAR_INTERPOLATION_AT_INDEX_(N_MINUS_1)_Q".into(),
        development_audit_runtime_parity: parity,
        queries: range_queries,
        aggregate,
    })
}

fn development_audit_runtime_parity(
    queries: &[CalibratedSample],
    profile: &ResolutionCalibrationProfile,
) -> DevelopmentAuditRuntimeParity {
    let mut resolved_actual = [0_usize; 3];
    let mut resolved_predicted = [0_usize; 3];
    let mut unresolved_actual = [0_usize; 3];
    let mut confusion = empty_direction_confusion_matrix();
    let mut unresolved_queries = Vec::new();
    for query in queries {
        if let Some(evaluation) = evaluate_vote(query, &profile.samples, &profile.estimator) {
            resolved_actual[direction_index(query.direction)] += 1;
            resolved_predicted[direction_index(evaluation.result.winning)] += 1;
            *confusion
                .get_mut(direction_key(query.direction))
                .expect("audit labels are resolved directions")
                .get_mut(direction_key(evaluation.result.winning))
                .expect("resolver winners are resolved directions") += 1;
        } else {
            let selected_neighbor_count =
                admissible_neighbors(query, &profile.samples, &profile.estimator)
                    .len()
                    .min(profile.estimator.neighbor_count);
            unresolved_actual[direction_index(query.direction)] += 1;
            unresolved_queries.push(UnresolvedAuditObservation {
                query_timestamp_ns: query.timestamp_ns,
                actual_direction: query.direction,
                selected_neighbor_count,
                runtime_minimum_support: profile.estimator.minimum_support,
            });
        }
    }
    DevelopmentAuditRuntimeParity {
        audit_observations: queries.len(),
        resolved_observations: queries.len() - unresolved_queries.len(),
        unresolved_observations: unresolved_queries.len(),
        resolved_actual_direction_counts: diagnostic_count(resolved_actual),
        resolved_predicted_direction_counts: diagnostic_count(resolved_predicted),
        unresolved_actual_direction_counts: diagnostic_count(unresolved_actual),
        resolved_only_confusion_matrix: confusion,
        unresolved_queries,
    }
}

fn empty_direction_confusion_matrix() -> BTreeMap<String, BTreeMap<String, usize>> {
    [Direction::Up, Direction::Down, Direction::Range]
        .into_iter()
        .map(|actual| {
            (
                direction_key(actual).into(),
                [Direction::Up, Direction::Down, Direction::Range]
                    .into_iter()
                    .map(|predicted| (direction_key(predicted).into(), 0))
                    .collect(),
            )
        })
        .collect()
}

fn range_distance_geometry_query(
    query: &CalibratedSample,
    profile: &ResolutionCalibrationProfile,
) -> RangeDistanceGeometryQuery {
    let admissible = admissible_neighbors(query, &profile.samples, &profile.estimator);
    let selected_neighbor_count = admissible.len().min(profile.estimator.neighbor_count);
    let runtime_resolvable = evaluate_vote(query, &profile.samples, &profile.estimator).is_some();
    let top_k_composition = [5_usize, 10, 20, 27, 50, 100]
        .into_iter()
        .map(|requested| {
            let actual_k_used = requested.min(admissible.len());
            let counts = count_neighbor_directions(&admissible[..actual_k_used]);
            (
                requested.to_string(),
                TopKClassComposition {
                    actual_k_used,
                    up: counts[0],
                    down: counts[2],
                    range: counts[1],
                },
            )
        })
        .collect();
    RangeDistanceGeometryQuery {
        query_timestamp_ns: query.timestamp_ns,
        actual_direction: query.direction,
        candidates_after_mask: profile
            .samples
            .iter()
            .filter(|sample| sample.availability_mask == query.availability_mask)
            .count(),
        candidates_within_maximum_distance: admissible.len(),
        selected_neighbor_count,
        runtime_minimum_support: profile.estimator.minimum_support,
        runtime_resolvable,
        class_distance_stats: ClassDistanceGeometry {
            up: class_distance_stats(&admissible, Direction::Up),
            down: class_distance_stats(&admissible, Direction::Down),
            range: class_distance_stats(&admissible, Direction::Range),
        },
        top_k_composition,
        nearest_by_class: NearestByClass {
            up: nearest_class_candidate(&admissible, Direction::Up),
            down: nearest_class_candidate(&admissible, Direction::Down),
            range: nearest_class_candidate(&admissible, Direction::Range),
        },
    }
}

fn count_neighbor_directions(neighbors: &[EvaluatedNeighbor<'_>]) -> [usize; 3] {
    let mut counts = [0_usize; 3];
    for neighbor in neighbors {
        counts[direction_index(neighbor.sample.direction)] += 1;
    }
    counts
}

fn class_distance_stats(
    neighbors: &[EvaluatedNeighbor<'_>],
    direction: Direction,
) -> ClassDistanceStats {
    let distances = neighbors
        .iter()
        .filter(|neighbor| neighbor.sample.direction == direction)
        .map(|neighbor| neighbor.breakdown.distance)
        .collect::<Vec<_>>();
    ClassDistanceStats {
        count: distances.len(),
        minimum: distances.first().copied(),
        p10: percentile(&distances, 0.10),
        p25: percentile(&distances, 0.25),
        median: percentile(&distances, 0.50),
        p75: percentile(&distances, 0.75),
        maximum: distances.last().copied(),
    }
}

fn percentile(sorted: &[f64], quantile: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let index = (sorted.len() - 1) as f64 * quantile;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let fraction = index - lower as f64;
    Some(sorted[lower] + (sorted[upper] - sorted[lower]) * fraction)
}

fn nearest_class_candidate(
    neighbors: &[EvaluatedNeighbor<'_>],
    direction: Direction,
) -> Option<NearestClassCandidate> {
    neighbors
        .iter()
        .enumerate()
        .find(|(_, neighbor)| neighbor.sample.direction == direction)
        .map(|(rank, neighbor)| NearestClassCandidate {
            rank_among_all_admissible: rank + 1,
            distance: neighbor.breakdown.distance,
            timestamp_ns: neighbor.sample.timestamp_ns,
        })
}

fn range_distance_geometry_aggregate(
    queries: &[RangeDistanceGeometryQuery],
) -> RangeDistanceGeometryAggregate {
    let mean_range_share_by_k = [5_usize, 10, 20, 27, 50, 100]
        .into_iter()
        .map(|requested| {
            let key = requested.to_string();
            let mean = if queries.is_empty() {
                0.0
            } else {
                queries
                    .iter()
                    .map(|query| {
                        let composition = &query.top_k_composition[&key];
                        if composition.actual_k_used == 0 {
                            0.0
                        } else {
                            composition.range as f64 / composition.actual_k_used as f64
                        }
                    })
                    .sum::<f64>()
                    / queries.len() as f64
            };
            (key, mean)
        })
        .collect();
    let mut range_ranks = queries
        .iter()
        .filter_map(|query| {
            query
                .nearest_by_class
                .range
                .as_ref()
                .map(|nearest| nearest.rank_among_all_admissible as f64)
        })
        .collect::<Vec<_>>();
    RangeDistanceGeometryAggregate {
        actual_range_queries: queries.len(),
        runtime_resolvable_range_queries: queries
            .iter()
            .filter(|query| query.runtime_resolvable)
            .count(),
        runtime_unresolved_range_queries: queries
            .iter()
            .filter(|query| !query.runtime_resolvable)
            .count(),
        mean_range_share_by_k,
        median_range_rank: median(&mut range_ranks),
        mean_class_median_distance: OptionalClassDistance {
            up: mean_optional(
                queries
                    .iter()
                    .map(|query| query.class_distance_stats.up.median),
            ),
            down: mean_optional(
                queries
                    .iter()
                    .map(|query| query.class_distance_stats.down.median),
            ),
            range: mean_optional(
                queries
                    .iter()
                    .map(|query| query.class_distance_stats.range.median),
            ),
        },
    }
}

fn mean_optional(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    let values = values.flatten().collect::<Vec<_>>();
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

pub fn build_range_intraclass_compactness_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<RangeIntraclassCompactnessAudit, CalibrationError> {
    validate_profile(profile)?;
    if profile.instrument_id != instrument_id || profile.timeframe != timeframe {
        return Err(CalibrationError::Incompatible(
            "instrument/timeframe differs from diagnostic profile".into(),
        ));
    }
    let samples = labeled_sample_pool(bars, frames, profile)?;
    let mut class_counts = [0_usize; 3];
    for sample in &samples {
        class_counts[direction_index(sample.direction)] += 1;
    }
    let k_values = vec![5_usize, 10, 20, 27];
    Ok(RangeIntraclassCompactnessAudit {
        diagnostic: "RANGE Intraclass Compactness Audit".into(),
        instrument_id: instrument_id.into(),
        timeframe,
        structural_vector_version: profile.structural_vector_version.clone(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().unwrap_or_default(),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        labeled_sample_count: samples.len(),
        labeled_class_counts: diagnostic_count(class_counts),
        self_neighbor_rule: "EXCLUDE_IDENTICAL_TIMESTAMP".into(),
        availability_rule: "EXACT_AVAILABILITY_MASK_EQUALITY".into(),
        distance: "RMS_OVER_ACTIVE_NORMALIZED_DIMENSIONS".into(),
        nearest_neighbor_cutoff_rule: "NO_MAXIMUM_DISTANCE_CENSORING".into(),
        top_k_cutoff_rule: "DISTANCE_LE_MAXIMUM_DISTANCE_THEN_DISTANCE_ASC_TIMESTAMP_ASC".into(),
        maximum_distance: profile.estimator.maximum_distance,
        k_values: k_values.clone(),
        leave_one_out_all_time: compactness_view(
            &samples,
            profile.estimator.maximum_distance,
            &k_values,
            false,
        ),
        causal_prefix: compactness_view(
            &samples,
            profile.estimator.maximum_distance,
            &k_values,
            true,
        ),
    })
}

fn labeled_sample_pool(
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
) -> Result<Vec<CalibratedSample>, CalibrationError> {
    let mut samples = profile.samples.clone();
    samples.extend(development_audit_queries(bars, frames, profile)?);
    samples.sort_by_key(|sample| sample.timestamp_ns);
    if samples
        .windows(2)
        .any(|pair| pair[0].timestamp_ns == pair[1].timestamp_ns)
    {
        return Err(CalibrationError::Diagnostic(
            "diagnostic sample timestamps are not unique".into(),
        ));
    }
    Ok(samples)
}

fn compactness_view(
    samples: &[CalibratedSample],
    maximum_distance: f64,
    k_values: &[usize],
    causal_prefix: bool,
) -> CompactnessView {
    let observations = samples
        .iter()
        .map(|query| {
            compactness_observation(query, samples, maximum_distance, k_values, causal_prefix)
        })
        .collect::<Vec<_>>();
    CompactnessView {
        candidate_time_rule: if causal_prefix {
            "STRICTLY_PRIOR_TIMESTAMP_ONLY"
        } else {
            "ALL_TIMESTAMPS_EXCEPT_QUERY"
        }
        .into(),
        query_samples: observations.len(),
        class_compactness: CompactnessByClass {
            up: summarize_compactness(&observations, Direction::Up, k_values),
            down: summarize_compactness(&observations, Direction::Down, k_values),
            range: summarize_compactness(&observations, Direction::Range, k_values),
        },
    }
}

fn compactness_observation(
    query: &CalibratedSample,
    samples: &[CalibratedSample],
    maximum_distance: f64,
    k_values: &[usize],
    causal_prefix: bool,
) -> CompactnessObservation {
    let mut neighbors = samples
        .iter()
        .filter(|sample| sample.timestamp_ns != query.timestamp_ns)
        .filter(|sample| !causal_prefix || sample.timestamp_ns < query.timestamp_ns)
        .filter(|sample| sample.availability_mask == query.availability_mask)
        .map(|sample| (distance(query, sample), sample))
        .collect::<Vec<_>>();
    neighbors.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.timestamp_ns.cmp(&right.1.timestamp_ns))
    });
    let nearest_same_class_distance = neighbors
        .iter()
        .find(|(_, sample)| sample.direction == query.direction)
        .map(|(distance, _)| *distance);
    let nearest_other_class_distance = neighbors
        .iter()
        .find(|(_, sample)| sample.direction != query.direction)
        .map(|(distance, _)| *distance);
    let admissible = neighbors
        .iter()
        .take_while(|(distance, _)| *distance <= maximum_distance)
        .collect::<Vec<_>>();
    let same_class_fraction_by_k = k_values
        .iter()
        .map(|requested| {
            let actual_k_used = (*requested).min(admissible.len());
            let same_class = admissible[..actual_k_used]
                .iter()
                .filter(|(_, sample)| sample.direction == query.direction)
                .count();
            (
                requested.to_string(),
                (
                    actual_k_used,
                    if actual_k_used == 0 {
                        0.0
                    } else {
                        same_class as f64 / actual_k_used as f64
                    },
                ),
            )
        })
        .collect();
    CompactnessObservation {
        direction: query.direction,
        nearest_same_class_distance,
        nearest_other_class_distance,
        nearest_same_within_maximum_distance: nearest_same_class_distance
            .map(|distance| distance <= maximum_distance),
        nearest_other_within_maximum_distance: nearest_other_class_distance
            .map(|distance| distance <= maximum_distance),
        same_class_fraction_by_k,
    }
}

fn summarize_compactness(
    observations: &[CompactnessObservation],
    direction: Direction,
    k_values: &[usize],
) -> ClassCompactnessSummary {
    let selected = observations
        .iter()
        .filter(|observation| observation.direction == direction)
        .collect::<Vec<_>>();
    let nearest_same = selected
        .iter()
        .filter_map(|observation| observation.nearest_same_class_distance)
        .collect::<Vec<_>>();
    let nearest_other = selected
        .iter()
        .filter_map(|observation| observation.nearest_other_class_distance)
        .collect::<Vec<_>>();
    let same_within = selected
        .iter()
        .filter_map(|observation| observation.nearest_same_within_maximum_distance)
        .collect::<Vec<_>>();
    let other_within = selected
        .iter()
        .filter_map(|observation| observation.nearest_other_within_maximum_distance)
        .collect::<Vec<_>>();
    ClassCompactnessSummary {
        sample_count: selected.len(),
        nearest_same_class_distance: numeric_distance_stats(nearest_same),
        nearest_other_class_distance: numeric_distance_stats(nearest_other),
        nearest_same_within_maximum_distance_fraction: boolean_fraction(&same_within),
        nearest_other_within_maximum_distance_fraction: boolean_fraction(&other_within),
        runtime_admissible_same_class_fraction_by_k: k_values
            .iter()
            .map(|requested| {
                let key = requested.to_string();
                let entries = selected
                    .iter()
                    .map(|observation| observation.same_class_fraction_by_k[&key])
                    .filter(|(actual_k_used, _)| *actual_k_used > 0)
                    .collect::<Vec<_>>();
                let mut fractions = entries
                    .iter()
                    .map(|(_, fraction)| *fraction)
                    .collect::<Vec<_>>();
                fractions.sort_by(f64::total_cmp);
                let actual_k_total = entries
                    .iter()
                    .map(|(actual_k_used, _)| *actual_k_used)
                    .sum::<usize>();
                (
                    key,
                    SameClassFractionStats {
                        requested_k: *requested,
                        evaluable_samples: entries.len(),
                        samples_with_full_k: entries
                            .iter()
                            .filter(|(actual_k_used, _)| actual_k_used == requested)
                            .count(),
                        mean_actual_k_used: if entries.is_empty() {
                            None
                        } else {
                            Some(actual_k_total as f64 / entries.len() as f64)
                        },
                        minimum: fractions.first().copied(),
                        median: percentile(&fractions, 0.50),
                        mean: if fractions.is_empty() {
                            None
                        } else {
                            Some(fractions.iter().sum::<f64>() / fractions.len() as f64)
                        },
                        maximum: fractions.last().copied(),
                    },
                )
            })
            .collect(),
    }
}

fn numeric_distance_stats(mut values: Vec<f64>) -> ClassDistanceStats {
    values.sort_by(f64::total_cmp);
    ClassDistanceStats {
        count: values.len(),
        minimum: values.first().copied(),
        p10: percentile(&values, 0.10),
        p25: percentile(&values, 0.25),
        median: percentile(&values, 0.50),
        p75: percentile(&values, 0.75),
        maximum: values.last().copied(),
    }
}

fn boolean_fraction(values: &[bool]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().filter(|value| **value).count() as f64 / values.len() as f64)
    }
}

pub fn build_range_trajectory_anatomy_audit(
    instrument_id: &str,
    timeframe: Timeframe,
    bars: &[MarketObservation],
    frames: &[StructuralFrame],
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
) -> Result<RangeTrajectoryAnatomyAudit, CalibrationError> {
    validate_profile(profile)?;
    if profile.instrument_id != instrument_id || profile.timeframe != timeframe {
        return Err(CalibrationError::Incompatible(
            "instrument/timeframe differs from diagnostic profile".into(),
        ));
    }
    let samples = labeled_sample_pool(bars, frames, profile)?;
    let bar_positions = bars
        .iter()
        .enumerate()
        .map(|(index, bar)| (bar.close_time_ns, index))
        .collect::<BTreeMap<_, _>>();
    let volatility = (0..bars.len())
        .map(|index| causal_volatility(bars, index, profile.outcome_label.volatility_lookback_bars))
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for sample in samples
        .iter()
        .filter(|sample| sample.direction == Direction::Range)
    {
        let bar_index = *bar_positions
            .get(&sample.timestamp_ns)
            .ok_or(CalibrationError::Alignment)?;
        let label_volatility = volatility[bar_index].ok_or_else(|| {
            CalibrationError::Diagnostic(format!(
                "RANGE sample {} lacks causal label volatility",
                sample.timestamp_ns
            ))
        })?;
        let (direction, first_passage_bars) =
            label_outcome(bars, bar_index, label_volatility, &profile.outcome_label).ok_or_else(
                || {
                    CalibrationError::Diagnostic(format!(
                        "RANGE sample {} lacks a reproducible outcome",
                        sample.timestamp_ns
                    ))
                },
            )?;
        if direction != Direction::Range || first_passage_bars != sample.first_passage_bars {
            return Err(CalibrationError::Diagnostic(format!(
                "RANGE sample {} differs from the stored first-passage label",
                sample.timestamp_ns
            )));
        }
        records.push(range_trajectory_record(
            bars,
            bar_index,
            label_volatility,
            sample,
            &profile.outcome_label,
        ));
    }
    records.sort_by_key(|record| record.query_timestamp_ns);
    let aggregate = range_trajectory_aggregate(&records);
    Ok(RangeTrajectoryAnatomyAudit {
        diagnostic: "RANGE Trajectory Anatomy Audit".into(),
        instrument_id: instrument_id.into(),
        timeframe,
        structural_vector_version: profile.structural_vector_version.clone(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().unwrap_or_default(),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        configured_horizon_bars: profile.outcome_label.maximum_horizon_bars,
        upper_barrier_volatility_multiple: profile.outcome_label.upper_barrier_volatility_multiple,
        lower_barrier_volatility_multiple: profile.outcome_label.lower_barrier_volatility_multiple,
        realized_volatility_definition:
            "SQRT_SUM_SQUARED_CLOSE_TO_CLOSE_LOG_RETURNS_OVER_OBSERVED_LABEL_PATH".into(),
        direction_reversal_definition:
            "SIGN_CHANGES_IN_NONZERO_CLOSE_TO_CLOSE_RETURNS_OVER_OBSERVED_LABEL_PATH".into(),
        records,
        aggregate,
    })
}

fn range_trajectory_record(
    bars: &[MarketObservation],
    bar_index: usize,
    label_volatility: f64,
    sample: &CalibratedSample,
    parameters: &OutcomeLabelParameters,
) -> RangeTrajectoryRecord {
    let origin = bars[bar_index].close;
    let upper_barrier_return = label_volatility * parameters.upper_barrier_volatility_multiple;
    let lower_barrier_return = label_volatility * parameters.lower_barrier_volatility_multiple;
    let path = &bars[bar_index + 1..=bar_index + sample.first_passage_bars];
    let upper_barrier = origin * (1.0 + upper_barrier_return);
    let lower_barrier = origin * (1.0 - lower_barrier_return);
    let label_mechanism = if path
        .last()
        .is_some_and(|bar| bar.high >= upper_barrier && bar.low <= lower_barrier)
    {
        "SIMULTANEOUS_BARRIER_HIT"
    } else {
        "NO_HIT"
    };
    let mut maximum_up_excursion = 0.0_f64;
    let mut maximum_down_excursion = 0.0_f64;
    let mut time_of_maximum_up_excursion_bars = 1_usize;
    let mut time_of_maximum_down_excursion_bars = 1_usize;
    let mut squared_log_returns = 0.0_f64;
    let mut direction_reversals = 0_usize;
    let mut previous_close = origin;
    let mut previous_sign = 0_i8;
    for (offset, bar) in path.iter().enumerate() {
        let time = offset + 1;
        let up_excursion = ((bar.high / origin) - 1.0).max(0.0);
        let down_excursion = (1.0 - bar.low / origin).max(0.0);
        if up_excursion > maximum_up_excursion {
            maximum_up_excursion = up_excursion;
            time_of_maximum_up_excursion_bars = time;
        }
        if down_excursion > maximum_down_excursion {
            maximum_down_excursion = down_excursion;
            time_of_maximum_down_excursion_bars = time;
        }
        let log_return = (bar.close / previous_close).ln();
        squared_log_returns += log_return * log_return;
        let sign = if log_return > 0.0 {
            1
        } else if log_return < 0.0 {
            -1
        } else {
            0
        };
        if sign != 0 {
            if previous_sign != 0 && sign != previous_sign {
                direction_reversals += 1;
            }
            previous_sign = sign;
        }
        previous_close = bar.close;
    }
    RangeTrajectoryRecord {
        query_timestamp_ns: sample.timestamp_ns,
        actual_direction: sample.direction,
        label_mechanism: label_mechanism.into(),
        configured_horizon_bars: parameters.maximum_horizon_bars,
        observed_label_path_bars: sample.first_passage_bars,
        first_passage_bars: sample.first_passage_bars,
        origin_close: origin,
        causal_label_volatility: label_volatility,
        upper_barrier_return,
        lower_barrier_return,
        maximum_up_excursion,
        maximum_down_excursion,
        upper_excursion_ratio: maximum_up_excursion / upper_barrier_return,
        lower_excursion_ratio: maximum_down_excursion / lower_barrier_return,
        terminal_displacement: path.last().expect("label path is nonempty").close / origin - 1.0,
        realized_volatility: squared_log_returns.sqrt(),
        direction_reversals,
        time_of_maximum_up_excursion_bars,
        time_of_maximum_down_excursion_bars,
    }
}

fn range_trajectory_aggregate(records: &[RangeTrajectoryRecord]) -> RangeTrajectoryAggregate {
    let mut label_mechanism_counts = BTreeMap::new();
    for record in records {
        *label_mechanism_counts
            .entry(record.label_mechanism.clone())
            .or_insert(0) += 1;
    }
    let by_label_mechanism = label_mechanism_counts
        .keys()
        .map(|mechanism| {
            let selected = records
                .iter()
                .filter(|record| &record.label_mechanism == mechanism)
                .collect::<Vec<_>>();
            (
                mechanism.clone(),
                range_trajectory_metric_summary(&selected),
            )
        })
        .collect();
    RangeTrajectoryAggregate {
        actual_range_samples: records.len(),
        label_mechanism_counts,
        full_configured_horizon_samples: records
            .iter()
            .filter(|record| record.observed_label_path_bars == record.configured_horizon_bars)
            .count(),
        truncated_horizon_samples: records
            .iter()
            .filter(|record| {
                record.observed_label_path_bars < record.configured_horizon_bars
                    && record.label_mechanism == "NO_HIT"
            })
            .count(),
        upper_ratio_at_or_above_one: records
            .iter()
            .filter(|record| record.upper_excursion_ratio >= 1.0)
            .count(),
        lower_ratio_at_or_above_one: records
            .iter()
            .filter(|record| record.lower_excursion_ratio >= 1.0)
            .count(),
        both_ratios_at_or_above_one: records
            .iter()
            .filter(|record| {
                record.upper_excursion_ratio >= 1.0 && record.lower_excursion_ratio >= 1.0
            })
            .count(),
        upper_lower_excursion_ratio_pearson_correlation: pearson_correlation(
            records.iter().map(|record| record.upper_excursion_ratio),
            records.iter().map(|record| record.lower_excursion_ratio),
        ),
        all_range: range_trajectory_metric_summary(&records.iter().collect::<Vec<_>>()),
        by_label_mechanism,
    }
}

fn range_trajectory_metric_summary(
    records: &[&RangeTrajectoryRecord],
) -> RangeTrajectoryMetricSummary {
    RangeTrajectoryMetricSummary {
        upper_excursion_ratio: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.upper_excursion_ratio)
                .collect(),
        ),
        lower_excursion_ratio: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.lower_excursion_ratio)
                .collect(),
        ),
        maximum_up_excursion: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.maximum_up_excursion)
                .collect(),
        ),
        maximum_down_excursion: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.maximum_down_excursion)
                .collect(),
        ),
        terminal_displacement: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.terminal_displacement)
                .collect(),
        ),
        realized_volatility: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.realized_volatility)
                .collect(),
        ),
        direction_reversals: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.direction_reversals as f64)
                .collect(),
        ),
        time_of_maximum_up_excursion_bars: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.time_of_maximum_up_excursion_bars as f64)
                .collect(),
        ),
        time_of_maximum_down_excursion_bars: numeric_distance_stats(
            records
                .iter()
                .map(|record| record.time_of_maximum_down_excursion_bars as f64)
                .collect(),
        ),
    }
}

fn pearson_correlation(
    left: impl Iterator<Item = f64>,
    right: impl Iterator<Item = f64>,
) -> Option<f64> {
    let pairs = left.zip(right).collect::<Vec<_>>();
    if pairs.len() < 2 {
        return None;
    }
    let left_mean = pairs.iter().map(|(left, _)| *left).sum::<f64>() / pairs.len() as f64;
    let right_mean = pairs.iter().map(|(_, right)| *right).sum::<f64>() / pairs.len() as f64;
    let covariance = pairs
        .iter()
        .map(|(left, right)| (left - left_mean) * (right - right_mean))
        .sum::<f64>();
    let left_scale = pairs
        .iter()
        .map(|(left, _)| (left - left_mean).powi(2))
        .sum::<f64>()
        .sqrt();
    let right_scale = pairs
        .iter()
        .map(|(_, right)| (right - right_mean).powi(2))
        .sum::<f64>()
        .sqrt();
    if left_scale == 0.0 || right_scale == 0.0 {
        None
    } else {
        Some(covariance / (left_scale * right_scale))
    }
}

fn anatomy_query(
    query: &CalibratedSample,
    evaluation: VoteEvaluation<'_>,
    parameters: &NeighborParameters,
) -> NeighborAnatomyQuery {
    let total_mass: f64 = evaluation.weighted_mass.iter().sum();
    let zero_total_weighted_mass = total_mass == 0.0;
    let normalized = if zero_total_weighted_mass {
        [0.0; 3]
    } else {
        evaluation.weighted_mass.map(|mass| mass / total_mass)
    };
    let nearest_range_rank = evaluation
        .neighbors
        .iter()
        .position(|neighbor| neighbor.sample.direction == Direction::Range)
        .map(|index| index + 1);
    let selected_neighbor_count = evaluation.neighbors.len();
    let neighbors = evaluation
        .neighbors
        .into_iter()
        .enumerate()
        .map(|(index, neighbor)| NeighborAnatomyRecord {
            rank: index + 1,
            neighbor_timestamp_ns: neighbor.sample.timestamp_ns,
            distance: neighbor.breakdown.distance,
            direction: neighbor.sample.direction,
            weight: neighbor.weight,
            neighbor_availability_mask: neighbor.sample.availability_mask.clone(),
            distance_dimension_mask: neighbor.breakdown.dimension_mask,
            active_dimension_count: neighbor.breakdown.active_dimension_count,
            normalized_abs_delta: neighbor.breakdown.normalized_abs_delta,
            dimension_contribution: neighbor.breakdown.dimension_contribution,
            contribution_basis: "SQUARED_NORMALIZED_DELTA_FRACTION_OF_SUM_SQUARED".into(),
            zero_distance: neighbor.breakdown.zero_distance,
        })
        .collect();
    NeighborAnatomyQuery {
        query_timestamp_ns: query.timestamp_ns,
        query_vector: query.vector.clone(),
        query_availability_mask: query.availability_mask.clone(),
        actual_direction: query.direction,
        requested_neighbor_count: parameters.neighbor_count,
        selected_neighbor_count,
        selection_note: if selected_neighbor_count == parameters.neighbor_count {
            "REQUESTED_COUNT_SELECTED"
        } else {
            "FEWER_THAN_REQUESTED_AFTER_EXISTING_MASK_AND_MAXIMUM_DISTANCE_FILTERS"
        }
        .into(),
        neighbors,
        weighted_mass: diagnostic_mass(evaluation.weighted_mass),
        normalized_weighted_mass: diagnostic_mass(normalized),
        total_weighted_mass: total_mass,
        zero_total_weighted_mass,
        unweighted_count: diagnostic_count(evaluation.unweighted_count),
        nearest_range_rank,
        predicted_direction: evaluation.result.winning,
    }
}

fn anatomy_summary(
    instrument_id: &str,
    timeframe: Timeframe,
    profile: &ResolutionCalibrationProfile,
    generation_timestamp_unix_seconds: u64,
    audit_tail: &[NeighborAnatomyQuery],
) -> NeighborAnatomySummary {
    let dimension = profile.normalization.names.len();
    let mut contribution_samples = vec![Vec::<f64>::new(); dimension];
    let mut top_dimension_frequency = vec![0_usize; dimension];
    let mut evaluated_query_neighbor_pairs = 0_usize;
    let mut zero_distance_neighbor_pairs = 0_usize;
    for query in audit_tail {
        for neighbor in &query.neighbors {
            evaluated_query_neighbor_pairs += 1;
            for (axis, contribution) in neighbor.dimension_contribution.iter().enumerate() {
                contribution_samples[axis].push(*contribution);
            }
            if neighbor.zero_distance {
                zero_distance_neighbor_pairs += 1;
            } else {
                let top = neighbor
                    .dimension_contribution
                    .iter()
                    .enumerate()
                    .max_by(|left, right| {
                        left.1.total_cmp(right.1).then_with(|| right.0.cmp(&left.0))
                    })
                    .map(|(axis, _)| axis)
                    .expect("structural vector is nonempty");
                top_dimension_frequency[top] += 1;
            }
        }
    }
    let mean_dimension_contribution = profile
        .normalization
        .names
        .iter()
        .enumerate()
        .map(|(axis, name)| {
            let values = &contribution_samples[axis];
            let mean = if values.is_empty() {
                0.0
            } else {
                values.iter().sum::<f64>() / values.len() as f64
            };
            (name.clone(), mean)
        })
        .collect();
    let median_dimension_contribution = profile
        .normalization
        .names
        .iter()
        .enumerate()
        .map(|(axis, name)| {
            let mut values = contribution_samples[axis].clone();
            (name.clone(), median(&mut values).unwrap_or(0.0))
        })
        .collect();
    let top_dimension_frequency = profile
        .normalization
        .names
        .iter()
        .cloned()
        .zip(top_dimension_frequency)
        .collect();
    let actual_range_diagnostics = audit_tail
        .iter()
        .filter(|query| query.actual_direction == Direction::Range)
        .map(|query| ActualRangeDiagnostic {
            query_timestamp_ns: query.query_timestamp_ns,
            predicted_direction: query.predicted_direction,
            neighbor_counts: query.unweighted_count.clone(),
            normalized_weighted_mass: query.normalized_weighted_mass.clone(),
            nearest_range_rank: query.nearest_range_rank,
            top_distance_dimensions: query_top_dimensions(query, &profile.normalization.names),
        })
        .collect::<Vec<_>>();
    NeighborAnatomySummary {
        diagnostic: "DIAGNOSTIC 0 — Neighbor Anatomy Dump".into(),
        instrument_id: instrument_id.into(),
        timeframe,
        structural_vector_version: profile.structural_vector_version.clone(),
        calibration_profile_id: profile.profile_id.clone(),
        calibration_profile_sha256: profile.profile_sha256.clone().unwrap_or_default(),
        diagnostic_generation_timestamp_unix_seconds: generation_timestamp_unix_seconds,
        number_of_neighbors: profile.estimator.neighbor_count,
        audit_minimum_support: profile.estimator.minimum_support,
        profile_runtime_minimum_support: profile.estimator.minimum_support,
        effective_dimension_count: profile.normalization.effective_dimension_count,
        audit_points: audit_tail.len(),
        actual_direction_counts: count_query_directions(audit_tail, true),
        predicted_direction_counts: count_query_directions(audit_tail, false),
        actual_range_points: actual_range_diagnostics.len(),
        actual_range_diagnostics,
        mean_normalized_class_mass: mean_query_mass(audit_tail),
        evaluated_query_neighbor_pairs,
        zero_distance_neighbor_pairs,
        top_dimension_frequency_eligible_pairs: evaluated_query_neighbor_pairs
            - zero_distance_neighbor_pairs,
        top_dimension_tie_break: "LOWEST_STRUCTURAL_VECTOR_INDEX".into(),
        mean_dimension_contribution,
        median_dimension_contribution,
        top_dimension_frequency,
    }
}

fn query_top_dimensions(
    query: &NeighborAnatomyQuery,
    names: &[String],
) -> Vec<TopDistanceDimension> {
    let mut dimensions: Vec<TopDistanceDimension> = names
        .iter()
        .enumerate()
        .map(|(axis, name)| TopDistanceDimension {
            dimension: name.clone(),
            mean_contribution: if query.neighbors.is_empty() {
                0.0
            } else {
                query
                    .neighbors
                    .iter()
                    .map(|neighbor| neighbor.dimension_contribution[axis])
                    .sum::<f64>()
                    / query.neighbors.len() as f64
            },
        })
        .collect();
    dimensions.sort_by(|left, right| {
        right
            .mean_contribution
            .total_cmp(&left.mean_contribution)
            .then_with(|| left.dimension.cmp(&right.dimension))
    });
    dimensions.truncate(5);
    dimensions
}

fn count_query_directions(queries: &[NeighborAnatomyQuery], actual: bool) -> DiagnosticClassCount {
    let direction = |query: &NeighborAnatomyQuery| {
        if actual {
            query.actual_direction
        } else {
            query.predicted_direction
        }
    };
    DiagnosticClassCount {
        up: queries
            .iter()
            .filter(|query| direction(query) == Direction::Up)
            .count(),
        down: queries
            .iter()
            .filter(|query| direction(query) == Direction::Down)
            .count(),
        range: queries
            .iter()
            .filter(|query| direction(query) == Direction::Range)
            .count(),
    }
}

fn mean_query_mass(queries: &[NeighborAnatomyQuery]) -> DiagnosticClassMass {
    if queries.is_empty() {
        return diagnostic_mass([0.0; 3]);
    }
    let denominator = queries.len() as f64;
    DiagnosticClassMass {
        up: queries
            .iter()
            .map(|query| query.normalized_weighted_mass.up)
            .sum::<f64>()
            / denominator,
        down: queries
            .iter()
            .map(|query| query.normalized_weighted_mass.down)
            .sum::<f64>()
            / denominator,
        range: queries
            .iter()
            .map(|query| query.normalized_weighted_mass.range)
            .sum::<f64>()
            / denominator,
    }
}

fn diagnostic_mass(values: [f64; 3]) -> DiagnosticClassMass {
    DiagnosticClassMass {
        up: values[0],
        down: values[2],
        range: values[1],
    }
}

fn diagnostic_count(values: [usize; 3]) -> DiagnosticClassCount {
    DiagnosticClassCount {
        up: values[0],
        down: values[2],
        range: values[1],
    }
}

pub fn resolve_direction(
    vector: &StructuralVector,
    profile: &ResolutionCalibrationProfile,
) -> Result<DirectionalResolution, CalibrationError> {
    validate_profile(profile)?;
    if vector.version != profile.structural_vector_version
        || vector.names != profile.normalization.names
        || vector.values.len() != profile.normalization.names.len()
        || vector.availability_mask.len() != vector.values.len()
    {
        return Err(CalibrationError::Incompatible(
            "version/names/dimension".into(),
        ));
    }
    let query = normalize_vector(vector, &profile.normalization)?;
    let vote = vote(&query, &profile.samples, &profile.estimator);
    let hash = profile.profile_sha256.clone().expect("validated");
    let Some(vote) = vote else {
        return Ok(unresolved(
            profile.scope,
            hash,
            0,
            "INSUFFICIENT_MATCHED_SUPPORT",
        ));
    };
    let reliability = profile
        .reliability
        .by_direction_lower_bound_bp
        .get(direction_key(vote.winning))
        .copied()
        .unwrap_or(profile.reliability.reliability_lower_bound_bp);
    if !profile.publication.profile_eligible_for_publication {
        let reason = if profile.publication.parameters_selected_on == "PREREGISTERED_PROTOCOL" {
            "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE"
        } else {
            "CALIBRATION_EVIDENCE_NOT_PREREGISTERED"
        };
        return Ok(unresolved(profile.scope, hash, vote.support, reason));
    }
    if profile.publication.requires_positive_brier_skill
        && profile.reliability.brier_skill_score <= 0.0
    {
        return Ok(unresolved(
            profile.scope,
            hash,
            vote.support,
            "HELD_OUT_PROBABILITY_SKILL_NOT_POSITIVE",
        ));
    }
    if vote.edge_bp < profile.publication.minimum_direction_edge_bp {
        return Ok(unresolved(
            profile.scope,
            hash,
            vote.support,
            "CALIBRATED_DIRECTION_EDGE_NOT_MET",
        ));
    }
    if reliability < profile.publication.minimum_reliability_bp {
        return Ok(unresolved(
            profile.scope,
            hash,
            vote.support,
            "HELD_OUT_RELIABILITY_NOT_MET",
        ));
    }
    let horizon = weighted_horizon(&vote.passages, profile.timeframe);
    Ok(DirectionalResolution {
        direction: vote.winning,
        probabilities_bp: Some(vote.probabilities),
        horizon,
        reliability_bp: Some(reliability),
        sample_support: vote.support as u64,
        calibration_scope: profile.scope,
        profile_sha256: hash,
        publication_reason: "CALIBRATED_EVIDENCE_SATISFIED".into(),
    })
}

fn unresolved(
    scope: CalibrationScope,
    hash: String,
    support: usize,
    reason: &str,
) -> DirectionalResolution {
    DirectionalResolution {
        direction: Direction::Unresolved,
        probabilities_bp: None,
        horizon: None,
        reliability_bp: None,
        sample_support: support as u64,
        calibration_scope: scope,
        profile_sha256: hash,
        publication_reason: reason.to_owned(),
    }
}

fn causal_volatility(bars: &[MarketObservation], index: usize, lookback: usize) -> Option<f64> {
    if index == 0 {
        return None;
    }
    let start = index.saturating_sub(lookback - 1).max(1);
    let mut ranges: Vec<f64> = (start..=index)
        .map(|position| (bars[position].high - bars[position].low) / bars[position - 1].close)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect();
    median(&mut ranges)
}

fn empirical_symmetric_barrier(
    bars: &[MarketObservation],
    volatility: &[Option<f64>],
    split_bar: usize,
    horizon: usize,
) -> Result<f64, CalibrationError> {
    let mut excursions = Vec::new();
    for index in 1..split_bar {
        let Some(scale) = volatility[index] else {
            continue;
        };
        let end = (index + horizon).min(split_bar.saturating_sub(1));
        if end <= index {
            continue;
        }
        let origin = bars[index].close;
        excursions.push(
            bars[index + 1..=end]
                .iter()
                .map(|bar| ((bar.high / origin) - 1.0).max(0.0) / scale)
                .fold(0.0, f64::max),
        );
        excursions.push(
            bars[index + 1..=end]
                .iter()
                .map(|bar| (1.0 - bar.low / origin).max(0.0) / scale)
                .fold(0.0, f64::max),
        );
    }
    median(&mut excursions)
        .filter(|value| *value > 0.0)
        .ok_or(CalibrationError::InsufficientData)
}

fn label_outcome(
    bars: &[MarketObservation],
    index: usize,
    volatility: f64,
    parameters: &OutcomeLabelParameters,
) -> Option<(Direction, usize)> {
    match outcome_label_state(bars, index, volatility, parameters)? {
        OutcomeLabelState::Resolved(direction, first_passage_bars) => {
            Some((direction, first_passage_bars))
        }
        OutcomeLabelState::RightCensored { .. } => None,
    }
}

fn outcome_label_state(
    bars: &[MarketObservation],
    index: usize,
    volatility: f64,
    parameters: &OutcomeLabelParameters,
) -> Option<OutcomeLabelState> {
    let required_end = index.checked_add(parameters.maximum_horizon_bars)?;
    let end = required_end.min(bars.len().checked_sub(1)?);
    if end <= index {
        return None;
    }
    let origin = bars[index].close;
    let upper = origin * (1.0 + volatility * parameters.upper_barrier_volatility_multiple);
    let lower = origin * (1.0 - volatility * parameters.lower_barrier_volatility_multiple);
    for (offset, bar) in bars[index + 1..=end].iter().enumerate() {
        let up = bar.high >= upper;
        let down = bar.low <= lower;
        match (up, down) {
            (true, false) => {
                return Some(OutcomeLabelState::Resolved(Direction::Up, offset + 1));
            }
            (false, true) => {
                return Some(OutcomeLabelState::Resolved(Direction::Down, offset + 1));
            }
            (true, true) => {
                return Some(OutcomeLabelState::Resolved(Direction::Range, offset + 1));
            }
            _ => {}
        }
    }
    Some(if end == required_end {
        OutcomeLabelState::Resolved(Direction::Range, parameters.maximum_horizon_bars)
    } else {
        OutcomeLabelState::RightCensored {
            observed_future_bars: end - index,
        }
    })
}

fn fit_normalization(
    names: &[String],
    rows: &[Candidate],
) -> Result<FeatureNormalization, CalibrationError> {
    let dimension = names.len();
    if dimension == 0 {
        return Err(CalibrationError::InsufficientData);
    }
    let mut medians = Vec::with_capacity(dimension);
    let mut scales = Vec::with_capacity(dimension);
    let mut effective_dimension_mask = Vec::with_capacity(dimension);
    for axis in 0..dimension {
        let mut values: Vec<f64> = rows
            .iter()
            .filter_map(|row| row.raw.get(axis).copied().flatten())
            .collect();
        let center = median(&mut values).unwrap_or(0.0);
        let mut deviations: Vec<f64> = values.iter().map(|value| (value - center).abs()).collect();
        let mad = median(&mut deviations).unwrap_or(0.0);
        let range = values
            .last()
            .zip(values.first())
            .map(|(maximum, minimum)| maximum - minimum)
            .unwrap_or(0.0);
        let variable = range > 0.0;
        let scale = if mad > 0.0 {
            mad
        } else if variable {
            range / 2.0
        } else {
            1.0
        };
        medians.push(center);
        scales.push(scale);
        effective_dimension_mask.push(variable);
    }
    let effective_dimension_count = effective_dimension_mask
        .iter()
        .filter(|included| **included)
        .count();
    if effective_dimension_count == 0 {
        return Err(CalibrationError::InsufficientData);
    }
    Ok(FeatureNormalization {
        names: names.to_vec(),
        median: medians,
        scale: scales,
        effective_dimension_mask,
        effective_dimension_count,
        fitted_sample_count: rows.len(),
    })
}

fn normalized_sample(
    row: &Candidate,
    normalization: &FeatureNormalization,
) -> Result<CalibratedSample, CalibrationError> {
    let vector = row
        .raw
        .iter()
        .enumerate()
        .map(|(axis, value)| match value {
            Some(value) => Ok((value - normalization.median[axis]) / normalization.scale[axis]),
            None => Ok(0.0),
        })
        .collect::<Result<Vec<_>, CalibrationError>>()?;
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(CalibrationError::NonFinite);
    }
    Ok(CalibratedSample {
        timestamp_ns: row.timestamp_ns,
        vector,
        availability_mask: row
            .mask
            .iter()
            .zip(&normalization.effective_dimension_mask)
            .map(|(available, effective)| *available && *effective)
            .collect(),
        direction: row.direction,
        first_passage_bars: row.first_passage_bars,
    })
}

fn normalize_vector(
    vector: &StructuralVector,
    normalization: &FeatureNormalization,
) -> Result<CalibratedSample, CalibrationError> {
    let values = vector
        .values
        .iter()
        .enumerate()
        .map(|(axis, value)| match value {
            Some(value) => Ok((value - normalization.median[axis]) / normalization.scale[axis]),
            None => Ok(0.0),
        })
        .collect::<Result<Vec<_>, CalibrationError>>()?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CalibrationError::NonFinite);
    }
    Ok(CalibratedSample {
        timestamp_ns: 0,
        vector: values,
        availability_mask: vector
            .availability_mask
            .iter()
            .zip(&normalization.effective_dimension_mask)
            .map(|(available, effective)| *available && *effective)
            .collect(),
        direction: Direction::Unresolved,
        first_passage_bars: 0,
    })
}

fn kth_distance(
    query: &CalibratedSample,
    samples: &[CalibratedSample],
    count: usize,
) -> Option<f64> {
    let mut distances: Vec<f64> = samples
        .iter()
        .filter(|sample| sample.availability_mask == query.availability_mask)
        .map(|sample| distance(query, sample))
        .collect();
    distances.sort_by(f64::total_cmp);
    distances
        .get(count.min(distances.len()).checked_sub(1)?)
        .copied()
}

fn vote(
    query: &CalibratedSample,
    samples: &[CalibratedSample],
    parameters: &NeighborParameters,
) -> Option<VoteResult> {
    evaluate_vote(query, samples, parameters).map(|evaluation| evaluation.result)
}

fn evaluate_vote<'a>(
    query: &CalibratedSample,
    samples: &'a [CalibratedSample],
    parameters: &NeighborParameters,
) -> Option<VoteEvaluation<'a>> {
    let mut neighbors = admissible_neighbors(query, samples, parameters);
    neighbors.truncate(parameters.neighbor_count);
    if neighbors.len() < parameters.minimum_support {
        return None;
    }
    let distance_floor = f64::EPSILON.sqrt();
    let mut weights = [0.0_f64; 3];
    let mut counts = [0_usize; 3];
    for neighbor in &mut neighbors {
        neighbor.weight = 1.0
            / neighbor
                .breakdown
                .distance
                .max(distance_floor)
                .powf(parameters.distance_power);
        let class = direction_index(neighbor.sample.direction);
        weights[class] += neighbor.weight;
        counts[class] += 1;
    }
    let probabilities = probability_basis_points(weights);
    let ranked = [
        (probabilities.up, Direction::Up),
        (probabilities.range, Direction::Range),
        (probabilities.down, Direction::Down),
    ];
    let mut ranked = ranked.to_vec();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(direction_order(left.1).cmp(&direction_order(right.1)))
    });
    let winning = ranked[0].1;
    let edge_bp = ranked[0].0.saturating_sub(ranked[1].0);
    let passages = neighbors
        .iter()
        .filter(|neighbor| neighbor.sample.direction == winning)
        .map(|neighbor| (neighbor.weight, neighbor.sample.first_passage_bars))
        .collect();
    Some(VoteEvaluation {
        result: VoteResult {
            probabilities,
            support: neighbors.len(),
            winning,
            edge_bp,
            passages,
        },
        neighbors,
        weighted_mass: weights,
        unweighted_count: counts,
    })
}

fn admissible_neighbors<'a>(
    query: &CalibratedSample,
    samples: &'a [CalibratedSample],
    parameters: &NeighborParameters,
) -> Vec<EvaluatedNeighbor<'a>> {
    let mut neighbors: Vec<EvaluatedNeighbor<'a>> = samples
        .iter()
        .filter(|sample| sample.availability_mask == query.availability_mask)
        .map(|sample| EvaluatedNeighbor {
            sample,
            breakdown: distance_breakdown(query, sample),
            weight: 0.0,
        })
        .filter(|neighbor| neighbor.breakdown.distance <= parameters.maximum_distance)
        .collect();
    neighbors.sort_by(|left, right| {
        left.breakdown
            .distance
            .total_cmp(&right.breakdown.distance)
            .then_with(|| left.sample.timestamp_ns.cmp(&right.sample.timestamp_ns))
    });
    neighbors
}

fn score_power(
    held_out: &[CalibratedSample],
    samples: &[CalibratedSample],
    neighbor_count: usize,
    minimum_support: usize,
    maximum_distance: f64,
    distance_power: f64,
) -> (usize, usize, Vec<u16>, Vec<ScoredCase>) {
    let parameters = score_parameters(
        neighbor_count,
        minimum_support,
        maximum_distance,
        distance_power,
    );
    let mut correct = 0;
    let mut evaluated = 0;
    let mut correct_edges = Vec::new();
    let mut cases = Vec::new();
    for row in held_out {
        if let Some(result) = vote(row, samples, &parameters) {
            let is_correct = result.winning == row.direction;
            evaluated += 1;
            correct += usize::from(is_correct);
            if is_correct {
                correct_edges.push(result.edge_bp);
            }
            cases.push(ScoredCase {
                actual: row.direction,
                predicted: result.winning,
                probabilities: result.probabilities,
                correct: is_correct,
            });
        }
    }
    (correct, evaluated, correct_edges, cases)
}

fn score_parameters(
    neighbor_count: usize,
    minimum_support: usize,
    maximum_distance: f64,
    distance_power: f64,
) -> NeighborParameters {
    NeighborParameters {
        neighbor_count,
        minimum_support,
        maximum_distance,
        distance_power,
    }
}

fn direction_reliability(results: &[ScoredCase]) -> BTreeMap<String, u16> {
    let mut output = BTreeMap::new();
    for direction in [Direction::Up, Direction::Range, Direction::Down] {
        let selected: Vec<bool> = results
            .iter()
            .filter(|case| case.predicted == direction)
            .map(|case| case.correct)
            .collect();
        if !selected.is_empty() {
            output.insert(
                direction_key(direction).to_owned(),
                ratio_bp(
                    selected.iter().filter(|value| **value).count(),
                    selected.len(),
                ),
            );
        }
    }
    output
}

fn direction_reliability_lower_bounds(results: &[ScoredCase]) -> BTreeMap<String, u16> {
    let mut output = BTreeMap::new();
    for direction in [Direction::Up, Direction::Range, Direction::Down] {
        let selected: Vec<&ScoredCase> = results
            .iter()
            .filter(|case| case.predicted == direction)
            .collect();
        if !selected.is_empty() {
            output.insert(
                direction_key(direction).to_owned(),
                wilson_lower_bp(
                    selected.iter().filter(|case| case.correct).count(),
                    selected.len(),
                ),
            );
        }
    }
    output
}

fn actual_direction_recall(results: &[ScoredCase]) -> BTreeMap<String, u16> {
    let mut output = BTreeMap::new();
    for direction in [Direction::Up, Direction::Range, Direction::Down] {
        let selected: Vec<&ScoredCase> = results
            .iter()
            .filter(|case| case.actual == direction)
            .collect();
        if !selected.is_empty() {
            output.insert(
                direction_key(direction).to_owned(),
                ratio_bp(
                    selected.iter().filter(|case| case.correct).count(),
                    selected.len(),
                ),
            );
        }
    }
    output
}

fn balanced_accuracy_bp(results: &[ScoredCase]) -> u16 {
    let recall = actual_direction_recall(results);
    if recall.is_empty() {
        return 0;
    }
    (recall.values().map(|value| u32::from(*value)).sum::<u32>() / recall.len() as u32) as u16
}

fn multiclass_brier_score(results: &[ScoredCase]) -> f64 {
    if results.is_empty() {
        return 0.0;
    }
    results
        .iter()
        .map(|case| {
            let probabilities = [
                f64::from(case.probabilities.up) / 10_000.0,
                f64::from(case.probabilities.range) / 10_000.0,
                f64::from(case.probabilities.down) / 10_000.0,
            ];
            probabilities
                .iter()
                .enumerate()
                .map(|(axis, probability)| {
                    let expected = usize::from(axis == direction_index(case.actual)) as f64;
                    (probability - expected).powi(2)
                })
                .sum::<f64>()
        })
        .sum::<f64>()
        / results.len() as f64
}

fn climatology_brier_score(results: &[ScoredCase], library: &[CalibratedSample]) -> f64 {
    if results.is_empty() || library.is_empty() {
        return 0.0;
    }
    let probabilities = [Direction::Up, Direction::Range, Direction::Down].map(|direction| {
        library
            .iter()
            .filter(|sample| sample.direction == direction)
            .count() as f64
            / library.len() as f64
    });
    results
        .iter()
        .map(|case| {
            probabilities
                .iter()
                .enumerate()
                .map(|(axis, probability)| {
                    let expected = usize::from(axis == direction_index(case.actual)) as f64;
                    (probability - expected).powi(2)
                })
                .sum::<f64>()
        })
        .sum::<f64>()
        / results.len() as f64
}

fn direction_support(results: &[ScoredCase], actual: bool) -> BTreeMap<String, usize> {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .map(|direction| {
            let count = results
                .iter()
                .filter(|case| {
                    if actual {
                        case.actual == direction
                    } else {
                        case.predicted == direction
                    }
                })
                .count();
            (direction_key(direction).to_owned(), count)
        })
        .collect()
}

fn confusion_matrix(results: &[ScoredCase]) -> BTreeMap<String, BTreeMap<String, usize>> {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .map(|actual| {
            let row = [Direction::Up, Direction::Range, Direction::Down]
                .into_iter()
                .map(|predicted| {
                    let count = results
                        .iter()
                        .filter(|case| case.actual == actual && case.predicted == predicted)
                        .count();
                    (direction_key(predicted).to_owned(), count)
                })
                .collect();
            (direction_key(actual).to_owned(), row)
        })
        .collect()
}

fn label_counts(rows: &[Candidate]) -> BTreeMap<String, usize> {
    [Direction::Up, Direction::Range, Direction::Down]
        .into_iter()
        .map(|direction| {
            (
                direction_key(direction).to_owned(),
                rows.iter().filter(|row| row.direction == direction).count(),
            )
        })
        .collect()
}

fn calibration_diagnostics(
    training: &[Candidate],
    validation: &[Candidate],
    test: &[Candidate],
    normalization: &FeatureNormalization,
    training_frames: &[StructuralFrame],
) -> CalibrationDiagnostics {
    let features = normalization
        .names
        .iter()
        .enumerate()
        .map(|(axis, name)| {
            let available_samples = training
                .iter()
                .filter(|row| row.mask.get(axis).copied().unwrap_or(false))
                .count();
            FeatureDiagnostic {
                name: name.clone(),
                available_samples,
                availability_bp: ratio_bp(available_samples, training.len()),
                empirically_variable: normalization.effective_dimension_mask[axis],
                included_in_distance: normalization.effective_dimension_mask[axis],
            }
        })
        .collect();
    CalibrationDiagnostics {
        train_samples: training.len(),
        validation_samples: validation.len(),
        evaluation_tail_samples: test.len(),
        train_label_counts: label_counts(training),
        validation_label_counts: label_counts(validation),
        test_label_counts: label_counts(test),
        total_vector_dimensions: normalization.names.len(),
        effective_vector_dimensions: normalization.effective_dimension_count,
        features,
        upper_lower_barrier_ratio: 1.0,
        d_o_transport_status_counts: training_frames.iter().fold(
            BTreeMap::new(),
            |mut counts, frame| {
                *counts
                    .entry(frame.d_o.transport_status.clone())
                    .or_insert(0) += 1;
                counts
            },
        ),
        d_o_transport_evaluable_bp: ratio_bp(
            training_frames
                .iter()
                .filter(|frame| frame.d_o.transport_coherence.is_some())
                .count(),
            training_frames.len(),
        ),
        odce_adaptive_organization_available_bp: ratio_bp(
            training_frames
                .iter()
                .filter(|frame| {
                    frame
                        .odce
                        .benefit_vector
                        .get("adaptive_organization_level")
                        .is_some_and(|value| value.value.is_some())
                })
                .count(),
            training_frames.len(),
        ),
        k_mem_strictly_prior_available_bp: ratio_bp(
            training_frames
                .iter()
                .filter(|frame| frame.k_mem.strictly_prior_state.value.is_some())
                .count(),
            training_frames.len(),
        ),
    }
}

fn probability_basis_points(weights: [f64; 3]) -> ProbabilitiesBp {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return ProbabilitiesBp {
            up: 0,
            range: 10_000,
            down: 0,
        };
    }
    let exact = weights.map(|weight| weight / total * 10_000.0);
    let mut base = exact.map(|value| value.floor() as u16);
    let mut remaining = 10_000_u16.saturating_sub(base.iter().sum());
    let mut order = [0usize, 1, 2];
    order.sort_by(|left, right| {
        (exact[*right] - exact[*right].floor())
            .total_cmp(&(exact[*left] - exact[*left].floor()))
            .then(left.cmp(right))
    });
    for axis in order {
        if remaining == 0 {
            break;
        }
        base[axis] += 1;
        remaining -= 1;
    }
    ProbabilitiesBp {
        up: base[0],
        range: base[1],
        down: base[2],
    }
}

fn weighted_horizon(passages: &[(f64, usize)], timeframe: Timeframe) -> Option<Horizon> {
    if passages.is_empty() {
        return None;
    }
    let p25 = weighted_quantile(passages, 0.25)? as u32;
    let median = weighted_quantile(passages, 0.50)? as u32;
    let p75 = weighted_quantile(passages, 0.75)? as u32;
    let seconds = timeframe.nominal_seconds();
    Some(Horizon {
        p25_bars: Some(p25),
        median_bars: Some(median),
        p75_bars: Some(p75),
        p25_seconds: Some(u64::from(p25) * seconds),
        median_seconds: Some(u64::from(median) * seconds),
        p75_seconds: Some(u64::from(p75) * seconds),
    })
}

fn weighted_quantile(values: &[(f64, usize)], quantile: f64) -> Option<usize> {
    let mut ordered = values.to_vec();
    ordered.sort_by_key(|(_, passage)| *passage);
    let total: f64 = ordered.iter().map(|(weight, _)| *weight).sum();
    let target = total * quantile;
    let mut cumulative = 0.0;
    for (weight, passage) in ordered {
        cumulative += weight;
        if cumulative >= target {
            return Some(passage);
        }
    }
    None
}

fn distance(left: &CalibratedSample, right: &CalibratedSample) -> f64 {
    distance_breakdown(left, right).distance
}

fn distance_breakdown(left: &CalibratedSample, right: &CalibratedSample) -> DistanceBreakdown {
    let dimension_mask: Vec<bool> = left
        .availability_mask
        .iter()
        .zip(&right.availability_mask)
        .map(|(left, right)| *left && *right)
        .collect();
    let active = dimension_mask.iter().filter(|value| **value).count();
    if active == 0 {
        return DistanceBreakdown {
            distance: f64::INFINITY,
            dimension_mask,
            active_dimension_count: 0,
            normalized_abs_delta: vec![0.0; left.vector.len()],
            dimension_contribution: vec![0.0; left.vector.len()],
            zero_distance: false,
        };
    }
    let normalized_abs_delta: Vec<f64> = left
        .vector
        .iter()
        .zip(&right.vector)
        .zip(&dimension_mask)
        .map(|((left, right), active)| if *active { (left - right).abs() } else { 0.0 })
        .collect();
    let squared_sum: f64 = normalized_abs_delta.iter().map(|delta| delta * delta).sum();
    let distance = (squared_sum / active as f64).sqrt();
    let zero_distance = squared_sum == 0.0;
    let dimension_contribution = if zero_distance {
        vec![0.0; normalized_abs_delta.len()]
    } else {
        normalized_abs_delta
            .iter()
            .map(|delta| delta * delta / squared_sum)
            .collect()
    };
    DistanceBreakdown {
        distance,
        dimension_mask,
        active_dimension_count: active,
        normalized_abs_delta,
        dimension_contribution,
        zero_distance,
    }
}

fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::Up => 0,
        Direction::Range | Direction::Unresolved => 1,
        Direction::Down => 2,
    }
}

fn direction_order(direction: Direction) -> usize {
    match direction {
        Direction::Up => 0,
        Direction::Range => 1,
        Direction::Down => 2,
        Direction::Unresolved => 3,
    }
}

fn direction_key(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "UP",
        Direction::Range => "RANGE",
        Direction::Down => "DOWN",
        Direction::Unresolved => "UNRESOLVED",
    }
}

fn ratio_bp(numerator: usize, denominator: usize) -> u16 {
    if denominator == 0 {
        0
    } else {
        ((numerator as u64 * 10_000 + denominator as u64 / 2) / denominator as u64) as u16
    }
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// One-sided conservative endpoint of the conventional two-sided 95% Wilson
/// interval. The confidence level is frozen in the profile contract.
fn wilson_lower_bp(successes: usize, trials: usize) -> u16 {
    if trials == 0 {
        return 0;
    }
    let z = 1.959_963_984_540_054_f64;
    let n = trials as f64;
    let p = successes as f64 / n;
    let z2 = z * z;
    let center = p + z2 / (2.0 * n);
    let radius = z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt();
    let lower = ((center - radius) / (1.0 + z2 / n)).clamp(0.0, 1.0);
    (lower * 10_000.0).round() as u16
}

fn integer_sqrt(value: usize) -> usize {
    (value as f64).sqrt().floor() as usize
}

fn integer_log2(value: usize) -> usize {
    usize::BITS as usize - value.max(1).leading_zeros() as usize
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

fn median_u16(values: &mut [u16]) -> Option<u16> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StructuralEngineAdapter;
    use crate::historical::{aggregate_weekly, load_daily_csv};
    use crate::observation::adapt_closed_bars;
    use crate::resolver::{AssetResolver, Resolution};
    use std::fs::File;
    use std::path::PathBuf;

    fn signed_profile() -> ResolutionCalibrationProfile {
        let mut profile = ResolutionCalibrationProfile {
            schema: RESOLUTION_PROFILE_SCHEMA.into(),
            calibration_version: RESOLUTION_CALIBRATION_VERSION.into(),
            profile_id: "test".into(),
            instrument_id: "crypto:test:TEST".into(),
            asset_class: AssetClass::Crypto,
            timeframe: Timeframe::D1,
            scope: CalibrationScope::Instrument,
            engine_version: "test-engine".into(),
            structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
            calibration_start_ns: 1,
            calibration_end_ns: 2,
            normalization: FeatureNormalization {
                names: vec!["x".into()],
                median: vec![0.0],
                scale: vec![1.0],
                effective_dimension_mask: vec![true],
                effective_dimension_count: 1,
                fitted_sample_count: 1,
            },
            outcome_label: OutcomeLabelParameters {
                volatility_lookback_bars: 2,
                upper_barrier_volatility_multiple: 1.0,
                lower_barrier_volatility_multiple: 1.0,
                maximum_horizon_bars: 2,
                up_down_symmetric: true,
                simultaneous_hit_rule: "RANGE".into(),
                no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".into(),
            },
            estimator: NeighborParameters {
                neighbor_count: 1,
                minimum_support: 1,
                maximum_distance: 1.0,
                distance_power: 1.0,
            },
            publication: PublicationPolicy {
                minimum_direction_edge_bp: 1,
                minimum_reliability_bp: 1,
                parameters_selected_on: "PREREGISTERED_PROTOCOL".into(),
                reliability_evaluated_on: "UNTOUCHED_TEMPORAL_TEST".into(),
                test_outcomes_used_for_parameter_selection: false,
                requires_positive_brier_skill: true,
                profile_eligible_for_publication: true,
                preregistered_protocol_sha256: Some(CalibrationProtocol::frozen().sha256()),
            },
            reliability: HeldOutReliability {
                correct: 1,
                evaluated: 1,
                reliability_bp: 10_000,
                reliability_lower_bound_bp: 2_065,
                confidence_level_bp: 9_500,
                balanced_accuracy_bp: 10_000,
                multiclass_brier_score: 0.0,
                climatology_brier_score: 1.0,
                brier_skill_score: 1.0,
                by_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                by_direction_lower_bound_bp: BTreeMap::from([("UP".into(), 2_065)]),
                by_actual_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                actual_support: BTreeMap::from([("UP".into(), 1)]),
                predicted_support: BTreeMap::from([("UP".into(), 1)]),
                confusion_matrix: BTreeMap::from([(
                    "UP".into(),
                    BTreeMap::from([("UP".into(), 1)]),
                )]),
                temporal_split_timestamp_ns: 2,
                untouched_test: true,
                evidence_status: "PREREGISTERED_UNTOUCHED_TEST".into(),
            },
            diagnostics: CalibrationDiagnostics {
                train_samples: 1,
                validation_samples: 1,
                evaluation_tail_samples: 1,
                train_label_counts: BTreeMap::from([("UP".into(), 1)]),
                validation_label_counts: BTreeMap::from([("UP".into(), 1)]),
                test_label_counts: BTreeMap::from([("UP".into(), 1)]),
                total_vector_dimensions: 1,
                effective_vector_dimensions: 1,
                features: vec![FeatureDiagnostic {
                    name: "x".into(),
                    available_samples: 1,
                    availability_bp: 10_000,
                    empirically_variable: true,
                    included_in_distance: true,
                }],
                upper_lower_barrier_ratio: 1.0,
                d_o_transport_status_counts: BTreeMap::from([("COHERENT".into(), 1)]),
                d_o_transport_evaluable_bp: 10_000,
                odce_adaptive_organization_available_bp: 10_000,
                k_mem_strictly_prior_available_bp: 10_000,
            },
            samples: vec![CalibratedSample {
                timestamp_ns: 1,
                vector: vec![0.0],
                availability_mask: vec![true],
                direction: Direction::Up,
                first_passage_bars: 2,
            }],
            prefix_causality_verified: true,
            runtime_recalibration: false,
            profile_sha256: None,
        };
        profile.profile_sha256 = Some(canonical::sha256(&profile).unwrap());
        profile
    }

    fn current_btc_diagnostic(
        generation_timestamp_unix_seconds: u64,
    ) -> (ResolutionCalibrationProfile, NeighborAnatomyArtifacts) {
        let (instrument_id, bars, frames, profile) = current_btc_inputs();
        let artifacts = build_neighbor_anatomy_artifacts(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp_unix_seconds,
        )
        .unwrap();
        (profile, artifacts)
    }

    fn current_btc_inputs() -> (
        String,
        Vec<MarketObservation>,
        Vec<StructuralFrame>,
        ResolutionCalibrationProfile,
    ) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let bars = load_daily_csv(
            root.join("data/corpus/btc_calib.csv"),
            &instrument,
            "supplied_corpus",
        )
        .unwrap();
        let observations = adapt_closed_bars(&bars).unwrap();
        let frames = StructuralEngineAdapter::default()
            .replay_frames(&observations)
            .unwrap();
        let profile: ResolutionCalibrationProfile = serde_json::from_reader(
            File::open(root.join("calibration/profiles/crypto_binance_BTCUSDT_D1.resolution.json"))
                .unwrap(),
        )
        .unwrap();
        (instrument.instrument_id, bars, frames, profile)
    }

    fn legacy_distance_reference(left: &CalibratedSample, right: &CalibratedSample) -> f64 {
        let mut sum = 0.0;
        let mut active = 0_usize;
        for (((left, right), left_available), right_available) in left
            .vector
            .iter()
            .zip(&right.vector)
            .zip(&left.availability_mask)
            .zip(&right.availability_mask)
        {
            if *left_available && *right_available {
                let delta = left - right;
                sum += delta * delta;
                active += 1;
            }
        }
        if active == 0 {
            f64::INFINITY
        } else {
            (sum / active as f64).sqrt()
        }
    }

    struct LegacyVoteReference {
        result: VoteResult,
        mass: [f64; 3],
        counts: [usize; 3],
        ordered: Vec<(f64, f64, i64)>,
    }

    fn legacy_vote_reference(
        query: &CalibratedSample,
        samples: &[CalibratedSample],
        parameters: &NeighborParameters,
    ) -> Option<LegacyVoteReference> {
        let mut neighbors: Vec<(f64, &CalibratedSample)> = samples
            .iter()
            .filter(|sample| sample.availability_mask == query.availability_mask)
            .map(|sample| (legacy_distance_reference(query, sample), sample))
            .filter(|(distance, _)| *distance <= parameters.maximum_distance)
            .collect();
        neighbors.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.timestamp_ns.cmp(&right.1.timestamp_ns))
        });
        neighbors.truncate(parameters.neighbor_count);
        if neighbors.len() < parameters.minimum_support {
            return None;
        }
        let distance_floor = f64::EPSILON.sqrt();
        let mut weights = [0.0_f64; 3];
        let mut counts = [0_usize; 3];
        let mut ordered = Vec::with_capacity(neighbors.len());
        for (neighbor_distance, sample) in &neighbors {
            let weight = 1.0
                / neighbor_distance
                    .max(distance_floor)
                    .powf(parameters.distance_power);
            let class = direction_index(sample.direction);
            weights[class] += weight;
            counts[class] += 1;
            ordered.push((*neighbor_distance, weight, sample.timestamp_ns));
        }
        let probabilities = probability_basis_points(weights);
        let mut ranked = [
            (probabilities.up, Direction::Up),
            (probabilities.range, Direction::Range),
            (probabilities.down, Direction::Down),
        ];
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then(direction_order(left.1).cmp(&direction_order(right.1)))
        });
        let winning = ranked[0].1;
        let passages = neighbors
            .iter()
            .filter(|(_, sample)| sample.direction == winning)
            .map(|(neighbor_distance, sample)| {
                (
                    1.0 / neighbor_distance
                        .max(distance_floor)
                        .powf(parameters.distance_power),
                    sample.first_passage_bars,
                )
            })
            .collect();
        Some(LegacyVoteReference {
            result: VoteResult {
                probabilities,
                support: neighbors.len(),
                winning,
                edge_bp: ranked[0].0.saturating_sub(ranked[1].0),
                passages,
            },
            mass: weights,
            counts,
            ordered,
        })
    }

    fn assert_approximately_equal(left: f64, right: f64) {
        let tolerance = 1.0e-12_f64.max(1.0e-12 * left.abs().max(right.abs()));
        assert!(
            (left - right).abs() <= tolerance,
            "{left} differs from {right} by more than {tolerance}"
        );
    }

    fn synthetic_label_bar(
        template: &MarketObservation,
        offset: usize,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
    ) -> MarketObservation {
        let mut bar = template.clone();
        let shift = i64::try_from(offset).unwrap() * 86_400_000_000_000_i64;
        bar.open_time_ns = template.open_time_ns + shift;
        bar.close_time_ns = template.close_time_ns + shift;
        bar.open = open;
        bar.high = high;
        bar.low = low;
        bar.close = close;
        bar
    }

    #[test]
    fn incomplete_no_hit_is_censored_but_observed_first_passage_is_labelable() {
        let (_, fixture, _, _) = current_btc_inputs();
        let template = &fixture[0];
        let parameters = OutcomeLabelParameters {
            volatility_lookback_bars: 2,
            upper_barrier_volatility_multiple: 1.0,
            lower_barrier_volatility_multiple: 1.0,
            maximum_horizon_bars: 2,
            up_down_symmetric: true,
            simultaneous_hit_rule: "RANGE".into(),
            no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".into(),
        };
        let origin = synthetic_label_bar(template, 0, 100.0, 100.0, 100.0, 100.0);
        let no_hit = synthetic_label_bar(template, 1, 100.0, 105.0, 95.0, 100.0);
        let full_no_hit = synthetic_label_bar(template, 2, 100.0, 105.0, 95.0, 100.0);

        assert_eq!(
            label_outcome(&[origin.clone(), no_hit.clone()], 0, 0.1, &parameters),
            None
        );
        assert_eq!(
            label_outcome(
                &[origin.clone(), no_hit.clone(), full_no_hit],
                0,
                0.1,
                &parameters,
            ),
            Some((Direction::Range, 2))
        );

        let up_hit = synthetic_label_bar(template, 1, 100.0, 111.0, 95.0, 110.0);
        assert_eq!(
            label_outcome(&[origin.clone(), up_hit], 0, 0.1, &parameters),
            Some((Direction::Up, 1))
        );
        let simultaneous = synthetic_label_bar(template, 1, 100.0, 111.0, 89.0, 100.0);
        assert_eq!(
            label_outcome(&[origin, simultaneous], 0, 0.1, &parameters),
            Some((Direction::Range, 1))
        );
    }

    #[test]
    fn extended_label_source_does_not_extend_audit_query_cutoff() {
        let (_, mut bars, feature_frames, profile) = current_btc_inputs();
        let base_queries = development_audit_queries(&bars, &feature_frames, &profile).unwrap();
        let mut prior = bars.last().unwrap().clone();
        for offset in 1..=profile.outcome_label.maximum_horizon_bars {
            let next = synthetic_label_bar(
                &prior,
                1,
                prior.close,
                prior.close,
                prior.close,
                prior.close,
            );
            bars.push(next.clone());
            prior = next;
            assert_eq!(offset, bars.len() - 800);
        }
        let queries = development_audit_queries(&bars, &feature_frames, &profile).unwrap();

        assert_eq!(queries.len(), 22);
        assert!(queries
            .iter()
            .all(|query| query.timestamp_ns <= profile.calibration_end_ns));
        for base in base_queries {
            let extended = queries
                .iter()
                .find(|query| query.timestamp_ns == base.timestamp_ns)
                .unwrap();
            assert_eq!(base.vector, extended.vector);
            assert_eq!(base.availability_mask, extended.availability_mask);
            assert_eq!(
                vote(&base, &profile.samples, &profile.estimator),
                vote(extended, &profile.samples, &profile.estimator)
            );
        }
    }

    #[test]
    fn right_censoring_audit_excludes_incomplete_targets_from_scoring() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, bars, frames, profile) = current_btc_inputs();
        let audit = build_right_censoring_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_right_censoring_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();

        assert_eq!(audit, repeated);
        assert_eq!(audit.audit_candidate_observations, 22);
        assert_eq!(audit.labelable_observations, 20);
        assert_eq!(audit.right_censored_observations, 2);
        assert_eq!(audit.runtime_resolved_observations, 18);
        assert_eq!(audit.runtime_support_unresolved_observations, 2);
        assert_eq!(audit.resolved_correct, 11);
        assert_eq!(
            audit
                .right_censored_queries
                .iter()
                .map(|query| (query.query_timestamp_ns, query.observed_future_bars))
                .collect::<Vec<_>>(),
            vec![
                (1_770_768_000_000_000_000, 8),
                (1_770_854_400_000_000_000, 7),
            ]
        );
        assert!(audit.resolved_multiclass_brier_score.is_finite());
        assert!(audit.resolved_climatology_brier_score.is_finite());
        assert!(audit.resolved_brier_skill_score.is_finite());
    }

    #[test]
    fn basis_points_are_exact_and_deterministic() {
        let probabilities = probability_basis_points([1.0, 1.0, 1.0]);
        assert_eq!(
            u32::from(probabilities.up)
                + u32::from(probabilities.range)
                + u32::from(probabilities.down),
            10_000
        );
        assert_eq!(probabilities, probability_basis_points([1.0, 1.0, 1.0]));
    }

    #[test]
    fn availability_masks_must_match() {
        let query = CalibratedSample {
            timestamp_ns: 0,
            vector: vec![0.0, 0.0],
            availability_mask: vec![true, false],
            direction: Direction::Unresolved,
            first_passage_bars: 0,
        };
        let sample = CalibratedSample {
            timestamp_ns: 1,
            vector: vec![0.0, 0.0],
            availability_mask: vec![true, true],
            direction: Direction::Up,
            first_passage_bars: 1,
        };
        assert!(vote(
            &query,
            &[sample],
            &NeighborParameters {
                neighbor_count: 1,
                minimum_support: 1,
                maximum_distance: 1.0,
                distance_power: 1.0,
            }
        )
        .is_none());
    }

    #[test]
    fn modified_profile_is_rejected_by_custody_hash() {
        let mut profile = signed_profile();
        match validate_profile(&profile) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Initial validation error: {:?}", e);
                panic!("Initial validation failed: {:?}", e);
            }
        }
        // Now modify to trigger hash mismatch
        profile.estimator.maximum_distance = 2.0;
        match validate_profile(&profile) {
            Ok(()) => panic!("Expected error after modification"),
            Err(e) => {
                eprintln!("Modified validation error: {:?}", e);
                assert!(
                    matches!(e, CalibrationError::InvalidProfile(reason) if reason == "profile hash mismatch")
                );
            }
        }
    }

    #[test]
    fn runtime_resolution_is_deterministic_and_uses_profile_only() {
        let profile = signed_profile();
        let mut vector = StructuralVector {
            version: STRUCTURAL_VECTOR_VERSION.into(),
            names: vec!["x".into()],
            values: vec![Some(0.0)],
            availability_mask: vec![true],
            vector_sha256: "not-used-by-resolver".into(),
        };
        let left = resolve_direction(&vector, &profile).unwrap();
        let right = resolve_direction(&vector, &profile).unwrap();
        assert_eq!(left, right);
        assert_eq!(left.direction, Direction::Up);
        assert_eq!(left.probabilities_bp.unwrap().up, 10_000);
        assert_eq!(left.horizon.unwrap().median_bars, Some(2));
        vector.availability_mask[0] = false;
        vector.values[0] = None;
        assert_eq!(
            resolve_direction(&vector, &profile).unwrap().direction,
            Direction::Unresolved
        );
    }

    #[test]
    fn neighbor_anatomy_is_observational_complete_and_deterministic() {
        let generation_timestamp = 1_800_000_000;
        let (profile, artifacts) = current_btc_diagnostic(generation_timestamp);
        let (_, repeated) = current_btc_diagnostic(generation_timestamp);
        assert_eq!(artifacts, repeated);

        assert_eq!(artifacts.audit_tail.len(), 18);
        assert_eq!(artifacts.summary.audit_points, 18);
        assert_eq!(
            artifacts.summary.actual_direction_counts,
            DiagnosticClassCount {
                up: 1,
                down: 17,
                range: 0,
            }
        );
        assert_eq!(
            artifacts.summary.predicted_direction_counts,
            DiagnosticClassCount {
                up: 8,
                down: 10,
                range: 0,
            }
        );
        assert_eq!(artifacts.summary.actual_range_points, 0);
        assert_eq!(
            artifacts.summary.audit_minimum_support,
            profile.estimator.minimum_support
        );
        assert_eq!(
            artifacts.summary.evaluated_query_neighbor_pairs,
            artifacts
                .audit_tail
                .iter()
                .map(|query| query.neighbors.len())
                .sum::<usize>()
        );
        assert_eq!(
            artifacts
                .summary
                .top_dimension_frequency
                .values()
                .sum::<usize>(),
            artifacts.summary.top_dimension_frequency_eligible_pairs
        );
        assert_eq!(
            artifacts.summary.top_dimension_frequency_eligible_pairs
                + artifacts.summary.zero_distance_neighbor_pairs,
            artifacts.summary.evaluated_query_neighbor_pairs
        );
        assert!(artifacts
            .audit_tail
            .windows(2)
            .all(|rows| rows[0].query_timestamp_ns < rows[1].query_timestamp_ns));
        let audit_parameters = score_parameters(
            profile.estimator.neighbor_count,
            profile.estimator.minimum_support,
            profile.estimator.maximum_distance,
            profile.estimator.distance_power,
        );

        let mut jsonl = String::new();
        for query in &artifacts.audit_tail {
            jsonl.push_str(&serde_json::to_string(query).unwrap());
            jsonl.push('\n');
        }
        assert_eq!(jsonl.lines().count(), artifacts.audit_tail.len());

        for exported in &artifacts.audit_tail {
            let query = CalibratedSample {
                timestamp_ns: exported.query_timestamp_ns,
                vector: exported.query_vector.clone(),
                availability_mask: exported.query_availability_mask.clone(),
                direction: exported.actual_direction,
                first_passage_bars: 0,
            };
            let legacy =
                legacy_vote_reference(&query, &profile.samples, &audit_parameters).unwrap();
            let current = vote(&query, &profile.samples, &audit_parameters).unwrap();
            let evaluation = evaluate_vote(&query, &profile.samples, &audit_parameters).unwrap();

            assert_eq!(legacy.result, current);
            assert_eq!(legacy.result.winning, exported.predicted_direction);
            assert_eq!(legacy.result.support, exported.selected_neighbor_count);
            assert_eq!(legacy.mass, evaluation.weighted_mass);
            assert_eq!(legacy.counts, evaluation.unweighted_count);
            assert_eq!(legacy.ordered.len(), exported.neighbors.len());
            assert_eq!(exported.neighbors.len(), evaluation.neighbors.len());

            for ((legacy_neighbor, observed_neighbor), exported_neighbor) in legacy
                .ordered
                .iter()
                .zip(&evaluation.neighbors)
                .zip(&exported.neighbors)
            {
                assert_eq!(legacy_neighbor.0, observed_neighbor.breakdown.distance);
                assert_eq!(legacy_neighbor.1, observed_neighbor.weight);
                assert_eq!(legacy_neighbor.2, observed_neighbor.sample.timestamp_ns);
                assert_eq!(
                    exported_neighbor.distance,
                    observed_neighbor.breakdown.distance
                );
                assert_eq!(exported_neighbor.weight, observed_neighbor.weight);
                assert_eq!(
                    exported_neighbor.neighbor_timestamp_ns,
                    observed_neighbor.sample.timestamp_ns
                );
                assert_eq!(
                    exported_neighbor.distance_dimension_mask,
                    observed_neighbor.breakdown.dimension_mask
                );
                assert_eq!(
                    exported_neighbor.normalized_abs_delta,
                    observed_neighbor.breakdown.normalized_abs_delta
                );
                assert_eq!(
                    exported_neighbor.dimension_contribution,
                    observed_neighbor.breakdown.dimension_contribution
                );

                let participating = exported_neighbor
                    .distance_dimension_mask
                    .iter()
                    .filter(|active| **active)
                    .count();
                assert_eq!(participating, exported_neighbor.active_dimension_count);
                for axis in 0..exported_neighbor.distance_dimension_mask.len() {
                    if !exported_neighbor.distance_dimension_mask[axis] {
                        assert_eq!(exported_neighbor.normalized_abs_delta[axis], 0.0);
                        assert_eq!(exported_neighbor.dimension_contribution[axis], 0.0);
                    }
                }
                if exported_neighbor.distance > 0.0 {
                    assert_approximately_equal(
                        exported_neighbor.dimension_contribution.iter().sum(),
                        1.0,
                    );
                }
            }

            assert_eq!(exported.weighted_mass, diagnostic_mass(legacy.mass));
            assert_eq!(exported.unweighted_count, diagnostic_count(legacy.counts));
            assert_eq!(
                exported.unweighted_count.up
                    + exported.unweighted_count.down
                    + exported.unweighted_count.range,
                exported.selected_neighbor_count
            );
            if exported.total_weighted_mass > 0.0 {
                assert_approximately_equal(
                    exported.normalized_weighted_mass.up
                        + exported.normalized_weighted_mass.down
                        + exported.normalized_weighted_mass.range,
                    1.0,
                );
            }
            let expected_range_rank = exported
                .neighbors
                .iter()
                .position(|neighbor| neighbor.direction == Direction::Range)
                .map(|index| index + 1);
            assert_eq!(exported.nearest_range_rank, expected_range_rank);
        }
    }

    #[test]
    fn range_distance_geometry_preserves_runtime_semantics_and_ordering() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, bars, frames, profile) = current_btc_inputs();
        let audit = build_range_distance_geometry_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_range_distance_geometry_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);

        let parameters = score_parameters(
            profile.estimator.neighbor_count,
            profile.estimator.minimum_support,
            profile.estimator.maximum_distance,
            profile.estimator.distance_power,
        );
        assert_eq!(parameters, profile.estimator);
        let queries = development_audit_queries(&bars, &frames, &profile).unwrap();
        let scored = score_power(
            &queries,
            &profile.samples,
            profile.estimator.neighbor_count,
            profile.estimator.minimum_support,
            profile.estimator.maximum_distance,
            profile.estimator.distance_power,
        );
        assert_eq!(scored.0, 11);
        assert_eq!(scored.1, 18);

        let parity = &audit.development_audit_runtime_parity;
        assert_eq!(parity.audit_observations, 20);
        assert_eq!(parity.resolved_observations, 18);
        assert_eq!(parity.unresolved_observations, 2);
        assert_eq!(
            parity.resolved_actual_direction_counts,
            DiagnosticClassCount {
                up: 1,
                down: 17,
                range: 0,
            }
        );
        assert_eq!(
            parity.resolved_predicted_direction_counts,
            DiagnosticClassCount {
                up: 8,
                down: 10,
                range: 0,
            }
        );
        assert_eq!(
            parity.unresolved_actual_direction_counts,
            DiagnosticClassCount {
                up: 0,
                down: 1,
                range: 1,
            }
        );
        assert_eq!(
            parity
                .unresolved_queries
                .iter()
                .map(|query| (
                    query.query_timestamp_ns,
                    query.actual_direction,
                    query.selected_neighbor_count,
                ))
                .collect::<Vec<_>>(),
            vec![
                (1_770_336_000_000_000_000, Direction::Range, 7),
                (1_770_681_600_000_000_000, Direction::Down, 1),
            ]
        );

        let current_anatomy = build_neighbor_anatomy_artifacts(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap()
        .audit_tail;
        assert_eq!(current_anatomy.len(), 18);

        let mut resolved = 0_usize;
        let mut unresolved = 0_usize;
        for query in &queries {
            match evaluate_vote(query, &profile.samples, &profile.estimator) {
                Some(evaluation) => {
                    resolved += 1;
                    let exported = current_anatomy
                        .iter()
                        .find(|row| row.query_timestamp_ns == query.timestamp_ns)
                        .unwrap();
                    assert_eq!(evaluation.result.winning, exported.predicted_direction);
                }
                None => {
                    unresolved += 1;
                    assert!(current_anatomy
                        .iter()
                        .all(|row| row.query_timestamp_ns != query.timestamp_ns));
                    let selected =
                        admissible_neighbors(query, &profile.samples, &profile.estimator)
                            .len()
                            .min(profile.estimator.neighbor_count);
                    assert!(selected < profile.estimator.minimum_support);
                    assert!(parity
                        .unresolved_queries
                        .iter()
                        .any(|row| row.query_timestamp_ns == query.timestamp_ns));
                }
            }
        }
        assert_eq!((resolved, unresolved), (18, 2));

        assert_eq!(audit.queries.len(), 1);
        for geometry in &audit.queries {
            let query = queries
                .iter()
                .find(|query| query.timestamp_ns == geometry.query_timestamp_ns)
                .unwrap();
            let admissible = admissible_neighbors(query, &profile.samples, &profile.estimator);
            assert_eq!(
                geometry.candidates_within_maximum_distance,
                admissible.len()
            );
            assert!(admissible.windows(2).all(|pair| {
                pair[0].breakdown.distance < pair[1].breakdown.distance
                    || (pair[0].breakdown.distance == pair[1].breakdown.distance
                        && pair[0].sample.timestamp_ns <= pair[1].sample.timestamp_ns)
            }));

            let top_27 = &geometry.top_k_composition["27"];
            let expected_counts = count_neighbor_directions(
                &admissible[..admissible.len().min(profile.estimator.neighbor_count)],
            );
            assert_eq!(top_27.actual_k_used, expected_counts.iter().sum::<usize>());
            assert_eq!(top_27.up, expected_counts[0]);
            assert_eq!(top_27.down, expected_counts[2]);
            assert_eq!(top_27.range, expected_counts[1]);

            for (direction, nearest) in [
                (Direction::Up, &geometry.nearest_by_class.up),
                (Direction::Down, &geometry.nearest_by_class.down),
                (Direction::Range, &geometry.nearest_by_class.range),
            ] {
                let expected = nearest_class_candidate(&admissible, direction);
                assert_eq!(*nearest, expected);
            }
            for stats in [
                &geometry.class_distance_stats.up,
                &geometry.class_distance_stats.down,
                &geometry.class_distance_stats.range,
            ] {
                if stats.count == 0 {
                    assert!(stats.minimum.is_none());
                    assert!(stats.p10.is_none());
                    assert!(stats.p25.is_none());
                    assert!(stats.median.is_none());
                    assert!(stats.p75.is_none());
                    assert!(stats.maximum.is_none());
                } else {
                    let ordered = [
                        stats.minimum.unwrap(),
                        stats.p10.unwrap(),
                        stats.p25.unwrap(),
                        stats.median.unwrap(),
                        stats.p75.unwrap(),
                        stats.maximum.unwrap(),
                    ];
                    assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]));
                }
            }
        }
    }

    #[test]
    fn compactness_excludes_self_and_future_candidates() {
        let query = CalibratedSample {
            timestamp_ns: 20,
            vector: vec![0.0],
            availability_mask: vec![true],
            direction: Direction::Up,
            first_passage_bars: 1,
        };
        let samples = vec![
            CalibratedSample {
                timestamp_ns: 10,
                vector: vec![2.0],
                availability_mask: vec![true],
                direction: Direction::Up,
                first_passage_bars: 1,
            },
            query.clone(),
            CalibratedSample {
                timestamp_ns: 30,
                vector: vec![0.1],
                availability_mask: vec![true],
                direction: Direction::Up,
                first_passage_bars: 1,
            },
            CalibratedSample {
                timestamp_ns: 15,
                vector: vec![1.0],
                availability_mask: vec![true],
                direction: Direction::Down,
                first_passage_bars: 1,
            },
        ];
        let all_time = compactness_observation(&query, &samples, 10.0, &[5], false);
        let causal = compactness_observation(&query, &samples, 10.0, &[5], true);
        assert_eq!(all_time.nearest_same_class_distance, Some(0.1));
        assert_eq!(causal.nearest_same_class_distance, Some(2.0));
        assert_eq!(causal.nearest_other_class_distance, Some(1.0));
    }

    #[test]
    fn intraclass_compactness_is_complete_bounded_and_deterministic() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, bars, frames, profile) = current_btc_inputs();
        let audit = build_range_intraclass_compactness_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_range_intraclass_compactness_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(audit.labeled_sample_count, 790);
        assert_eq!(
            audit.labeled_class_counts,
            DiagnosticClassCount {
                up: 379,
                down: 316,
                range: 95,
            }
        );
        assert_eq!(audit.leave_one_out_all_time.query_samples, 790);
        assert_eq!(audit.causal_prefix.query_samples, 790);

        for view in [&audit.leave_one_out_all_time, &audit.causal_prefix] {
            for summary in [
                &view.class_compactness.up,
                &view.class_compactness.down,
                &view.class_compactness.range,
            ] {
                for stats in [
                    &summary.nearest_same_class_distance,
                    &summary.nearest_other_class_distance,
                ] {
                    if stats.count > 0 {
                        let ordered = [
                            stats.minimum.unwrap(),
                            stats.p10.unwrap(),
                            stats.p25.unwrap(),
                            stats.median.unwrap(),
                            stats.p75.unwrap(),
                            stats.maximum.unwrap(),
                        ];
                        assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]));
                    }
                }
                for fraction in summary.runtime_admissible_same_class_fraction_by_k.values() {
                    for value in [
                        fraction.minimum,
                        fraction.median,
                        fraction.mean,
                        fraction.maximum,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        assert!((0.0..=1.0).contains(&value));
                    }
                    assert!(fraction.samples_with_full_k <= fraction.evaluable_samples);
                    assert!(fraction
                        .mean_actual_k_used
                        .is_none_or(|mean| mean <= fraction.requested_k as f64));
                }
            }
        }
    }

    #[test]
    fn range_trajectory_anatomy_reproduces_labels_and_is_deterministic() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, bars, frames, profile) = current_btc_inputs();
        let audit = build_range_trajectory_anatomy_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_range_trajectory_anatomy_audit(
            &instrument_id,
            Timeframe::D1,
            &bars,
            &frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(audit.records.len(), 95);
        assert_eq!(audit.aggregate.actual_range_samples, 95);
        assert_eq!(
            audit
                .aggregate
                .label_mechanism_counts
                .values()
                .sum::<usize>(),
            95
        );
        assert!(audit
            .records
            .windows(2)
            .all(|pair| pair[0].query_timestamp_ns < pair[1].query_timestamp_ns));
        for record in &audit.records {
            assert_eq!(record.actual_direction, Direction::Range);
            assert_eq!(record.first_passage_bars, record.observed_label_path_bars);
            assert!((1..=record.configured_horizon_bars).contains(&record.observed_label_path_bars));
            assert!((1..=record.observed_label_path_bars)
                .contains(&record.time_of_maximum_up_excursion_bars));
            assert!((1..=record.observed_label_path_bars)
                .contains(&record.time_of_maximum_down_excursion_bars));
            assert!(record.maximum_up_excursion.is_finite());
            assert!(record.maximum_down_excursion.is_finite());
            assert!(record.upper_excursion_ratio.is_finite());
            assert!(record.lower_excursion_ratio.is_finite());
            assert!(record.realized_volatility.is_finite());
            assert!(record.maximum_up_excursion >= 0.0);
            assert!(record.maximum_down_excursion >= 0.0);
            assert_approximately_equal(
                record.upper_excursion_ratio,
                record.maximum_up_excursion / record.upper_barrier_return,
            );
            assert_approximately_equal(
                record.lower_excursion_ratio,
                record.maximum_down_excursion / record.lower_barrier_return,
            );
            match record.label_mechanism.as_str() {
                "SIMULTANEOUS_BARRIER_HIT" => {
                    assert!(record.upper_excursion_ratio >= 1.0);
                    assert!(record.lower_excursion_ratio >= 1.0);
                }
                "NO_HIT" => {
                    assert!(record.upper_excursion_ratio < 1.0);
                    assert!(record.lower_excursion_ratio < 1.0);
                }
                mechanism => panic!("unexpected RANGE mechanism: {mechanism}"),
            }
        }
        for summary in std::iter::once(&audit.aggregate.all_range)
            .chain(audit.aggregate.by_label_mechanism.values())
        {
            for stats in [
                &summary.upper_excursion_ratio,
                &summary.lower_excursion_ratio,
                &summary.maximum_up_excursion,
                &summary.maximum_down_excursion,
                &summary.terminal_displacement,
                &summary.realized_volatility,
                &summary.direction_reversals,
                &summary.time_of_maximum_up_excursion_bars,
                &summary.time_of_maximum_down_excursion_bars,
            ] {
                if stats.count > 0 {
                    let ordered = [
                        stats.minimum.unwrap(),
                        stats.p10.unwrap(),
                        stats.p25.unwrap(),
                        stats.median.unwrap(),
                        stats.p75.unwrap(),
                        stats.maximum.unwrap(),
                    ];
                    assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]));
                }
            }
        }
    }

    #[test]
    fn dynamics_ablation_uses_common_targets_and_is_deterministic() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, feature_bars, frames, profile) = current_btc_inputs();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let mut label_bars = feature_bars.clone();
        label_bars.extend(
            load_daily_csv(
                root.join("data/development/btc_stooq_label_extension_2026-02-23_2026-03-10.csv"),
                &instrument,
                "label_extension",
            )
            .unwrap(),
        );
        let weekly_bars = aggregate_weekly(&feature_bars).unwrap();
        let weekly_frames = StructuralEngineAdapter::default()
            .replay_frames(&adapt_closed_bars(&weekly_bars).unwrap())
            .unwrap();
        let audit = build_dynamics_ablation_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_dynamics_ablation_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(audit.variants.len(), 4);
        assert_eq!(audit.common_evaluation_observations, 22);
        assert_eq!(audit.variant_ranking.len(), 4);
        let reference: Vec<(i64, Direction)> = audit.variants[0]
            .predictions
            .iter()
            .map(|prediction| (prediction.query_timestamp_ns, prediction.actual_direction))
            .collect();
        for variant in &audit.variants {
            assert_eq!(variant.predictions.len(), 22);
            assert_eq!(
                variant
                    .predictions
                    .iter()
                    .map(|prediction| {
                        (prediction.query_timestamp_ns, prediction.actual_direction)
                    })
                    .collect::<Vec<_>>(),
                reference
            );
            assert_eq!(
                variant.walk_forward_evaluation.resolved
                    + variant.walk_forward_evaluation.unresolved,
                22
            );
            assert!(variant
                .predictions
                .windows(2)
                .all(|pair| pair[0].query_timestamp_ns < pair[1].query_timestamp_ns));
            assert!(variant
                .predictions
                .windows(2)
                .all(|pair| pair[0].causal_library_size <= pair[1].causal_library_size));
        }
    }

    #[test]
    fn conditional_dynamics_is_individual_deterministic_and_keeps_a_unchanged() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, feature_bars, frames, profile) = current_btc_inputs();
        let original_profile = profile.clone();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let mut label_bars = feature_bars.clone();
        label_bars.extend(
            load_daily_csv(
                root.join("data/development/btc_stooq_label_extension_2026-02-23_2026-03-10.csv"),
                &instrument,
                "label_extension",
            )
            .unwrap(),
        );
        let weekly_bars = aggregate_weekly(&feature_bars).unwrap();
        let weekly_frames = StructuralEngineAdapter::default()
            .replay_frames(&adapt_closed_bars(&weekly_bars).unwrap())
            .unwrap();
        let audit = build_dynamics_conditional_information_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_dynamics_conditional_information_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(profile, original_profile);
        assert!(!audit.runtime_or_profile_modified);
        assert_eq!(audit.features.len(), 7);
        assert_eq!(audit.tested_features.len(), 7);
        assert_eq!(
            audit
                .feature_ranking_by_feature_only_brier_delta_vs_raw_a
                .len(),
            7
        );
        for feature in &audit.features {
            assert_eq!(feature.validation_observations, 33);
            assert_eq!(feature.evaluation_observations, 20);
            assert_eq!(feature.evaluation_queries.len(), 20);
            assert_approximately_equal(
                feature.raw_a_metrics.multiclass_brier_score,
                audit.baseline_walk_forward_metrics.multiclass_brier_score,
            );
            for query in &feature.evaluation_queries {
                for probabilities in [
                    query.feature_only_probabilities,
                    query.bounded_feature_only_probabilities,
                    query.intercept_only_probabilities,
                    query.feature_adjusted_probabilities,
                ] {
                    assert!(probabilities.iter().all(|value| *value >= 0.0));
                    assert_approximately_equal(probabilities.iter().sum(), 1.0);
                }
                assert!(
                    query.bounded_standardized_feature_value
                        >= feature.support_geometry.validation_standardized_minimum
                );
                assert!(
                    query.bounded_standardized_feature_value
                        <= feature.support_geometry.validation_standardized_maximum
                );
            }
        }
    }

    #[test]
    fn sequential_acceleration_is_bounded_deterministic_and_post_velocity() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, feature_bars, frames, profile) = current_btc_inputs();
        let original_profile = profile.clone();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let mut label_bars = feature_bars.clone();
        label_bars.extend(
            load_daily_csv(
                root.join("data/development/btc_stooq_label_extension_2026-02-23_2026-03-10.csv"),
                &instrument,
                "label_extension",
            )
            .unwrap(),
        );
        let weekly_bars = aggregate_weekly(&feature_bars).unwrap();
        let weekly_frames = StructuralEngineAdapter::default()
            .replay_frames(&adapt_closed_bars(&weekly_bars).unwrap())
            .unwrap();
        let audit = build_dynamics_sequential_residual_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_dynamics_sequential_residual_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(profile, original_profile);
        assert!(!audit.runtime_or_profile_modified);
        assert_eq!(audit.validation_observations, 33);
        assert_eq!(audit.evaluation_observations, 20);
        assert_eq!(audit.queries.len(), 20);
        assert_approximately_equal(
            audit
                .a_plus_bounded_velocity_plus_bounded_acceleration_metrics
                .brier_delta_vs_intercept_only
                .unwrap(),
            audit
                .a_plus_bounded_velocity_plus_bounded_acceleration_metrics
                .multiclass_brier_score
                - audit.a_plus_bounded_velocity_metrics.multiclass_brier_score,
        );
        for query in &audit.queries {
            assert!(
                query.velocity_standardized_bounded
                    >= audit.velocity_support.validation_standardized_minimum
            );
            assert!(
                query.velocity_standardized_bounded
                    <= audit.velocity_support.validation_standardized_maximum
            );
            assert!(
                query.acceleration_standardized_bounded
                    >= audit.acceleration_support.validation_standardized_minimum
            );
            assert!(
                query.acceleration_standardized_bounded
                    <= audit.acceleration_support.validation_standardized_maximum
            );
            for probabilities in [
                query.a_probabilities,
                query.a_plus_velocity_probabilities,
                query.a_plus_velocity_plus_acceleration_probabilities,
            ] {
                assert!(probabilities.iter().all(|value| *value >= 0.0));
                assert_approximately_equal(probabilities.iter().sum(), 1.0);
            }
        }
    }

    #[test]
    fn frozen_velocity_forward_uses_original_fit_and_later_matured_outcomes() {
        let generation_timestamp = 1_800_000_000;
        let (instrument_id, _, _, profile) = current_btc_inputs();
        let original_profile = profile.clone();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let instrument = match AssetResolver::default().resolve("BTCUSDT") {
            Resolution::Found { instrument } => instrument,
            other => panic!("BTCUSDT resolution failed: {other:?}"),
        };
        let feature_bars = load_daily_csv(
            root.join("data/development/btc_stooq_forward_features_through_2026-07-31.csv"),
            &instrument,
            "forward_features",
        )
        .unwrap();
        let mut label_bars = feature_bars.clone();
        label_bars.extend(
            load_daily_csv(
                root.join("data/development/btc_stooq_forward_labels_2026-08-03_2026-08-14.csv"),
                &instrument,
                "forward_labels",
            )
            .unwrap(),
        );
        let frames = StructuralEngineAdapter::default()
            .replay_frames(&adapt_closed_bars(&feature_bars).unwrap())
            .unwrap();
        let weekly_bars = aggregate_weekly(&feature_bars).unwrap();
        let weekly_frames = StructuralEngineAdapter::default()
            .replay_frames(&adapt_closed_bars(&weekly_bars).unwrap())
            .unwrap();
        let audit = build_dynamics_frozen_velocity_forward_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        let repeated = build_dynamics_frozen_velocity_forward_audit(
            &instrument_id,
            Timeframe::D1,
            &label_bars,
            &frames,
            &weekly_frames,
            &profile,
            generation_timestamp,
        )
        .unwrap();
        assert_eq!(audit, repeated);
        assert_eq!(profile, original_profile);
        assert!(!audit.slope_or_support_refitted_on_forward);
        assert!(!audit.runtime_or_profile_modified);
        assert_eq!(audit.forward_a_metrics.observations, 120);
        assert_eq!(audit.queries.len(), 120);
        assert_eq!(
            audit.forward_a_metrics.resolved + audit.forward_a_metrics.unresolved,
            120
        );
        assert!(audit.frozen_feature_source_end_timestamp_ns > profile.calibration_end_ns);
        assert!(audit
            .queries
            .iter()
            .all(|query| query.query_timestamp_ns > audit.frozen_feature_source_end_timestamp_ns));
        assert_approximately_equal(audit.source_a_metrics.multiclass_brier_score, 0.493559595);
        assert_approximately_equal(
            audit
                .source_a_plus_bounded_velocity_metrics
                .multiclass_brier_score,
            0.44425033240318273,
        );
        for query in audit
            .queries
            .iter()
            .filter(|query| query.velocity_standardized_bounded.is_some())
        {
            let bounded = query.velocity_standardized_bounded.unwrap();
            assert!(
                bounded
                    >= audit
                        .forward_velocity_support
                        .validation_standardized_minimum
            );
            assert!(
                bounded
                    <= audit
                        .forward_velocity_support
                        .validation_standardized_maximum
            );
            assert_approximately_equal(query.corrected_probabilities.unwrap().iter().sum(), 1.0);
        }
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    #[test]
    fn test_frozen_protocol_canonical_serialization() {
        let p1 = CalibrationProtocol::frozen();
        let p2 = CalibrationProtocol::frozen();
        let json1 = p1.canonical_json();
        let json2 = p2.canonical_json();
        assert_eq!(
            json1, json2,
            "identical protocol → identical canonical JSON"
        );
    }

    #[test]
    fn test_frozen_protocol_hash_deterministic() {
        let p = CalibrationProtocol::frozen();
        let h1 = p.sha256();
        let h2 = p.sha256();
        assert_eq!(h1, h2, "identical protocol → identical SHA-256");
    }

    #[test]
    fn test_protocol_parameter_change_changes_hash() {
        let p1 = CalibrationProtocol::frozen();
        let mut p2 = CalibrationProtocol::frozen();
        p2.calibration_procedure
            .neighbor_selection
            .distance_power_selection = "fixed_2_0".into();
        let h1 = p1.sha256();
        let h2 = p2.sha256();
        assert_ne!(h1, h2, "different protocol → different SHA-256");
    }

    #[test]
    fn test_temporal_validation_profile_not_retroactively_promoted() {
        let mut profile = ResolutionCalibrationProfile {
            schema: RESOLUTION_PROFILE_SCHEMA.into(),
            calibration_version: RESOLUTION_CALIBRATION_VERSION.into(),
            profile_id: "test".into(),
            instrument_id: "crypto:test:TEST".into(),
            asset_class: AssetClass::Crypto,
            timeframe: Timeframe::D1,
            scope: CalibrationScope::Instrument,
            engine_version: "test".into(),
            structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
            calibration_start_ns: 1,
            calibration_end_ns: 2,
            normalization: FeatureNormalization {
                names: vec!["x".into()],
                median: vec![0.0],
                scale: vec![1.0],
                effective_dimension_mask: vec![true],
                effective_dimension_count: 1,
                fitted_sample_count: 1,
            },
            outcome_label: OutcomeLabelParameters {
                volatility_lookback_bars: 2,
                upper_barrier_volatility_multiple: 1.0,
                lower_barrier_volatility_multiple: 1.0,
                maximum_horizon_bars: 2,
                up_down_symmetric: true,
                simultaneous_hit_rule: "RANGE".into(),
                no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".into(),
            },
            estimator: NeighborParameters {
                neighbor_count: 1,
                minimum_support: 1,
                maximum_distance: 1.0,
                distance_power: 1.0,
            },
            publication: PublicationPolicy {
                minimum_direction_edge_bp: 1,
                minimum_reliability_bp: 1,
                parameters_selected_on: "TEMPORAL_VALIDATION".into(),
                reliability_evaluated_on: "CONSUMED_DEVELOPMENT_AUDIT".into(),
                test_outcomes_used_for_parameter_selection: false,
                requires_positive_brier_skill: true,
                profile_eligible_for_publication: false,
                preregistered_protocol_sha256: Some(CalibrationProtocol::frozen().sha256()),
            },
            reliability: HeldOutReliability {
                correct: 1,
                evaluated: 1,
                reliability_bp: 10_000,
                reliability_lower_bound_bp: 2_065,
                confidence_level_bp: 9_500,
                balanced_accuracy_bp: 10_000,
                multiclass_brier_score: 0.0,
                climatology_brier_score: 1.0,
                brier_skill_score: 1.0,
                by_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                by_direction_lower_bound_bp: BTreeMap::from([("UP".into(), 2_065)]),
                by_actual_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                actual_support: BTreeMap::from([("UP".into(), 1)]),
                predicted_support: BTreeMap::from([("UP".into(), 1)]),
                confusion_matrix: BTreeMap::from([(
                    "UP".into(),
                    BTreeMap::from([("UP".into(), 1)]),
                )]),
                temporal_split_timestamp_ns: 2,
                untouched_test: false,
                evidence_status: "DEVELOPMENT_AUDIT_CONSUMED".into(),
            },
            diagnostics: CalibrationDiagnostics {
                train_samples: 1,
                validation_samples: 1,
                evaluation_tail_samples: 1,
                train_label_counts: BTreeMap::from([("UP".into(), 1)]),
                validation_label_counts: BTreeMap::from([("UP".into(), 1)]),
                test_label_counts: BTreeMap::from([("UP".into(), 1)]),
                total_vector_dimensions: 1,
                effective_vector_dimensions: 1,
                features: vec![FeatureDiagnostic {
                    name: "x".into(),
                    available_samples: 1,
                    availability_bp: 10_000,
                    empirically_variable: true,
                    included_in_distance: true,
                }],
                upper_lower_barrier_ratio: 1.0,
                d_o_transport_status_counts: BTreeMap::from([("COHERENT".into(), 1)]),
                d_o_transport_evaluable_bp: 10_000,
                odce_adaptive_organization_available_bp: 10_000,
                k_mem_strictly_prior_available_bp: 10_000,
            },
            samples: vec![CalibratedSample {
                timestamp_ns: 1,
                vector: vec![0.0],
                availability_mask: vec![true],
                direction: Direction::Up,
                first_passage_bars: 2,
            }],
            prefix_causality_verified: true,
            runtime_recalibration: false,
            profile_sha256: None,
        };
        profile.profile_sha256 = Some(canonical::sha256(&profile).unwrap());

        let vector = StructuralVector {
            version: STRUCTURAL_VECTOR_VERSION.into(),
            names: vec!["x".into()],
            values: vec![Some(0.0)],
            availability_mask: vec![true],
            vector_sha256: "sha256:test".into(),
        };
        let result = resolve_direction(&vector, &profile);
        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.direction, Direction::Unresolved);
        assert_eq!(
            res.publication_reason,
            "CALIBRATION_EVIDENCE_NOT_PREREGISTERED"
        );
    }

    #[test]
    fn test_preregistered_awaiting_evidence_remains_non_publishable() {
        let mut profile = ResolutionCalibrationProfile {
            schema: RESOLUTION_PROFILE_SCHEMA.into(),
            calibration_version: RESOLUTION_CALIBRATION_VERSION.into(),
            profile_id: "test".into(),
            instrument_id: "crypto:test:TEST".into(),
            asset_class: AssetClass::Crypto,
            timeframe: Timeframe::D1,
            scope: CalibrationScope::Instrument,
            engine_version: "test".into(),
            structural_vector_version: STRUCTURAL_VECTOR_VERSION.into(),
            calibration_start_ns: 1,
            calibration_end_ns: 2,
            normalization: FeatureNormalization {
                names: vec!["x".into()],
                median: vec![0.0],
                scale: vec![1.0],
                effective_dimension_mask: vec![true],
                effective_dimension_count: 1,
                fitted_sample_count: 1,
            },
            outcome_label: OutcomeLabelParameters {
                volatility_lookback_bars: 2,
                upper_barrier_volatility_multiple: 1.0,
                lower_barrier_volatility_multiple: 1.0,
                maximum_horizon_bars: 2,
                up_down_symmetric: true,
                simultaneous_hit_rule: "RANGE".into(),
                no_hit_rule: "RANGE_AT_MAXIMUM_HORIZON".into(),
            },
            estimator: NeighborParameters {
                neighbor_count: 1,
                minimum_support: 1,
                maximum_distance: 1.0,
                distance_power: 1.0,
            },
            publication: PublicationPolicy {
                minimum_direction_edge_bp: 1,
                minimum_reliability_bp: 1,
                parameters_selected_on: "PREREGISTERED_PROTOCOL".into(),
                reliability_evaluated_on: "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE".into(),
                test_outcomes_used_for_parameter_selection: false,
                requires_positive_brier_skill: true,
                profile_eligible_for_publication: false,
                preregistered_protocol_sha256: Some(CalibrationProtocol::frozen().sha256()),
            },
            reliability: HeldOutReliability {
                correct: 0,
                evaluated: 0,
                reliability_bp: 0,
                reliability_lower_bound_bp: 0,
                confidence_level_bp: 9_500,
                balanced_accuracy_bp: 0,
                multiclass_brier_score: 0.0,
                climatology_brier_score: 1.0,
                brier_skill_score: 1.0,
                by_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                by_direction_lower_bound_bp: BTreeMap::from([("UP".into(), 2_065)]),
                by_actual_direction_bp: BTreeMap::from([("UP".into(), 10_000)]),
                actual_support: BTreeMap::from([("UP".into(), 1)]),
                predicted_support: BTreeMap::from([("UP".into(), 1)]),
                confusion_matrix: BTreeMap::from([(
                    "UP".into(),
                    BTreeMap::from([("UP".into(), 1)]),
                )]),
                temporal_split_timestamp_ns: 2,
                untouched_test: false,
                evidence_status: "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE".into(),
            },
            diagnostics: CalibrationDiagnostics {
                train_samples: 1,
                validation_samples: 1,
                evaluation_tail_samples: 1,
                train_label_counts: BTreeMap::from([("UP".into(), 1)]),
                validation_label_counts: BTreeMap::from([("UP".into(), 1)]),
                test_label_counts: BTreeMap::from([("UP".into(), 1)]),
                total_vector_dimensions: 1,
                effective_vector_dimensions: 1,
                features: vec![FeatureDiagnostic {
                    name: "x".into(),
                    available_samples: 1,
                    availability_bp: 10_000,
                    empirically_variable: true,
                    included_in_distance: true,
                }],
                upper_lower_barrier_ratio: 1.0,
                d_o_transport_status_counts: BTreeMap::from([("COHERENT".into(), 1)]),
                d_o_transport_evaluable_bp: 10_000,
                odce_adaptive_organization_available_bp: 10_000,
                k_mem_strictly_prior_available_bp: 10_000,
            },
            samples: vec![CalibratedSample {
                timestamp_ns: 1,
                vector: vec![0.0],
                availability_mask: vec![true],
                direction: Direction::Up,
                first_passage_bars: 2,
            }],
            prefix_causality_verified: true,
            runtime_recalibration: false,
            profile_sha256: None,
        };
        profile.profile_sha256 = Some(canonical::sha256(&profile).unwrap());

        let vector = StructuralVector {
            version: STRUCTURAL_VECTOR_VERSION.into(),
            names: vec!["x".into()],
            values: vec![Some(0.0)],
            availability_mask: vec![true],
            vector_sha256: "sha256:test".into(),
        };
        let result = resolve_direction(&vector, &profile);
        assert!(result.is_ok());
        let res = result.unwrap();
        assert_eq!(res.direction, Direction::Unresolved);
        assert_eq!(
            res.publication_reason,
            "PREREGISTERED_AWAITING_PROSPECTIVE_EVIDENCE"
        );
    }
}
