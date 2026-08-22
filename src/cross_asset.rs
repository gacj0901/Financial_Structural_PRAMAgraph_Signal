//! Cross-Asset Relation Computation
//!
//! This module provides deterministic cross-asset structural relation analysis
//! based on temporally aligned instrument observations. It consumes existing
//! instrument-level structural outputs and computes observed relations.

use crate::{
    engine::ENGINE_VERSION, structural::StructuralFrame, structural::STRUCTURAL_VECTOR_VERSION,
    StructuralSnapshot, TechnicalCounterReading, TechnicalDirectionHead,
    TechnicalStructuralContrast,
};
#[allow(unused_imports)]
use std::collections::BTreeMap;

/// Cross-asset relation computation result
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
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

/// Alignment configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignmentConfig {
    /// Minimum aligned observations required
    pub min_aligned_observations: usize,
    /// Maximum allowed gap in nanoseconds for temporal alignment
    pub max_temporal_gap_ns: i64,
}

impl Default for AlignmentConfig {
    fn default() -> Self {
        Self {
            min_aligned_observations: 30,
            max_temporal_gap_ns: 86_400_000_000_000, // 1 day in nanoseconds
        }
    }
}

/// Compute cross-asset relation between two instruments
///
/// This function takes two instrument's structural frames and computes
/// deterministic structural relation evidence. It returns None if
/// the relation cannot be computed (fail-closed).
pub fn compute_cross_asset_relation(
    _primary_instrument_id: &str,
    primary_frames: &[StructuralFrame],
    reference_instrument_id: &str,
    reference_frames: &[StructuralFrame],
    config: AlignmentConfig,
) -> Option<CrossAssetRelation> {
    // Align frames by timestamp
    let aligned_pairs =
        align_frames_by_timestamp(primary_frames, reference_frames, config.max_temporal_gap_ns);

    if aligned_pairs.len() < config.min_aligned_observations {
        return None;
    }

    // Compute structural vector cosine similarity
    let cosine_similarity = compute_structural_vector_cosine_similarity(&aligned_pairs);

    // Compute component state agreements
    let prama_agreement = compute_state_agreement(
        &aligned_pairs,
        |f| f.prama.valid,
        |f| f.d_o.structural_state.clone(),
    );
    let do_agreement = compute_state_agreement(
        &aligned_pairs,
        |f| f.d_o.causal,
        |f| f.d_o.structural_state.clone(),
    );
    let odce_agreement = compute_state_agreement(
        &aligned_pairs,
        |f| f.odce.causal,
        |f| f.odce.normalization_status.clone(),
    );
    let k_mem_agreement =
        compute_state_agreement(&aligned_pairs, |f| f.k_mem.causal, |f| f.k_mem.mode.clone());

    // Technical direction agreement (requires technical data)
    // Note: TechnicalStructuralContrast is computed per-instrument, not in frames
    // This will be UNAVAILABLE unless we have technical data
    let technical_agreement = "UNAVAILABLE".to_string();
    let counter_agreement = "UNAVAILABLE".to_string();

    // Overall classification
    let classification = classify_relation(
        cosine_similarity,
        &prama_agreement,
        &do_agreement,
        &odce_agreement,
        &k_mem_agreement,
    );

    // Get vector SHA-256 for provenance
    let primary_vector_sha256 = primary_frames
        .last()
        .map(|f| f.vector.vector_sha256.clone())
        .unwrap_or_default();
    let reference_vector_sha256 = reference_frames
        .last()
        .map(|f| f.vector.vector_sha256.clone())
        .unwrap_or_default();

    let overlap_start_ns = aligned_pairs
        .first()
        .map(|(primary, _)| primary.timestamp_ns)
        .unwrap_or(0);
    let overlap_end_ns = aligned_pairs
        .last()
        .map(|(primary, _)| primary.timestamp_ns)
        .unwrap_or(0);

    Some(CrossAssetRelation {
        reference_instrument_id: reference_instrument_id.to_string(),
        overlap_start_ns,
        overlap_end_ns,
        aligned_observation_count: aligned_pairs.len(),
        structural_vector_cosine_similarity: cosine_similarity,
        prama_state_agreement: prama_agreement,
        do_state_agreement: do_agreement,
        odce_state_agreement: odce_agreement,
        k_mem_state_agreement: k_mem_agreement,
        technical_direction_agreement: technical_agreement,
        counter_reading_agreement: counter_agreement,
        relation_classification: classification,
        provenance: CrossAssetProvenance {
            primary_vector_sha256,
            reference_vector_sha256,
            structural_engine_version: ENGINE_VERSION.to_string(),
            structural_vector_version: STRUCTURAL_VECTOR_VERSION.to_string(),
            // The relation is a deterministic function of the aligned prefix;
            // its as-of time is therefore the last aligned observation rather
            // than wall-clock execution time.
            computed_at_ns: overlap_end_ns,
        },
    })
}

/// Align frames by timestamp within a maximum gap
fn align_frames_by_timestamp(
    primary: &[StructuralFrame],
    reference: &[StructuralFrame],
    max_gap_ns: i64,
) -> Vec<(StructuralFrame, StructuralFrame)> {
    let mut aligned = Vec::new();
    let mut primary_idx = 0;
    let mut reference_idx = 0;

    while primary_idx < primary.len() && reference_idx < reference.len() {
        let p_ts = primary[primary_idx].timestamp_ns;
        let r_ts = reference[reference_idx].timestamp_ns;
        let gap = (p_ts - r_ts).abs();

        if gap <= max_gap_ns {
            // Within alignment window - pair them
            aligned.push((
                primary[primary_idx].clone(),
                reference[reference_idx].clone(),
            ));
            primary_idx += 1;
            reference_idx += 1;
        } else if p_ts < r_ts {
            // Primary is behind, advance primary
            primary_idx += 1;
        } else {
            // Reference is behind, advance reference
            reference_idx += 1;
        }
    }

    aligned
}

/// Compute cosine similarity between structural vectors of aligned frames
fn compute_structural_vector_cosine_similarity(
    aligned_pairs: &[(StructuralFrame, StructuralFrame)],
) -> Option<f64> {
    if aligned_pairs.is_empty() {
        return None;
    }

    let mut dot_product: f64 = 0.0;
    let mut norm_a: f64 = 0.0;
    let mut norm_b: f64 = 0.0;
    let mut valid_dimensions = 0;

    for (primary_frame, reference_frame) in aligned_pairs {
        let primary_vec = &primary_frame.vector;
        let reference_vec = &reference_frame.vector;

        // Ensure vectors have same dimensions
        if primary_vec.values.len() != reference_vec.values.len()
            || primary_vec.availability_mask.len() != reference_vec.availability_mask.len()
        {
            continue;
        }

        // Compute on common available dimensions
        let mut frame_dot = 0.0;
        let mut frame_norm_a = 0.0;
        let mut frame_norm_b = 0.0;
        let mut frame_valid = 0;

        for i in 0..primary_vec.values.len() {
            if primary_vec.availability_mask[i] && reference_vec.availability_mask[i] {
                if let (Some(a), Some(b)) = (primary_vec.values[i], reference_vec.values[i]) {
                    if a.is_finite() && b.is_finite() {
                        frame_dot += a * b;
                        frame_norm_a += a * a;
                        frame_norm_b += b * b;
                        frame_valid += 1;
                    }
                }
            }
        }

        if frame_valid > 0 && frame_norm_a > 0.0 && frame_norm_b > 0.0 {
            dot_product += frame_dot;
            norm_a += frame_norm_a;
            norm_b += frame_norm_b;
            valid_dimensions += frame_valid;
        }
    }

    if valid_dimensions == 0 || norm_a == 0.0 || norm_b == 0.0 {
        None
    } else {
        Some(dot_product / (norm_a.sqrt() * norm_b.sqrt()))
    }
}

/// Compute state agreement between aligned frames
fn compute_state_agreement<F, G>(
    aligned_pairs: &[(StructuralFrame, StructuralFrame)],
    validity_check: F,
    state_extractor: G,
) -> String
where
    F: Fn(&StructuralFrame) -> bool,
    G: Fn(&StructuralFrame) -> String,
{
    if aligned_pairs.is_empty() {
        return "UNAVAILABLE".to_string();
    }

    let mut aligned_count = 0;
    let mut diverging_count = 0;

    for (primary, reference) in aligned_pairs {
        if validity_check(primary) && validity_check(reference) {
            let primary_state = state_extractor(primary);
            let reference_state = state_extractor(reference);

            if primary_state == reference_state {
                aligned_count += 1;
            } else {
                diverging_count += 1;
            }
        }
    }

    if aligned_count == 0 && diverging_count == 0 {
        "UNAVAILABLE".to_string()
    } else if aligned_count > diverging_count {
        "ALIGNED".to_string()
    } else {
        "DIVERGING".to_string()
    }
}

/// Classify overall cross-asset relation
fn classify_relation(
    cosine_similarity: Option<f64>,
    prama: &str,
    do_state: &str,
    odce: &str,
    k_mem: &str,
) -> String {
    let structural_aligned = [prama, do_state, odce, k_mem]
        .iter()
        .filter(|s| **s == "ALIGNED")
        .count();

    let structural_diverging = [prama, do_state, odce, k_mem]
        .iter()
        .filter(|s| **s == "DIVERGING")
        .count();

    match (cosine_similarity, structural_aligned, structural_diverging) {
        (Some(sim), 3..=4, 0) if sim >= 0.7 => "STRONG_ALIGNMENT".to_string(),
        (Some(sim), 1..=4, 0) if sim >= 0.4 => "WEAK_ALIGNMENT".to_string(),
        (_, 0, 2..=4) => "DIVERGENCE".to_string(),
        (Some(sim), _, _) if sim >= 0.5 => "WEAK_ALIGNMENT".to_string(),
        _ => "UNAVAILABLE".to_string(),
    }
}

/// Compute cross-asset relation from two instrument's structural snapshots and technical data
///
/// This is the main entry point for cross-asset computation using available
/// instrument-level outputs (snapshots + technical).
#[allow(clippy::too_many_arguments)]
pub fn compute_cross_asset_from_snapshots(
    primary_snapshot: &StructuralSnapshot,
    primary_technical: Option<&TechnicalDirectionHead>,
    primary_counter: Option<&TechnicalCounterReading>,
    _primary_contrast: Option<&TechnicalStructuralContrast>,
    primary_frames: &[StructuralFrame],
    reference_snapshot: &StructuralSnapshot,
    reference_technical: Option<&TechnicalDirectionHead>,
    reference_counter: Option<&TechnicalCounterReading>,
    _reference_contrast: Option<&TechnicalStructuralContrast>,
    reference_frames: &[StructuralFrame],
    config: AlignmentConfig,
) -> Option<CrossAssetRelation> {
    // First try frame-based alignment
    let mut relation = compute_cross_asset_relation(
        &primary_snapshot.instrument_id,
        primary_frames,
        &reference_snapshot.instrument_id,
        reference_frames,
        config,
    )?;

    // Enhance with technical agreement if available
    if let (Some(p_tech), Some(r_tech)) = (primary_technical, reference_technical) {
        relation.technical_direction_agreement = if p_tech.direction == r_tech.direction {
            "SAME".to_string()
        } else {
            "OPPOSITE".to_string()
        };
    }

    if let (Some(p_counter), Some(r_counter)) = (primary_counter, reference_counter) {
        relation.counter_reading_agreement = if p_counter.direction == r_counter.direction {
            "SAME".to_string()
        } else {
            "OPPOSITE".to_string()
        };
    }

    // Reclassify with enhanced data
    relation.relation_classification = classify_relation(
        relation.structural_vector_cosine_similarity,
        &relation.prama_state_agreement,
        &relation.do_state_agreement,
        &relation.odce_state_agreement,
        &relation.k_mem_state_agreement,
    );

    Some(relation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::{DoObservation, KMemState, OdceState, PramaState};
    #[allow(unused_imports)]
    use crate::{AvailabilityStatus, AvailableValue, ComponentSnapshot, Timeframe};
    #[allow(unused_imports)]
    use std::collections::BTreeMap;

    fn make_test_frame(ts: i64, state: &str) -> StructuralFrame {
        StructuralFrame {
            timestamp_ns: ts,
            prama: PramaState {
                delta: 0.1,
                delta_tilde: 0.05,
                e: 0.01,
                xi: 0.02,
                A: 0.3,
                lambda: 0.5,
                theta: 0.1,
                M: 1.0,
                G: 0.8,
                u_lambda: 0.2,
                sigma_op: true,
                valid: true,
                input_index: 0,
                state_index: 0,
            },
            d_o: DoObservation {
                observer: "test".into(),
                observer_version: "test".into(),
                financial_adapter_version: "test".into(),
                index: 0,
                geometry_ready: true,
                transport_status: "test".into(),
                recurrence_status: "test".into(),
                contraction_status: "test".into(),
                mobility_status: None,
                structural_state: state.into(),
                movement: 0.1,
                transport_coherence: Some(0.8),
                operator_prediction_residual: Some(0.01),
                operator_training_support: 10,
                recurrence_persistence: 0.7,
                variation_capacity: Some(0.5),
                variation_contraction: Some(0.1),
                alert_eligible: false,
                transport_deficit: Some(0.0),
                cumulative_transport_deficit: 0.0,
                diagnostics: vec![],
                causal: true,
                external_outcome_used: false,
            },
            odce: OdceState {
                operator: "test".into(),
                operator_version: "test".into(),
                index: 0,
                window_start: 0,
                window_end: 32,
                raw_cost_vector: BTreeMap::new(),
                raw_benefit_vector: BTreeMap::new(),
                cost_vector: BTreeMap::new(),
                benefit_vector: BTreeMap::new(),
                normalization_reference: BTreeMap::new(),
                differential_vector: BTreeMap::new(),
                differential_trend: BTreeMap::new(),
                cumulative_conversion_deficit_exposure: BTreeMap::new(),
                positive_persistence: BTreeMap::new(),
                normalization_status: "test".into(),
                causal: true,
                predictive_model_used: false,
                future_outcome_used: false,
            },
            k_mem: KMemState {
                schema: "test".into(),
                runtime_version: "test".into(),
                mode: "test".into(),
                topology: "test".into(),
                index: 0,
                timescale: 32.0,
                source_channel: "test".into(),
                source_status: AvailabilityStatus::Available,
                strictly_prior_state: AvailableValue::unavailable(),
                state_after_update: AvailableValue::unavailable(),
                update_applied: false,
                causal: true,
                state_sha256: "sha256:test".into(),
            },
            vector: crate::structural::StructuralVector {
                version: crate::STRUCTURAL_VECTOR_VERSION.into(),
                names: vec!["dim1".into(), "dim2".into()],
                values: vec![Some(1.0), Some(2.0)],
                availability_mask: vec![true, true],
                vector_sha256: format!("sha256:frame_{ts}"),
            },
        }
    }

    #[test]
    fn test_align_frames_by_timestamp() {
        let primary = vec![
            make_test_frame(1000, "UP"),
            make_test_frame(2000, "UP"),
            make_test_frame(3000, "DOWN"),
        ];
        let reference = vec![
            make_test_frame(1000, "UP"),
            make_test_frame(2000, "UP"),
            make_test_frame(3000, "DOWN"),
        ];

        let config = AlignmentConfig::default();
        let aligned = align_frames_by_timestamp(&primary, &reference, config.max_temporal_gap_ns);

        assert_eq!(aligned.len(), 3);
        assert_eq!(aligned[0].0.timestamp_ns, 1000);
        assert_eq!(aligned[1].0.timestamp_ns, 2000);
        assert_eq!(aligned[2].0.timestamp_ns, 3000);
    }

    #[test]
    fn test_align_frames_with_gaps() {
        let primary = vec![
            make_test_frame(1000, "UP"),
            make_test_frame(2000, "UP"),
            make_test_frame(100000000000000, "DOWN"), // > 1 day gap
        ];
        let reference = vec![
            make_test_frame(1000, "UP"),
            make_test_frame(3000, "UP"),
            make_test_frame(100000000000000, "DOWN"), // > 1 day gap
        ];

        let config = AlignmentConfig::default();
        let aligned = align_frames_by_timestamp(&primary, &reference, config.max_temporal_gap_ns);

        // Should align: 1000 with 1000, 2000 with 3000 (gap=1000ns), 100000000000000 with 100000000000000
        assert_eq!(aligned.len(), 3);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let frame = make_test_frame(1000, "UP");
        let aligned = vec![(frame.clone(), frame.clone())];

        let sim = compute_structural_vector_cosine_similarity(&aligned);
        assert!(sim.is_some());
        assert!((sim.unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let mut frame1 = make_test_frame(1000, "UP");
        frame1.vector.values = vec![Some(1.0), Some(0.0)];
        let mut frame2 = make_test_frame(1000, "UP");
        frame2.vector.values = vec![Some(0.0), Some(1.0)];

        let aligned = vec![(frame1, frame2)];
        let sim = compute_structural_vector_cosine_similarity(&aligned);
        assert!(sim.is_some());
        assert!(sim.unwrap().abs() < 1e-10);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let mut frame1 = make_test_frame(1000, "UP");
        frame1.vector.values = vec![Some(1.0), Some(1.0)];
        let mut frame2 = make_test_frame(1000, "UP");
        frame2.vector.values = vec![Some(-1.0), Some(-1.0)];

        let aligned = vec![(frame1, frame2)];
        let sim = compute_structural_vector_cosine_similarity(&aligned);
        assert!(sim.is_some());
        assert!((sim.unwrap() - (-1.0)).abs() < 1e-10);
    }

    #[test]
    fn test_state_agreement_aligned() {
        let primary = vec![make_test_frame(1000, "UP"), make_test_frame(2000, "UP")];
        let reference = vec![make_test_frame(1000, "UP"), make_test_frame(2000, "UP")];
        let aligned: Vec<_> = primary.into_iter().zip(reference).collect();

        let agreement = compute_state_agreement(
            &aligned,
            |f| f.prama.valid,
            |f| f.d_o.structural_state.clone(),
        );
        assert_eq!(agreement, "ALIGNED");
    }

    #[test]
    fn test_state_agreement_diverging() {
        let primary = vec![make_test_frame(1000, "UP"), make_test_frame(2000, "UP")];
        let reference = vec![make_test_frame(1000, "DOWN"), make_test_frame(2000, "DOWN")];
        let aligned: Vec<_> = primary.into_iter().zip(reference).collect();

        let agreement = compute_state_agreement(
            &aligned,
            |f| f.prama.valid,
            |f| f.d_o.structural_state.clone(),
        );
        assert_eq!(agreement, "DIVERGING");
    }

    #[test]
    fn test_cross_asset_relation_symmetric() {
        let primary_frames: Vec<_> = (0..50).map(|i| make_test_frame(1000 * i, "UP")).collect();
        let reference_frames: Vec<_> = (0..50).map(|i| make_test_frame(1000 * i, "UP")).collect();

        let relation1 = compute_cross_asset_relation(
            "inst_A",
            &primary_frames,
            "inst_B",
            &reference_frames,
            AlignmentConfig::default(),
        );
        let relation2 = compute_cross_asset_relation(
            "inst_B",
            &reference_frames,
            "inst_A",
            &primary_frames,
            AlignmentConfig::default(),
        );

        assert!(relation1.is_some());
        assert!(relation2.is_some());
        let r1 = relation1.unwrap();
        let r2 = relation2.unwrap();

        // Check symmetry for mathematical relations
        assert_eq!(
            r1.structural_vector_cosine_similarity,
            r2.structural_vector_cosine_similarity
        );
        assert_eq!(r1.aligned_observation_count, r2.aligned_observation_count);
        assert_eq!(r1.prama_state_agreement, r2.prama_state_agreement);
        assert_eq!(r1.relation_classification, r2.relation_classification);
    }

    #[test]
    fn cross_asset_relation_is_deterministic_for_identical_inputs() {
        let primary_frames: Vec<_> = (0..50).map(|i| make_test_frame(1000 * i, "UP")).collect();
        let reference_frames: Vec<_> = (0..50).map(|i| make_test_frame(1000 * i, "DOWN")).collect();
        let config = AlignmentConfig::default();

        let first = compute_cross_asset_relation(
            "inst_A",
            &primary_frames,
            "inst_B",
            &reference_frames,
            config.clone(),
        )
        .expect("relation");
        let second = compute_cross_asset_relation(
            "inst_A",
            &primary_frames,
            "inst_B",
            &reference_frames,
            config,
        )
        .expect("relation");

        assert_eq!(first, second);
        assert_eq!(first.provenance.computed_at_ns, first.overlap_end_ns);
    }

    #[test]
    fn test_cross_asset_relation_insufficient_overlap() {
        let primary_frames: Vec<_> = (0..10).map(|i| make_test_frame(1000 * i, "UP")).collect();
        let reference_frames: Vec<_> = (0..10).map(|i| make_test_frame(1000 * i, "UP")).collect();

        let config = AlignmentConfig {
            min_aligned_observations: 30,
            ..Default::default()
        };
        let relation = compute_cross_asset_relation(
            "inst_A",
            &primary_frames,
            "inst_B",
            &reference_frames,
            config.clone(),
        );
        assert!(relation.is_none());
    }

    #[test]
    fn test_cross_asset_relation_unavailable_instrument() {
        let primary_frames: Vec<_> = (0..50).map(|i| make_test_frame(1000 * i, "UP")).collect();
        let reference_frames = vec![];

        let config = AlignmentConfig::default();
        let relation = compute_cross_asset_relation(
            "inst_A",
            &primary_frames,
            "inst_B",
            &reference_frames,
            config,
        );
        assert!(relation.is_none());
    }
}
