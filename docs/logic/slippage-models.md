# Slippage models

**Slippage** is the gap between the reference price and the price you actually
fill at, adverse to the trader (buys fill higher, sells fill lower). It's the
single biggest "is this backtest realistic?" knob after the data itself. The
policy field `fills.slippage.model` selects how it's estimated; `fills.slippage.bps`
(a.k.a. `slippage_bps`) parameterizes the bps-based ones.

Applied in two places:
- `crates/execution-models/src/pricing.rs` — `apply_slippage` (used by `fill_price`
  for **both swaps and perps**).
- `crates/execution-models/src/swap.rs` — `amm_price` / `swap_at` (**swaps only**).

| Model | Formula | Size-dependent? | Fits | Status |
| --- | --- | --- | --- | --- |
| `fixed_bps` | `price · (1 ± bps/10000)` | no | any venue (CEX, perp, simple DEX proxy) | implemented |
| `amm_price_impact` | constant-product avg from pool reserves | **yes** | on-chain AMM DEX (needs a reserves series) | implemented (**swap-only**) |
| `volume_based` | `base_bps + coef·√(amount/volume)` (√-law) | **yes** | thin / volume-limited markets | implemented (#137); falls back to `fixed_bps` w/o volume |
| `none` | `price` (no haircut) | no | research / idealized | implemented |

## `fixed_bps` — flat adverse haircut

Buys fill at `price · (1 + bps/10000)`, sells at `price · (1 − bps/10000)`.
**Size-independent**: a $100 and a $10M trade get the same per-unit haircut.

- **Market:** any venue. A good proxy for a **liquid CEX or perp** where the
  bid/ask spread plus typical impact is roughly a small constant for normal
  sizes. It's the default — `strict_v1` uses 10 bps, `conservative_v1` 25 bps,
  `research_v1` 5 bps.
- **Choose it when:** you want a simple, robust, venue-agnostic cost you can tune
  to a venue's liquidity, and your trade sizes are small relative to market depth.
- **Don't, when:** trades are large relative to pool/book depth — fixed bps then
  *understates* cost (a whale pays far more than 10 bps). Use `amm_price_impact`.

## `amm_price_impact` — depth-aware (constant product)

The realistic model for an on-chain AMM: your execution price **is** the pool's
price impact. From reserves `(rb, rq)` and trade `amount`, the average fill price
is constant-product (x·y=k):

- buy `amount` quote → avg price `(rq + amount) / rb`
- sell `amount` base → avg price `rq / (rb + amount)`

**Size-dependent**: a bigger trade, or a thinner pool, moves the price more.

- **Market:** on-chain **AMM DEX** (Uniswap-style, Base DEX). Requires a
  `liquidity` reserves series for the `(venue, symbol)`.
- **Choose it when:** backtesting DEX swaps where size vs. pool depth matters —
  this is the only model that charges a whale more than a minnow.
- **Caveats (important):**
  1. **Swap-only depth model; falls back to `fixed_bps` elsewhere.** A *perp*
     under `amm_price_impact` doesn't read reserves, so it falls back to the
     configured `slippage_bps` (a real cost), not zero (#136). For perps it's
     therefore equivalent to `fixed_bps`.
  2. **Falls back to `fixed_bps`** when reserves are absent for the series — so
     without a `liquidity` series it charges the configured bps, not nothing.
  3. Models a single constant-product pool — not routed/multi-hop fills,
     concentrated liquidity (Uniswap v3), or MEV/sandwich effects.
  4. **Market swaps only — never resting limit fills (#162).** A resting limit is
     a maker order and fills at limit-or-better; the AMM model does not reprice
     it. The theoretical impact price is emitted in the fill detail
     (`amm_theoretical_price` / `amm_impact_exceeds_limit`) for honesty. See
     [limit-orders.md](limit-orders.md).

## `volume_based` — participation-scaled impact

> **Status: implemented (#137) — square-root law.** A trade pays more the bigger
> its share of the bar's volume; falls back to `fixed_bps` when the bar has no
> volume. Applies to swaps and perps.

**Intended market:** thin / volume-constrained venues where the cost of a trade
depends on how much of the available volume it consumes. The driver is the
**participation rate** `p = amount / bar_volume` — your trade as a fraction of
the bar's traded volume. Bigger `p` ⇒ more slippage.

The open question is *how fast* cost grows with `p`. Two candidate models:

### Candidate A — linear participation
```
effective_bps = base_bps · (1 + p)
```
Extra impact grows **proportionally** with size (a straight line): double the
trade ⇒ double the extra cost. A trade equal to the whole bar's volume (`p = 1`)
pays 2× the base bps.

### Candidate B — square-root law
```
effective_bps = base_bps + coef · √p
```
Extra impact grows **sub-linearly / concavely**: quadruple the trade ⇒ only
*double* the extra cost. This is the empirical **"square-root law of market
impact"** (Almgren, Kyle, et al.), observed across equities, FX, and crypto.

### How they compare (base 10 bps)
| participation `p` | A: linear `10·(1+p)` | B: √-law `10 + 30·√p` |
| --- | --- | --- |
| 1% (tiny) | 10.1 | 13.0 |
| 25% | 12.5 | 25.0 |
| 100% (whole bar) | 20.0 | 40.0 |
| 400% | 50.0 | 70.0 |

The **shape** is the difference: from 25%→100% participation (4× the size),
linear's *extra* impact rises 4× (2.5→10 bps) while √-law's rises only 2×
(15→30 bps). For **small** trades (`p ≪ 1`, the common case) the two are nearly
identical; they diverge only for large trades.

### Decision: square-root law (Candidate B)

`volume_based` will use `effective_bps = base_bps + coef · √(amount / bar_volume)`.

**Why:**
- **Realism where it matters.** The model exists precisely to penalize *large*
  trades; the square-root law is what real markets exhibit there, while linear
  over-penalizes size (makes large-size strategies look worse than reality).
- **Right answer for capacity questions** — "how big can this strategy get before
  impact eats the edge?" — which is the main reason to use a volume model at all.
- **No downside for small trades**, where it's indistinguishable from linear.
- Cost is only a `√` (an f64 round-trip on the `Decimal`, fine for a slippage
  estimate).

**Behavior details:**
- Falls back to **`fixed_bps`** when the bar has **no volume** (Dune-derived
  candles carry none; Binance/HL do) or zero volume — never silently zero.
- `base_bps` is the policy's `slippage_bps`; `coef` is the policy knob
  `fills.slippage.volume_impact_coef_bps` (#169) — the *additional* impact, in
  bps, paid at 100% bar participation (`p = 1`). Default `"50"` in every
  profile (the value previously hard-coded in the engine). Consumed only by
  `volume_based`; validated as a non-negative decimal exactly when that model
  is active (the consume-gated #163 pattern), and echoed in the executed
  policy (#157). `"0"` removes the participation term entirely (degenerates to
  `fixed_bps`).
- Applies to both swaps and perps (unlike `amm_price_impact`, which is swap-only).
  For a buy/perp, the base-unit size is `notional / reference_price`.
- **Fidelity note (`next_open` price selection):** participation is computed
  against the **decision bar's** volume while the fill prices at the **next
  bar's** open — the impact estimate uses the freshest volume known at decision
  time, one bar earlier than the bar the trade actually prints on.

## `none` — idealized

Zero slippage; fills exactly at the reference price.

- **Market:** none — this is a *research* setting. Use it to **isolate strategy
  logic** from execution cost (debugging, an optimistic upper bound, conformance
  checks). Never trust a `none`-priced result as realistic.

## Decision guide

| Situation | Model |
| --- | --- |
| DEX swap, size matters vs. pool depth | `amm_price_impact` (with a reserves series) |
| CEX/perp, or a simple conservative proxy on any venue | `fixed_bps` (tune bps to liquidity) |
| Perp (any) | `fixed_bps` (`amm_price_impact` falls back to it for perps) |
| Isolate strategy logic / idealized upper bound | `none` |
| Thin/volume-limited market | `volume_based` (tune `volume_impact_coef_bps` to the venue; needs bar volume) |

## Worked example (from the tests)

Buy **2000 USDC of ETH** into a **100 ETH / 200,000 USDC** pool (mid 2000):

| Model | Fill price | ETH received | Why |
| --- | --- | --- | --- |
| `none` | 2000 | 1.0000 | reference, no haircut |
| `fixed_bps` (10) | 2002 | 0.9990 | +10 bps flat |
| `volume_based` | 2002 | 0.9990 | this bar has **no volume** → `fixed_bps` fallback |
| `amm_price_impact` | 2020 | 0.9901 | `(200000+2000)/100` — depth impact |

Adverse ordering for this size/pool: `none < fixed_bps = volume_based < amm_price_impact`.
(With **volume** on the bar, `volume_based` rises above `fixed_bps` as the trade's
participation grows — see the volume test below: $20k→2003, $500k→2007, $2M→2012
into a 1000-ETH bar.) A larger trade widens the `amm` gap further while
`fixed_bps` stays flat — that's the point of the depth-aware models.

## Tests (executable documentation)

`crates/execution-models/tests/execution.rs`:
- `slippage_models_produce_distinct_swap_fills` — the same DEX buy under all four
  models, asserting the prices above.
- `volume_based_charges_more_for_a_larger_share_of_bar_volume` — same bar (1000
  ETH volume); $20k/$500k/$2M buys fill at 2003/2007/2012 (sub-linear in size).
- `volume_impact_coefficient_is_a_policy_knob_that_scales_the_impact_term` —
  the same $500k buy (p = 0.25) under coef "50" vs "100" fills at ~2007 vs
  ~2012; coef "0" degenerates to `fixed_bps` (#169).
- `volume_based_falls_back_to_fixed_bps_when_bar_has_no_volume` — 2002, like
  `fixed_bps`, never silently zero.
- `amm_price_impact_falls_back_to_fixed_bps_for_perps` — a perp opens at 2002
  under both `fixed_bps` and `amm_price_impact` (the depth model is swap-only, so
  it falls back to bps — a real cost, not zero).
- `amm_buy_applies_price_impact_from_reserves` / `amm_falls_back_to_fixed_bps_without_reserves`
  — the reserve-driven path and its no-reserves `fixed_bps` fallback.

## Related issues

- [#136](https://github.com/ftvision/catalyst-backtest/issues/136) — amm_price_impact fallback to fixed_bps — FIXED
- [#137](https://github.com/ftvision/catalyst-backtest/issues/137) — volume_based square-root law — IMPLEMENTED
- [#169](https://github.com/ftvision/catalyst-backtest/issues/169) — impact coefficient promoted to the `volume_impact_coef_bps` policy knob — FIXED
