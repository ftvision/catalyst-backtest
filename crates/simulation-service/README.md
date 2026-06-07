# catalyst-simulation-service

The Catalyst **backtest service** (Axum). Per
[ADR 0001](../../docs/adr/0001-language-boundary.md) the deterministic run path is
Rust; this is the user-facing API. It serves both the run lifecycle and the
workbench-setup endpoints.

Runs are **asynchronous**: `POST /backtests` only *enqueues* a job and returns
immediately (`202 Accepted`). A bounded pool of in-process workers drains the
queue and orchestrates each run — compile (graph) → resolve policy → load/accept
market data → run the engine → summarize — using the Rust crates directly (no
internal HTTP hop). Clients poll status and fetch the result once it's done. The
CPU-bound engine run goes to `spawn_blocking`, so it never ties up the async HTTP
threads. This is in-process for now; the same **submit → poll → fetch** contract
survives a future move to an external queue/worker.

Market data is either **inline** in the request or read from the configured
**Parquet store** (`CATALYST_STORE_ROOT`) via `catalyst-market-data-loader`. The
service does no *fetching* — ingestion (Python) writes the store; the service
reads it.

## Endpoints

### Run lifecycle (submit → poll → fetch)
| Method & path | Purpose |
| --- | --- |
| `POST /backtests` | Enqueue a run; returns `{ id, status: "queued" }` (`202`). `503 queue_full` when the queue is at capacity. Uses inline `market_data` or the configured store. |
| `GET /backtests?graph_hash=` | Compact run history. |
| `GET /backtests/{id}` | Run status: `queued` → `running` → `succeeded`/`failed`, with `created_at`/`started_at`/`finished_at`. |
| `GET /backtests/{id}/result` | Summarized `BacktestResult` (`200`). `409 not_ready` while `queued`/`running`; `422 backtest_failed` once a run has failed. |
| `GET /backtests/{id}/metadata` | Graph hash, resolved policy, data coverage, warnings, timestamps. |
| `GET /backtests/{id}/events` | Event log, paginated + filterable (`type`/`node_id`/`status`/`cursor`/`limit`) → `{ items, next_cursor, total }`. |

### Workbench setup (no run created)
| Method & path | Purpose |
| --- | --- |
| `POST /backtests/preview` | Validate graph → `{ graph_hash, valid, graph_summary, data_requirements, resolved_policy, warnings }`. Invalid graphs return `valid:false` (200). |
| `GET /market-data/catalog` | Configured Parquet-store series, partition spans, and warnings for setup selection. |
| `POST /market-data/coverage` | Per-series coverage for `{ graph, start, end, interval }`: start/end, `completeness_pct`, and interior `missing_ranges` against the interval grid — so a series with holes doesn't read as fully present. |
| `POST /market-data/window` | Normalized `MarketDataBundle` for `{ graph, start, end, interval }` (inline `market_data` or the configured store). |
| `GET /policy-profiles` | `strict_v1` / `conservative_v1` / `research_v1` + resolved policies. |

### Low-level
| Method & path | Purpose |
| --- | --- |
| `GET /health` | Liveness. |
| `POST /simulate` | Inputs in, raw `SimulationTrace` out (inline `market_data` or `market_data_ref`). |

Errors are structured: `{ "error": { "code", "message" }, ... }` — e.g.
`400 invalid_request`, `503 queue_full`, `409 not_ready` (with `status`),
`422 backtest_failed` / `simulation_error` / `data_load_error`.

## Configuration

| Env var | Default | Purpose |
| --- | --- | --- |
| `CATALYST_SIM_BIND` | `127.0.0.1:8080` | Listen address. |
| `CATALYST_STORE_ROOT` | _(unset)_ | Parquet store root for by-store runs/coverage (local path, `file://`, `s3://…`, `gs://…`). |
| `CATALYST_SIM_WORKERS` | `4` | Queue-draining worker tasks. |
| `CATALYST_SIM_QUEUE` | `1024` | Job queue capacity (over capacity → `503 queue_full`). |

## Running locally

```bash
CATALYST_STORE_ROOT=data/market-data cargo run -p catalyst-simulation-service
# listening on http://127.0.0.1:8080 (4 workers)

# submit -> poll -> fetch
id=$(curl -s localhost:8080/backtests -H 'content-type: application/json' \
      -d @request.json | jq -r .id)
curl -s localhost:8080/backtests/$id                 # status: queued|running|succeeded|failed
curl -s localhost:8080/backtests/$id/result          # 409 until done, then the BacktestResult
```

## Tests

```bash
cargo test -p catalyst-simulation-service
```

Drive the router via `tower`'s `oneshot` (no socket): the async submit→poll→fetch
lifecycle (workers started in-test), `409 not_ready` before completion, a failed
run surfaced via status + result, run history, preview (valid/invalid), coverage,
policy-profiles, paginated/filtered events, and a by-store run reading a temp
Parquet store.
