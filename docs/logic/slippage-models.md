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
| `volume_based` | *(intended: scale by size ÷ bar volume)* | — | thin / volume-limited markets | **not implemented — behaves as `none`** |
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

## `volume_based` — not yet implemented

**Declared in the contract but not implemented.** It currently returns **zero**
slippage, i.e. behaves identically to `none`. Selecting it gives an idealized
fill, *not* a volume-scaled cost. (The test below asserts this aliasing so the
gap is visible; remove that assertion when the model ships.)

- **Intended market:** thin / volume-constrained venues where impact scales with
  participation rate (trade size ÷ bar volume). Until implemented, don't rely on it.

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
| Thin/volume-limited market | *(`volume_based` once implemented; today falls back to `none`)* |

## Worked example (from the tests)

Buy **2000 USDC of ETH** into a **100 ETH / 200,000 USDC** pool (mid 2000):

| Model | Fill price | ETH received | Why |
| --- | --- | --- | --- |
| `none` | 2000 | 1.0000 | reference, no haircut |
| `volume_based` | 2000 | 1.0000 | **stub — aliases `none`** |
| `fixed_bps` (10) | 2002 | 0.9990 | +10 bps flat |
| `amm_price_impact` | 2020 | 0.9901 | `(200000+2000)/100` — depth impact |

Adverse ordering for this size/pool: `none = volume_based < fixed_bps < amm_price_impact`.
A larger trade widens the `amm` gap further while `fixed_bps` stays flat — that's
the whole point of the depth-aware model.

## Tests (executable documentation)

`crates/execution-models/tests/execution.rs`:
- `slippage_models_produce_distinct_swap_fills` — the same DEX buy under all four
  models, asserting the prices above (and that `volume_based` currently aliases
  `none`).
- `amm_price_impact_falls_back_to_fixed_bps_for_perps` — a perp opens at 2002
  under both `fixed_bps` and `amm_price_impact` (the depth model is swap-only, so
  it falls back to bps — a real cost, not zero).
- `amm_buy_applies_price_impact_from_reserves` / `amm_falls_back_to_fixed_bps_without_reserves`
  — the reserve-driven path and its no-reserves `fixed_bps` fallback.
