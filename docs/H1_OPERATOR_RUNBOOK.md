# H1 Miner Operator Runbook

---

Quick operational reference for running the PRAMAgraph Financial Signal H1 Miner.

---

## 1. Start the Miner

```powershell
# From repository root
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles
```

With live provider (requires Massive API key):
```powershell
$env:MASSIVE_API_KEY = '<your key>'
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles
```

**Expected startup output:**
```
Finished `dev` profile [unoptimized + debuginfo] target(s) in X.XXs
Running `target\debug\pramagraph-financial.exe serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles`
```

---

## 2. Verify `/health/live`

```powershell
curl http://127.0.0.1:8080/health/live
```
**Expected:** `{ "status": "OK" }` — Process is running.

---

## 3. Verify `/health/ready`

```powershell
curl http://127.0.0.1:8080/health/ready
```

**Ready (HTTP 200):** `{ "status": "READY" }` — Corpus audit passed, service can serve requests.

**Not ready (HTTP 503):** `{ "status": "NOT_READY" }` — Corpus audit failed or not initialized.

---

## 4. Send One Local Test Request

### Native endpoint (multi-timeframe):
```powershell
curl -X POST http://127.0.0.1:8080/v1/financial/signal `
  -H "Content-Type: application/json" `
  -d '{"asset":"BTC","timeframes":["D1","W1"]}'
```

### Telegraph adapter (single timeframe):
```powershell
curl -X POST http://127.0.0.1:8080/v1/telegraph/financial-data `
  -H "Content-Type: application/json" `
  -d '{"asset":"BTC","timeframe":"D1"}'
```

**Expected:** HTTP 200 with JSON response containing `label: "UP"|"DOWN"|"RANGE"|"UNAVAILABLE"`, `structural`, `technical`, `counter_reading`, `structural_contrast`, `response_sha256`.

---

## 5. Inspect Response

Key fields to verify:
- `label` = `"UP" | "DOWN" | "RANGE" | "UNAVAILABLE"` (authoritative technical direction)
- `status` = `"OK"` (or `"STALE_DATA"` if from supplied corpus)
- `technical.direction` = UP/DOWN/RANGE/UNAVAILABLE
- `technical.votes` = {ema_trend, ema_slope, macd, rsi_centerline}
- `counter_reading.direction` = UP/DOWN/NONE
- `structural_contrast.state` = CONFIRMING/CONFLICTING/MIXED/NEUTRAL/UNAVAILABLE
- `response_sha256` = present and deterministic

---

## 6. Watch REQUEST_RECEIVED

```powershell
# In terminal running the server, you'll see:
[2026-08-17 16:17:08.022] REQUEST_RECEIVED /v1/telegraph/financial-data asset=BTC timeframes=["D1"] body_hash=sha256:...
```

Or tail the log file:
```powershell
Get-Content results\runtime\request_events.ndjson -Wait -Tail 10
```

---

## 7. Watch REQUEST_SERVED

Console output:
```
[2026-08-17 16:17:09.306] REQUEST_SERVED req_id status=Ok http=200 asset=BTCUSDT timeframes=["D1"] elapsed_ms=1282 resp_hash=sha256:7a6416f51a2e33bea77628925a88b762fc52cbac986ade1cc55abc68a2845e2c
```

Log file entry:
```json
{"event":"REQUEST_SERVED","timestamp":"2026-08-17T16:17:09.306Z","request_id":"...","http_status":200,"pramagraph_status":"Ok","asset":"BTCUSDT","returned_timeframes":["D1"],"elapsed_ms":1282,"response_sha256":"sha256:7a6416f51a2e33bea77628925a88b762fc52cbac986ade1cc55abc68a2845e2c"}
```

---

## 8. Identify REQUEST_FAILED

Console output:
```
[2026-08-17 16:19:02.632] REQUEST_FAILED req_id http=400 error=asset resolution failed: UnsupportedAsset { query: "INVALID_ASSET" } elapsed_ms=0
```

Common failure causes:
| HTTP | Error | Cause |
|------|-------|-------|
| 400 | `asset resolution failed` | Unknown asset symbol |
| 400 | `timeframe not in {D1, W1}` | Unsupported timeframe |
| 400 | `insufficient bars` | < 60 closed bars in corpus |
| 500 | `engine error` | Internal structural failure |

---

## 9. Locate Request Events Log

**File:** `results/runtime/request_events.ndjson`

```powershell
# View last 10 lines
Get-Content results\runtime\request_events.ndjson -Tail 10

# Follow live
Get-Content results\runtime\request_events.ndjson -Wait
```

Each line is a complete JSON event (REQUEST_RECEIVED, REQUEST_SERVED, REQUEST_FAILED).

---

## 10. Stop/Restart the Service

**Stop:** `Ctrl+C` in the server terminal

**Restart:**
```powershell
# Kill any existing process on port 8080 first
netstat -ano | findstr :8080
taskkill /PID <PID> /F

# Restart
cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles
```

---

## 11. Recognize STALE_DATA vs Actual Service Failure

| Signal | Meaning | Action |
|--------|---------|--------|
| `status: "STALE_DATA"` | Served from supplied corpus (not live provider) | **Normal** if using `SUPPLIED_CORPUS` or no live key. Data is valid but not real-time. |
| `status: "OK"` with `STALE_DATA` | Never happens — mutually exclusive | N/A |
| `status: "INSUFFICIENT_DATA"` | < 60 bars for indicators | Add more corpus data or use live provider |
| `status: "UNSUPPORTED_ASSET"` | Asset not in resolver | Check asset spelling / resolver |
| `status: "ENGINE_ERROR"` | Internal structural failure | Check logs, restart service |

**Key distinction:**
- `STALE_DATA` = **data freshness warning**, service is healthy
- `ENGINE_ERROR` / 500 = **service failure**, needs investigation

---

## Quick Reference Card

| Task | Command |
|------|---------|
| Start | `cargo run -- serve --bind 127.0.0.1:8080 --corpus data\corpus --calibration calibration\profiles` |
| Health live | `curl http://127.0.0.1:8080/health/live` |
| Health ready | `curl http://127.0.0.1:8080/health/ready` |
| Test native | `curl -X POST .../v1/financial/signal -d '{"asset":"BTC","timeframes":["D1","W1"]}'` |
| Test Telegraph | `curl -X POST .../v1/telegraph/financial-data -d '{"asset":"BTC","timeframe":"D1"}'` |
| View logs | `Get-Content results\runtime\request_events.ndjson -Tail 20` |
| Kill port 8080 | `taskkill /PID $(netstat -ano | findstr :8080 | awk '{print $5}') /F` |