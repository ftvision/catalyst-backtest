# Market-data intervals (1h vs 4h) and how ticks work

This explains why we ingest both **1h** and **4h** candle series, how far each
data source can actually go back, and how the store + engine handle the tick
interval (including the funding subtlety that makes 4h correct).

## Why 1h *and* 4h

The interval is a tradeoff between **granularity** and **history**:

- **1h** — fine-grained execution and signals; best for short, recent backtests.
  But for some venues the *history* is short (see Hyperliquid below).
- **4h** — coarser fills, but **much deeper history** on the venues whose APIs cap
  results. A 4h series can reach years where 1h reaches only months.

So we keep both: 1h for recent, high-resolution runs, and 4h for long-horizon
runs that 1h simply can't cover on a retention-limited source.

## How far each interval reaches (Hyperliquid ETH, measured 2026-06-07)

Hyperliquid's `candleSnapshot` returns a **rolling most-recent ~5,000 candles per
interval** and will *not* serve older windows (an explicit older request returns
empty). So reach is interval-dependent:

| Interval | Reach (ETH, from now = 2026-06-07)                | Limited by                     |
| -------- | ------------------------------------------------- | ------------------------------ |
| 1h       | ~208 days → back to ~Nov 10 2025                  | 5,000-candle cap               |
| 4h       | ~833 days → back to ~Feb 25 2024                  | 5,000-candle cap               |
| 1d       | back to 2020-08-19 (~2,119 candles, ~6 yrs)       | data availability, not the cap |
| funding  | ~last 500–1,000 points (sparse) → ~last few weeks | separate API cap               |

Takeaways:
- At **1h**, HL-native data only goes back ~7 months — you cannot paginate
  further; the old 1h candles aren't retained.
- At **4h**, HL-native reaches ~2.3 years — the practical way to get long
  HL-native history.
- **Funding** is separately capped and sparse, ending a few weeks back regardless
  of candle interval (see the funding note below).

Other sources are not so constrained:
- **Binance** (klines) serves all intervals (1m…1d) with deep history; it's a CEX
  **reference** price, not venue-native.
- **Dune** (ETH/Base price + gas) and **DefiLlama** (Aave yield) go back as far as
  you query. *Caveat:* our Dune query SQL buckets to a fixed interval (`date_trunc`)
  — to ingest 4h from Dune you must change the query's bucketing, not just the
  store label.

| Source                 | Native 4h candles? | Notes                                            |
| ---------------------- | ------------------ | ------------------------------------------------ |
| Binance klines         | yes (server-side)  | deep history; reference (CEX) price              |
| Hyperliquid API        | yes (server-side)  | 4h reaches ~2.3 yr; native mark                  |
| Dune (our queries)     | needs SQL change   | SQL hardcodes hourly buckets; `--interval` only relabels the store |

## How the store + engine handle the interval

**Candles are partitioned by interval.** Each interval is a *separate* series:

```
candles/venue=<v>/symbol=<s>/interval=1h/<date>.parquet
candles/venue=<v>/symbol=<s>/interval=4h/<date>.parquet
```

A backtest picks one interval via `config.interval`; the loader reads the
matching partition. **The tick clock is the loaded candles' timestamps** — 1h
candles → hourly ticks, 4h candles → 4-hourly ticks. So to run a 4h backtest you
must have ingested a 4h candle series for the venue.

**Non-candle series (gas, funding, yields) are not interval-partitioned** — they
are stored once and sampled per tick:

- **gas / yield**: read as *last-known ≤ tick* (forward-filled). Correct at any
  tick interval — they're sampled, not accumulated.
- **funding**: *accumulated*. The engine sums every funding point in the bar
  `(prev_tick, tick]`, so a tick interval **coarser** than the funding interval
  (e.g. 4h ticks over hourly funding) still accrues *all* of it, not 1/N. (Before
  this fix, funding used an exact match at the tick and under-charged 4h runs by
  ~4×.) Funding granularity finer than the tick is handled; funding sparser than
  the tick simply accrues what exists.

## Practical guidance

- **Recent, high-resolution**: use **1h** everywhere (HL-native 1h covers ~7
  months back).
- **Long horizon (years)**: use **4h** — HL-native reaches ~2.3 yr at 4h; pair
  with Binance/Dune which have deep history. (For multi-year HL-native at 1h
  you'd need a paid vendor; the public API can't.)
- Keep the **tick interval aligned with the venue's funding interval** where
  possible; the engine now sums intra-bar funding, but it can only accrue funding
  points that exist in the data.
