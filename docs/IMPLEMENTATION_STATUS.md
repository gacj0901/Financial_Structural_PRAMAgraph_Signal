# Implementation status

## Implemented

- Stage 1 contracts, availability semantics, basis-point validation and schema generation.
- Stage 2 generic D1 OHLCV loader, malformed/duplicate rejection, cadence diagnostics and W1 aggregation.
- Stage 3 strict adapter to pinned PRAMA Protokol Rust `0.3.0` with deterministic replay hashes.
- Stage 4 causal D1/W1 replay through `financial_observation_interface_v1`.
- Ten-file corpus audit with immutable raw hashes and explicit data-quality exclusions.
- Rust HTTP service plus Binance closed-bar bootstrap for live BTC/XRP D1/W1 responses.
- Massive REST adapter for closed D1/W1 stock, index and FX observations.
- Auditable COMEX gold contract selection and closed futures-session ingestion.
- Telegraph miner 111 registered for `FINANCIAL_DATA`, with a stable D1/W1 contract and
  generated request/response schemas.
- Causal Rust D_O v9 port with prior-only ridge transport, recurrence geometry,
  variation contraction and the v9 hysteresis state machine.
- ODCE v0.1 causal cost/benefit differentials and cumulative conversion-deficit exposure,
  adapted through strictly-prior financial magnitude references while preserving raw channels;
  absent external return channels remain explicitly unavailable.
- K-MEM K1 exponential memory at `tau=32`, exposing the strictly-prior state separately
  from the post-update state.
- Offline directional calibration with causal volatility, symmetric future-only first-passage
  labels, separate train/validation/untouched-test regions, effective-dimension masks,
  deterministic neighbors, Wilson reliability, Brier skill and canonical hashes.
- Runtime profile loading in the HTTP service with explicit `UNRESOLVED` behavior.

## Deliberately unavailable

- ODCE external integration, functional gain and verified outcome channels: no conformant
  independent source has been supplied.
- Direction/horizon for a scale without a valid immutable calibration profile.
- Live M1 stream, WebSocket and final task-specific Telegraph serialization.

`Prama_smm` contains synthetic sample-state generation. It is not an admissible source for
financial evidence or calibrated thresholds and has not been ported.

## Next acceptance gate

Nine profiles exercise the D1/W1 path and all remain development evidence. Seven currently
have positive resolved-only Brier Skill and two do not, but none is publicable and none
predicted RANGE in its resolved audit tail. The detailed BTC diagnostics were regenerated
against the current BTC D1 profile hash `sha256:e985d84b...`; they remain development evidence
and are not a prospective claim.

A future publicable profile requires the exact registered protocol hash, parameter-selection
labels matured before the conservative boundary, an untouched segment strictly after
2026-08-23T00:00:00Z, adequate coverage and
class-specific reliability. Resolved-only aggregate metrics are not sufficient by themselves.

## Observation Interface v1

For each closed bar after the first:

```text
omega[t]    = (high[t] - low[t]) / close[t-1]
expected[t] = omega[t-1]
u_lambda    = NOT_APPLICABLE
sigma_op    = NOT_APPLICABLE
```

The mapping has no fitted source constants, is invariant to price-unit scaling and uses
no future observation. Automated tests pin all three properties.
