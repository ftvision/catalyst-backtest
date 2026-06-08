# Trading fees

A **trading fee** is the venue/protocol charge taken on each fill, separate from
slippage (the price haircut) and gas (the on-chain action cost). It matters for
correctness because it is real money that must leave the account exactly once,
on the right notional, without ever crediting a negative balance. The policy
field `fees.fee_model` selects how it's computed; `fees.fee_bps` parameterizes
the bps-based model.

Applied in three places, all through one helper:
- `crates/execution-models/src/pricing.rs:94` — `fee_usd(notional_usd, policy)`,
  the single fee computation.
- `crates/execution-models/src/swap.rs:147,171` — swap buy / sell.
- `crates/execution-models/src/perp.rs:97,186` — perp open / close.

## What it is

`fee_usd` is a flat lookup on the model (`pricing.rs:94-99`):

```rust
match policy.fee_model {
    FeeModel::FixedBps => notional_usd * parse(&policy.fee_bps) / Decimal::from(BPS),
    FeeModel::VenueFeeTable | FeeModel::None => Decimal::ZERO,
}
```

(`BPS = 10_000`, `pricing.rs:12`.)

| Model | Formula | Notional-dependent? | Status |
| --- | --- | --- | --- |
| `fixed_bps` | `notional_usd · fee_bps/10000` | yes (flat rate) | implemented (`pricing.rs:96`) |
| `venue_fee_table` | *(intended: per-venue maker/taker tiers)* | — | **NOT implemented — returns 0** (`pricing.rs:97`) |
| `none` | `0` | no | implemented (`pricing.rs:97`) |

The `FeeModel` enum (`crates/simulation-policies/src/lib.rs:61`) defines exactly
three variants: `{ FixedBps, VenueFeeTable, None }`. (Note: the design doc
`docs/simulation-policies.md:272-275` also lists a `pool_fee_tier` option, but
that variant does not exist in the code — only the three above do.)

### Which notional the fee is charged on

The fee is always charged on a USD notional, but *which* notional differs by
action. In every case it's the **post-slippage** value (the reference price is
already slipped before these call sites):

| Action | Notional passed to `fee_usd` | Source |
| --- | --- | --- |
| Swap **buy** | the input USD amount (stable in = USD notional) | `swap.rs:146-147` |
| Swap **sell** | `proceeds = amount · fill_price` (gross, before fee/gas) | `swap.rs:170-171` |
| Perp **open** | `notional = size_usd` (the full position notional, **not** the margin) | `perp.rs:93,97` |
| Perp **close** | `notional_closed = close_base · fill_price` (the closed fraction's value) | `perp.rs:185-186` |

Two things to note:
- **Perp fees are on full notional, not margin.** A 5x $500 position
  (margin $100) pays fee on $500, charged at open and again at close. Leverage
  multiplies fee exposure relative to capital posted.
- **Swap sell fee is on the slipped proceeds**, so a worse fill price lowers both
  proceeds and the fee proportionally (the fee tracks the realized value, not the
  reference value).

### Profile defaults

All three named profiles use `FeeModel::FixedBps`; the conservative/research
profiles inherit `fee_model` from `strict_v1` via `..strict_v1()`
(`profiles.rs:46,59`):

| Profile | `fee_model` | `fee_bps` |
| --- | --- | --- |
| `strict_v1` | `fixed_bps` | 5 (`profiles.rs:18-19`) |
| `conservative_v1` | `fixed_bps` (inherited) | 8 (`profiles.rs:43`) |
| `research_v1` | `fixed_bps` (inherited) | 5 (inherited, `profiles.rs:59`) |

`venue_fee_table` and `none` are not used by any built-in profile; reaching them
requires a custom resolved policy.

## When to use which

- **`fixed_bps`** — the default and the only working model. Tune `fee_bps` to a
  venue's taker rate (CEX ~5-10 bps, on-chain perp taker varies). Use it for all
  realistic backtests.
- **`none`** — research/idealized: isolate strategy logic from fee drag, or as an
  optimistic upper bound. Never trust a `none`-fee result as realistic.
- **`venue_fee_table`** — **do not select it expecting tiered fees.** It is a stub
  that silently charges **zero**, identical to `none` (`pricing.rs:97`). There is
  no maker/taker or per-venue table behind it yet.

## Correctness notes / edge cases

- **No look-ahead.** The fee is a pure function of the notional and the policy
  `fee_bps`; it uses no future bars. The notional itself derives from the slipped
  reference price, which the slippage layer already guarantees is past/known data.

- **Money conservation — buys.** On a swap buy, `total_out = notional + fee + gas`
  is debited atomically and only `notional/price` of the target asset is credited
  (`swap.rs:148-153`). The fee leaves the account once; nothing offsets it.

- **Money conservation — sells, negative-net guard (#117).** On a swap sell,
  `net = proceeds - fee - gas` is checked **before any ledger mutation**; if
  `net <= 0` (fee+gas swallow the proceeds, e.g. dust sold where gas exceeds trade
  value) the swap is **rejected** and the ledger is untouched (`swap.rs:172-186`).
  This prevents crediting a negative amount, which would mint phantom debt in the
  destination asset.

- **Money conservation — perp close, settlement floor (#117).** The fee is
  subtracted inside the settlement, and the whole settlement is floored at zero:
  `settlement = (returned_margin + realized_pnl - fee).max(0)` (`perp.rs:192`).
  A loss can never exceed the posted margin for the closed fraction; an underwater
  close returns nothing rather than clawing back unposted collateral. So the fee
  cannot push settlement negative.

- **Perp open: fee must be funded as collateral.** At open, `margin + fee` is
  debited from USDC up front (`perp.rs:124`); if the account can't cover it the
  open is rejected. The fee is not financed by the position.

- **Fees are recorded to the ledger total.** Every path calls
  `ledger.record_fee(fee)` (`swap.rs:154,187`; `perp.rs:127,205`), accumulating
  into the `Ledger::fees_usd` field (`portfolio-ledger/src/lib.rs:36`, mutated by
  `record_fee` at `lib.rs:111-113`). Recorded once per fill. For the swap buy it's
  recorded after the balance mutation; for the perp full-close it's recorded after
  the settlement credit (`perp.rs:194-205`).

- **Determinism.** `fixed_bps` is pure `Decimal` arithmetic — no float, no
  reserve/volume lookup — so it's bit-for-bit deterministic across runs.

- **Malformed `fee_bps` defaults to 0.** `parse` returns `Decimal::ZERO` on a
  bad string (`pricing.rs:117-119`), but `validate` already guarantees `fee_bps`
  parses as a non-negative decimal whenever `fee_model == FixedBps`
  (`resolve.rs:161-163`), so in practice the default-to-zero branch is
  unreachable for the active model.

- **Known limitation — `venue_fee_table` is a zero stub.** It compiles and
  validates but charges nothing (`pricing.rs:97`); there is no maker/taker
  distinction, no per-venue tier, and no funding-rate-style schedule. Treat it as
  "fees off." (No tracking issue number is referenced in the code; flagged here as
  unverified.)

- **Not modeled:** maker rebates, fee discounts/tiers by volume, and any fee
  asymmetry between buy and sell beyond the notional difference. `fixed_bps`
  applies the same rate to every side and action.

## Tests (executable documentation)

`crates/execution-models/tests/execution.rs`:
- `evm_buy_applies_slippage_fee_and_gas` (line 92) — a 100-USDC buy at `fee_bps=5`
  yields `fee_usd = 0.05` and the account drops by exactly `100 + 0.05 fee +
  0.02 gas = 899.93` (lines 99-102): fee on the input notional, charged once.
- `open_perp_debits_margin_and_fee` (line 164) — a $500 / 5x long charges
  `fee_usd = 0.25` (500 · 5 bps) on the **full notional** and debits `margin 100 +
  0.25 fee` → USDC 899.75 (lines 170-172), proving the fee is on notional, not
  margin.
- `open_then_full_close_removes_position_and_settles` (line 186) — a same-bar
  round trip ends below the starting 1000 USDC, losing "a little to slippage +
  fees" (lines 196-198): fee charged on both open and close.
- `leveraged_long_loss_is_capped_at_posted_margin` (line 202) — the settlement
  floor (`max(0)`) that bounds fee+loss at the posted margin (#117).
- `zero_slippage_zero_fee_policy_fills_at_close` (line 318) — with `fee_bps=0`,
  `fee_usd == 0` and the fill is exactly at close (lines 326-328): the bps path
  collapses to no fee.

No test exercises `venue_fee_table` (consistent with it being an unimplemented
stub).

## Related issues

- [#143](https://github.com/ftvision/catalyst-backtest/issues/143) — venue_fee_table fee model is a zero stub
