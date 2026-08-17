# Telegraph integration research

Verified on 2026-08-16 against the public testnet and official documentation.

- `FINANCIAL_DATA` is a canonical Tier-A deterministic intent evaluated through
  WASM exact match.
- The live registry described it as market data, company fundamentals, or financial
  statistics beyond a single quoted price, and reported two active miners.
- The two active miners were CoinGecko (`/price`) and Alpha Vantage (`/quote`).
- Miner integration is declarative YAML: endpoints, JSON Schemas, semantics,
  supported intents, operational limits and optional direct on-chain extraction.
- The final YAML must be hosted, SHA-256 committed and validated against the live API.

Official references:

- https://hackathon.telegraphprotocol.com/supported-intents
- https://docs.telegraphprotocol.com/docs/using/intents
- https://docs.telegraphprotocol.com/docs/using/engine-ask
- https://docs.telegraphprotocol.com/docs/miners/yaml-config
- https://docs.telegraphprotocol.com/docs/miners/miner-registration
- https://devnode.telegraphprotocol.com/engine/v1/intents
- https://devnode.telegraphprotocol.com/api/miners?intent=FINANCIAL_DATA

The participant exact-match task payload was not public during this verification.
`telegraph/miner.local.yaml` therefore isolates the provisional adapter so it can be
changed without modifying the financial structural core.
