# Gas cost

**Gas** is the on-chain transaction fee an action pays to a blockchain (not the
venue's trading fee). It matters for correctness because gas is real, asset-side
money that must leave the ledger when an action executes, and it must be charged
*only* where a chain actually levies it — charging gas on a venue that has none
(or omitting it where one exists) silently mis-states P&L. The policy field
`gas.model` selects how gas is estimated; `gas.fallback.amount` (resolved as
`gas_fixed_amount`) parameterizes the fixed-cost paths.

Computed in one place:
- `crates/execution-models/src/pricing.rs:102` — `gas_usd(venue, ctx, policy)`,
  called by the swap and yield paths (see below).

## What it is

`gas_usd` returns a USD gas cost for one on-chain action on `venue`, in two
steps (`crates/execution-models/src/pricing.rs:102-113`):

1. **Hyperliquid short-circuit.** If `venue == "hyperliquid"` it returns
   `Decimal::ZERO` unconditionally — Hyperliquid is an app-chain/L1 order book
   that carries no EVM gas for the modeled actions, regardless of `gas.model`
   (`pricing.rs:103-105`).
2. **Otherwise, dispatch on `policy.gas_model`** (`pricing.rs:106-112`). The
   `GasModel` enum has exactly these four variants
   (`crates/simulation-policies/src/lib.rs:66`):

| `gas.model` | Behavior | Source | Status |
| --- | --- | --- | --- |
| `none` | returns `0` (no gas charged) | `pricing.rs:107` | implemented |
| `fixed_usd` | returns `gas_fixed_amount` (a flat USD constant) | `pricing.rs:108` | implemented |
| `fixed_native` | **identical to `fixed_usd`** — returns `gas_fixed_amount` verbatim; the value is *not* multiplied by a native-token price | `pricing.rs:108` | partial — enum variant exists but is treated as fixed USD |
| `historical_fee_history` | `ctx.gas_usd(venue)`, i.e. the per-chain gas series at this tick; falls back to `gas_fixed_amount` when no series point exists | `pricing.rs:109-111` | implemented |

The `gas_fixed_amount` string is parsed by `parse` (`pricing.rs:117-119`), which
defaults to `0` on malformed input; the policy crate validates it as a
non-negative decimal upstream whenever a fixed path can be reached — i.e. when
`gas_model` *or* `gas_fallback_model` is a fixed-amount model
(`crates/simulation-policies/src/resolve.rs:164-168`).

### Default profile wiring

All three built-in profiles (`strict_v1`, `conservative_v1`, `research_v1`) use
`gas_model: HistoricalFeeHistory`, `gas_fallback_model: FixedUsd`, and
`gas_fixed_amount: "0.25"` (`crates/simulation-policies/src/profiles.rs:20-22`;
`conservative_v1`/`research_v1` inherit via `..strict_v1()` at
`profiles.rs:46,59`). So out of the box, an EVM action uses the historical gas
series and falls back to $0.25 when a tick has no gas data.

### Which actions charge gas vs. not

| Action | Charges gas? | Where |
| --- | --- | --- |
| Swap (buy/sell) | **yes** — `gas = gas_usd(venue, …)`, debited and recorded | `crates/execution-models/src/swap.rs:142,155,188` |
| Yield deposit | **yes** | `crates/execution-models/src/yields.rs:30,45-48` |
| Yield withdraw | **yes** | `crates/execution-models/src/yields.rs:72,86-89` |
| Perp open | **no** — `gas_usd: Decimal::ZERO` hard-coded | `crates/execution-models/src/perp.rs:150` |
| Perp close | **no** — `gas_usd: Decimal::ZERO` hard-coded | `crates/execution-models/src/perp.rs:216` |

Perps are modeled as Hyperliquid-style venue actions and never charge gas at all
(the zero is hard-coded in the fill, independent of `gas.model` or venue). Only
swaps and yields route through `gas_usd`, and for those the Hyperliquid
short-circuit zeroes gas on that venue.

## Which model / when to use

- **`historical_fee_history`** — the realistic default for EVM chains (e.g.
  `base`): gas tracks the actual on-chain fee at each tick from a loaded gas
  series, so a backtest over a congested period pays more than a quiet one. Use
  it whenever a per-chain gas series is available.
- **`fixed_usd`** — a flat, data-free approximation. Use when you have no gas
  series, want a deterministic constant, or are stress-testing a worst-case
  fixed cost. It is also the *configured* fallback amount the historical model
  reverts to when a tick lacks data.
- **`fixed_native`** — intended to express gas in native-token units priced into
  USD, but **currently behaves exactly like `fixed_usd`** (the code path is
  shared at `pricing.rs:108` and does no native→USD conversion). Treat it as
  fixed USD until that is implemented.
- **`none`** — idealized, zero gas. A research setting to isolate strategy P&L
  from on-chain cost; never trust a `none`-gas result as realistic on an EVM
  chain.

## Correctness notes / edge cases

- **No look-ahead.** The historical path reads `ctx.gas_usd(venue)`
  (`TickContext::gas_usd` at `crates/simulation-engine/src/market.rs:206-208`),
  which the engine resolves to `BundleIndex::gas_at(chain, ts)`: an exact point
  at `ts`, else the **last known point ≤ ts** (`market.rs:175-178`). It never
  reads a future point, so gas at tick `t` depends only on data available at or
  before `t`.
- **Money conservation.** Gas is debited from the asset balance and recorded in
  the cost accumulator. A swap *buy* sums `notional + fee + gas` into one debit
  then `record_gas(gas)` (`swap.rs:149-155`); a swap *sell* debits `amount` and
  credits `net = proceeds - fee - gas`, then `record_gas(gas)`
  (`swap.rs:172,183-188`); yields debit gas then `record_gas`
  (`yields.rs:45-48`, `:86-89`). `Ledger::record_gas` accumulates into
  `gas_usd` (`crates/portfolio-ledger/src/lib.rs:115-117`), and the reporter
  sums per-fill `gas_usd` from EXECUTED/ORDER_FILLED events into `total_gas_usd`
  (`crates/result-reporter/src/lib.rs:220-223,231`). The reported gas total
  equals the sum of gas actually debited.
- **Dust-sell guard (money-conservation).** A swap *sell* is rejected before any
  ledger mutation when `proceeds - fee - gas <= 0`, so gas (plus fee) exceeding
  proceeds cannot mint negative/phantom balances (`swap.rs:172,178-182`;
  fix #117).
- **Yield "all" reserves gas.** A deposit of the full balance subtracts gas
  first so the deposit cannot leave the account unable to pay gas:
  `(ledger.balance(...) - gas).max(0)` (`yields.rs:32-37`).
- **Determinism.** `gas_usd` is pure given `(venue, ctx@ts, policy)`; the only
  numeric softness elsewhere in pricing is the slippage `√`, not gas. Gas values
  are exact `Decimal` throughout.
- **Fallback is `gas_fixed_amount`, not the chain default `0`.** Under
  `historical_fee_history`, a missing series point falls back to the configured
  fixed amount (`pricing.rs:110`), so a data gap charges the policy's constant
  (e.g. $0.25), never silently zero.

### Known limitations

- **`gas_fallback_model` is declared but unused by `gas_usd`.** The resolved
  policy carries a separate `gas_fallback_model` field
  (`crates/simulation-policies/src/lib.rs:131`), parsed from `gas.fallback.model`
  (`resolve.rs:88-91`) and defaulted to `FixedUsd` in the profiles
  (`profiles.rs:21`). It *is* consulted by policy validation (forcing
  `gas_fixed_amount` to be a valid decimal, `resolve.rs:164-168`), but
  **`gas_usd` does not dispatch through it**: the `historical_fee_history`
  fallback hard-codes `gas_fixed_amount` (`pricing.rs:110`). So setting
  `gas.fallback.model` to anything other than a fixed-amount model has **no
  effect** on the gas charged today; the fallback is always the fixed amount.
  (No tracking issue confirmed in-repo; flagged here as documentation-only —
  verify against the issue tracker before relying on the fallback model field.)
- **`fixed_native` is not a true native-unit model** (see table above): it
  returns the configured amount as USD with no token-price conversion
  (`pricing.rs:108`).
- **Perp gas is structurally zero**, not policy-driven: even a perp on an EVM
  chain would charge no gas because the fill hard-codes `Decimal::ZERO`
  (`perp.rs:150,216`). This is correct for the Hyperliquid-only perp model but
  is a limitation if EVM perps are ever modeled.

## Data dependency

The `historical_fee_history` model needs a **gas series per chain**:

- **Contract shape:** `GasSeries { chain, points: [GasPoint { ts, gas_usd }] }`,
  where `gas_usd` is a `Decimal` (`crates/contracts/src/market_data.rs:46-57`).
- **Loading:** the market-data loader reads `gas/chain=<chain>` parquet rows into
  a `GasSeries`, pushing a warning `"no gas for <chain> from 'parquet-store'"`
  when the window has no rows (`crates/market-data-loader/src/lib.rs:751-767`).
- **Indexing:** the engine inserts each point into a per-chain
  `BTreeMap<i64, Decimal>` keyed by `ts` (`crates/simulation-engine/src/market.rs:74-80`),
  queried by `gas_at` (`market.rs:175-178`).

If no series is loaded for an EVM chain, every historical lookup misses and the
model charges `gas_fixed_amount` on every action.

## Tests (executable documentation)

`crates/execution-models/tests/execution.rs`:
- `evm_buy_applies_slippage_fee_and_gas` (`:92`) — an EVM (`base`) buy debits the
  historical gas point ($0.02): `fill.gas_usd == 0.02` (`:100`) and the balance
  reflects notional + fee + gas (`:101`).
- `sell_applies_adverse_slippage` (`:107`) — a Hyperliquid spot sell asserts
  `fill.gas_usd == Decimal::ZERO` (the venue short-circuit) (`:115`).
- `dust_sell_where_gas_exceeds_proceeds_is_rejected_not_credited` (`:229`) — with
  gas $0.5 on `base` (`:233`), a dust sell is `Execution::Rejected` rather than
  crediting a negative net (`:236`).
- `yield_deposit_moves_principal_and_charges_gas` (`:269`) — a yield deposit on
  `base` debits gas: balance `49.98 = 300 - 250 - 0.02` (`:275`).
- `yield_withdraw_partial_returns_funds` (`:290`) — a partial withdraw on `base`
  asserts `149.96 = 49.98 + 100 - 0.02` gas (`:297`).

`crates/simulation-engine/tests/engine.rs`:
- `yield_deposit_failing_on_gas_leaves_ledger_untouched` (`:238`) — a deposit
  sized to leave nothing for gas leaves the ledger unchanged (trial-copy commit);
  uses a `base` gas series point of $0.02 (`:256`).

`crates/result-reporter/tests/reporter.rs`:
- gas aggregation: an EXECUTED event with `gas_usd: "0.02"` (`:71`) yields
  `total_gas_usd == "0.02"` (`:87`), and a Hyperliquid perp fill carries
  `gas_usd: "0"` (`:101`).

## Related issues

- [#145](https://github.com/ftvision/catalyst-backtest/issues/145) — gas.fallback.model selector is ignored
- [#146](https://github.com/ftvision/catalyst-backtest/issues/146) — fixed_native treated as fixed_usd
