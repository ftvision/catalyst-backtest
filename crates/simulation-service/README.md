# catalyst-simulation-service

The Catalyst **backtest service** (Axum). Per
[ADR 0001](../../docs/adr/0001-language-boundary.md) the deterministic run path is
Rust; this is the user-facing API. It orchestrates a run **in-process** — compile
(graph) → resolve policy → load/accept market data → run the engine → summarize —
using the Rust crates directly (no internal HTTP hop), and serves both the run
lifecycle and the workbench-setup endpoints.

Market data is either **inline** in the request or read from the configured
**Parquet store** (`CATALYST_STORE_ROOT`) via `catalyst-market-data-loader`. The
service does no *fetching* — ingestion (Python) writes the store; the service
reads it.

## Endpoints

### Run lifecycle
| Method & path | Purpose |
| --- | --- |
| `POST /backtests` | Compile + run + persist; returns `{ id, status }` (`201`). Uses inline `market_data` or the configured store. |
| `GET /backtests?graph_hash=` | Compact run history. |
| `GET /backtests/{id}` | Run status. |
| `GET /backtests/{id}/result` | Summarized `BacktestResult`. |
| `GET /backtests/{id}/metadata` | Graph hash, resolved policy, data coverage, warnings, timestamps. |
| `GET /backtests/{id}/events` | Event log, paginated + filterable (`type`/`node_id`/`status`/`cursor`/`limit`) → `{ items, next_cursor, total }`. |

### Workbench setup (no run created)
| Method & path | Purpose |
| --- | --- |
| `POST /backtests/preview` | Validate graph → `{ graph_hash, valid, graph_summary, data_requirements, resolved_policy, warnings }`. Invalid graphs return `valid:false` (200). |
| `POST /market-data/coverage` | Per-series coverage + warnings for `{ graph, start, end, interval }` (inline `market_data` or the store). |
| `GET /policy-profiles` | `strict_v1` / `conservative_v1` / `research_v1` + resolved policies. |

### Low-level
| Method & path | Purpose |
| --- | --- |
| `GET /health` | Liveness. |
| `POST /simulate` | Inputs in, raw `SimulationTrace` out (inline `market_data` or `market_data_ref`). |

Errors are structured: `{ "error": { "code", "message" }, "id"? }` — e.g.
`400 invalid_request`, `422 backtest_failed` / `simulation_error` / `data_load_error`.

## Running locally

```bash
CATALYST_STORE_ROOT=data/market-data cargo run -p catalyst-simulation-service
# listening on http://127.0.0.1:8080  (override bind with CATALYST_SIM_BIND)

curl -s localhost:8080/policy-profiles
curl -s localhost:8080/backtests -H 'content-type: application/json' -d @request.json
```

## Tests

```bash
cargo test -p catalyst-simulation-service
```

Drive the router via `tower`'s `oneshot` (no socket): full create→status→result→
events→metadata lifecycle, run history, preview (valid/invalid), coverage,
policy-profiles, paginated/filtered events, a failed run, and a by-store run
reading a temp Parquet store.
