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

- **Fill price** = reference price (`close`/`open`/`mid`/`next_open`/
  `worse_side_ohlc`, per `price_selection`) adjusted by **slippage** adverse to
  the trader (buys higher, sells lower). `next_open` (the `strict_v1` default)
  uses the *next* bar's open to avoid intra-bar look-ahead, falling back to the
  current close only on the final bar.
- **Slippage** = `fixed_bps` by default; with `amm_price_impact` and a pool
  `liquidity` series for the `(venue, symbol)`, swaps fill at the constant-product
  average price (`x·y=k`) for the trade size — real depth-aware impact — falling
  back to the reference price when no pool data is present.
- **Fees** = `fee_bps` × notional (USD).
- **Gas** = 0 on Hyperliquid; otherwise `historical` gas from the market context
  with the policy's fixed fallback, or a fixed amount.

## Limit orders (resting, fill-when-touched)

Swaps and perps accept `order_type: "limit"` with a `limit_price`. A market order
(the default) fills at the current bar as above. A **limit** order does not fill
on placement — it *rests* until a later bar's price touches it. This module
provides the instrument-independent pieces; the engine owns the resting book,
time-in-force expiry, and `order_placed`/`order_filled`/`order_expired` events.

- `place_swap_limit` / `place_perp_limit` — validate a placement and resolve its
  side: buys rest below the market (open long / close short), sells rest above
  (open short / close long). A reduce-only limit (take-profit) requires an open
  position.
- `limit_fill_price(bar, side, limit)` — the touch test and fill price:
  - **buy** fills when `bar.low <= limit`; **sell** when `bar.high >= limit`.
  - fills **at the limit**, except a bar that gaps *through* it fills at the
    **open** (in the trader's favor).
  - a resting limit is a **maker** order: **no taker slippage** is applied
    (unlike `price_selection`/`slippage` on market fills). Fees still apply.
- `execute_swap_at` / `execute_perp_at` — apply a fill at an explicit price (used
  by the engine when a resting order touches).

Bar-resolution honesty: the engine only makes a resting order eligible from the
bar *after* it was placed, so a fill never depends on intra-placement-bar path
information that wasn't knowable when the order was sent.

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
fees, gas, reduce-only validation, perp open/add/close, yield deposit/withdraw
basics, and limit-order touch logic + placement validation — including custom
policies flowing through. Resting-order lifecycle (rest → fill/expire) is covered
end-to-end in `simulation-engine`'s `tests/limit_orders.rs`.
