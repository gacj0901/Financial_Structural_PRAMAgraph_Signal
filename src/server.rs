use crate::{
    contracts::SignalMode,
    corpus::audit_corpus,
    engine::{StructuralEngineAdapter, ENGINE_VERSION},
    logging::{init_logging, RequestFailed, RequestReceived, RequestServed},
    observation::OBSERVATION_INTERFACE_VERSION,
    resolver::AssetResolver,
    service::FinancialDataResponse,
    signal::{build_financial_signal_response, FinancialSignalRequest},
    structural::STRUCTURAL_VECTOR_VERSION,
    Timeframe,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use hex;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    corpus_directory: PathBuf,
    #[allow(dead_code)]
    calibration_directory: PathBuf,
    ready: bool,
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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct TelegraphFinancialDataRequest {
    asset: String,
    #[serde(default = "default_timeframe")]
    timeframe: Timeframe,
    #[serde(default)]
    source: DataSourcePreference,
}

fn default_timeframe() -> Timeframe {
    Timeframe::D1
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum DataSourcePreference {
    #[default]
    Auto,
    SuppliedCorpus,
}

async fn health_live() -> Json<HealthResponse> {
    Json(HealthResponse { status: "OK" })
}

async fn health_ready(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    if state.ready {
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
    let request_hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(request_body.as_bytes()))
    );

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
    let _all_bars_used = 0;
    let mut data_watermark_ns = 0i64;
    let _primary_provider = "supplied_corpus".to_string();

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

        let bars = match tf {
            Timeframe::D1 => daily,
            Timeframe::W1 => crate::historical::aggregate_weekly(&daily).map_err(|e| {
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
                StatusCode::BAD_REQUEST.as_u16(),
                "corpus contains no bars".into(),
                start.elapsed(),
            );
            err.log();
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    status: "ERROR",
                    error: "corpus contains no bars".into(),
                }),
            ));
        }

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

        let engine = StructuralEngineAdapter::default();
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
    let mode = request.mode;
    let response = build_financial_signal_response(
        instrument.clone(),
        scales,
        mode,
        "supplied_corpus".into(),
        None,
        data_watermark_ns,
        ENGINE_VERSION.into(),
        "default".into(), // engine_config_sha256
        STRUCTURAL_VECTOR_VERSION.into(),
        None,             // resolution_calibration_version
        None,             // resolution_profile_sha256
        "default".into(), // runtime_config_sha256
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

/// Telegraph adapter endpoint
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

    // Convert to native request
    let native_request = FinancialSignalRequest {
        asset: request.asset,
        venue: "auto".into(),
        quote: "auto".into(),
        timeframes: vec![request.timeframe],
        mode: SignalMode::Confirmed,
    };

    // Delegate to native handler
    match native_financial_signal(State(state), headers, Json(native_request)).await {
        Ok(Json(native_response)) => {
            // Convert to Telegraph format
            let telegraph_response = FinancialDataResponse {
                schema: "pramagraph.telegraph.financial_data.v1".into(),
                intent: "FINANCIAL_DATA".into(),
                status: native_response.status,
                label: format!("{:?}", native_response.direction),
                reason: format!(
                    "multi-scale signal: {}",
                    native_response
                        .scales
                        .first()
                        .map(|s| format!("{:?}", s.timeframe))
                        .unwrap_or("D1".into())
                ),
                instrument: native_response.instrument,
                timeframe: native_response
                    .scales
                    .first()
                    .map(|s| s.timeframe)
                    .unwrap_or(Timeframe::D1),
                as_of_ns: native_response.as_of_ns,
                market: crate::service::MarketBarResponse {
                    open_time_ns: 0,
                    close_time_ns: native_response.as_of_ns,
                    open: 0.0,
                    high: 0.0,
                    low: 0.0,
                    close: 0.0,
                    volume: crate::AvailableValue::unavailable(),
                },
                structural: native_response
                    .scales
                    .first()
                    .map(|s| s.structural.clone())
                    .unwrap_or(crate::StructuralSnapshot {
                        instrument_id: "".into(),
                        timeframe: Timeframe::D1,
                        as_of_ns: 0,
                        engine_version: "".into(),
                        structural_state: "".into(),
                        prama: crate::ComponentSnapshot::unavailable(""),
                        d_o: crate::ComponentSnapshot::unavailable(""),
                        odce: crate::ComponentSnapshot::unavailable(""),
                        k_mem: crate::ComponentSnapshot::unavailable(""),
                        availability: std::collections::BTreeMap::new(),
                        source_watermark: "".into(),
                        snapshot_sha256: None,
                    }),
                directional: native_response
                    .scales
                    .first()
                    .and_then(|s| s.structural.d_o.value.as_ref())
                    .and_then(|v| v.get("structural_state"))
                    .map(|v| crate::calibration::DirectionalResolution {
                        direction: match v.as_str().unwrap_or("") {
                            "CRYSTALLIZED" | "RECURRENT" | "VIABLE" | "CRYSTALLIZING" => {
                                crate::Direction::Up
                            }
                            "STAGNANT" | "INACTIVE" => crate::Direction::Range,
                            _ => crate::Direction::Down,
                        },
                        probabilities_bp: None,
                        horizon: None,
                        reliability_bp: None,
                        sample_support: 0,
                        calibration_scope: crate::CalibrationScope::Unavailable,
                        profile_sha256: "".into(),
                        publication_reason: "technical proxy".into(),
                    }),
                provenance: crate::service::ServiceProvenance {
                    primary_provider: native_response.provenance.primary_provider.clone(),
                    provider_instrument: None,
                    corpus_file: None,
                    input_sha256: native_response.provenance.input_window_sha256.clone(),
                    engine_version: native_response.provenance.engine_version.clone(),
                    observation_interface_version: OBSERVATION_INTERFACE_VERSION.into(),
                },
                response_sha256: native_response.provenance.response_sha256.clone(),
            };

            let log_resp = RequestServed::new(
                Uuid::new_v4().to_string(),
                StatusCode::OK.as_u16(),
                telegraph_response.status,
                Some(telegraph_response.instrument.symbol.clone()),
                Some(vec![telegraph_response.timeframe]),
                start.elapsed(),
                telegraph_response.response_sha256.clone(),
            );
            log_resp.log();

            Ok(Json(telegraph_response))
        }
        Err(e) => {
            let log_err = RequestFailed::new(
                Uuid::new_v4().to_string(),
                e.0.as_u16(),
                e.1.error.clone(),
                start.elapsed(),
            );
            log_err.log();
            Err(e)
        }
    }
}

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
    // Initialize logging
    let log_path = PathBuf::from("results/runtime/request_events.ndjson");
    init_logging(&log_path).unwrap_or_else(|e| eprintln!("Failed to init logging: {e}"));

    let ready = audit_corpus(&corpus_directory)
        .map(|report| report.all_files_accepted)
        .unwrap_or(false);

    let state = AppState {
        corpus_directory,
        calibration_directory,
        ready,
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
