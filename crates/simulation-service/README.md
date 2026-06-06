# catalyst-simulation-service

HTTP wrapper (Axum) around the Rust simulation engine. The HTTP boundary uses the
shared contract types end to end. The service does **no** market-data fetching —
the caller supplies a fully normalized `MarketDataBundle` in the request.

## Endpoints

### `GET /health`

```json
{ "status": "ok", "service": "catalyst-simulation-service" }
```

### `POST /simulate`

Request body:

```json
{
  "graph": { ... },              // catalyst graph
  "config": { ... },             // backtest config (start/end/interval/initial_portfolio)
  "policy": { "profile": "strict_v1" },
  "market_data": { ... }         // normalized MarketDataBundle
}
```

Responses:

- `200` — a `SimulationTrace`.
- `400 invalid_request` — body did not match the request contract.
- `422 simulation_error` — the engine rejected the run (e.g. unknown interval).

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
