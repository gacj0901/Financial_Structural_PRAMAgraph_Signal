# Financial Structural PRAMAgraph Signal

Deterministic Rust runtime for the `FINANCIAL_DATA` intent described by the
construction blueprint in this repository.

## Current milestone

The current implementation covers build stages 1–5 and a local Telegraph adapter:

- versioned, serializable financial contracts and JSON Schema emission;
- explicit availability (`AVAILABLE`, `UNAVAILABLE`, `NOT_APPLICABLE`, `STALE`);
- deterministic RFC 8785 JSON hashing and integer basis-point invariants;
- asset resolution without guessing ambiguous symbols;
- generic OHLCV CSV canonicalization, cadence diagnostics and causal D1 → W1 aggregation;
- a strict financial-to-kernel boundary consuming the certified PRAMA Protokol Rust crate;
- a frozen, unit-invariant Observation Interface using relative range and its causal lag;
- an auditable ten-file corpus manifest with raw SHA-256 hashes and explicit exclusions;
- a Rust HTTP service and Telegraph Miner Standard YAML template for `FINANCIAL_DATA`;
- Binance Spot bootstrap using only closed D1 bars for live BTC/XRP responses;
- Massive REST bootstrap using closed D1 bars for stocks, indices and FX, plus
  active-contract session bars for COMEX gold (`GC`);
- causal Rust ports of D_O v9, ODCE v0.1 and K-MEM K1 (`tau=32`), with
  versioned financial state adaptation and strictly-prior K-MEM inference;
- an immutable direction/horizon calibration profile with volatility-normalized
  first-passage labels, exact-mask distance-weighted neighbors, separate temporal
  train/validation/untouched-test regions, deterministic basis points and SHA-256 custody;
- fail-closed directional publication: structural state remains available when
  support, calibrated edge, or held-out reliability is insufficient.

No LLM participates in signal generation. Future outcomes are visible only to the offline
profile builder; runtime resolution consumes only the current structural vector and a
hash-verified profile. A generated BTC D1 profile is included for local replay and currently
rejects the final corpus state as `UNRESOLVED`: its untouched-test probabilities do not
outperform train/validation climatology under the Brier skill gate. Because this tail was
inspected during development, the artifact is explicitly marked `DEVELOPMENT_AUDIT_CONSUMED`
and is not eligible for publication.

## Kernel authority

`Cargo.toml` pins `PRAMA-Protokol-rs` to commit
`ddb91cad792fed3674aa81a5650fab6c187fc1a5`. The financial project owns the
market adapter; the kernel remains domain-blind and is not copied or rewritten here.

## Commands

```powershell
cargo test --all-targets
cargo run -- schema --output schemas
cargo run -- resolve BTC
cargo run -- validate-csv --input path\to\btc_calib.csv --instrument BTC
cargo run -- kernel-replay --input path\to\normalized_kernel_input.csv
cargo run -- audit-corpus --input data\corpus --output results\corpus-audit.json
cargo run -- replay-market --input data\corpus\btc_calib.csv --instrument BTC --timeframe D1
cargo run -- calibrate-direction --input data\corpus\btc_calib.csv --instrument BTC --timeframe D1 --output calibration\profiles\crypto_binance_BTCUSDT_D1.resolution.json
cargo run -- resolve-direction --input data\corpus\btc_calib.csv --instrument BTC --timeframe D1 --profile calibration\profiles\crypto_binance_BTCUSDT_D1.resolution.json
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles
```

For a genuinely new sealed evaluation, pass
`--preregistered-protocol-sha256 sha256:<64 lowercase hex>` only when that protocol hash
was frozen before any test outcomes were inspected. Without it the profile remains a
development audit and direction/horizon publication is disabled.

For Massive-backed `AUTO` requests, inject the credential into the process
environment. Do not place the key in source, YAML, requests, or logs:

```powershell
$env:MASSIVE_API_KEY = '<your key>'
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles
```

`SP500` maps to `I:SPX`, `NASDAQ` to `I:NDX`, and `GOLD` resolves an active
single `GC` contract with at least 20 calendar days to maturity when possible.
The exact provider ticker is returned as `provenance.provider_instrument`.

Kernel replay input columns:

```csv
timestamp_ns,omega,expected,u_lambda,sigma_op
1704067200000000000,1.0,0.9,0.0,true
```

`u_lambda` or `sigma_op` may be omitted only when their status is
`NOT_APPLICABLE`. Missing required financial-to-kernel mappings are rejected,
not converted silently to zero.

## Structural authority and adaptation

The exact source inventory, equations, financial adapter boundary, unavailable external
ODCE channels, and Ergonektim-derived custody rules are documented in
[`docs/STRUCTURAL_STACK.md`](docs/STRUCTURAL_STACK.md).

## Next milestone

1. Generate and evaluate immutable D1/W1 profiles for the remaining supported instruments.
2. Add cross-scale integration after every participating scale has valid held-out evidence.
3. Bind the final hackathon exact-match task schema when Telegraph publishes it.
4. Deploy the HTTP service and validate the final YAML against the public endpoint.

The discontinued `Prama_smm` application was reviewed only for product context. Its synthetic
signal generator and heuristic trust/risk values are deliberately not reused.
