# Telegraph adapter

`miner.local.yaml` is the contract currently registered as Telegraph miner **111**,
`financial-structural-pramagraph-signal`, for the `FINANCIAL_DATA` intent. The public
registration date is 2026-08-21 and the registered base URL is
`https://pramagraph-financial-h1-production.up.railway.app`.

Compatibility rule for implementation updates:

1. Keep `id`, slug, protocol, intent, base URL, endpoint path and D1/W1 request shape stable.
2. Keep the registered required response fields and on-chain source paths stable.
3. Internal bug fixes, provenance hardening, schema regeneration and fail-closed calibration
   changes do not require a new registration while those public invariants remain unchanged.
4. Re-register only if a declared public invariant changes.

The top-level `label` is the conventional deterministic price-state classification declared
in the registration. PRAMAgraph remains an independent structural reading; a development
KNN result may be included in `directional` but cannot override publication gates.

Local run:

```powershell
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus
```

Request:

```powershell
Invoke-RestMethod -Method Post `
  -Uri http://127.0.0.1:8080/v1/telegraph/financial-data `
  -ContentType application/json `
  -Body '{"asset":"BTC","timeframe":"D1","source":"AUTO"}'
```

`AUTO` uses Binance's latest closed D1 bars for BTC/XRP and Massive REST for
stocks, indices, FX and COMEX gold when `MASSIVE_API_KEY` is present. Provider failure
falls back deterministically to the pinned corpus and returns `STALE_DATA`; an explicit
`SUPPLIED_CORPUS` request always selects that stale replay path.
