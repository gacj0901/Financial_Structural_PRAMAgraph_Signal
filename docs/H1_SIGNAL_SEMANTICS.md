# H1 Signal Semantics

---

This document is the authoritative human-readable explanation of every signal field produced by the PRAMAgraph Financial Signal H1 Miner.

---

## Technical Direction

The **authoritative H1 directional signal** — a deterministic market-state classification from closed-bar price action only.

| Value | Meaning |
|-------|---------|
| `UP` | Market state classified as upward |
| `DOWN` | Market state classified as downward |
| `RANGE` | Market state classified as range-bound (sideways) |
| `UNAVAILABLE` | Insufficient closed bars for computation (< 60 bars) |

### How it is computed (auditable summary)

The Technical Direction Head uses **only closed OHLCV bars** and conventional indicators:

| Component | UP vote when | DOWN vote when |
|-----------|--------------|----------------|
| **EMA Trend** | EMA20 > EMA50 | EMA20 < EMA50 |
| **EMA Slope** | EMA20 slope > 0 | EMA20 slope < 0 |
| **MACD Histogram** | MACD(12,26,9) histogram > 0 | MACD histogram < 0 |
| **RSI Centerline** | RSI14 ≥ 50 | RSI14 < 50 |

**RANGE detection (positive, not mere tie):**
- ADX14 < 20 **AND** |EMA20 - EMA50| / ATR14 < 0.5
- If RANGE detected → Technical Direction = `RANGE` (overrides votes)

**Tie-break (exact 2/2 after RANGE rejected):**
- EMA20 slope sign decides

**Minimum bars:** 60 closed bars required (EMA50 needs 50, MACD needs 35, RSI 15, ADX 28, Bollinger 20). Fewer → `UNAVAILABLE`.

### Determinism
- Same closed bars → identical indicators → identical direction
- No randomness, no LLM, no hidden state
- Closed bars only (CONFIRMED mode)

---

## Technical Counter-Reading

**Independent exhaustion/mean-reversion pressure** — does NOT replace technical direction.

| Value | Meaning |
|-------|---------|
| `UP` | Upside counter-pressure detected (oversold/mean-reversion up) |
| `DOWN` | Downside counter-pressure detected (overbought/mean-reversion down) |
| `NONE` | No clear counter-pressure detected |

### Evidence components (all must align for non-NONE):

| Component | UP pressure when | DOWN pressure when |
|-----------|------------------|-------------------|
| **RSI Extreme** | RSI14 ≤ 30 | RSI14 ≥ 70 |
| **EMA Extension** | (close - EMA20) / ATR14 < -2 | (close - EMA20) / ATR14 > 2 |
| **Bollinger Position** | Close below lower band (20, 2σ) | Close above upper band (20, 2σ) |

**Priority order:** RSI Extreme → Bollinger Position → EMA Extension
- First matching component determines counter-reading
- All evidence components preserved in output

**Not a probability** — deterministic classification, not a confidence score.

---

## Structural State (PRAMAgraph)

Independent causal/invariant characterization from PRAMAgraph Protokol. **Not a price direction prediction.**

### Core Fields (from `StructuralSnapshot`)

| Field | Source | Meaning |
|-------|--------|---------|
| `structural_state` | D_O v9 | Primary structural regime label |
| `prama` | PRAMA | A, G, M, delta, xi, theta, etc. |
| `d_o` | D_O v9 | Transport coherence, recurrence, mobility |
| `odce` | ODCE v0.1 | Benefit/cost vectors, differential trends |
| `k_mem` | K-MEM K1 (τ=32) | Strictly-prior state, state_sha256 |
| `availability` | Map | Per-component AVAILABLE/UNAVAILABLE/NOT_APPLICABLE/STALE |

### D_O `structural_state` values (with explicit semantics):

| Value | Structural Meaning | Maps to Technical Direction |
|-------|-------------------|----------------------------|
| `CRYSTALLIZED` | Fully organized, stable regime | → UP |
| `RECURRENT` | Recurring organized regime | → UP |
| `VIABLE` | Viable organized regime | → UP |
| `CRYSTALLIZING` | Organizing toward crystallization | → UP |
| `PROVISIONAL` | Inherits from `mobility_status` | → inherits |
| `STAGNANT` | Inactive, no transport | → RANGE |
| `INACTIVE` | No structural activity | → RANGE |
| `DISRUPTED` | Structural disruption | → DOWN |
| `TRANSPORT_DISRUPTED` | Transport disruption | → DOWN |
| `TRANSPORT_UNRESOLVED` | Transport inconclusive | → DOWN |
| `UNRESOLVED` | No structural resolution | → DOWN |

**PROVISIONAL** inherits from D_O `mobility_status`:
- `VIABLE`/`RECURRENT`/`CRYSTALLIZING`/`CRYSTALLIZED` → UP
- `STAGNANT` → RANGE
- Other → DOWN

---

## Structural Contrast

**Agreement/conflict relation** between Technical Direction and structural evidence.

| State | Meaning |
|-------|---------|
| `CONFIRMING` | All available structural evidence aligns with technical direction |
| `CONFLICTING` | All available structural evidence opposes technical direction |
| `MIXED` | Some evidence aligns, some opposes |
| `NEUTRAL` | Evidence available but neither clearly confirming nor conflicting |
| `UNAVAILABLE` | No structural evidence with explicit directional semantics |

### Evidence sources (only explicitly available fields used):

| Structural Field | Direction Mapping | Agreement Logic |
|------------------|-------------------|-----------------|
| D_O `structural_state` | See table above | Technical dir matches mapped dir → aligned |
| D_O `transport_coherence` ≥ 0.5 | "coherent" → supports trending | Technical trending + coherent → aligned |
| D_O `recurrence_persistence` ≥ 0.3 | "recurrent" → supports trending | Technical trending + recurrent → aligned |
| K-MEM `strictly_prior_state` > 0 | UP | Technical UP + positive → aligned |
| K-MEM `strictly_prior_state` < 0 | DOWN | Technical DOWN + negative → aligned |
| K-MEM `strictly_prior_state` = 0 | RANGE | Technical RANGE + zero → aligned |

**No semantic invention** — only fields with explicit semantics in contracts/structural.rs are used.

---

## Cross-Scale Composition

For multi-timeframe requests (e.g., D1 + W1):

| Field | Computation |
|-------|-------------|
| `dominant_direction` | Majority vote among available scales (UP/DOWN/RANGE/UNAVAILABLE) |
| `agreement` | `UNANIMOUS` (all agree) / `MAJORITY` (most agree) / `SPLIT` (tie) / `UNAVAILABLE` |
| `agreeing_scales` | Timeframes agreeing with dominant |
| `disagreeing_scales` | Timeframes disagreeing with dominant |
| `dominant_scale` | Finest granularity among agreeing scales |

---

## Top-Level Label

The response `label` field = **Technical Direction** (UP/DOWN/RANGE/UNAVAILABLE)

**NOT** structural state. The label is the authoritative financial directional answer.

---

## Response Identity

- `response_sha256`: SHA-256 of canonical response (excludes the hash field itself)
- Same closed market data + same runtime/config = identical response + identical hash
- Request logging timestamps do NOT contaminate financial response hash

---

## Status Values

| Status | Meaning |
|--------|---------|
| `OK` | Signal generated successfully |
| `STALE_DATA` | Served from supplied corpus (not live provider) |
| `INSUFFICIENT_DATA` | < 60 closed bars for indicators |
| `UNSUPPORTED_ASSET` | Asset not in resolver |
| `UNSUPPORTED_TIMEFRAME` | Timeframe not in {D1, W1} |
| `ENGINE_ERROR` | Internal structural computation failure |