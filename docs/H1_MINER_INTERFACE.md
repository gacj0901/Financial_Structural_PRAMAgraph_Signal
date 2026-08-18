# H1 Miner Interface Document

---

## Native Financial Signal Endpoint

**Endpoint:** `POST /v1/financial/signal`

### Request

```json
{
  "asset": "string",           // required: canonical symbol or alias
  "venue": "string",           // optional, default: "auto"
  "quote": "string",           // optional, default: "auto"
  "timeframes": ["D1", "W1"],  // optional, default: all supported for asset
  "mode": "CONFIRMED"          // optional, default: "CONFIRMED"
}
```

**Field details:**
- `asset`: Required. Canonical symbol or alias (e.g., `BTC`, `XRP`, `GOLD`, `SP500`, `NASDAQ`)
- `venue`: Optional. Exchange/venue override. Default: `"auto"` (resolver picks primary)
- `quote`: Optional. Quote currency override. Default: `"auto"`
- `timeframes`: Optional. Array of timeframes. Default: all supported for the asset (currently `["D1", "W1"]`)
- `mode`: Optional. `"CONFIRMED"` (closed bars only) or `"LIVE_PREVIEW"`. Default: `"CONFIRMED"`

**Example request:**
```json
{
  "asset": "BTC",
  "timeframes": ["D1", "W1"]
}
```

### Response

```json
{
  "schema": "pramagraph.financial_signal.v1",
  "status": "OK",
  "instrument": { ... },
  "as_of_ns": 1771632000000000000,
  "data_watermark_ns": 1771632000000000000,
  "mode": "CONFIRMED",
  "signal": {
    "direction": "UP",
    "structural_state": "VIABLE",
    "dominant_scale": "D1",
    "cross_scale": {
      "dominant_direction": "UP",
      "agreement": "UNANIMOUS",
      "agreeing_scales": ["D1", "W1"],
      "disagreeing_scales": [],
      "dominant_scale": "D1"
    }
  },
  "scales": [
    {
      "timeframe": "D1",
      "structural": { ... },
      "technical": {
        "direction": "UP",
        "votes": { "ema_trend": "UP", "ema_slope": "UP", "macd": "UP", "rsi_centerline": "UP" },
        "range_detection": { "adx14": 25.3, "ema_separation_atr": 0.8, "is_range": false },
        "indicators": { "ema20": 50000, "ema50": 49000, "ema20_slope": 100, "macd_histogram": 500, "rsi14": 65, "adx14": 25.3, "atr14": 1000, "bollinger_upper": 52000, "bollinger_lower": 48000, "bollinger_middle": 50000 },
        "bars_used": 100
      },
      "counter_reading": {
        "direction": "NONE",
        "evidence": { "rsi_extreme": null, "ema_extension": 0.5, "bollinger_position": null }
      },
      "structural_contrast": {
        "timeframe": "D1",
        "structural": { ... },
        "technical": { ... },
        "counter_reading": { ... },
        "structural_contrast": { "state": "CONFIRMING", "evidence": [...] }
      }
    }
  ],
  "provenance": {
    "primary_provider": "supplied_corpus",
    "secondary_provider": null,
    "source_watermark": "",
    "input_window_sha256": "sha256:...",
    "engine_version": "prama-protokol-rs/0.3.0@ddb91cad+...",
    "engine_config_sha256": "default",
    "structural_vector_version": "financial_structural_vector_v2",
    "resolution_calibration_version": null,
    "resolution_profile_sha256": null,
    "runtime_config_sha256": "default",
    "response_sha256": "sha256:7a6416f51a2e33bea77628925a88b762fc52cbac986ade1cc55abc68a2845e2c"
  }
}
```

---

## Telegraph Adapter Endpoint

**Endpoint:** `POST /v1/telegraph/financial-data`

### Request

```json
{
  "asset": "BTC",
  "timeframe": "D1",       // optional, default: "D1"
  "source": "AUTO"         // optional, default: "AUTO"
}
```

**Field details:**
- `asset`: Required. Canonical symbol or alias
- `timeframe`: Optional. `"D1"` or `"W1"`. Default: `"D1"`
- `source`: Optional. `"AUTO"` or `"SUPPLIED_CORPUS"`. Default: `"AUTO"`

**Example request:**
```json
{
  "asset": "BTC",
  "timeframe": "D1"
}
```

### Response

```json
{
  "schema": "pramagraph.telegraph.financial_data.v1",
  "intent": "FINANCIAL_DATA",
  "status": "OK",
  "label": "UP",
  "reason": "multi-scale signal: D1",
  "instrument": {
    "instrument_id": "crypto:binance:BTCUSDT",
    "asset_class": "crypto",
    "symbol": "BTCUSDT",
    "base": "BTC",
    "quote": "USDT",
    "venue": "binance",
    "timezone": "UTC",
    "session_calendar": "CONTINUOUS_UTC"
  },
  "timeframe": "D1",
  "as_of_ns": 1771632000000000000,
  "market": {
    "open_time_ns": 0,
    "close_time_ns": 1771632000000000000,
    "open": 0.0,
    "high": 0.0,
    "low": 0.0,
    "close": 0.0,
    "volume": { "value": null, "availability": "UNAVAILABLE" }
  },
  "structural": { ... },
  "technical": {
    "direction": "UP",
    "votes": { ... },
    "range_detection": { ... },
    "indicators": { ... },
    "bars_used": 100
  },
  "counter_reading": {
    "direction": "NONE",
    "evidence": { "rsi_extreme": null, "ema_extension": 0.5, "bollinger_position": null }
  },
  "structural_contrast": {
    "timeframe": "D1",
    "structural": { ... },
    "technical": { ... },
    "counter_reading": { ... },
    "structural_contrast": { "state": "CONFIRMING", "evidence": [...] }
  },
  "directional": {
    "direction": "UP",
    "probabilities_bp": null,
    "horizon": null,
    "reliability_bp": null,
    "sample_support": 0,
    "calibration_scope": "UNAVAILABLE",
    "profile_sha256": "",
    "publication_reason": "technical proxy"
  },
  "provenance": {
    "primary_provider": "supplied_corpus",
    "provider_instrument": null,
    "corpus_file": null,
    "input_sha256": "",
    "engine_version": "prama-protokol-rs/0.3.0@ddb91cad+...",
    "observation_interface_version": "financial_observation_interface_v1"
  },
  "response_sha256": "sha256:7a6416f51a2e33bea77628925a88b762fc52cbac986ade1cc55abc68a2845e2c"
}
```

---

## Health Endpoints

### `GET /health/live`

**Response:**
```json
{ "status": "OK" }
```
Process is running.

---

### `GET /health/ready`

**Response (ready):**
```json
{ "status": "READY" }
```
HTTP 200 — Financial service can serve requests (corpus audit passed).

**Response (not ready):**
```json
{ "status": "NOT_READY" }
```
HTTP 503 — Corpus audit failed or not initialized.

---

## Request Lifecycle

```
Client / Telegraph
    │
    ▼
Telegraph Adapter (/v1/telegraph/financial-data)
    │   Converts request to native format
    ▼
Native Financial Service (/v1/financial/signal)
    │   Resolves asset
    │   Loads market data (closed bars only)
    │   ┌──────────────────────────────────────┐
    │   │ PRAMAgraph Structural Path           │
    │   │  adapt_closed_bars → replay_frames   │
    │   │  → snapshot_from_frames              │
    │   └──────────────────────────────────────┘
    │   ┌──────────────────────────────────────┐
    │   │ Technical Direction Path             │
    │   │  compute_technical_direction()       │
    │   │  compute_counter_reading()           │
    │   └──────────────────────────────────────┘
    │   ┌──────────────────────────────────────┐
    │   │ Contrast / Composition               │
    │   │  compute_structural_contrast()       │
    │   │  cross-scale composition             │
    │   └──────────────────────────────────────┘
    ▼
FinancialSignalResponse (canonical, SHA-256 hashed)
    │
    ▼
Telegraph Adapter
    │   Extracts first scale components
    │   Maps to Telegraph FinancialDataResponse
    ▼
Client
    │
    ▼
Request Event Log (NDJSON)
    │
    ├─ REQUEST_RECEIVED { request_id, endpoint, asset, timeframes, body_sha256, timestamp }
    ├─ REQUEST_SERVED   { request_id, http_status, status, asset, timeframes, elapsed_ms, response_sha256 }
    └─ REQUEST_FAILED   { request_id, http_status, error, elapsed_ms, timestamp }
```

---

## Request Events Logging (NDJSON)

**File:** `results/runtime/request_events.ndjson`

**Event types:**

1. **REQUEST_RECEIVED**
```json
{
  "event": "REQUEST_RECEIVED",
  "timestamp": "2026-08-17T16:17:08.022863500Z",
  "request_id": "2ea49143-8ced-4167-ba1a-14e43e08b303",
  "endpoint": "/v1/telegraph/financial-data",
  "asset": "BTC",
  "requested_timeframes": ["D1"],
  "user_agent": "curl/8.21.0",
  "request_body_sha256": "sha256:8d8656058add9bedd744376dc0156d28d8d3d4f2d7b5b3fead91dbde0c1748c9"
}
```

2. **REQUEST_SERVED**
```json
{
  "event": "REQUEST_SERVED",
  "timestamp": "2026-08-17T16:17:09.306073600Z",
  "request_id": "ee5a9f93-0cfb-4889-8399-6cb601338bb0",
  "http_status": 200,
  "pramagraph_status": "Ok",
  "asset": "BTCUSDT",
  "returned_timeframes": ["D1"],
  "elapsed_ms": 1282,
  "response_sha256": "sha256:7a6416f51a2e33bea77628925a88b762fc52cbac986ade1cc55abc68a2845e2c"
}
```

3. **REQUEST_FAILED**
```json
{
  "event": "REQUEST_FAILED",
  "timestamp": "2026-08-17T16:19:02.632946400Z",
  "request_id": "4f235873-cd7d-4980-ac05-3ae43f2672bf",
  "http_status": 400,
  "error": "asset resolution failed: UnsupportedAsset { query: \"INVALID_ASSET\" }",
  "elapsed_ms": 0
}
```

**Console output** is also emitted in real-time for operator visibility.