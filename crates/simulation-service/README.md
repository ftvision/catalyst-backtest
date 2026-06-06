# catalyst-simulation-service

HTTP wrapper (Axum) around the Rust simulation engine. The HTTP boundary uses the
shared contract types end to end. The service does **no** market-data *fetching*;
market data is supplied either inline or by reference to the Parquet store.

## Endpoints

### `GET /health`

```json
{ "status": "ok", "service": "catalyst-simulation-service" }
```

### `POST /simulate`

Provide **exactly one** of `market_data` (inline) or `market_data_ref` (read from
the Parquet store directly — issue #29, so bulk data doesn't cross the wire):

```json
// inline bundle
{ "graph": {...}, "config": {...}, "policy": {"profile": "strict_v1"},
  "market_data": { ... } }

// by reference (the service loads candles/funding/... from the Parquet store)
{ "graph": {...}, "config": {...}, "policy": {"profile": "strict_v1"},
  "market_data_ref": {
    "root": "data/market-data",
    "data_requirements": { "candles": [{"venue": "base", "symbol": "ETH"}] }
  } }
```

Responses:

- `200` — a `SimulationTrace`.
- `400 invalid_request` — body didn't match the contract, or neither/both of `market_data`/`market_data_ref` supplied.
- `422 simulation_error` — the engine rejected the run (e.g. unknown interval).
- `422 data_load_error` — a `market_data_ref` could not be read from the store.

Errors are structured:

```json
{ "error": { "code": "simulation_error", "message": "config error: unknown interval \"3w\"" } }
```

## Running locally

```bash
cargo run -p catalyst-simulation-service
# listening on http://127.0.0.1:8080  (override with CATALYST_SIM_BIND)

curl -s localhost:8080/health
curl -s localhost:8080/simulate -H 'content-type: application/json' -d @request.json
```

## Tests

```bash
cargo test -p catalyst-simulation-service
```

Drive the router via `tower`'s `oneshot` (no socket): health, a fixture request
returning a trace, a structured 400, and a structured 422.
