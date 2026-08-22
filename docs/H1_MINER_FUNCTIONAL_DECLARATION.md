# PRAMAgraph Financial Signal
## H1 Miner Functional Declaration

---

### 1. WHAT THE MINER IS

A deterministic financial intelligence service for the Telegraph **FINANCIAL_DATA** intent (Tier A, WASM Exact Match).

It processes closed-bar market data without an LLM and produces a machine-readable, hash-addressable financial signal with full provenance.

---

### 2. WHAT A REQUEST REPRESENTS

One request targets **one financial instrument** (asset).

The request may specify:
- **asset** (required): canonical symbol or alias (e.g., `BTC`, `XRP`, `GOLD`, `SP500`, `NASDAQ`)
- **timeframe** (optional): observation timeframe — `D1` or `W1` (default: `D1`)
- **source** (optional): data source mode — `AUTO` or `SUPPLIED_CORPUS` (default: `AUTO`)
- **venue/quote** (optional): venue/quote override (default: `auto`)

If `timeframe` is omitted, **all supported timeframes** for that asset are evaluated.

---

### 3. WHAT THE MINER RETURNS

The response separates independent analytical channels:

#### A. Technical Direction (Authoritative H1 Directional)
```
UP | DOWN | RANGE | UNAVAILABLE
```
Computed from **closed OHLCV bars only** using conventional indicators:
- EMA 20 vs EMA 50 trend
- EMA 20 slope
- MACD 12/26/9 histogram
- RSI 14 centerline (≥50 / <50)
- RANGE positively detected: ADX14 < 20 AND |EMA20-EMA50|/ATR14 < 0.5
- Tie-break (2/2 after RANGE rejected): EMA 20 slope sign

#### B. Technical Counter-Reading
Independent exhaustion/mean-reversion pressure:
- RSI extreme (≥70 → downside pressure, ≤30 → upside pressure)
- Normalized EMA extension: (close - EMA20) / ATR14
- Bollinger Bands 20, 2σ: close above upper → downside, below lower → upside
- Output: `NONE | UP | DOWN` with evidence components preserved

#### C. PRAMAgraph Structural State (Independent Channel)
Complete `StructuralSnapshot` per timeframe:
- PRAMA state (A, G, M, delta, xi, etc.)
- D_O v9: structural_state, transport_coherence, recurrence_persistence, mobility_status
- ODCE v0.1: benefit/cost vectors, differential trends
- K-MEM K1 (τ=32): strictly_prior_state, state_sha256
- Availability map per component

#### D. Structural Contrast
Descriptive co-presentation of technical direction and independent structural evidence:
| State | Meaning |
|-------|---------|
| `CONFIRMING` | Reserved; not emitted without an empirically validated mapping |
| `CONFLICTING` | Reserved; not emitted without an empirically validated mapping |
| `MIXED` | Reserved; not emitted without an empirically validated mapping |
| `NEUTRAL` | Both channels are available and remain independent |
| `UNAVAILABLE` | No structural evidence with explicit directional semantics |

The runtime exports D_O transport coherence and recurrence plus K-MEM prior state as
descriptive evidence. It does not translate any of them into price direction or let them
vote for/against the registered technical label.

#### E. Per-Scale Readings
Array of `ScaleSignal` per evaluated timeframe containing:
- Technical Direction Head
- Technical Counter-Reading
- Structural Contrast
- StructuralSnapshot
- (Optional) Calibration-based DirectionalResolution from KNN (provenance only)

#### F. Cross-Scale / Global Composition
- **Dominant direction**: majority vote among available scales (UP/DOWN/RANGE/UNAVAILABLE)
- **Agreement**: `UNANIMOUS` / `MAJORITY` / `SPLIT` / `UNAVAILABLE`
- **Agreeing/Disagreeing scales**: explicit listing
- **Dominant scale**: finest granularity among agreeing scales

#### G. Provenance / Deterministic Response Identity
- `response_sha256`: SHA-256 of canonical response (excluding the hash field itself)
- `input_window_sha256`: hash of request body
- Engine version, config hash, structural vector version, calibration profile hash
- Primary/secondary provider, observation interface version
- **Determinism guarantee**: same closed market data + same runtime/config = identical response + identical `response_sha256`

---

### 4. CRITICAL SEMANTIC DISTINCTION

> **The technical direction is NOT inferred from PRAMA structural state.**
>
> PRAMAgraph structural state and price-direction classification are **independent analytical channels**.
>
> The miner does NOT claim that structural state predicts price direction.
>
> - Technical Direction = market-state classification from price action
> - Structural State = independent causal/invariant characterization from PRAMAgraph
> - Structural Contrast = descriptive co-presentation; current runtime does not infer
>   confirmation or conflict between the two

---

### 5. DETERMINISM

**Guarantee supported by implementation:**
```
same closed market observations
+ same runtime/configuration
= same financial computation + identical canonical response_sha256
```

- All indicators computed from closed bars only (CONFIRMED mode)
- No LLM, no random sampling, no hidden state
- Same input → identical SHA-256 response hash (verified by tests)
- Request logging timestamps do NOT contaminate financial response hash

---

### 6. NO LLM

**No Large Language Model participates in financial signal generation.**
Signal generation is pure deterministic Rust computation from closed market data.

---

### 7. DATA AVAILABILITY

| Status | Meaning | HTTP Behavior |
|--------|---------|---------------|
| `OK` | Signal generated successfully | 200 |
| `STALE_DATA` | Served from supplied corpus (not live) | 200 + status field |
| `INSUFFICIENT_DATA` | Not enough closed bars for indicators | 400 |
| `UNSUPPORTED_ASSET` | Asset not in resolver | 400 |
| `UNSUPPORTED_TIMEFRAME` | Timeframe not in {D1, W1} | 400 |
| `ENGINE_ERROR` | Internal structural computation failure | 500 |

**Closed-bar behavior (CONFIRMED mode):**
- Only fully closed OHLCV bars used
- No live/current forming bar
- No look-ahead

**Stale-data behavior:**
- `SUPPLIED_CORPUS` source → `STALE_DATA` status
- `AUTO` with live provider → `OK` status
- Data watermark (`as_of_ns`, `data_watermark_ns`) always exposed

**Unsupported asset/timeframe:**
- Explicit error, never silent fallback
- `directional` field present in response even when calibration unavailable

---

### 8. LIMITATIONS

- **Technical Direction** is a deterministic market-state classification from closed-bar indicators — **not a guarantee of future price movement**
- **Structural Contrast** preserves independent structural evidence — **not financial advice**
- **Stale or insufficient source data** is surfaced via explicit status — never hidden behind silent defaults
- Only **actually supported timeframes** (D1, W1) and **actually resolvable assets** are returned
- **No forward-looking prediction**: the signal classifies current market state from closed history
- **Calibration-based KNN directional** is optional provenance only — not the primary signal
- **Profile Brier skill gate** must be positive vs. climatology for production publication;
  all current profiles remain `DEVELOPMENT_AUDIT_CONSUMED` and non-publicable.

---

### STATUS SUMMARY

| Component | Status |
|-----------|--------|
| Technical Direction Head | ✅ IMPLEMENTED |
| Technical Counter-Reading | ✅ IMPLEMENTED |
| Structural Contrast | ✅ IMPLEMENTED |
| Per-Scale Readings (D1, W1) | ✅ IMPLEMENTED |
| Cross-Scale Composition | ✅ IMPLEMENTED |
| Native Endpoint (`/v1/financial/signal`) | ✅ IMPLEMENTED |
| Telegraph Adapter (`/v1/telegraph/financial-data`) | ✅ IMPLEMENTED |
| Health/Live + Health/Ready | ✅ IMPLEMENTED |
| Request Logging (JSONL + console) | ✅ IMPLEMENTED |
| Deterministic SHA-256 Response Hash | ✅ IMPLEMENTED |
| Nine D1/W1 Calibration Profiles | ⚠️ `DEVELOPMENT_AUDIT_CONSUMED`, non-publicable |
| Live Provider (Binance/Massive) | ✅ IMPLEMENTED (`AUTO` mode) |
| Telegraph miner 111 contract | ✅ REGISTERED (D1/W1 `FINANCIAL_DATA`) |
