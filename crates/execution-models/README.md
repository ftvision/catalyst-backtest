# catalyst-execution-models

Action execution models for the simulator. Each model takes an action config, a
read-only `MarketContext` for the current tick, and the resolved policy, then
drives the `Ledger` through its explicit accounting operations and returns an
`Execution` outcome — a `Fill` or a `Rejected { reason }`. Models never mutate
global state directly, and **a rejection leaves the ledger unchanged**.

## Models

| Function | Covers |
| --- | --- |
| `execute_swap` | EVM + Hyperliquid spot swaps. Exactly one side must be a stable/quote asset; buys spend the stable amount as USD notional, sells dispose `amount` base units. |
| `execute_perp` | Hyperliquid perp open/add (same-side netting with blended entry) and reduce-only close (full + partial, with realized PnL). |
| `execute_yield_deposit` / `execute_yield_withdraw` | Aave-style principal moves; supports `amount: "all"`; gas reserved/charged on the chain balance. |

## How pricing works (policy-driven)

- **Fill price** = reference price (`close`/`open`/`mid`/`worse_side_ohlc`, per
  `price_selection`) adjusted by **slippage** adverse to the trader (buys higher,
  sells lower).
- **Fees** = `fee_bps` × notional (USD).
- **Gas** = 0 on Hyperliquid; otherwise `historical` gas from the market context
  with the policy's fixed fallback, or a fixed amount.

## Market context

```rust
pub trait MarketContext {
    fn bar(&self, venue: &str, symbol: &str) -> Option<Bar>; // current OHLC
    fn gas_usd(&self, chain: &str) -> Option<Decimal>;
}
```

The engine implements this over the normalized market data bundle for the tick.

## Tests

```bash
cargo test -p catalyst-execution-models
```

Cover insufficient balance (rejection leaves the ledger untouched), slippage,
fees, gas, reduce-only validation, perp open/add/close, and yield
deposit/withdraw basics — including custom policies flowing through.
