use crate::{
    calibration::{
        resolve_direction, validate_profile_authority, validate_profile_for_engine,
        DirectionalResolution, ResolutionCalibrationProfile,
    },
    canonical,
    corpus::{audit_runtime_corpus, CorpusAuditReport},
    engine::{StructuralEngineAdapter, ENGINE_VERSION},
    logging::{init_logging, RequestFailed, RequestReceived, RequestServed},
    resolver::AssetResolver,
    service::{
        build_financial_data_response_with_profiles, profile_file_name, DataSourcePreference,
        FinancialDataRequest, FinancialDataResponse, ServiceError,
    },
    signal::{build_financial_signal_response, FinancialSignalRequest},
    structural::{StructuralVector, STRUCTURAL_VECTOR_VERSION},
    SignalMode, Timeframe,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    corpus_directory: PathBuf,
    calibration_directory: PathBuf,
}

#[derive(Debug, serde::Serialize)]
struct InputWindowDigest {
    timeframe: Timeframe,
    sha256: String,
}

#[derive(Debug, serde::Serialize)]
struct NativeResolutionProfileDigest {
    timeframe: Timeframe,
    calibration_version: String,
    profile_sha256: String,
}

#[derive(Debug, serde::Serialize)]
struct NativeRuntimeConfigDigest<'a> {
    schema: &'static str,
    mode: SignalMode,
    source: &'static str,
    timeframes: &'a [Timeframe],
    resolution_profiles: &'a [NativeResolutionProfileDigest],
}

struct LoadedResolution {
    directional: DirectionalResolution,
    calibration_version: String,
    profile_sha256: String,
}

#[derive(Debug, Default)]
struct RuntimeReadiness {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl RuntimeReadiness {
    fn is_ready(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Debug, serde::Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct ErrorResponse {
    status: &'static str,
    error: String,
}

#[derive(Debug, serde::Deserialize)]
struct ResolveRequest {
    query: String,
}

fn default_timeframe() -> Timeframe {
    Timeframe::D1
}

fn validate_native_request(request: &FinancialSignalRequest) -> Result<(), String> {
    if !request.venue.eq_ignore_ascii_case("auto") {
        return Err("venue overrides are not supported; use `venue: \"auto\"`".into());
    }
    if !request.quote.eq_ignore_ascii_case("auto") {
        return Err("quote overrides are not supported; use `quote: \"auto\"`".into());
    }
    if request.reference_asset.is_some() {
        return Err("reference_asset is not implemented on this endpoint".into());
    }
    if request.mode != SignalMode::Confirmed {
        return Err("LIVE_PREVIEW is not implemented on this closed-corpus endpoint".into());
    }
    if request
        .timeframes
        .iter()
        .any(|timeframe| !matches!(timeframe, Timeframe::D1 | Timeframe::W1))
    {
        return Err("native supplied-corpus requests support only D1 and W1".into());
    }
    if request
        .timeframes
        .iter()
        .enumerate()
        .any(|(index, timeframe)| request.timeframes[..index].contains(timeframe))
    {
        return Err("duplicate timeframes are not supported".into());
    }
    Ok(())
}

fn load_native_resolution(
    calibration_directory: &Path,
    instrument_id: &str,
    timeframe: Timeframe,
    vector: &StructuralVector,
) -> Result<Option<LoadedResolution>, String> {
    let path = calibration_directory.join(profile_file_name(instrument_id, timeframe));
    if !path.is_file() {
        return Ok(None);
    }
    let profile: ResolutionCalibrationProfile = serde_json::from_slice(
        &fs::read(&path).map_err(|error| format!("profile read failed: {error}"))?,
    )
    .map_err(|error| format!("profile JSON is invalid: {error}"))?;
    validate_profile_authority(&profile, ENGINE_VERSION, instrument_id, timeframe)
        .map_err(|error| format!("profile validation failed: {error}"))?;
    let profile_sha256 = profile
        .profile_sha256
        .clone()
        .ok_or_else(|| "validated profile is missing profile_sha256".to_owned())?;
    let directional = resolve_direction(vector, &profile)
        .map_err(|error| format!("direction resolution failed: {error}"))?;
    Ok(Some(LoadedResolution {
        directional,
        calibration_version: profile.calibration_version,
        profile_sha256,
    }))
}

fn report_end_exclusive_ns(end: &str) -> Option<i64> {
    let day = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d").ok()?;
    day.and_hms_opt(0, 0, 0)?
        .and_utc()
        .timestamp_nanos_opt()?
        .checked_add(86_400_000_000_000)
}

fn inspect_profiles(
    calibration_directory: &Path,
    corpus: &CorpusAuditReport,
    readiness: &mut RuntimeReadiness,
) {
    let entries = match fs::read_dir(calibration_directory) {
        Ok(entries) => entries,
        Err(error) => {
            readiness
                .errors
                .push(format!("calibration directory is unavailable: {error}"));
            return;
        }
    };
    let corpus_by_instrument: BTreeMap<&str, _> = corpus
        .files
        .iter()
        .map(|file| (file.instrument_id.as_str(), file))
        .collect();
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".resolution.json"))
        })
        .collect::<Vec<_>>();
    paths.sort();

    if paths.is_empty() {
        readiness
            .errors
            .push("no resolution profiles are available".into());
        return;
    }

    let mut d1_profiles = BTreeSet::new();
    for path in paths {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<invalid-profile-name>");
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                readiness
                    .errors
                    .push(format!("profile {file_name} cannot be read: {error}"));
                continue;
            }
        };
        let profile: ResolutionCalibrationProfile = match serde_json::from_slice(&bytes) {
            Ok(profile) => profile,
            Err(error) => {
                readiness
                    .errors
                    .push(format!("profile {file_name} is invalid JSON: {error}"));
                continue;
            }
        };
        if let Err(error) = validate_profile_for_engine(&profile, ENGINE_VERSION) {
            readiness
                .errors
                .push(format!("profile {file_name} failed validation: {error}"));
            continue;
        }
        let expected_name = profile_file_name(&profile.instrument_id, profile.timeframe);
        if file_name != expected_name {
            readiness.errors.push(format!(
                "profile {file_name} identity requires filename {expected_name}"
            ));
        }
        let Some(corpus_file) = corpus_by_instrument.get(profile.instrument_id.as_str()) else {
            readiness.errors.push(format!(
                "profile {file_name} has no serving corpus for {}",
                profile.instrument_id
            ));
            continue;
        };
        if report_end_exclusive_ns(&corpus_file.actual_end)
            .is_some_and(|end_ns| profile.calibration_end_ns > end_ns)
        {
            readiness.errors.push(format!(
                "profile {file_name} extends beyond serving-corpus coverage"
            ));
        }
        if profile.publication.profile_eligible_for_publication && corpus_file.cadence_anomalies > 0
        {
            readiness.errors.push(format!(
                "publication-eligible profile {file_name} uses a corpus with {} cadence anomalies",
                corpus_file.cadence_anomalies
            ));
        }
        if profile.timeframe == Timeframe::D1 {
            d1_profiles.insert(profile.instrument_id);
        }
    }

    for file in &corpus.files {
        if !d1_profiles.contains(&file.instrument_id) {
            readiness.errors.push(format!(
                "serving corpus {} has no validated D1 resolution profile",
                file.file
            ));
        }
    }
}

fn evaluate_runtime_readiness(
    corpus_directory: &Path,
    calibration_directory: &Path,
) -> RuntimeReadiness {
    let mut readiness = RuntimeReadiness::default();
    let corpus = match audit_runtime_corpus(corpus_directory) {
        Ok(report) => report,
        Err(error) => {
            readiness
                .errors
                .push(format!("runtime corpus audit failed: {error}"));
            return readiness;
        }
    };
    for file in &corpus.files {
        if !file.accepted {
            readiness.errors.push(format!(
                "runtime corpus {} failed pinned structure/hash validation",
                file.file
            ));
        }
        if file.cadence_anomalies > 0 {
            readiness.warnings.push(format!(
                "runtime corpus {} has {} cadence anomalies; publication remains gated",
                file.file, file.cadence_anomalies
            ));
        }
        if let Some(end_ns) = report_end_exclusive_ns(&file.actual_end) {
            let now_ns = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
            if end_ns < now_ns {
                readiness.warnings.push(format!(
                    "runtime corpus {} is historical; native responses are marked STALE_DATA",
                    file.file
                ));
            }
        }
    }
    inspect_profiles(calibration_directory, &corpus, &mut readiness);
    readiness
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct TelegraphFinancialDataRequest {
    asset: String,
    #[serde(default = "default_timeframe")]
    timeframe: Timeframe,
    #[serde(default)]
    source: DataSourcePreference,
}

async fn health_live() -> Json<HealthResponse> {
    Json(HealthResponse { status: "OK" })
}

async fn health_ready(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    if evaluate_runtime_readiness(&state.corpus_directory, &state.calibration_directory).is_ready()
    {
        (StatusCode::OK, Json(HealthResponse { status: "READY" }))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "NOT_READY",
            }),
        )
    }
}

async fn resolve_asset(Json(request): Json<ResolveRequest>) -> Json<crate::resolver::Resolution> {
    Json(AssetResolver::default().resolve(&request.query))
}

/// Native financial signal endpoint
async fn native_financial_signal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<FinancialSignalRequest>,
) -> Result<Json<crate::FinancialSignalResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let request_body = serde_json::to_string(&request).unwrap_or_default();
    let request_hash = canonical::sha256(&request)
        .expect("FinancialSignalRequest contains only canonically serializable values");

    // Log request received
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let log_req = RequestReceived::new(
        request_id.clone(),
        "/v1/financial/signal".into(),
        Some(request.asset.clone()),
        Some(request.resolve_timeframes(&[Timeframe::D1, Timeframe::W1])),
        user_agent,
        &request_body,
    );
    log_req.log();

    if let Err(error) = validate_native_request(&request) {
        RequestFailed::new(
            request_id,
            StatusCode::BAD_REQUEST.as_u16(),
            error.clone(),
            start.elapsed(),
        )
        .log();
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "ERROR",
                error,
            }),
        ));
    }

    // Resolve asset
    let instrument = match AssetResolver::default().resolve(&request.asset) {
        crate::resolver::Resolution::Found { instrument } => instrument,
        other => {
            let err = RequestFailed::new(
                request_id.clone(),
                StatusCode::BAD_REQUEST.as_u16(),
                format!("asset resolution failed: {other:?}"),
                start.elapsed(),
            );
            err.log();
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: format!("asset resolution failed: {other:?}"),
                }),
            ));
        }
    };

    // Determine supported timeframes for this asset
    let supported_timeframes = [Timeframe::D1, Timeframe::W1];
    let requested_tfs = request.resolve_timeframes(&supported_timeframes);

    if requested_tfs.is_empty() {
        let err = RequestFailed::new(
            request_id.clone(),
            StatusCode::BAD_REQUEST.as_u16(),
            "no supported timeframes for this asset".into(),
            start.elapsed(),
        );
        err.log();
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "ERROR",
                error: "no supported timeframes for this asset".into(),
            }),
        ));
    }

    // For each timeframe, load data and compute signal
    let mut scales = Vec::new();
    let mut data_watermark_ns = 0i64;
    let mut input_window_digests = Vec::new();
    let mut resolution_profiles = Vec::new();
    let engine = StructuralEngineAdapter::default();
    let engine_config_sha256 = engine.config_sha256().map_err(|error| {
        let message = format!("engine configuration hashing failed: {error}");
        RequestFailed::new(
            request_id.clone(),
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            message.clone(),
            start.elapsed(),
        )
        .log();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "ERROR",
                error: message,
            }),
        )
    })?;

    for tf in &requested_tfs {
        // Load D1 data
        let file = match instrument.symbol.as_str() {
            "BTCUSDT" => "btc_calib.csv",
            "XRPUSDT" => "xrp_calib.csv",
            "GC" => "gold_calib.csv",
            "NDX" => "nasdaq_calib.csv",
            "SPX" => "sp500_calib.csv",
            _ => {
                let err = RequestFailed::new(
                    request_id.clone(),
                    StatusCode::BAD_REQUEST.as_u16(),
                    format!("no corpus mapping for instrument {}", instrument.symbol),
                    start.elapsed(),
                );
                err.log();
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        status: "ERROR",
                        error: format!("no corpus mapping for instrument {}", instrument.symbol),
                    }),
                ));
            }
        };
        let path = state.corpus_directory.join(file);
        let daily = crate::historical::load_daily_csv(&path, &instrument, "supplied_corpus")
            .map_err(|e| {
                let err = RequestFailed::new(
                    request_id.clone(),
                    StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    e.to_string(),
                    start.elapsed(),
                );
                err.log();
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        status: "ERROR",
                        error: e.to_string(),
                    }),
                )
            })?;

        let bars = match tf {
            Timeframe::D1 => daily,
            Timeframe::W1 => crate::historical::aggregate_weekly(&daily).map_err(|e| {
                let err = RequestFailed::new(
                    request_id.clone(),
                    StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                    e.to_string(),
                    start.elapsed(),
                );
                err.log();
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ErrorResponse {
                        status: "ERROR",
                        error: e.to_string(),
                    }),
                )
            })?,
            _ => {
                let err = RequestFailed::new(
                    request_id.clone(),
                    StatusCode::BAD_REQUEST.as_u16(),
                    format!("timeframe {:?} not supported from supplied corpus", tf),
                    start.elapsed(),
                );
                err.log();
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        status: "ERROR",
                        error: format!("timeframe {:?} not supported from supplied corpus", tf),
                    }),
                ));
            }
        };

        if bars.is_empty() {
            let err = RequestFailed::new(
                request_id.clone(),
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                "corpus contains no bars".into(),
                start.elapsed(),
            );
            err.log();
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: "corpus contains no bars".into(),
                }),
            ));
        }

        let window_sha256 = canonical::sha256(&bars).map_err(|error| {
            let message = format!("input-window hashing failed: {error}");
            RequestFailed::new(
                request_id.clone(),
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                message.clone(),
                start.elapsed(),
            )
            .log();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: message,
                }),
            )
        })?;
        input_window_digests.push(InputWindowDigest {
            timeframe: *tf,
            sha256: window_sha256,
        });

        // Get last bar watermark
        let last_bar = bars.last().unwrap();
        let watermark = last_bar.close_time_ns;
        data_watermark_ns = data_watermark_ns.max(watermark);

        // Compute structural snapshot
        let kernel_input = crate::observation::adapt_closed_bars(&bars).map_err(|e| {
            let err = RequestFailed::new(
                request_id.clone(),
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                e.to_string(),
                start.elapsed(),
            );
            err.log();
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: e.to_string(),
                }),
            )
        })?;

        let frames =
            engine
                .replay_frames(&kernel_input)
                .map_err(|e: crate::engine::EngineError| {
                    let err = RequestFailed::new(
                        request_id.clone(),
                        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        e.to_string(),
                        start.elapsed(),
                    );
                    err.log();
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            status: "ERROR",
                            error: e.to_string(),
                        }),
                    )
                })?;

        let structural = engine
            .snapshot_from_frames(
                &instrument.instrument_id,
                *tf,
                &watermark.to_string(),
                &frames,
            )
            .map_err(|e: crate::engine::EngineError| {
                let err = RequestFailed::new(
                    request_id.clone(),
                    StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    e.to_string(),
                    start.elapsed(),
                );
                err.log();
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        status: "ERROR",
                        error: e.to_string(),
                    }),
                )
            })?;

        let vector = &frames
            .last()
            .expect("successful structural replay has at least one frame")
            .vector;
        let loaded_resolution = load_native_resolution(
            &state.calibration_directory,
            &instrument.instrument_id,
            *tf,
            vector,
        )
        .map_err(|error| {
            RequestFailed::new(
                request_id.clone(),
                StatusCode::SERVICE_UNAVAILABLE.as_u16(),
                error.clone(),
                start.elapsed(),
            )
            .log();
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    status: "ERROR",
                    error,
                }),
            )
        })?;
        let directional = loaded_resolution.map(|loaded| {
            resolution_profiles.push(NativeResolutionProfileDigest {
                timeframe: *tf,
                calibration_version: loaded.calibration_version,
                profile_sha256: loaded.profile_sha256,
            });
            loaded.directional
        });

        // Compute full technical + structural contrast in one call
        let technical_structural =
            crate::technical::compute_technical_structural_contrast(&bars, &structural).map_err(
                |e| {
                    let err = RequestFailed::new(
                        request_id.clone(),
                        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                        e.to_string(),
                        start.elapsed(),
                    );
                    err.log();
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            status: "ERROR",
                            error: e.to_string(),
                        }),
                    )
                },
            )?;

        let scale_signal = crate::signal::ScaleSignal {
            timeframe: *tf,
            structural,
            technical: technical_structural.technical.clone(),
            counter_reading: technical_structural.counter_reading.clone(),
            structural_contrast: technical_structural,
            directional,
        };
        scales.push(scale_signal);
    }

    if scales.is_empty() {
        let err = RequestFailed::new(
            request_id.clone(),
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            "no scales computed".into(),
            start.elapsed(),
        );
        err.log();
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "ERROR",
                error: "no scales computed".into(),
            }),
        ));
    }

    // Build response
    let input_window_sha256 = canonical::sha256(&input_window_digests).map_err(|error| {
        let message = format!("input-window digest hashing failed: {error}");
        RequestFailed::new(
            request_id.clone(),
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            message.clone(),
            start.elapsed(),
        )
        .log();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "ERROR",
                error: message,
            }),
        )
    })?;
    let distinct_versions = resolution_profiles
        .iter()
        .map(|profile| &profile.calibration_version)
        .collect::<BTreeSet<_>>();
    let resolution_calibration_version = (distinct_versions.len() == 1)
        .then(|| (*distinct_versions.first().expect("one distinct version")).clone());
    let resolution_profile_sha256 =
        (resolution_profiles.len() == 1).then(|| resolution_profiles[0].profile_sha256.clone());
    let mode = request.mode;
    let runtime_config_sha256 = canonical::sha256(&NativeRuntimeConfigDigest {
        schema: "pramagraph.native_runtime_config.v1",
        mode,
        source: "supplied_corpus",
        timeframes: &requested_tfs,
        resolution_profiles: &resolution_profiles,
    })
    .map_err(|error| {
        let message = format!("runtime configuration hashing failed: {error}");
        RequestFailed::new(
            request_id.clone(),
            StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
            message.clone(),
            start.elapsed(),
        )
        .log();
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "ERROR",
                error: message,
            }),
        )
    })?;
    let response = build_financial_signal_response(
        instrument.clone(),
        scales,
        mode,
        "supplied_corpus".into(),
        None,
        data_watermark_ns,
        input_window_sha256,
        ENGINE_VERSION.into(),
        engine_config_sha256,
        STRUCTURAL_VECTOR_VERSION.into(),
        resolution_calibration_version,
        resolution_profile_sha256,
        runtime_config_sha256,
        request_hash,
    );

    // Log response
    let response_tfs = response
        .scales
        .iter()
        .map(|s| s.timeframe)
        .collect::<Vec<_>>();
    let log_resp = RequestServed::new(
        request_id,
        StatusCode::OK.as_u16(),
        response.status,
        Some(instrument.symbol.clone()),
        Some(response_tfs),
        start.elapsed(),
        response.provenance.response_sha256.clone(),
    );
    log_resp.log();

    Ok(Json(response))
}

/// Telegraph adapter endpoint - delegates directly to canonical service implementation
async fn telegraph_financial_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TelegraphFinancialDataRequest>,
) -> Result<Json<FinancialDataResponse>, (StatusCode, Json<ErrorResponse>)> {
    let start = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let request_body = serde_json::to_string(&request).unwrap_or_default();

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let log_req = RequestReceived::new(
        request_id.clone(),
        "/v1/telegraph/financial-data".into(),
        Some(request.asset.clone()),
        Some(vec![request.timeframe]),
        user_agent,
        &request_body,
    );
    log_req.log();

    // Convert to service FinancialDataRequest
    let service_request = FinancialDataRequest {
        asset: request.asset,
        timeframe: request.timeframe,
        source: request.source,
    };

    // Delegate to canonical service implementation
    match build_financial_data_response_with_profiles(
        &state.corpus_directory,
        &state.calibration_directory,
        &service_request,
    )
    .await
    {
        Ok(response) => {
            // Log successful response
            let log_resp = RequestServed::new(
                request_id,
                StatusCode::OK.as_u16(),
                response.status,
                Some(response.instrument.symbol.clone()),
                Some(vec![response.timeframe]),
                start.elapsed(),
                response.response_sha256.clone(),
            );
            log_resp.log();

            Ok(Json(response))
        }
        Err(e) => {
            let status = match &e {
                ServiceError::Asset(_)
                | ServiceError::Timeframe
                | ServiceError::CorpusMapping(_) => StatusCode::BAD_REQUEST,
                ServiceError::Pipeline(_) => StatusCode::SERVICE_UNAVAILABLE,
            };
            let public_error = match &e {
                ServiceError::Pipeline(_) => "financial data pipeline unavailable".to_owned(),
                _ => e.to_string(),
            };
            let log_err =
                RequestFailed::new(request_id, status.as_u16(), e.to_string(), start.elapsed());
            log_err.log();
            Err((
                status,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: public_error,
                }),
            ))
        }
    }
}

/// Request logging middleware
/// Request logging middleware
async fn logging_middleware(req: axum::http::Request<axum::body::Body>, next: Next) -> Response {
    let start = Instant::now();
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    let response = next.run(req).await;

    let elapsed = start.elapsed();
    let status = response.status().as_u16();

    // Only log non-health endpoints
    if !path.starts_with("/health") {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = chrono::DateTime::from_timestamp(now.as_secs() as i64, now.subsec_nanos())
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
            .unwrap_or_else(|| "1970-01-01 00:00:00.000".into());
        eprintln!(
            "[{}] {} {} {} {}ms",
            timestamp,
            method,
            path,
            status,
            elapsed.as_millis()
        );
    }

    response
}

pub async fn serve(
    bind: SocketAddr,
    corpus_directory: PathBuf,
    calibration_directory: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging - path can be configured via env var
    let log_path = std::env::var("LOG_PATH")
        .unwrap_or_else(|_| "results/runtime/request_events.ndjson".to_string());
    let log_path = PathBuf::from(log_path);
    init_logging(&log_path).unwrap_or_else(|e| eprintln!("Failed to init logging: {e}"));

    let readiness = evaluate_runtime_readiness(&corpus_directory, &calibration_directory);
    for warning in &readiness.warnings {
        eprintln!("readiness warning: {warning}");
    }
    for error in &readiness.errors {
        eprintln!("readiness error: {error}");
    }
    let state = AppState {
        corpus_directory,
        calibration_directory,
    };

    let app = Router::new()
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready))
        .route("/v1/assets/resolve", post(resolve_asset))
        .route("/v1/financial/signal", post(native_financial_signal))
        .route(
            "/v1/telegraph/financial-data",
            post(telegraph_financial_data),
        )
        .layer(middleware::from_fn(logging_middleware))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_native_request() -> FinancialSignalRequest {
        FinancialSignalRequest {
            asset: "BTC".into(),
            venue: "auto".into(),
            quote: "auto".into(),
            timeframes: vec![Timeframe::D1],
            mode: SignalMode::Confirmed,
            reference_asset: None,
        }
    }

    #[test]
    fn native_request_rejects_controls_the_endpoint_does_not_apply() {
        assert!(validate_native_request(&valid_native_request()).is_ok());

        let mut request = valid_native_request();
        request.venue = "binance".into();
        assert_eq!(
            validate_native_request(&request).unwrap_err(),
            "venue overrides are not supported; use `venue: \"auto\"`"
        );

        let mut request = valid_native_request();
        request.quote = "USD".into();
        assert_eq!(
            validate_native_request(&request).unwrap_err(),
            "quote overrides are not supported; use `quote: \"auto\"`"
        );

        let mut request = valid_native_request();
        request.reference_asset = Some("SPX".into());
        assert_eq!(
            validate_native_request(&request).unwrap_err(),
            "reference_asset is not implemented on this endpoint"
        );

        let mut request = valid_native_request();
        request.mode = SignalMode::LivePreview;
        assert_eq!(
            validate_native_request(&request).unwrap_err(),
            "LIVE_PREVIEW is not implemented on this closed-corpus endpoint"
        );

        let mut request = valid_native_request();
        request.timeframes = vec![Timeframe::D1, Timeframe::D1];
        assert_eq!(
            validate_native_request(&request).unwrap_err(),
            "duplicate timeframes are not supported"
        );
    }

    #[test]
    fn readiness_checks_only_serving_corpus_and_validated_profiles() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let readiness = evaluate_runtime_readiness(
            &root.join("data/corpus"),
            &root.join("calibration/profiles"),
        );
        assert!(readiness.is_ready(), "{:?}", readiness.errors);
        assert!(readiness
            .warnings
            .iter()
            .any(|warning| warning.contains("btc_calib.csv") && warning.contains("cadence")));
    }

    #[test]
    fn readiness_rejects_a_profile_whose_internal_hash_was_not_updated() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let profiles = tempfile::tempdir().unwrap();
        for entry in fs::read_dir(root.join("calibration/profiles")).unwrap() {
            let path = entry.unwrap().path();
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".resolution.json"))
            {
                fs::copy(&path, profiles.path().join(path.file_name().unwrap())).unwrap();
            }
        }
        let btc = profiles
            .path()
            .join("crypto_binance_BTCUSDT_D1.resolution.json");
        let mut profile: serde_json::Value =
            serde_json::from_slice(&fs::read(&btc).unwrap()).unwrap();
        profile["profile_id"] = serde_json::Value::String("tampered-profile".into());
        fs::write(&btc, serde_json::to_vec(&profile).unwrap()).unwrap();

        let readiness = evaluate_runtime_readiness(&root.join("data/corpus"), profiles.path());
        assert!(!readiness.is_ready());
        assert!(readiness.errors.iter().any(|error| {
            error.contains("crypto_binance_BTCUSDT_D1.resolution.json")
                && error.contains("failed validation")
        }));
    }
}
