# How we construct market data (and how to validate it)

The candles, funding, gas, and yield series in our store are **constructed** — we
derive them from a chosen methodology, they are not handed to us as ground truth.
This matters because *the methodology choice can create artifacts*. This doc
records exactly how each series is built, the artifact we hit and fixed, where
our data sits on the spectrum of "how constructed," and how to validate quality
against an independent reference.

See also [data-sources.md](data-sources.md) (what we fetch) and
[market-data-intervals.md](market-data-intervals.md) (1h vs 4h).

## How each series is constructed

| Series | Source | Construction |
| --- | --- | --- |
| `candles/venue=ethereum/ETH` | Dune `prices.usd` | Hourly OHLC from a price *feed* (already a clean reference, not raw trades). |
| `candles/venue=base/ETH` | Dune `dex.trades` | Hourly OHLC built from **on-chain DEX trades** — we compute the price per trade and aggregate. This is the DIY-est series and the one that needed the most cleaning (see below). |
| `candles/venue=hyperliquid/ETH` | Hyperliquid `candleSnapshot` | Venue-native OHLC the exchange already aggregated from its order book. |
| `gas/chain=…` | Dune `*.blocks` | Hourly average base fee × a gas-units assumption × ETH price → USD. |
| `funding/venue=hyperliquid` | Hyperliquid `fundingHistory` | The venue's published funding points. |
| `yields/protocol=aave` | DefiLlama | The protocol's historical supply APY. |

For Base candles specifically, price per trade is `amount_usd / token_bought_amount`,
filtered to the canonical WETH contract, restricted to non-dust trades, and
bucketed to the interval. Open/close are the first/last trade in the bucket;
high/low are the max/min.

## The artifact we hit: one-sided wick inflation

The first Base chart was wildly spiky — highs in the **trillions**, lows of
**zero** — from scam tokens spoofing the WETH symbol and dust trades. After
filtering those out, a subtler artifact remained: **every 1h candle had a ~4.5%
upper wick and almost no lower wick**, while the clean ethereum reference showed
real wicks under ~1%.

The asymmetry is the tell. Two methodology choices combined to cause it:

1. **A DEX trade price is an *execution* price, not a mid quote.** Buying on an
   AMM executes *above* mid because of slippage / price impact (more for larger
   or lower-liquidity swaps; even more for sandwiched trades). We filtered to
   WETH **buys** only, so every trade carried an upward bias and nothing pulled
   the other way.
2. **High = `max(price)`, Low = `min(price)`.** Max/min are extreme-seeking, so
   the high specifically grabbed the single worst-slipped buy of the hour.

Open/close were unaffected — they're the *first/last* trade, not the extreme, and
they matched the ethereum reference exactly.

### The fix (two stages)

Because the **body (open/close) was trustworthy**, we could keep it and repair
just the wicks — collapsing an implausible high/low down to the body
(`high → max(open, close)`), which invents no price the bar didn't already print:

- **At the query** (`scripts/fetch_dune_base.py`): canonical WETH address (not
  the spoofable symbol), `amount_usd ≥ 500`, `≥ 5 trades`/bucket, and each trade
  bounded to ±10% of the bucket median.
- **At ingest** (`filter_candle_outliers`, `repair_wicks=True`,
  `wick_tolerance=0.02`): remove candles whose *body* is out of band; collapse a
  high/low that extends past the body by more than 2% (matching the ethereum
  reference's real max wick).

The "more correct" source-level fix would be to derive price from a **mid** (both
sides, or pool reserves / a liquidity-weighted price) instead of buys-only
execution prices. We didn't need to go that far because the bodies were reliable.

## The spectrum of "how constructed"

No price is truly unconstructed — price discovery is inherently an aggregation.
What varies is *who* constructs it, whether the methodology is standardized, and
whether it's two-sided (order book) or one-sided (like our buys-only DEX query):

| Source | Who constructs OHLC | Artifact risk | Role |
| --- | --- | --- | --- |
| **Raw on-chain trades** (our Dune `dex.trades`) | **you** | **high** — you pick the price formula, filters, max/min | native, but DIY methodology = our bug |
| **CEX klines** (Binance, Coinbase, Kraken) | the exchange, from its order book | low — two-sided book, tight spreads, no buys-only bias | clean reference (not your venue) |
| **Aggregators / vendors** (Kaiko, CoinAPI, Amberdata, CryptoCompare, CoinGecko) | the vendor, blended across venues with published, outlier-filtered methodology | low, and documented | institutional reference |
| **On-chain oracles** (Chainlink) | a decentralized network, aggregated off-chain with outlier rejection | low | the price DeFi itself trusts |

Two implications for us:

- **CEX klines are "pre-cleaned" for free** — the exchange aggregated them off a
  real order book, so the slippage/buys-only artifact can't arise. We already
  have an `ingest-binance` command, so this is cheap to add as a reference.
- **Chainlink is special for Aave** — Aave values collateral using Chainlink
  feeds, so for protocol-faithful yield simulation Chainlink is the relevant
  "truth," not any single exchange.

The catch: a CEX/oracle price is a *reference* for a different venue than where a
strategy trades. For a fungible asset like ETH, arbitrage pins cross-venue prices
within a fraction of a percent, so a reference is an excellent validation baseline
even when it isn't your exact venue. This is the `native` vs `reference`
[provenance](market-data-storage.md) distinction already in the store.

## Validating quality against a reference

Validation means **agreement with an independent construction**, not comparison
to absolute truth. Two checks ship in `catalyst-market-data-core`:

- **Internal invariants** (`check_ohlc_invariants`) — every candle must have
  positive prices and obey `low ≤ open,close ≤ high`. Catches zero-lows and
  impossible bars with no reference needed.
- **Cross-reference deviation** (`compare_to_reference`) — per-candle
  `|ours − ref| / ref` for open/high/low/close at matching timestamps. On a
  fungible asset, a clean series stays within a small tolerance; anything beyond
  flags our construction (it's what caught the one-sided wick).

Run the QA check (`scripts/validate_market_data.py`) — it prints per-field
deviation stats and exits non-zero on failure, so it can gate CI or a post-ingest
step:

```bash
# Base ETH vs the ethereum reference (both already in the store)
uv run python scripts/validate_market_data.py \
    --venue base --symbol ETH --interval 1h --ref-venue ethereum

# Against an independent Binance reference (recommended): ingest it first, then
uv run python -m catalyst_market_data.cli ingest-binance \
    --root data/market-data --venue binance --symbol ETH \
    --binance-symbol ETHUSDT --interval 1h --start 2024-01-01T00:00:00Z --end 2026-06-07T00:00:00Z
uv run python scripts/validate_market_data.py \
    --venue base --symbol ETH --interval 1h --ref-venue binance --write
```

`--write` records the report in `_quality.json`'s sibling `_validation.json`.

**Interpreting results.** A 2% tolerance is strict; a handful of candles over it
on a DEX-vs-feed comparison is expected (genuine cross-venue basis during
volatile or thin hours, or — for 4h — our wick repair leaving the high slightly
*under* the reference's real intra-bar peak). The signal to act on is a
*systematic* deviation (a whole period off, or one field consistently biased) —
that's a construction problem, like the one-sided wick was.

### Notes on the references we use

- Our `ethereum` series is a Dune `prices.usd` *feed* — cleaner than raw trades
  but still Dune-sourced, so it's a good interim baseline, not fully independent.
- **Binance klines** (order-book-based, free, deep history) is the recommended
  independent reference. **Chainlink** is the most protocol-faithful for Aave.
