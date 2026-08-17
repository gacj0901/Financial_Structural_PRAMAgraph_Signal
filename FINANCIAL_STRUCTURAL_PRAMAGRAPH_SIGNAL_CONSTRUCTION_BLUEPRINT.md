# Financial Structural PRAMAgraph Signal
## Construction Blueprint / Implementation Guide 

**Project:** PRAMAgraph by PRAMA Protokol  
**Product:** Financial Structural PRAMAgraph Signal  
**Runtime:** PRAMAgraph-Dynamagh  
**Status:** Build Authority  
**Date:** 2026-08-16

---

# 0. Build Decision

Build one financial signal runtime that can resolve a requested instrument, obtain its market history and live feed, construct the required timeframes, execute the current PRAMA / D_O / ODCE / K-MEM stack, and emit a deterministic machine-readable signal.

The financial runtime SHALL NOT use an LLM to generate, explain, rank, or reinterpret the signal.

The current structural components remain authoritative. This project adds:

```text
market data ingestion
→ canonical market observations
→ multiscale structural execution
→ directional resolution
→ manifestation horizon
→ empirical reliability
→ cross-scale integration
→ provenance
→ API / Telegraph adapter
```

The financial layer SHALL NOT copy, approximate, or rewrite the current PRAMA, D_O, ODCE, or K-MEM logic.

---

# 1. Product Output

For any supported instrument, the service returns:

```text
instrument identity
as-of timestamp
data watermark

global structural state
most probable direction
P(UP)
P(RANGE)
P(DOWN)

expected manifestation window
empirical reliability

M1 structural signal
M5 structural signal
H1 structural signal
H4 structural signal
D1 structural signal
W1 structural signal

cross-scale agreement
dominant scale
propagation state

calibration scope
sample support

input / engine / configuration provenance
```

Directions:

```text
UP
DOWN
RANGE
UNRESOLVED
```

A structural state and a directional signal are separate outputs.

A structural state may be valid even when direction or manifestation horizon is unavailable.

---

# 2. Runtime Topology

```text
                     ASSET QUERY
                         |
                         v
                  Asset Resolver
                         |
                         v
                  Provider Router
                         |
             +-----------+-----------+
             |                       |
             v                       v
      Historical Bootstrap       Live Stream
             |                       |
             +-----------+-----------+
                         |
                         v
                  Raw Market Ledger
                         |
                         v
             Canonical Observation Layer
                         |
                         v
                  Timeframe Builder
            M1 | M5 | H1 | H4 | D1 | W1
                         |
                         v
                Structural Adapter
                         |
                         v
             PRAMA -> D_O -> ODCE -> K-MEM
                         |
                         v
              Structural Snapshot / Scale
                         |
                         v
              Directional Resolution Layer
                         |
                         v
                Horizon Estimation Layer
                         |
                         v
                 Cross-Scale Integrator
                         |
                         v
              Reliability + Provenance
                         |
                         v
          FINANCIAL STRUCTURAL PRAMAGRAPH SIGNAL
                         |
              +----------+----------+
              |                     |
              v                     v
          REST / WS           Telegraph Adapter
```

---

# 3. Data Sources Used

## 3.1 Supplied Calibration / Historical Corpus

The supplied files are retained as the initial D1 calibration and historical corpus.

### Calibration windows

| Instrument | File | Rows | Start | End | Fields |
|---|---|---:|---|---|---|
| BTC | `btc_calib.csv` | 800 | 2023-01-18 | 2026-02-20 | OHLC |
| GOLD | `gold_calib.csv` | 800 | 2023-01-17 | 2026-02-20 | OHLC |
| NASDAQ | `nasdaq_calib.csv` | 800 | 2022-12-12 | 2026-02-20 | OHLCV |
| SP500 | `sp500_calib.csv` | 800 | 2022-12-12 | 2026-02-20 | OHLCV |
| XRP | `xrp_calib.csv` | 800 | 2023-12-16 | 2026-02-22 | OHLCV |

These files are D1 inputs.

They SHALL NOT be resampled into M1, M5, H1, or H4.

They MAY be used directly for D1 and aggregated causally into W1.

### Historical files

| Instrument | File | Rows | Start | End |
|---|---|---:|---|---|
| BTC | `btc_stooq.csv` | 4,036 | 2010-07-19 | 2026-02-20 |
| GOLD | `gold_stooq.csv` | 15,186 | 1793-03-01 | 2026-02-20 |
| NASDAQ | `nasdaq_stooq.csv` | 10,176 | 1985-10-01 | 2026-02-20 |
| SP500 | `sp500_stooq.csv` | 39,639 | 1789-05-01 | 2026-02-20 |
| XRP | `xrp_stooq.csv` | 4,048 | 2015-01-21 | 2026-02-22 |

The historical files SHALL pass through `HistoricalGranularityValidator` before entering the calibration library.

The validator rejects or excludes ranges whose cadence is incompatible with the requested timeframe.

Volume availability is feature-specific:

```text
BTC historical file      → volume unavailable
GOLD historical file     → volume unavailable
NASDAQ historical file   → volume available
SP500 historical file    → volume partially available
XRP historical file      → volume partially available
```

Unavailable volume remains unavailable. It is never replaced with zero.

---

## 3.2 Live / Intraday Providers

The provider layer is pluggable.

Initial provider set:

```text
Crypto spot:
    Binance Spot       primary
    Coinbase           secondary verification

Stocks / indices / forex / futures:
    Massive            primary cross-asset provider
```

The provider adapter is responsible for symbol syntax, authentication, venue-specific timestamps, session calendars, sequence handling, reconnect logic, and raw payload parsing.

The structural engine receives no provider-specific payload.

---

# 4. Asset Resolver

The resolver converts a user query into a canonical instrument.

Examples:

```text
BTC
XRP
BTCUSDT
ETH-USD
AAPL
MSFT
SPX
NDX
EURUSD
GC
```

Canonical identity:

```json
{
  "instrument_id": "crypto:binance:XRPUSDT",
  "asset_class": "crypto",
  "symbol": "XRPUSDT",
  "base": "XRP",
  "quote": "USDT",
  "venue": "binance",
  "timezone": "UTC",
  "session_calendar": "continuous"
}
```

The resolver SHALL determine:

```text
asset class
canonical provider
provider symbol
venue
base / quote
timezone
session calendar
price precision
quantity precision when relevant
live-data capability
historical-data capability
```

If a name is ambiguous, return candidate instruments instead of guessing.

Response:

```text
AMBIGUOUS_ASSET
```

If no configured provider can supply the required data:

```text
UNSUPPORTED_ASSET
```

---

# 5. Provider Contract

All providers implement the same interface:

```python
class MarketDataProvider:
    async def resolve(self, query): ...
    async def metadata(self, instrument): ...
    async def historical_bars(self, instrument, interval, start, end): ...
    async def latest_quote(self, instrument): ...
    async def stream_bars(self, instrument, interval): ...
    async def stream_trades(self, instrument): ...
    async def stream_quotes(self, instrument): ...
    async def stream_depth(self, instrument): ...
```

Optional methods return explicit capability status rather than fake data.

Example:

```text
stream_depth = UNAVAILABLE
```

The provider router selects the configured primary source and, when available, a secondary verification source.

---

# 6. Raw Market Ledger

Store observations before any PRAMA transformation.

Canonical ledger event:

```json
{
  "provider": "binance",
  "instrument_id": "crypto:binance:XRPUSDT",
  "event_type": "bar",
  "event_time_ns": 0,
  "received_time_ns": 0,
  "sequence_id": "provider-specific-or-null",
  "payload": {},
  "payload_sha256": "sha256:..."
}
```

Storage:

```text
live:
    bounded in-memory ring buffer

persistent:
    Parquet

query / replay:
    DuckDB
```

Partition persistent data by:

```text
asset_class / instrument_id / UTC-date
```

Confirmed observations are append-only.

---

# 7. Canonical Market Observation

Provider payloads are normalized into `MarketObservation`.

## 7.1 Mandatory bar fields

```text
instrument_id
timeframe
open_time
close_time
open
high
low
close
is_closed
source
```

## 7.2 Availability-controlled fields

```text
volume
quote_volume
trade_count

best_bid
best_ask
bid_size
ask_size

spread_bps

buy_volume
sell_volume
order_flow_imbalance

bid_depth
ask_depth
depth_imbalance
```

Every optional field has an availability flag.

Example:

```json
{
  "value": null,
  "availability": "UNAVAILABLE"
}
```

Do not encode missing values as `0.0`.

---

# 8. Canonical Derived Market Features

The financial adapter may derive market observables before structural execution.

Required price-domain features:

```text
log_return
normalized_range
realized_volatility
relative_range
price_displacement
return_velocity
return_acceleration
gap_state
```

When data is available:

```text
relative_volume
volume_impulse
spread_bps
spread_expansion
order_flow_imbalance
depth_imbalance
liquidity_change
trade_intensity
```

All scale-sensitive features are normalized with parameters from the active calibration profile.

The current PRAMA integration context supplied to Codex determines the final mapping from these features into the kernel observation contract.

---

# 9. Timeframe Construction

Required scales:

```text
M1
M5
H1
H4
D1
W1
```

## 9.1 Base data

For intraday-capable providers, M1 is the canonical live base interval.

Higher intraday scales are built internally:

```text
M5 = aggregate closed M1 bars
H1 = aggregate closed M1 bars
H4 = aggregate closed M1 bars
```

D1 and W1 are built according to the canonical session calendar for the instrument.

Crypto:

```text
continuous calendar
UTC boundaries
```

Exchange-traded assets:

```text
provider / exchange session calendar
```

The system SHALL NOT force crypto calendar rules onto equities, indices, futures, or other session-based instruments.

## 9.2 Closed-bar requirement

The default machine signal is:

```text
mode = CONFIRMED
```

Confirmed mode uses only closed input bars.

A separate live preview may use the current incomplete bar but MUST be marked:

```text
mode = LIVE_PREVIEW
```

Confirmed and preview signals are never mixed.

---

# 10. Historical Bootstrap for a New Asset

When an asset is queried for the first time:

```text
resolve instrument
    ↓
check local history
    ↓
fetch missing historical data
    ↓
validate cadence
    ↓
build M1 base history if available
    ↓
construct M5/H1/H4/D1/W1
    ↓
normalize observations
    ↓
run structural engine historically
    ↓
build asset structural library
    ↓
activate live signal
```

The runtime SHALL cache the generated asset profile.

A later query reuses the existing profile and only appends new observations.

The first request may return:

```text
BOOTSTRAPPING
```

until minimum history required by the current structural engine is available.

---

# 11. Calibration Architecture

Calibration is split into two independent objects.

## 11.1 Structural Engine Calibration

This is the calibration required by the current PRAMA / D_O / ODCE / K-MEM implementation.

The financial project SHALL call the calibration procedure supplied with the current engine context.

It SHALL NOT invent replacement kernel parameters.

Produced artifact:

```text
StructuralCalibrationProfile
```

Key:

```text
engine_version
asset_class
instrument_id
timeframe
calibration_version
```

---

## 11.2 Direction / Horizon Calibration

This is downstream from the structural engine.

It maps structural states to empirically observed future resolutions.

Produced artifact:

```text
ResolutionCalibrationProfile
```

Contains:

```text
structural feature normalization
structural sample vectors
future outcome labels
first-passage times
direction statistics
horizon distributions
held-out reliability
sample counts
calibration scope
profile hash
```

Runtime recalibration is disabled.

New profiles are created offline and versioned.

---

# 12. Calibration Scope

Resolution lookup order:

```text
1. instrument
2. asset class
3. global structural library
```

Example:

```text
SOL-specific support sufficient
    → use SOL profile

SOL-specific support insufficient
    → crypto profile

crypto support insufficient
    → global structural profile
```

The scope used is published in the response.

Allowed values:

```text
INSTRUMENT
ASSET_CLASS
GLOBAL
UNAVAILABLE
```

No direction or horizon is published if empirical support is insufficient.

---

# 13. Structural Engine Adapter

Only this module connects market observations to the current structural stack.

```text
MarketObservation
        ↓
FinancialObservationAdapter
        ↓
current PRAMA input contract
        ↓
PRAMA
        ↓
D_O
        ↓
ODCE
        ↓
K-MEM
        ↓
StructuralSnapshot
```

The adapter SHALL:

```text
preserve causal order
preserve availability
preserve timeframe identity
preserve engine/config version
preserve source watermark
```

The adapter SHALL NOT:

```text
rename unavailable values into zero
generate fake structural metrics
reinterpret the meaning of core fields
modify current structural equations
replace current engine calibration
```

---

# 14. Structural Snapshot Contract

The wrapper stores the current engine output without altering its semantics.

```json
{
  "instrument_id": "...",
  "timeframe": "H4",
  "as_of": "...",
  "engine_version": "...",

  "prama": {},
  "d_o": {},
  "odce": {},
  "k_mem": {},

  "availability": {},
  "source_watermark": "...",
  "snapshot_sha256": "sha256:..."
}
```

The concrete fields inside `prama`, `d_o`, `odce`, and `k_mem` come from the current implementation supplied to Codex.

---

# 15. Financial Structural Vector

Create a versioned vector from available structural outputs.

```text
financial_structural_vector_v1
```

The vector SHALL contain only current, valid structural outputs.

No raw asset price is used as a structural dimension unless explicitly required by the current structural contract.

Market price remains available to the downstream outcome-labeling layer.

The vector encoder publishes an availability mask so two structural samples are never treated as identical merely because one has missing dimensions.

---

# 16. Directional Resolution

Direction is a downstream financial inference.

It does not change the kernel state.

Classes:

```text
UP
DOWN
RANGE
UNRESOLVED
```

## 16.1 Outcome Labeling

For each historical structural snapshot:

```text
snapshot at t0
    ↓
observe future price path causally after t0
    ↓
record first directional resolution
    ↓
record time-to-resolution
```

The calibration pipeline determines, per asset class and timeframe:

```text
up barrier
down barrier
maximum observation horizon
range rule
```

Barriers SHALL be volatility-normalized.

The calibration pipeline stores the resulting parameters.

Runtime SHALL NOT choose or optimize these values.

## 16.2 Structural Similarity Estimator

Use a deterministic distance-weighted nearest-state estimator.

Inputs:

```text
current structural vector
availability mask
active resolution calibration profile
```

Generated offline parameters:

```text
neighbor_count
minimum_support
distance metric parameters
distance weighting
maximum admissible distance
```

Runtime procedure:

```text
normalize current vector
    ↓
retrieve comparable prior structural states
    ↓
reject incompatible availability patterns
    ↓
apply distance weights
    ↓
aggregate historical outcomes
    ↓
produce P(UP), P(RANGE), P(DOWN)
```

Direction is the largest probability only when the calibrated evidence threshold is satisfied.

Otherwise:

```text
UNRESOLVED
```

---

# 17. Manifestation Horizon

For the selected direction, use the time-to-first-resolution observations from comparable structural states.

Publish:

```text
p25_bars
median_bars
p75_bars
```

Also publish wall-clock conversion.

Examples of unit conversion:

```text
H1:
    6 bars → 6 hours

H4:
    5 bars → 20 hours

D1:
    7 bars → 7 market/continuous days according to calendar

W1:
    3 bars → 3 canonical weeks
```

The observation timeframe and manifestation horizon remain separate concepts.

If support is insufficient:

```text
horizon = null
horizon_status = UNAVAILABLE
```

---

# 18. Per-Scale Signal

Each timeframe returns:

```text
structural state
direction
direction probabilities
manifestation horizon
empirical reliability
sample support
calibration scope
structural summary
```

Canonical object:

```json
{
  "timeframe": "H4",

  "structural": {
    "state": "CRITICAL_TRANSITION",
    "summary": {}
  },

  "direction": "DOWN",

  "probabilities_bp": {
    "up": 0,
    "range": 0,
    "down": 0
  },

  "horizon": {
    "p25_bars": null,
    "median_bars": null,
    "p75_bars": null,
    "p25_seconds": null,
    "median_seconds": null,
    "p75_seconds": null
  },

  "reliability_bp": null,
  "sample_support": 0,
  "calibration_scope": "UNAVAILABLE"
}
```

Example zeros above are schema placeholders only. Production probabilities are populated only from calibrated evidence.

---

# 19. Reliability

`reliability` means held-out empirical reliability of the resolution estimator for the applicable:

```text
engine version
structural vector version
calibration scope
timeframe
direction class
horizon region
```

It is not:

```text
LLM confidence
kernel confidence
current maximum probability
subjective confidence
```

Reliability is generated during offline calibration and loaded at runtime.

If held-out reliability does not satisfy the generated publication policy:

```text
direction = UNRESOLVED
```

The structural state remains available.

---

# 20. Cross-Scale Integrator

Inputs:

```text
M1
M5
H1
H4
D1
W1
```

Only scales with valid directional evidence participate in directional aggregation.

For each participating scale, compute:

```text
directional edge
empirical reliability
sample-support quality
availability quality
```

The cross-scale calibration profile generates the weighting parameters.

Outputs:

```text
dominant_direction
global P(UP)
global P(RANGE)
global P(DOWN)
cross_scale_agreement
dominant_scale
propagation
```

---

# 21. Cross-Scale Propagation

Maintain a rolling history of per-scale state transitions.

Allowed values:

```text
MICRO_TO_MACRO
MACRO_TO_MICRO
MIXED
NONE
UNAVAILABLE
```

Propagation is emitted only when the configured cross-scale policy observes a consistent transition across the required adjacent scales.

Example topology:

```text
M1 → M5 → H1 → H4 → D1 → W1
```

The propagation algorithm operates on structural/directional transition timestamps, not on narrative interpretation.

---

# 22. Canonical Numeric Representation

Published normalized probabilities and scores use integer basis points.

```text
0      = 0.00%
10000  = 100.00%
```

Directional probabilities SHALL satisfy exactly:

```text
up_bp + range_bp + down_bp = 10000
```

Use deterministic integer normalization.

No binary floating-point value is used in the canonical public response when a basis-point representation exists.

Internal engine values may remain in their native format and are preserved in the hashed structural snapshot.

---

# 23. Final Response Schema

```json
{
  "schema": "pramagraph.financial_signal.v1",
  "status": "OK",

  "instrument": {
    "instrument_id": "crypto:binance:XRPUSDT",
    "asset_class": "crypto",
    "symbol": "XRPUSDT",
    "base": "XRP",
    "quote": "USDT",
    "venue": "binance"
  },

  "as_of": "2026-08-16T00:00:00Z",
  "data_watermark": "2026-08-16T00:00:00Z",
  "mode": "CONFIRMED",

  "signal": {
    "structural_state": "CRITICAL_TRANSITION",
    "direction": "DOWN",

    "probabilities_bp": {
      "up": 1800,
      "range": 2100,
      "down": 6100
    },

    "horizon": {
      "source_timeframe": "H4",
      "p25_bars": 3,
      "median_bars": 5,
      "p75_bars": 8,
      "p25_seconds": 43200,
      "median_seconds": 72000,
      "p75_seconds": 115200
    },

    "reliability_bp": 7000,
    "cross_scale_agreement_bp": 7800,
    "dominant_scale": "H4",
    "propagation": "MACRO_TO_MICRO"
  },

  "scales": [
    {
      "timeframe": "M1",
      "structural": {
        "state": "STABLE",
        "summary": {}
      },
      "direction": "RANGE",
      "probabilities_bp": {
        "up": 2500,
        "range": 5200,
        "down": 2300
      },
      "horizon": {
        "p25_bars": 2,
        "median_bars": 4,
        "p75_bars": 7
      },
      "reliability_bp": 6100,
      "sample_support": 0,
      "calibration_scope": "INSTRUMENT"
    }
  ],

  "provenance": {
    "primary_provider": "binance",
    "secondary_provider": "coinbase",
    "source_watermark": "...",
    "input_window_sha256": "sha256:...",
    "engine_version": "...",
    "engine_config_sha256": "sha256:...",
    "structural_vector_version": "financial_structural_vector_v1",
    "resolution_calibration_version": "...",
    "resolution_profile_sha256": "sha256:...",
    "runtime_config_sha256": "sha256:...",
    "response_sha256": "sha256:..."
  }
}
```

The numeric values in the example define the response shape only.

They SHALL NOT be copied into runtime defaults.

---

# 24. API Surface

## 24.1 Resolve instrument

```http
POST /v1/assets/resolve
```

Request:

```json
{
  "query": "XRP"
}
```

---

## 24.2 One-shot confirmed signal

```http
POST /v1/financial/signal
```

Request:

```json
{
  "asset": "XRP",
  "venue": "auto",
  "quote": "auto",
  "timeframes": ["M1", "M5", "H1", "H4", "D1", "W1"],
  "mode": "CONFIRMED"
}
```

---

## 24.3 Streaming signal

```text
WebSocket /v1/financial/stream
```

Subscription:

```json
{
  "asset": "XRP",
  "timeframes": ["M1", "M5", "H1", "H4", "D1", "W1"],
  "mode": "CONFIRMED"
}
```

Emit a new confirmed signal when a new canonical M1 bar closes or when the provider/calendar defines a relevant scale close.

---

## 24.4 Health

```http
GET /health/live
GET /health/ready
```

Readiness requires:

```text
provider connection available
engine loaded
configuration loaded
calibration store readable
ledger writable
```

---

# 25. Runtime Statuses

```text
OK
BOOTSTRAPPING
UNRESOLVED
INSUFFICIENT_DATA
INSUFFICIENT_CALIBRATION
STALE_DATA
PROVIDER_DIVERGENCE
AMBIGUOUS_ASSET
UNSUPPORTED_ASSET
ENGINE_ERROR
```

`UNRESOLVED` is a valid analysis result.

It is not an execution error.

---

# 26. Provider Verification

When a secondary provider exists for the same instrument:

```text
primary canonical source
        +
secondary comparison source
```

Compare:

```text
event timestamp
closed-bar close
normalized range
return displacement
```

Do not average providers.

The primary source remains canonical.

Material divergence is surfaced as:

```text
PROVIDER_DIVERGENCE
```

and both provider timestamps are included in provenance.

---

# 27. Provenance

Every confirmed response stores:

```text
instrument identity
data watermark
raw input reference
input window hash
engine version
engine configuration hash
structural snapshot hash
structural vector version
resolution calibration version
resolution calibration hash
runtime configuration hash
canonical response hash
```

Canonical JSON serialization is required before hashing.

A signal must be replayable from these artifacts.

---

# 28. No-Lookahead Rule

No observation after the signal watermark may influence:

```text
feature normalization
structural execution
K-MEM state available at t0
direction neighbor selection
direction probabilities
manifestation horizon
reliability selection
cross-scale state at t0
```

Outcome data after `t0` is used only by the offline calibration builder to label historical cases.

Runtime never has access to the future outcome for the current query.

---

# 29. Configuration Model

Static runtime configuration:

```yaml
service:
  schema: pramagraph.financial_signal.v1
  default_mode: CONFIRMED

timeframes:
  enabled:
    - M1
    - M5
    - H1
    - H4
    - D1
    - W1
  intraday_base: M1

providers:
  crypto:
    primary: binance_spot
    secondary: coinbase
  stocks:
    primary: massive
  indices:
    primary: massive
  forex:
    primary: massive
  futures:
    primary: massive

storage:
  live_buffer: memory
  historical: parquet
  query_engine: duckdb

calibration:
  runtime_recalibration: false
  fallback_order:
    - instrument
    - asset_class
    - global

direction:
  estimator: distance_weighted_structural_neighbors
  parameters_source: resolution_calibration_profile

horizon:
  estimator: comparable_state_first_passage
  parameters_source: resolution_calibration_profile

cross_scale:
  parameters_source: cross_scale_calibration_profile

provenance:
  hash: sha256
  canonical_json: true
```

All thresholds, sample minima, barrier magnitudes, neighbor counts, distance cutoffs, reliability thresholds, and scale weights are generated by calibration artifacts.

They SHALL NOT be invented as source-code constants.

---

# 30. Generated Calibration Configuration

Example structure only:

```yaml
resolution_profile:
  profile_id: "..."
  instrument_id: "..."
  asset_class: "..."
  timeframe: "H4"

  structural_vector_version: "financial_structural_vector_v1"

  outcome_label:
    upper_barrier: "<generated>"
    lower_barrier: "<generated>"
    max_horizon_bars: "<generated>"

  estimator:
    neighbor_count: "<generated>"
    minimum_support: "<generated>"
    maximum_distance: "<generated>"
    distance_power: "<generated>"

  publication:
    minimum_direction_edge: "<generated>"
    minimum_reliability: "<generated>"

  reliability:
    held_out_score: "<generated>"
    sample_support: "<generated>"

  provenance:
    calibration_start: "..."
    calibration_end: "..."
    profile_sha256: "..."
```

Codex should implement the schema and loader, not invent values for the placeholders.

---

# 31. Project Layout

```text
pramagraph_financial/
|
|-- api/
|   |-- routes.py
|   |-- websocket.py
|   `-- schemas.py
|
|-- assets/
|   |-- resolver.py
|   |-- instrument.py
|   `-- calendars.py
|
|-- providers/
|   |-- base.py
|   |-- router.py
|   |-- binance_spot.py
|   |-- coinbase.py
|   `-- massive.py
|
|-- market/
|   |-- ledger.py
|   |-- observation.py
|   |-- features.py
|   |-- timeframe_builder.py
|   `-- historical_validator.py
|
|-- structural/
|   |-- engine_adapter.py
|   |-- snapshot.py
|   `-- vector.py
|
|-- calibration/
|   |-- structural_profile.py
|   |-- resolution_profile.py
|   |-- historical_runner.py
|   |-- outcome_labeler.py
|   |-- neighbor_index.py
|   |-- horizon.py
|   |-- reliability.py
|   `-- cross_scale_profile.py
|
|-- signal/
|   |-- per_scale.py
|   |-- direction.py
|   |-- horizon.py
|   |-- cross_scale.py
|   `-- composer.py
|
|-- provenance/
|   |-- canonical_json.py
|   `-- hashes.py
|
|-- telegraph/
|   |-- adapter.py
|   `-- schemas.py
|
|-- storage/
|   |-- parquet_store.py
|   `-- duckdb_store.py
|
|-- config/
|   |-- runtime.yaml
|   `-- providers.yaml
|
`-- tests/
    |-- test_asset_resolution.py
    |-- test_historical_granularity.py
    |-- test_timeframe_aggregation.py
    |-- test_closed_bar_only.py
    |-- test_missing_feature_availability.py
    |-- test_engine_adapter.py
    |-- test_no_lookahead.py
    |-- test_direction_determinism.py
    |-- test_horizon_determinism.py
    |-- test_cross_scale.py
    |-- test_replay.py
    |-- test_unseen_asset_bootstrap.py
    `-- test_provider_divergence.py
```

---

# 32. Build Order

## Stage 1 — Contracts

Implement:

```text
Instrument
MarketObservation
StructuralSnapshot
PerScaleSignal
FinancialSignalResponse
status enums
configuration schemas
```

Acceptance:

```text
all schemas serialize deterministically
all optional values preserve availability
```

---

## Stage 2 — Supplied D1 Corpus Loader

Implement loaders for:

```text
btc_calib.csv
btc_stooq.csv
gold_calib.csv
gold_stooq.csv
nasdaq_calib.csv
nasdaq_stooq.csv
sp500_calib.csv
sp500_stooq.csv
xrp_calib.csv
xrp_stooq.csv
```

Implement:

```text
HistoricalGranularityValidator
D1 canonicalization
W1 aggregation
```

Acceptance:

```text
no malformed OHLC
date order strictly increasing
duplicates rejected
cadence anomalies identified
missing volume preserved as unavailable
```

---

## Stage 3 — Current Structural Engine Integration

User-supplied current PRAMA / D_O / ODCE / K-MEM context becomes the implementation authority.

Implement only:

```text
FinancialObservationAdapter
StructuralEngineAdapter
StructuralSnapshot serialization
```

Acceptance:

```text
same observation + same engine/config = same structural snapshot
no component logic duplicated
```

---

## Stage 4 — Historical Structural Replay

Execute the current structural stack over the validated historical corpus.

Persist:

```text
timestamp
instrument
timeframe
market close
structural snapshot
structural vector
availability mask
```

Acceptance:

```text
strict temporal order
replay deterministic
no future observation enters current state
```

---

## Stage 5 — Direction / Horizon Calibration

Build:

```text
outcome labels
first-passage times
structural neighbor index
direction probabilities
horizon distributions
held-out reliability
```

Generate versioned `ResolutionCalibrationProfile`.

Acceptance:

```text
outcomes always occur after snapshot timestamp
runtime parameters loaded only from generated profile
publication can return UNRESOLVED
```

---

## Stage 6 — D1 / W1 Financial Signal

Complete working signal for the supplied corpus first:

```text
D1
W1
cross-scale D1/W1
```

Acceptance:

```text
request → deterministic financial response
direction/horizon published only when calibrated
full provenance present
```

---

## Stage 7 — Crypto Live + Intraday

Implement:

```text
Binance Spot historical bootstrap
Binance Spot live stream
Coinbase verification adapter
M1 base series
M5/H1/H4/D1/W1 aggregation
```

Acceptance:

```text
BTC and XRP can bootstrap from provider history
all six scales update
confirmed mode uses closed bars only
stream survives reconnect
sequence/data gaps are detected
```

---

## Stage 8 — Cross-Asset Provider

Implement Massive adapter for:

```text
stocks
indices
forex
futures
```

Acceptance:

```text
new supported instrument resolves without code changes
historical bootstrap produces canonical observations
live M1 feed enters same pipeline
session calendar is respected
```

---

## Stage 9 — Universal Asset Bootstrap

Implement automatic creation of an asset profile when first queried.

Acceptance:

```text
unknown-but-supported instrument
    → resolve
    → fetch history
    → build structural history
    → select calibration scope
    → emit signal or explicit insufficient-calibration state
```

No manual per-asset YAML is required for normal operation.

---

## Stage 10 — Streaming API

Implement:

```text
one-shot signal endpoint
stream subscription
cache
health/readiness
```

Acceptance:

```text
machine client can subscribe to an asset
confirmed updates arrive without LLM mediation
```

---

## Stage 11 — Telegraph Adapter

The current public Telegraph catalog identifies `FINANCIAL_DATA` as a deterministic Tier-A / WASM Exact Match intent.

Do not modify the financial core to imitate an unknown task contract.

Implement the adapter as:

```text
FinancialSignalResponse
        ↓
Telegraph FINANCIAL_DATA adapter
        ↓
exact response contract required by Telegraph
```

When the participant task schema is available, bind:

```text
Telegraph request fields
Telegraph response fields
serialization
error contract
```

Only the adapter changes.

---

# 33. Minimum Test Suite

Required tests:

```text
asset resolver ambiguity
provider capability fallback
historical cadence validation
D1/W1 aggregation
M1/M5/H1/H4 aggregation
market-calendar boundaries
closed-bar enforcement
missing-volume availability
provider sequence gaps
structural adapter determinism
historical replay determinism
no-lookahead
outcome-label temporal causality
neighbor search determinism
probability normalization
UNRESOLVED behavior
horizon first-passage causality
cross-scale agreement
propagation
provider divergence
response hash reproducibility
unseen-asset bootstrap
```

---

# 34. Mandatory Invariants

```text
1. No LLM participates in Financial Structural PRAMAgraph Signal generation.

2. The current PRAMA / D_O / ODCE / K-MEM implementation is consumed, not rewritten.

3. Market-data provider syntax never enters the structural core.

4. Missing data remains unavailable; it is never silently replaced with zero.

5. M1, M5, H1, H4, D1, and W1 retain explicit identity.

6. The observation timeframe is not the predicted manifestation horizon.

7. Direction is a downstream calibrated financial inference.

8. Direction is not forced when empirical evidence is insufficient.

9. Horizon is derived from historical first-passage behavior of comparable structural states.

10. Runtime uses no future observations.

11. Calibration profiles are immutable during live inference.

12. Every published confirmed signal is replayable and hash-addressable.

13. A new supported asset can bootstrap without a manually authored asset configuration.

14. Provider disagreement is surfaced, not averaged away.

15. Structural state remains publishable when direction/horizon are unavailable.

16. Public probabilities use deterministic integer representation.

17. Cross-scale aggregation cannot invent evidence missing at the scale level.

18. Telegraph-specific serialization remains outside the financial structural core.
```

---

# 35. Definition of Done

`Financial Structural PRAMAgraph Signal v1` is complete when:

```text
A user or machine requests a supported asset
        ↓
the asset resolves automatically
        ↓
required history is available or bootstrapped
        ↓
live / latest closed observations are ingested
        ↓
M1 / M5 / H1 / H4 / D1 / W1 are constructed
        ↓
current PRAMA / D_O / ODCE / K-MEM execute
        ↓
each scale emits a structural snapshot
        ↓
direction and horizon are derived from calibrated prior cases
        ↓
cross-scale state is computed
        ↓
the response contains reliability + provenance
        ↓
the same input and versions replay to the same response
        ↓
the output can be consumed directly by a machine without an LLM
```

The resulting object is the canonical **Financial Structural PRAMAgraph Signal**.
