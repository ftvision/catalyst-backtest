# catalyst-graph-compiler

Validates and normalizes a raw Catalyst graph into a deterministic, serializable
`CompiledGraph` that the market-data planner and simulation engine consume.

## What it does

```python
from catalyst_graph_compiler import compile_graph

compiled = compile_graph(raw_graph)   # raw dict or catalyst_contracts.Graph
```

Given a graph it:

1. **Validates structure** via `catalyst_contracts.Graph`, then rejects duplicate
   node ids and unknown edge endpoints with a clear `CompileError`.
2. **Validates each node's config** against its typed contract model
   (`SwapConfig`, `PerpOrderConfig`, `YieldConfig`, `PriceThresholdConfig`).
   Errors carry the offending `node_id`.
3. **Handles enabled/disabled nodes** — disabled nodes are excluded and any edge
   touching them is dropped, each with a warning.
4. **Derives execution triggers** for every action:
   - `initial` — no incoming edges; runs once at start.
   - `signal` — runs when the source signal fires.
   - `action` — runs immediately after the source action succeeds (chaining).
5. **Extracts data requirements** the market-data package must source: candle,
   funding, gas, and yield series (deduplicated and sorted for determinism).

## Output shape

`CompiledGraph` holds `actions` (with `triggers`), `signals` (with `targets`),
`data_requirements`, and `warnings`. It is a Pydantic model, so
`compiled.model_dump()` gives a stable JSON-able dict.

## Semantic choices (MVP)

- **Stable assets** (`USDC`, `USDT`, `USD`, `DAI`) are treated as cash and need
  no price feed.
- **Signal price feeds** resolve to the venue where the symbol is traded when
  unambiguous, otherwise fall back to `hyperliquid` (`DEFAULT_PRICE_VENUE`).
- **Multiple incoming edges** to one action produce multiple triggers (OR
  semantics): the action runs whenever any trigger fires.

## Tests

```bash
uv run pytest packages/graph-compiler
```

All 15 sample graphs from the problem statement
(`tests/fixtures/sample_graphs.json`) compile cleanly; tests also cover trigger
classification, data-requirement extraction, disabled handling, and error cases.
