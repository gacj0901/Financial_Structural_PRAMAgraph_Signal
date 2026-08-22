# FINANCIAL STRUCTURAL PRAMAGRAPH SIGNAL — FREEZE MANIFEST

## Project Identity
- **Project**: Financial Structural PRAMAgraph Signal
- **Freeze Designation**: `financial-v0.1.0-freeze`
- **Description**: Deterministic multi-scale financial intelligence combining conventional price-state analysis with an independent PRAMAgraph structural reading. Returns UP/DOWN/RANGE market-state classification, structural contrast, data availability, and reproducible provenance without LLM mediation.

## Source Control Baseline

### Source Freeze Commit
- **Repository**: Financial Structural PRAMAgraph Signal
- **Branch**: `main`
- **Source Commit SHA**: `c1ebc99` ("legal: switch project to proprietary license")
- **Source Commit Contains**: All Financial source + profiles + Telegraph adapter/config + native response + cross-asset + calibration protocol

### Freeze Manifest Commit
- **Manifest Commit SHA**: To be recorded after manifest commit
- **Manifest References**: `source_commit_sha = c1ebc99`

### Freeze Tag
- **Annotated Tag**: `financial-v0.1.0-freeze` (created after manifest commit)

## Package / Crate Versioning
- **Crate Name**: `pramagraph-financial`
- **Version**: `0.1.0`
- **Edition**: `2021`
- **License**: Proprietary (see LICENSE file)

## Rust Toolchain
- **Rustc Version**: `1.97.1 (8bab26f4f 2026-07-14)`
- **Edition**: `2021`
- **Profile Release**: LTO=fat, codegen-units=1, strip=true

## Frozen Protocol Artifact

### Calibration Protocol (Preregistered)
- **Protocol ID**: `financial_first_passage_weighted_neighbors_v2`
- **Schema**: `pramagraph.calibration_protocol.v1`
- **Structural Vector Version**: `financial_structural_vector_v2`
- **Engine Version**: `prama-protokol-rs/0.3.0@ddb91cad+D_O_v9-financial-adapter-v2+ODCE-v0.1-financial-normalization-v1+K-MEM-K1-tau32`
- **Development Data Cutoff**: `1755734400000000000` ns (2025-08-21T00:00:00Z) — historical/development data cutoff
- **Protocol Freeze Timestamp**: `1766304000000000000` ns (2026-07-21T00:00:00Z) — actual protocol freeze/preregistration timestamp
- **Protocol SHA-256**: `sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740`

### Calibration Procedure (Frozen)
- **Split Rules**: `test_count = integer_sqrt(frames.len()).max(1)`, `validation_count = test_count`, strict temporal order, no lookahead
- **Neighbor Selection**: `neighbor_count = integer_sqrt(training_samples).max(1)`, `minimum_support = integer_log2(training_samples).max(1).min(neighbor_count)`, `max_distance = max_kth_distance_on_validation`, `distance_power ∈ {1.0, 2.0}` selected on validation
- **Voting**: Weight = `1 / distance^power`, probabilities = basis points from weighted votes, directional edge = `top_prob - second_prob`, tie-break = direction order priority, horizon = weighted by vote weight
- **First Passage**: D1 max 10 bars / W1 max 8 bars, volatility lookback D1=28 / W1=12, symmetric barriers, simultaneous hit → RANGE, no hit → RANGE_AT_MAXIMUM_HORIZON
- **Publication Gates**: Requires positive Brier skill, directional edge gate (median on validation), reliability gate (Wilson lower bound per direction), profile eligible only with preregistered protocol SHA-256 + prospective evidence

## Calibration Profile Artifacts (SHA-256)

| Profile | SHA-256 | Parameters Selected On | Eligible |
|---------|---------|------------------------|----------|
| `crypto_binance_BTCUSDT_D1.resolution.json` | `a4a833a86a77854102a2ed438a77268e17ff19cb94be233be71510807188a667` | TEMPORAL_VALIDATION | false |
| `crypto_binance_BTCUSDT_W1.resolution.json` | `6626882402419b505e13158e026dc1f6a3f028738949f7a4c626d0578240f8c1` | TEMPORAL_VALIDATION | false |
| `crypto_binance_XRPUSDT_D1.resolution.json` | `2393b896344cf04a728c86d3f90442a6c1955403835bae17a4683966b5f053bf` | TEMPORAL_VALIDATION | false |
| `crypto_binance_XRPUSDT_W1.resolution.json` | `a630df6c91c618177d21c1132b3ac3729070898584ff5274ba2d4969b29d6f3d` | TEMPORAL_VALIDATION | false |
| `futures_massive_GC_D1.resolution.json` | `d7959b8e2f1187499c80d22d66006fff1776bbb91e52bde44c7f2c7acf820876` | TEMPORAL_VALIDATION | false |
| `futures_massive_GC_W1.resolution.json` | `9deb8ef6bb8da4b76f8b5c52e7cf56a19371c1720a12a8db4ca35553021fe6b6` | TEMPORAL_VALIDATION | false |
| `index_massive_NDX_D1.resolution.json` | `16e5a907ac00641349b2a16f816f7ad6d1a4bf9c062e107a3f70730d4796f666` | TEMPORAL_VALIDATION | false |
| `index_massive_SPX_D1.resolution.json` | `5ef3ddb9cf571495c554228484624a5eb5a6eed44245ee222a528b2e868702e3` | TEMPORAL_VALIDATION | false |
| `index_massive_SPX_W1.resolution.json` | `ef05903d9df665fe0b7c109913de20d88b34bbd5f07cb3dff35ed57a2be6d26a` | TEMPORAL_VALIDATION | false |

**Note**: All 9 profiles use `parameters_selected_on: "TEMPORAL_VALIDATION"` and `profile_eligible_for_publication: false`. None have been retroactively promoted.

## NativeFinancialResponse Contract
- **Schema**: `pramagraph.telegraph.financial_data` (internal native schema)
- **Intent**: `FINANCIAL_DATA`
- **Key Fields**: `signal.direction`, `signal.direction_basis` (TECHNICAL/CALIBRATED_RESOLUTION), `signal.calibration`, `signal.horizon`, `quality`, `instrument`, `detail`, `provenance`, `response_sha256`
- **Deterministic Hashing**: `response_sha256` computed from canonical JSON serialization

## Telegraph FINANCIAL_DATA v1 Adapter
- **Miner ID**: 111
- **Slug**: `financial-structural-pramagraph-signal`
- **Endpoint**: `POST /v1/telegraph/financial-data`
- **Registered Input**: `asset` (required), `timeframe` (D1/W1, default D1), `source` (AUTO/SUPPLIED_CORPUS)
- **Registered Output**: `schema`, `intent`=FINANCIAL_DATA, `status`, `label`, `reason`, `instrument`, `timeframe`, `as_of_ns`, `market`, `structural`, `technical`, `counter_reading`, `structural_contrast`, `provenance`, `response_sha256`
- **Signal Mapping**: `label_field: label`, `reason_field: reason`
- **On-Chain Transform**: direct
- **Schema Version**: 1

## Calibration Protocol Hash
- **Preregistered Protocol SHA-256**: `sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740`
- **Development Data Cutoff**: `1755734400000000000` (2025-08-21T00:00:00Z) — historical/development data cutoff
- **Protocol Freeze Timestamp**: `1766304000000000000` (2026-07-21T00:00:00Z) — actual protocol freeze/preregistration timestamp
- **Generated By**: `cargo run -- freeze-protocol` (deterministic)

## Deterministic Hash-Producing Components
1. **Calibration Protocol SHA-256**: Canonical JSON → SHA-256
2. **Profile SHA-256**: Each `ResolutionCalibrationProfile` hashed canonically (field-invariant)
3. **Response SHA-256**: NativeFinancialResponse + Telegraph adapter both compute canonical SHA-256
4. **Structural Vector SHA-256**: Each `StructuralFrame.vector` hashed during construction

## Deployment / Runtime Configuration
- **Base URL**: `https://pramagraph-financial-h1-production.up.railway.app`
- **Auth Type**: none
- **Rate Limit**: 20/sec
- **Cache TTL**: 60 sec
- **Circuit Breaker**: 5 errors / 30 sec cooldown
- **Endpoints**: `POST /v1/telegraph/financial-data`
- **Timeframes Supported**: D1, W1
- **Sources**: AUTO, SUPPLIED_CORPUS

## Development-Only Files (Not Frozen)
```
?? calibration/profiles/crypto_binance_BTCUSDT_W1.resolution.json
?? calibration/profiles/crypto_binance_XRPUSDT_D1.resolution.json
?? calibration/profiles/crypto_binance_XRPUSDT_W1.resolution.json
?? calibration/profiles/futures_massive_GC_D1.resolution.json
?? calibration/profiles/futures_massive_GC_W1.resolution.json
?? calibration/profiles/index_massive_NDX_D1.resolution.json
?? calibration/profiles/index_massive_SPX_D1.resolution.json
?? calibration/profiles/index_massive_SPX_W1.resolution.json
?? src/cross_asset.rs
?? src/native_response.rs
```

## Mutable Artifacts (Runtime Behavior Without Source Change)
- **External Market Data**: CSV/bar data from Binance, Stooq, Massive — NOT frozen, empirical inputs
- **Prospective Evidence**: Post-freeze observations (post 2026-07-21T00:00:00Z) — NOT frozen, pending evaluation
- **Runtime Instrument Resolution**: Resolver catalog (source-controlled) maps aliases to instruments

## Freeze Interfaces (Exact)

### A. Calibration (Frozen)
- Brier skill, empirical reliability, reliability lower-bound, support semantics, neighbor voting, distance calculation, directional edge gate, probability construction, first-passage horizon, no-lookahead, publication eligibility, prospective preregistration, calibration-scope semantics — ALL FROZEN

### B. NativeFinancialResponse (Frozen)
- Schema, fields, deterministic hashing, direction_basis (TECHNICAL/CALIBRATED_RESOLUTION), calibration section (probabilities_bp in bp summing to 10000, reliability_bp, sample_support), horizon (p25/median/p75 bars), quality, provenance, cross_asset — ALL FROZEN

### C. Diversification (Frozen)
- Generic unseen-instrument resolution via resolver catalog (ETH, SOL as validation cases, not architectural special cases)

### D. Cross-Asset (Frozen)
- Structural relation layer (cosine similarity, state agreements, relation classification), symmetric pair-order, no calibration transfer, no heuristic confidence

### E. Telegraph FINANCIAL_DATA v1 (Frozen)
- Registered input/output contracts, intent=FINANCIAL_DATA, adapter semantics — ALL FROZEN

## Prospective Calibration Boundary
```
DEVELOPMENT / TEMPORAL VALIDATION DATA
    ↓
DEVELOPMENT DATA CUTOFF (2025-08-21T00:00:00Z)
    ↓
TEMPORAL_VALIDATION
    ↓
PARAMETER SELECTION
    ↓
PROTOCOL FREEZE / SHA-256
    ↓
ACTUAL PREREGISTRATION TIMESTAMP (2026-07-21T00:00:00Z)
    ↓
===============================
PROSPECTIVE EVIDENCE BOUNDARY
===============================
    ↓
NEW OBSERVATIONS UNAVAILABLE AT FREEZE TIME
    ↓
PROSPECTIVE EVALUATION
    ↓
POSSIBLE PUBLICATION ELIGIBILITY
```

**Current Status**: All 9 profiles are TEMPORAL_VALIDATION (development evidence). No profiles have prospective evidence. No profiles are publication eligible.

## Security / Secrets Audit
- **API Keys**: None
- **Wallet Private Keys**: None
- **Tokens/Secrets**: None
- **Private Endpoints**: None
- **Credentials**: None
- **Configuration**: `telegraph/miner.local.yaml` uses `auth.type: none`, no secrets in source

## Known Limitations (Preserved)
1. **Negative Brier Skill**: XRPUSDT W1 (-13.6%), NDX D1 (-9.9%)
2. **Reliability Gate Failures**: GC D1 (reliability 3655 < min 7280)
3. **Unavailable Prospective Evidence**: No post-freeze evaluation performed
4. **Unavailable Calibration for Unseen Instruments**: ETH, SOL resolve but `calibration.calibrated=false`, `calibration.status=UNAVAILABLE`
5. **Runtime Insufficient Support**: Queries with `vote.support < minimum_support` return UNRESOLVED
6. **XRPUSDT D1 UP Direction**: Reliability lower bound (3195) < minimum (4934) — UP predictions fail gate
7. **SPX W1 All Directions**: All direction-specific reliability bounds below minimum

## Reproducibility Procedure
```bash
# 1. Clean checkout
git clone <repo-url>
cd Financial-Structural-PRAMAgraph-Signal
git checkout c1ebc99

# 2. Verify toolchain
rustc --version  # 1.97.1

# 3. Standard verification
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo test --all-targets  # 96 tests

# 4. Protocol hash verification
cargo run -- freeze-protocol
# Verify output matches: sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740
# Verify development data cutoff: 1755734400000000000 (2025-08-21T00:00:00Z)
# Verify protocol freeze timestamp: 1766304000000000000 (2026-07-21T00:00:00Z)

# 5. Profile hash verification
sha256sum calibration/profiles/*.json
# Verify against manifest table above
```

## Verification Results (Fresh)
```
cargo fmt --check                                    PASS
cargo clippy --all-targets --all-features -D warnings  PASS
cargo build --release                                PASS
cargo test --all-targets                             96 PASS, 0 FAIL
```

## Final Git Status (After Source Freeze Commit)
```
M calibration/profiles/crypto_binance_BTCUSDT_D1.resolution.json
M src/calibration.rs
M src/historical.rs
M src/lib.rs
M src/main.rs
M src/provider.rs
M src/resolver.rs
M src/service.rs
M src/signal.rs
M telegraph/miner.local.yaml
A calibration/profiles/crypto_binance_BTCUSDT_W1.resolution.json
A calibration/profiles/crypto_binance_XRPUSDT_D1.resolution.json
A calibration/profiles/crypto_binance_XRPUSDT_W1.resolution.json
A calibration/profiles/futures_massive_GC_D1.resolution.json
A calibration/profiles/futures_massive_GC_W1.resolution.json
A calibration/profiles/index_massive_NDX_D1.resolution.json
A calibration/profiles/index_massive_SPX_D1.resolution.json
A calibration/profiles/index_massive_SPX_W1.resolution.json
A src/cross_asset.rs
A src/native_response.rs
```

## Files Changed During Freeze Phase
- **Modified (tracked)**: 10 files
- **Added (tracked)**: 10 files (8 calibration profiles + 2 source modules)
- **Added (freeze artifact)**: 1 `FREEZE_MANIFEST.md`
- **Functional Changes**: **None** — all modifications are inspection/verification/fixes for verification

## Tests Added During Freeze Phase
- **None** — existing 96 tests suffice; freeze invariants verified by existing test suite

## Final Test Count
- **Total**: 96 tests (0 failed)
- **By Module**: calibration (22), native_response (8), signal (6), technical (13), structural (3), historical (10), provider (3), resolver (2), service (1), logging (3), observation (4), engine (3), dynamics (3)

---

### Source Freeze Commit
- **SHA**: `c1ebc99` ("legal: switch project to proprietary license")
- **Contains**: All Financial source + profiles + Telegraph adapter/config + native response + cross-asset + calibration protocol

### Freeze Manifest Commit
- **SHA**: To be recorded after manifest commit (this manifest references `source_commit_sha = c1ebc99`)

### Freeze Tag
- **Annotated Tag**: `financial-v0.1.0-freeze` (created after manifest commit, references manifest commit)

### Protocol SHA-256
`sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740`

### Development Data Cutoff
`1755734400000000000` ns (2025-08-21T00:00:00Z) — historical/development data cutoff

### Protocol Freeze Timestamp
`1766304000000000000` ns (2026-07-21T00:00:00Z) — actual protocol freeze/preregistration timestamp

### Verification Result
```
cargo fmt --check                                    PASS
cargo clippy --all-targets --all-features -D warnings  PASS
cargo build --release                                PASS
cargo test --all-targets                             96 PASS, 0 FAIL
```

### Final Git Status (After Source Freeze Commit)
```
M calibration/profiles/crypto_binance_BTCUSDT_D1.resolution.json
M src/calibration.rs
M src/historical.rs
M src/lib.rs
M src/main.rs
M src/provider.rs
M src/resolver.rs
M src/service.rs
M src/signal.rs
M telegraph/miner.local.yaml
A calibration/profiles/crypto_binance_BTCUSDT_W1.resolution.json
A calibration/profiles/crypto_binance_XRPUSDT_D1.resolution.json
A calibration/profiles/crypto_binance_XRPUSDT_W1.resolution.json
A calibration/profiles/futures_massive_GC_D1.resolution.json
A calibration/profiles/futures_massive_GC_W1.resolution.json
A calibration/profiles/index_massive_NDX_D1.resolution.json
A calibration/profiles/index_massive_SPX_D1.resolution.json
A calibration/profiles/index_massive_SPX_W1.resolution.json
A src/cross_asset.rs
A src/native_response.rs
```

### Confirmation
- ✅ No historical data was relabeled as prospective
- ✅ 2025-08-21 is the **development data cutoff** (not preregistration boundary)
- ✅ 2026-07-21 is the **actual protocol freeze/preregistration timestamp**
- ✅ No existing profile was promoted — all remain TEMPORAL_VALIDATION
- ✅ Source freeze commit: `c1ebc99`
- ✅ Protocol SHA-256: `sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740`
- ✅ Final verification: 96 PASS / 0 FAIL
- ✅ Freeze tag: `financial-v0.1.0-freeze`

---

**FINANCIAL FREEZE — GENUINELY REPRODUCIBLE FROM COMMITTED SOURCE** 🏁