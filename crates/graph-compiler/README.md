# catalyst-graph-compiler (Rust)

Compiles a raw Catalyst graph into a deterministic `CompiledGraph`: typed
operations, execution **triggers** (initial / signal-driven / action-chained),
and the **data requirements** the market-data layer must source.

Rust port of the Python `catalyst_graph_compiler`; per
[ADR 0001](../../docs/adr/0001-language-boundary.md) this is the **single
authoritative trigger derivation** for the run path — the simulation engine
consumes it (the engine no longer derives triggers itself), which removes the
Python↔Rust duplication tracked in #28.

## What it does

```rust
let compiled = catalyst_graph_compiler::compile(&graph)?;
```

1. Validates structure (duplicate ids, unknown edge endpoints) and each enabled
   node's config against its typed contract model — errors carry the `node_id`.
2. Handles enabled/disabled nodes (disabled excluded; touching edges dropped; warned).
3. Derives per-action `triggers`: `initial` / `signal` / `action` (multiple
   incoming edges → multiple triggers).
4. Extracts deduped, sorted `data_requirements` (candles / funding / gas / yields).

Semantics match the Python compiler: stable assets (`USDC`, `USDT`, …) need no
price feed; signal price feeds resolve to the traded venue when unambiguous, else
`hyperliquid`.

## Tests

```bash
cargo test -p catalyst-graph-compiler
```

All 15 sample graphs (`tests/fixtures/sample_graphs.json`) compile; tests also
cover trigger classification, data-requirement extraction, disabled handling, and
error cases — the same suite the Python compiler passes, keeping the two aligned
until the Python compiler is retired (#43 step 5).
