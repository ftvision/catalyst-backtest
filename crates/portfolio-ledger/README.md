# catalyst-portfolio-ledger

Deterministic portfolio accounting: the single source of truth for balances,
perp/yield positions, and cumulative costs during a simulation. Uses
`rust_decimal` for exact arithmetic (no float drift).

## What it tracks

| State | Model |
| --- | --- |
| Spot/cash balances | `venue -> asset -> Decimal` |
| Perp positions | `PerpPosition` keyed by `(venue, symbol)` — side, size, entry, leverage, margin |
| Yield positions | `YieldPosition` keyed by `(protocol, asset, chain, pool)` — principal + accrued |
| Costs | running totals: fees, gas, funding (signed), yield (signed) |

## Operations

Execution models drive the ledger through small, explicit operations rather than
mutating state directly:

- `credit` / `debit` — move spot balances. **`debit` refuses to overdraw** under
  strict policy, returning `LedgerError::InsufficientBalance` and leaving
  balances unchanged. `Ledger::new(allow_negative = true)` relaxes this for the
  `allow_negative` balance policy.
- `open_perp` / `close_perp` — debit margin on open; credit margin ± realized PnL
  on close. `PerpPosition::unrealized_pnl(mark)` marks to market.
- `deposit_yield` / `accrue_yield` / `withdraw_yield` — principal + accrued
  bookkeeping; withdrawals draw accrued interest first, then principal; fully
  redeemed positions are removed. `yield_value(...)` supports `amount: "all"`.
- `record_fee` / `record_gas` / `record_funding` / `record_yield` — accumulate
  cost totals (balance movement is a separate `debit`/`credit`).

## Snapshot

`to_portfolio()` projects current state to a `catalyst_contracts::Portfolio`
(zero balances dropped, decimals as normalized strings) for inclusion in a
simulation trace.

## Tests

```bash
cargo test -p catalyst-portfolio-ledger
```

Cover spot credit/debit and the negative-balance guard, cost accumulators, perp
open/close bookkeeping and mark-to-market PnL, and yield deposit/accrue/withdraw
(including `all` and overdraw rejection).
