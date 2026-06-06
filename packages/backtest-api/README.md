# catalyst-backtest-api

User-facing HTTP API (FastAPI) for creating and inspecting backtest runs. It
validates requests against the **contract models** and hands work to the
**worker** layer.

## Endpoints

### Run lifecycle

| Method & path | Purpose |
| --- | --- |
| `POST /backtests` | Validate a `BacktestRequest` and run it; returns `{ id, status }` (`201`). |
| `GET /backtests/{id}` | Run status (`{ id, status, error }`). |
| `GET /backtests/{id}/result` | The summarized `BacktestResult`. |
| `GET /backtests/{id}/metadata` | Run-level metadata: graph hash, resolved policy, data coverage, warnings, artifact refs, timestamps. |
| `GET /backtests/{id}/events` | Event log, **paginated + filterable** (`type`, `node_id`, `status`, `cursor`, `limit`) → `{ items, next_cursor, total }`. |
| `GET /backtests?graph_hash=...` | Compact **run history** for a graph (`{ items: [...] }`). |
| `GET /health` | Liveness. |

### Workbench setup (no run created)

These power the Run Setup screen without creating a run, reusing the compiler,
policy resolver, and market-data planner so the frontend never reimplements them.

| Method & path | Purpose |
| --- | --- |
| `POST /backtests/preview` | Validate a graph and return `{ graph_hash, valid, graph_summary, data_requirements, resolved_policy, warnings }`. Invalid graphs return `valid:false` (HTTP 200), not an error. |
| `POST /market-data/coverage` | Per-series coverage + provider metadata + missing-data warnings for `{ graph, start, end, interval }`. |
| `GET /policy-profiles` | `strict_v1` / `conservative_v1` / `research_v1` with id, label, description, and resolved policy. |

## Validation & errors

- Request bodies are validated by FastAPI against the `BacktestRequest` contract
  model — malformed requests get a `422` automatically.
- An invalid-but-well-formed graph/config (e.g. an edge to a missing node) yields
  a stable `422 { "error": { "code": "backtest_failed", "message": ... }, "id": ... }`.
- Unknown run ids return `404`.

## Building the app

```python
from catalyst_backtest_api import create_app
from catalyst_backtest_worker import HttpSimulationClient, FileArtifactStore

app = create_app(
    client=HttpSimulationClient("http://sim:8080", transport=my_http),
    source=my_market_data_source,
    store=FileArtifactStore("artifacts"),
)
# uvicorn catalyst_backtest_api:app  (after wiring a module-level app)
```

Dependencies (simulation client, market data source, artifact store) are
injected, so the API is fully testable offline.

## Tests

```bash
uv run pytest packages/backtest-api
```

Via FastAPI's `TestClient`: full create→status→result→events lifecycle, contract
validation (`422`), stable error for an invalid graph, and `404`s for unknown
runs.
