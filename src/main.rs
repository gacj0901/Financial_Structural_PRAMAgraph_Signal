use clap::{Parser, Subcommand};
use pramagraph_financial::calibration::{
    build_dynamics_ablation_audit, build_dynamics_conditional_information_audit,
    build_dynamics_frozen_velocity_forward_audit, build_dynamics_sequential_residual_audit,
    build_neighbor_anatomy_artifacts, build_range_distance_geometry_audit,
    build_range_intraclass_compactness_audit, build_range_trajectory_anatomy_audit,
    build_resolution_profile, build_right_censoring_audit, resolve_direction,
    validate_profile_for_engine, NeighborAnatomyQuery, ResolutionCalibrationProfile,
};
use pramagraph_financial::corpus::{audit_corpus, CorpusAuditReport};
use pramagraph_financial::engine::{KernelObservation, StructuralEngineAdapter};
use pramagraph_financial::historical::{
    aggregate_weekly, cadence_anomalies, load_daily_csv, load_daily_csv_with_policy, CadencePolicy,
    HistoricalLoadPolicy,
};
use pramagraph_financial::observation::adapt_closed_bars;
use pramagraph_financial::resolver::{AssetResolver, Resolution};
use pramagraph_financial::server;
use pramagraph_financial::service::{FinancialDataRequest, FinancialDataResponse};
use pramagraph_financial::{AvailableValue, FinancialSignalResponse, MarketObservation, Timeframe};
use schemars::schema_for;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Parser)]
#[command(name = "pramagraph-financial", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Resolve {
        query: String,
    },
    ValidateCsv {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "supplied_corpus")]
        source: String,
    },
    KernelReplay {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "financial:local:UNKNOWN")]
        instrument_id: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
    },
    ReplayMarket {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long, default_value_t = false)]
        zero_volume_is_unavailable: bool,
    },
    CalibrateDirection {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = false)]
        overwrite: bool,
        /// SHA-256 of a protocol frozen before any test outcomes were inspected.
        #[arg(long)]
        preregistered_protocol_sha256: Option<String>,
    },
    ResolveDirection {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
    },
    NeighborAnatomy {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, default_value = "results/diagnostics")]
        output: PathBuf,
    },
    RangeDistanceGeometry {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/range_distance_geometry_audit.json"
        )]
        output: PathBuf,
    },
    RangeIntraclassCompactness {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/range_intraclass_compactness_audit.json"
        )]
        output: PathBuf,
    },
    RangeTrajectoryAnatomy {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/range_trajectory_anatomy_audit.json"
        )]
        output: PathBuf,
    },
    RightCensoringAudit {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long, default_value = "results/diagnostics/right_censoring_audit.json")]
        output: PathBuf,
    },
    DynamicsAblation {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/dynamics_ablation_audit.json"
        )]
        output: PathBuf,
    },
    DynamicsConditionalInformation {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/dynamics_conditional_information_audit.json"
        )]
        output: PathBuf,
    },
    DynamicsSequentialResidual {
        #[arg(long)]
        input: PathBuf,
        /// Strictly post-input OHLC bars used only to construct outcomes.
        #[arg(long)]
        label_extension: Option<PathBuf>,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/dynamics_sequential_residual_audit.json"
        )]
        output: PathBuf,
    },
    DynamicsFrozenVelocityForward {
        /// Original feature prefix followed by strictly later feature bars.
        #[arg(long)]
        input: PathBuf,
        /// Bars strictly after the feature source, used only to mature outcomes.
        #[arg(long)]
        label_extension: PathBuf,
        #[arg(long)]
        instrument: String,
        #[arg(long, default_value = "D1")]
        timeframe: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(
            long,
            default_value = "results/diagnostics/dynamics_frozen_velocity_forward_audit.json"
        )]
        output: PathBuf,
    },
    AuditCorpus {
        #[arg(long, default_value = "data/corpus")]
        input: PathBuf,
        #[arg(long, default_value = "results/corpus-audit.json")]
        output: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: std::net::SocketAddr,
        #[arg(long, default_value = "data/corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "calibration/profiles")]
        calibration: PathBuf,
    },
    Schema {
        #[arg(long, default_value = "schemas")]
        output: PathBuf,
    },
}

#[derive(Debug, Serialize)]
struct ValidationReport {
    instrument_id: String,
    daily_bars: usize,
    weekly_bars: usize,
    cadence_anomalies: Vec<pramagraph_financial::historical::CadenceAnomaly>,
    first_open_time_ns: Option<i64>,
    last_open_time_ns: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CalibrationWriteReport {
    profile_id: String,
    output: String,
    runtime_library_samples: usize,
    train_samples: usize,
    validation_samples: usize,
    evaluation_tail_samples: usize,
    test_accuracy_bp: u16,
    test_reliability_lower_bound_bp: u16,
    test_balanced_accuracy_bp: u16,
    test_brier_score: f64,
    climatology_brier_score: f64,
    brier_skill_score: f64,
    effective_vector_dimensions: usize,
    d_o_transport_evaluable_bp: u16,
    odce_organization_available_bp: u16,
    k_mem_prior_available_bp: u16,
    profile_sha256: String,
}

type MarketReplay = (
    pramagraph_financial::Instrument,
    Timeframe,
    Vec<MarketObservation>,
    Vec<pramagraph_financial::structural::StructuralFrame>,
);

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Resolve { query } => print_json(&AssetResolver::default().resolve(&query))?,
        Command::ValidateCsv {
            input,
            instrument,
            source,
        } => {
            let resolved = AssetResolver::default().resolve(&instrument);
            let instrument = match resolved {
                Resolution::Found { instrument } => instrument,
                other => {
                    print_json(&other)?;
                    return Ok(());
                }
            };
            let daily = load_daily_csv(input, &instrument, &source)?;
            let anomalies =
                cadence_anomalies(&daily, CadencePolicy::daily(instrument.session_calendar));
            let weekly = aggregate_weekly(&daily)?;
            print_json(&ValidationReport {
                instrument_id: instrument.instrument_id,
                daily_bars: daily.len(),
                weekly_bars: weekly.len(),
                cadence_anomalies: anomalies,
                first_open_time_ns: daily.first().map(|bar| bar.open_time_ns),
                last_open_time_ns: daily.last().map(|bar| bar.open_time_ns),
            })?;
        }
        Command::KernelReplay {
            input,
            instrument_id,
            timeframe,
        } => {
            let timeframe = parse_timeframe(&timeframe)?;
            let observations = read_kernel_csv(&input)?;
            let watermark = observations
                .last()
                .map(|row| row.timestamp_ns.to_string())
                .unwrap_or_default();
            let snapshot = StructuralEngineAdapter::default().replay(
                &instrument_id,
                timeframe,
                &watermark,
                &observations,
            )?;
            print_json(&snapshot)?;
        }
        Command::ReplayMarket {
            input,
            instrument,
            timeframe,
            zero_volume_is_unavailable,
        } => {
            let instrument = match AssetResolver::default().resolve(&instrument) {
                Resolution::Found { instrument } => instrument,
                other => {
                    print_json(&other)?;
                    return Ok(());
                }
            };
            let daily = load_daily_csv_with_policy(
                input,
                &instrument,
                "supplied_corpus",
                HistoricalLoadPolicy {
                    zero_volume_is_unavailable,
                    exclude_malformed_ohlc: false,
                },
            )?;
            let timeframe = parse_timeframe(&timeframe)?;
            let bars = match timeframe {
                Timeframe::D1 => daily,
                Timeframe::W1 => aggregate_weekly(&daily)?,
                _ => return Err("supplied corpus replay supports only D1 or W1".into()),
            };
            let kernel_input = adapt_closed_bars(&bars)?;
            let watermark = bars
                .last()
                .map(|bar| bar.close_time_ns.to_string())
                .unwrap_or_default();
            let snapshot = StructuralEngineAdapter::default().replay(
                &instrument.instrument_id,
                timeframe,
                &watermark,
                &kernel_input,
            )?;
            print_json(&snapshot)?;
        }
        Command::CalibrateDirection {
            input,
            instrument,
            timeframe,
            output,
            overwrite,
            preregistered_protocol_sha256,
        } => {
            let (instrument, timeframe, bars, frames) =
                replay_market_frames(&input, &instrument, &timeframe)?;
            let profile = build_resolution_profile(
                &instrument.instrument_id,
                instrument.asset_class,
                timeframe,
                pramagraph_financial::engine::ENGINE_VERSION,
                &bars,
                &frames,
                preregistered_protocol_sha256.as_deref(),
            )?;
            if output.exists() && !overwrite {
                return Err(format!(
                    "calibration artifact already exists: {}; use --overwrite explicitly",
                    output.display()
                )
                .into());
            }
            write_json(&output, &profile)?;
            print_json(&CalibrationWriteReport {
                profile_id: profile.profile_id.clone(),
                output: output.display().to_string(),
                runtime_library_samples: profile.samples.len(),
                train_samples: profile.diagnostics.train_samples,
                validation_samples: profile.diagnostics.validation_samples,
                evaluation_tail_samples: profile.diagnostics.evaluation_tail_samples,
                test_accuracy_bp: profile.reliability.reliability_bp,
                test_reliability_lower_bound_bp: profile.reliability.reliability_lower_bound_bp,
                test_balanced_accuracy_bp: profile.reliability.balanced_accuracy_bp,
                test_brier_score: profile.reliability.multiclass_brier_score,
                climatology_brier_score: profile.reliability.climatology_brier_score,
                brier_skill_score: profile.reliability.brier_skill_score,
                effective_vector_dimensions: profile.diagnostics.effective_vector_dimensions,
                d_o_transport_evaluable_bp: profile.diagnostics.d_o_transport_evaluable_bp,
                odce_organization_available_bp: profile
                    .diagnostics
                    .odce_adaptive_organization_available_bp,
                k_mem_prior_available_bp: profile.diagnostics.k_mem_strictly_prior_available_bp,
                profile_sha256: profile
                    .profile_sha256
                    .clone()
                    .expect("profile builder hashes"),
            })?;
        }
        Command::ResolveDirection {
            input,
            instrument,
            timeframe,
            profile,
        } => {
            let (_, _, _, frames) = replay_market_frames(&input, &instrument, &timeframe)?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let vector = &frames
                .last()
                .ok_or("structural replay produced no frames")?
                .vector;
            print_json(&resolve_direction(vector, &profile)?)?;
        }
        Command::NeighborAnatomy {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let artifacts = build_neighbor_anatomy_artifacts(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            let audit_path = output.join("neighbor_anatomy_audit_tail.jsonl");
            let summary_path = output.join("neighbor_anatomy_summary.json");
            write_jsonl(&audit_path, &artifacts.audit_tail)?;
            write_json(&summary_path, &artifacts.summary)?;
            print_json(&artifacts.summary)?;
        }
        Command::RangeDistanceGeometry {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_range_distance_geometry_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::RangeIntraclassCompactness {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_range_intraclass_compactness_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::RangeTrajectoryAnatomy {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_range_trajectory_anatomy_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::RightCensoringAudit {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_right_censoring_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::DynamicsAblation {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            if timeframe != Timeframe::D1 {
                return Err("dynamics ablation currently requires D1 input".into());
            }
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let feature_bars = bars
                .get(..frames.len())
                .ok_or("feature frame count exceeds label-source bars")?;
            let weekly_bars = aggregate_weekly(feature_bars)?;
            let weekly_frames = StructuralEngineAdapter::default()
                .replay_frames(&adapt_closed_bars(&weekly_bars)?)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_dynamics_ablation_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &weekly_frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::DynamicsConditionalInformation {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            if timeframe != Timeframe::D1 {
                return Err("conditional dynamics diagnostic currently requires D1 input".into());
            }
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let feature_bars = bars
                .get(..frames.len())
                .ok_or("feature frame count exceeds label-source bars")?;
            let weekly_bars = aggregate_weekly(feature_bars)?;
            let weekly_frames = StructuralEngineAdapter::default()
                .replay_frames(&adapt_closed_bars(&weekly_bars)?)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_dynamics_conditional_information_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &weekly_frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::DynamicsSequentialResidual {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) = replay_diagnostic_frames(
                &input,
                label_extension.as_deref(),
                &instrument,
                &timeframe,
            )?;
            if timeframe != Timeframe::D1 {
                return Err("sequential residual diagnostic currently requires D1 input".into());
            }
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let feature_bars = bars
                .get(..frames.len())
                .ok_or("feature frame count exceeds label-source bars")?;
            let weekly_bars = aggregate_weekly(feature_bars)?;
            let weekly_frames = StructuralEngineAdapter::default()
                .replay_frames(&adapt_closed_bars(&weekly_bars)?)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_dynamics_sequential_residual_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &weekly_frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::DynamicsFrozenVelocityForward {
            input,
            label_extension,
            instrument,
            timeframe,
            profile,
            output,
        } => {
            let (instrument, timeframe, bars, frames) =
                replay_diagnostic_frames(&input, Some(&label_extension), &instrument, &timeframe)?;
            if timeframe != Timeframe::D1 {
                return Err("frozen velocity forward audit currently requires D1 input".into());
            }
            let profile: ResolutionCalibrationProfile =
                serde_json::from_reader(File::open(profile)?)?;
            validate_profile_for_engine(&profile, pramagraph_financial::engine::ENGINE_VERSION)?;
            let feature_bars = bars
                .get(..frames.len())
                .ok_or("feature frame count exceeds label-source bars")?;
            let weekly_bars = aggregate_weekly(feature_bars)?;
            let weekly_frames = StructuralEngineAdapter::default()
                .replay_frames(&adapt_closed_bars(&weekly_bars)?)?;
            let generation_timestamp_unix_seconds =
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
            let audit = build_dynamics_frozen_velocity_forward_audit(
                &instrument.instrument_id,
                timeframe,
                &bars,
                &frames,
                &weekly_frames,
                &profile,
                generation_timestamp_unix_seconds,
            )?;
            write_json(&output, &audit)?;
            print_json(&audit)?;
        }
        Command::AuditCorpus { input, output } => {
            let report = audit_corpus(input)?;
            write_json(&output, &report)?;
            print_json(&report)?;
            if !report.all_files_accepted {
                return Err("one or more corpus files failed the blueprint manifest".into());
            }
        }
        Command::Serve {
            bind,
            corpus,
            calibration,
        } => server::serve(bind, corpus, calibration).await?,
        Command::Schema { output } => emit_schemas(&output)?,
    }
    Ok(())
}

fn print_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn emit_schemas(output: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(output)?;
    write_schema(
        output.join("market-observation.schema.json"),
        &schema_for!(MarketObservation),
    )?;
    write_schema(
        output.join("financial-signal-response.schema.json"),
        &schema_for!(FinancialSignalResponse),
    )?;
    write_schema(
        output.join("kernel-observation.schema.json"),
        &schema_for!(KernelObservation),
    )?;
    write_schema(
        output.join("corpus-audit.schema.json"),
        &schema_for!(CorpusAuditReport),
    )?;
    write_schema(
        output.join("resolution-calibration-profile.schema.json"),
        &schema_for!(ResolutionCalibrationProfile),
    )?;
    write_schema(
        output.join("telegraph-financial-data-request.schema.json"),
        &schema_for!(FinancialDataRequest),
    )?;
    write_schema(
        output.join("telegraph-financial-data-response.schema.json"),
        &schema_for!(FinancialDataResponse),
    )?;
    Ok(())
}

fn replay_market_frames(
    input: &Path,
    query: &str,
    timeframe: &str,
) -> Result<MarketReplay, Box<dyn std::error::Error>> {
    let instrument = match AssetResolver::default().resolve(query) {
        Resolution::Found { instrument } => instrument,
        other => return Err(format!("asset resolution failed: {other:?}").into()),
    };
    let timeframe = parse_timeframe(timeframe)?;
    if !matches!(timeframe, Timeframe::D1 | Timeframe::W1) {
        return Err("supplied corpus calibration supports only D1 or W1".into());
    }
    let daily = load_daily_csv(input, &instrument, "supplied_corpus")?;
    let bars = if timeframe == Timeframe::D1 {
        daily
    } else {
        aggregate_weekly(&daily)?
    };
    let observations = adapt_closed_bars(&bars)?;
    let frames = StructuralEngineAdapter::default().replay_frames(&observations)?;
    Ok((instrument, timeframe, bars, frames))
}

fn replay_diagnostic_frames(
    input: &Path,
    label_extension: Option<&Path>,
    query: &str,
    timeframe: &str,
) -> Result<MarketReplay, Box<dyn std::error::Error>> {
    let (instrument, timeframe, mut label_bars, frames) =
        replay_market_frames(input, query, timeframe)?;
    let Some(label_extension) = label_extension else {
        return Ok((instrument, timeframe, label_bars, frames));
    };
    if timeframe != Timeframe::D1 {
        return Err("label extension is currently supported only for D1 diagnostics".into());
    }
    let extension = load_daily_csv(label_extension, &instrument, "label_extension")?;
    let feature_source_end = label_bars
        .last()
        .ok_or("feature/query source contains no bars")?
        .open_time_ns;
    if extension.is_empty()
        || extension
            .iter()
            .any(|bar| bar.open_time_ns <= feature_source_end)
    {
        return Err(
            "label extension must contain only bars strictly after the feature/query source".into(),
        );
    }
    label_bars.extend(extension);
    Ok((instrument, timeframe, label_bars, frames))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn write_jsonl(
    path: &Path,
    rows: &[NeighborAnatomyQuery],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    for row in rows {
        serde_json::to_writer(&mut writer, row)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

fn write_schema(path: PathBuf, schema: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, schema)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn parse_timeframe(value: &str) -> Result<Timeframe, String> {
    match value.to_ascii_uppercase().as_str() {
        "M1" => Ok(Timeframe::M1),
        "M5" => Ok(Timeframe::M5),
        "H1" => Ok(Timeframe::H1),
        "H4" => Ok(Timeframe::H4),
        "D1" => Ok(Timeframe::D1),
        "W1" => Ok(Timeframe::W1),
        _ => Err(format!("unsupported timeframe: {value}")),
    }
}

fn read_kernel_csv(path: &Path) -> Result<Vec<KernelObservation>, Box<dyn std::error::Error>> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let index = |name: &str| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(name))
    };
    let timestamp = index("timestamp_ns").ok_or("missing timestamp_ns")?;
    let omega = index("omega").ok_or("missing omega")?;
    let expected = index("expected").ok_or("missing expected")?;
    let u_lambda = index("u_lambda");
    let sigma_op = index("sigma_op");
    let mut output = Vec::new();
    for record in reader.records() {
        let record = record?;
        output.push(KernelObservation {
            timestamp_ns: record
                .get(timestamp)
                .ok_or("missing timestamp value")?
                .parse()?,
            omega: record.get(omega).ok_or("missing omega value")?.parse()?,
            expected: record
                .get(expected)
                .ok_or("missing expected value")?
                .parse()?,
            u_lambda: match u_lambda
                .and_then(|position| record.get(position))
                .filter(|value| !value.trim().is_empty())
            {
                Some(value) => AvailableValue::available(value.parse()?),
                None => AvailableValue::not_applicable(),
            },
            sigma_op: match sigma_op
                .and_then(|position| record.get(position))
                .filter(|value| !value.trim().is_empty())
            {
                Some(value) => AvailableValue::available(value.parse()?),
                None => AvailableValue::not_applicable(),
            },
        });
    }
    Ok(output)
}
