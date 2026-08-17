# Supplied corpus

This directory contains the ten blueprint CSV files, copied byte-for-byte from
the supplied `Prama_smm/data` corpus:

- `btc_calib.csv`, `btc_stooq.csv`
- `gold_calib.csv`, `gold_stooq.csv`
- `nasdaq_calib.csv`, `nasdaq_stooq.csv`
- `sp500_calib.csv`, `sp500_stooq.csv`
- `xrp_calib.csv`, `xrp_stooq.csv`

The files are not synthesized. Calibration files enter as D1;
they may be aggregated causally into W1 but never expanded into intraday bars.

Run `cargo run -- audit-corpus` to verify counts, date bounds, availability and
raw SHA-256 hashes. `nasdaq_stooq.csv` contains two one-cent OHLC violations
(2012-10-09 and 2017-07-21); the historical validator excludes those records
explicitly and reports `ACCEPTED_WITH_EXCLUSIONS`. The source bytes are never
rewritten.
