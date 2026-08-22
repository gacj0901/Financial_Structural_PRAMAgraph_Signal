# FINANCIAL STRUCTURAL PRAMAGRAPH SIGNAL — CUSTODY MANIFEST

This file records immutable public compatibility boundaries and the status of development
evidence. It does **not** freeze ordinary implementation work and it does not promote any
calibration profile. Internal fixes may be deployed without a new Telegraph registration
provided every public invariant below remains unchanged.

## Public Telegraph registration

- Miner ID: `111`
- Slug: `financial-structural-pramagraph-signal`
- Name: `Financial Structural PRAMAgraph Signal`
- Protocol: `generic`
- Intent: `FINANCIAL_DATA`
- Registration date shown by Telegraph Explorer: `2026-08-21`
- Base URL: `https://pramagraph-financial-h1-production.up.railway.app`
- External endpoint: `POST /v1/telegraph/financial-data`
- Registered timeframes: `D1`, `W1`
- Registered sources: `AUTO`, `SUPPLIED_CORPUS`
- On-chain transform: `direct`
- Minimum price: `0.01 USDC`
- Registered YAML byte SHA-256:
  `sha256:f7e1fba0d171979cd7c4881ac9058405e070ca9bf6b9e0edd2894f9497d323d0`

The registered description defines the top-level `label` as conventional deterministic
price-state analysis combined with an **independent** PRAMAgraph structural reading. D_O,
ODCE and K-MEM observables are not silently mapped into price direction.

### Compatibility invariants

The following require a registration review before alteration:

1. miner ID, slug, protocol, intent, name or base URL;
2. external path or HTTP method;
3. D1/W1 input shape and required response fields;
4. on-chain source paths `instrument.base`, `timeframe`, `label`, and `as_of_ns`;
5. the declared interpretation of the top-level label.

Bug fixes, provider hardening, provenance corrections, regenerated schemas, readiness checks,
development diagnostics and fail-closed calibration changes do not alter those invariants.

## Source-control history

- Historical tag: `financial-v0.1.0-freeze`
- Tag target observed before the current development update:
  `8bd12a2e6bf79871bc6203c1adc39983ff24e152`
- The earlier `c1ebc99` commit does **not** contain the complete nine-profile package and must
  not be used as the reproduction target for that package.
- The tag is historical evidence only; the current development working tree is not represented
  by that tag until the user deliberately creates a later commit/tag.

No new version, commit, tag or release is created by ordinary development updates.

## Calibration-protocol identity

- Protocol ID: `financial_first_passage_weighted_neighbors_v2`
- Schema: `pramagraph.calibration_protocol.v1`
- Structural vector: `financial_structural_vector_v2`
- Engine:
  `prama-protokol-rs/0.3.0@ddb91cad+D_O_v9-financial-adapter-v2+ODCE-v0.1-financial-normalization-v1+K-MEM-K1-tau32`
- Registered protocol byte identity:
  `sha256:d715790d6ce60d0f53a0672becc2bad1d354cd51b2fbb46c17eaedbcf54ea740`
- Historical development-data marker:
  `1755734400000000000` (`2025-08-21T00:00:00Z`)
- Conservative Telegraph evidence boundary:
  `1787443200000000000` (`2026-08-23T00:00:00Z`). Telegraph Explorer exposes the
  registration date, not an exact instant; this boundary is safely after the displayed date
  in every civil timezone.

The legacy numeric metadata value `1766304000000000000` was previously described as
`2026-07-21T00:00:00Z`; that description was false (the number represents
`2025-12-21T08:00:00Z`). It is retained only to preserve the registered protocol byte/hash
identity and is not accepted as evidence of a preregistration date.

`--preregistered-protocol-sha256` must equal the exact protocol SHA above. Supplying the hash
does not transform old data into prospective evidence. Every outcome used for parameter
selection must mature by the conservative boundary, and an untouched evaluation segment must
begin strictly after it before it can be considered prospective.

## Development calibration profiles

All committed profiles currently state:

```text
parameters_selected_on: TEMPORAL_VALIDATION
profile_eligible_for_publication: false
untouched_test: false
evidence_status: DEVELOPMENT_AUDIT_CONSUMED
```

| Profile | Raw-file SHA-256 |
|---|---|
| `crypto_binance_BTCUSDT_D1.resolution.json` | `a4a833a86a77854102a2ed438a77268e17ff19cb94be233be71510807188a667` |
| `crypto_binance_BTCUSDT_W1.resolution.json` | `6626882402419b505e13158e026dc1f6a3f028738949f7a4c626d0578240f8c1` |
| `crypto_binance_XRPUSDT_D1.resolution.json` | `2393b896344cf04a728c86d3f90442a6c1955403835bae17a4683966b5f053bf` |
| `crypto_binance_XRPUSDT_W1.resolution.json` | `a630df6c91c618177d21c1132b3ac3729070898584ff5274ba2d4969b29d6f3d` |
| `futures_massive_GC_D1.resolution.json` | `d7959b8e2f1187499c80d22d66006fff1776bbb91e52bde44c7f2c7acf820876` |
| `futures_massive_GC_W1.resolution.json` | `9deb8ef6bb8da4b76f8b5c52e7cf56a19371c1720a12a8db4ca35553021fe6b6` |
| `index_massive_NDX_D1.resolution.json` | `16e5a907ac00641349b2a16f816f7ad6d1a4bf9c062e107a3f70730d4796f666` |
| `index_massive_SPX_D1.resolution.json` | `5ef3ddb9cf571495c554228484624a5eb5a6eed44245ee222a528b2e868702e3` |
| `index_massive_SPX_W1.resolution.json` | `ef05903d9df665fe0b7c109913de20d88b34bd5f07cb3dff35ed57a2be6d26a` |

These hashes identify the currently committed profile bytes; they are not performance claims.

## Diagnostic lineage

The detailed BTC diagnostics in `results/diagnostics/` were regenerated on 2026-08-21 against
the current BTC D1 profile hash
`sha256:e985d84b5583f7a17970955a764424c915d9279e17d79811b5ee82709d3e2f1f`.
They are development diagnostics, not prospective production evidence. The JSONL neighbor
dump is bound to the same run through `neighbor_anatomy_summary.json`. Diagnostics must be
regenerated whenever the profile identity changes.

## Reproducibility and validation

The toolchain is pinned by `rust-toolchain.toml`; the Docker builder and CI use the same Rust
release. The PRAMA dependency is pinned to Git revision
`ddb91cad792fed3674aa81a5650fab6c187fc1a5`.

Standard validation:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo run -- schema --output schemas
cargo run -- audit-corpus --input data/corpus --output results/corpus-audit.json
```

Validation results must be recorded only after those commands are actually executed against
the current working tree. Historical `96 PASS` statements are not evidence for later source
states.

## Publication rule

No current profile may publish calibrated direction or horizon. Future publication requires,
at minimum:

1. exact protocol identity;
2. post-registration untouched evidence;
3. positive probabilistic skill;
4. adequate coverage and minimum support;
5. per-predicted-class reliability, including RANGE;
6. instrument, timeframe, engine and structural-vector identity checks.

Until those conditions hold, the calibrated resolver remains `UNRESOLVED` and the registered
technical/structural response continues to operate independently.
