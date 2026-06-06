# catalyst-backtest-worker

Coordinates a full backtest run across the graph compiler, market data, the Rust
simulation service, and the result reporter.

## Pipeline

```python
from catalyst_backtest_worker import run_backtest, HttpSimulationClient, FileArtifactStore
from catalyst_market_data import FixtureSource

record = run_backtest(
    request,                       # a BacktestRequest (or dict)
    client=HttpSimulationClient("http://sim:8080", transport=my_http),
    source=FixtureSource.from_file("eth_2h.json"),
    store=FileArtifactStore("artifacts"),
)
```

`run_backtest`:

1. validates + **compiles** the graph,
2. **plans + builds** the market data bundle from the data requirements,
3. calls the **simulation service** (HTTP) for the trace,
4. **persists the raw trace** and **the summarized result separately**,
5. writes a **metadata** document recording the selected policy and data providers.

## Building blocks

| Piece | Role |
| --- | --- |
| `SimulationClient` | Runs a simulation. `HttpSimulationClient` (injected transport, offline by default) or `CallableSimulationClient` (adapts any `fn(payload)->trace`). |
| `ArtifactStore` | `FileArtifactStore` writes `<root>/<run_id>/{trace,result,metadata}.json`; `InMemoryArtifactStore` for tests. |
| `RunRecord` / `RunStatus` | Outcome of a run: `succeeded`/`failed`, error, and artifact references. |

## Error propagation

Failures at any stage (graph compile error, missing required data, simulation
service error) are captured into a `RunRecord(status=failed, error=...)` rather
than raised — nothing is persisted for a failed run.

## Tests

```bash
uv run pytest packages/backtest-worker
```

End-to-end (offline) tests run a fixture-backed backtest, assert the raw trace
and summarized result are persisted separately, check policy + provider metadata
is recorded, exercise error propagation, and verify HTTP payload building.
