# Telegraph adapter

`miner.local.yaml` follows the Telegraph Miner Standard documented on
2026-08-16. It is intentionally a local template, not a registration artifact.

Before registration:

1. Deploy the Rust HTTP service behind a stable HTTPS URL.
2. Replace YAML `id: 0` and `base_url` with the deployed values.
3. Recheck the live canonical intent registry for `FINANCIAL_DATA`.
4. Validate the YAML and live endpoint at `integrate.telegraphprotocol.com`.
5. Hash the final raw YAML bytes with SHA-256 and do not modify them afterward.

The current endpoint contract is isolated here because the hackathon participant
task schema has not yet been published. Only this adapter should change when the
exact-match task payload becomes available.

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
stocks, indices, FX and COMEX gold when `MASSIVE_API_KEY` is present in the
server environment. A missing credential or provider error fails explicitly;
use `SUPPLIED_CORPUS` only when a deliberate stale replay is required.
