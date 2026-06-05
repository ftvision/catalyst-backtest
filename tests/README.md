# Tests

Shared cross-language fixtures and golden tests live here. Everything is
**network-free** and is the single fixture suite both the Rust and Python
implementations are checked against — so a future re-implementation of any
component can be validated against the same cases.

## Layout

- `fixtures/` — deterministic inputs reused across packages:
  - `sample_graphs.json` — all 15 problem-statement graphs.
  - `market_data/eth_2h.json` — a small normalized market data bundle.
- `golden/` — end-to-end conformance cases, one JSON file per graph family. Each
  is language-neutral: `input` (graph + config + policy + market_data) and
  `expect` (invariants the run must satisfy — executed/rejected counts, open
  perp/yield positions, signals fired, balances present).
- `conformance/` — the Python side of the conformance harness.

## Golden cases

| File | Family |
| --- | --- |
| `01_evm_swap_buy` | EVM spot swap (initial action) |
| `02_hl_spot_buy_then_sell` | Hyperliquid spot + action chaining |
| `03_signal_gated_swap` | `price_threshold` signal crossing |
| `04_perp_round_trip` | Hyperliquid perp open + reduce-only close |
| `05_yield_deposit_withdraw` | Aave-style yield deposit + full withdraw |

## Running

```bash
make conformance      # both languages over tests/golden/
```

- **Rust** (`crates/simulation-engine/tests/conformance.rs`) runs the engine over
  each golden `input` and asserts the `expect` invariants.
- **Python** (`tests/conformance/test_conformance.py`) validates each `input`
  against the JSON schemas, compiles the graph, and proves the embedded market
  data bundle satisfies the compiled data requirements (`missing="fail"`).

`make test` runs the full per-package suites for both languages.
