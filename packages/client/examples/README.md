# `catalyst-bt` examples

Ready-to-run `run.toml` files for the CLI. Each describes one backtest —
**graph + config + policy** — that the service runs against market data it loads
from its own store. (See the [package README](../README.md) for the full command
reference.)

| File | Strategy | Notes |
| --- | --- | --- |
| [`run.toml`](run.toml) | `g05` HL perp — open a 5x ETH long | 1h, strict |
| [`hl-perp-roundtrip-4h.toml`](hl-perp-roundtrip-4h.toml) | `g06` HL perp — open then close | 4h, longer window |
| [`hl-spot-ladder-conservative.toml`](hl-spot-ladder-conservative.toml) | `g04` HL spot buy/sell ladder | conservative policy |
| [`hl-perp-swings.toml`](hl-perp-swings.toml) | `g13` alternating long/short swings | price-threshold directional |
| [`hl-perp-research-overrides.toml`](hl-perp-research-overrides.toml) | `g07` HL perp round trips | research policy + execution overrides |

ADR-0002 strategies (data-driven sources, derived indicators, composition,
relative sizing, variables) — all verified end-to-end against the deployed
service:

| File | Strategy | New surface |
| --- | --- | --- |
| [`funding-carry.toml`](funding-carry.toml) | `g19` long spot + short perp basis | funding-rate source |
| [`golden-cross.toml`](golden-cross.toml) | `g20` SMA(10) vs SMA(50) | derived source vs derived source |
| [`donchian-breakout.toml`](donchian-breakout.toml) | `g21` 20-bar high/low | rolling_high / rolling_low |
| [`momentum-roc.toml`](momentum-roc.toml) | `g22` ROC(12) entry + SMA exit | rate-of-change source |
| [`trend-filter-dip.toml`](trend-filter-dip.toml) | `g23` buy the dip in an uptrend | `all` combinator |
| [`stop-loss.toml`](stop-loss.toml) | `g24` entry + protective stop | graph `variables` |
| [`yield-rotation.toml`](yield-rotation.toml) | `g25` deposit/withdraw on APR | yield (APR) source |
| [`short-momentum.toml`](short-momentum.toml) | `g26` short on negative ROC | signed threshold |

Examples cover Hyperliquid (candles + funding) and Base (candles, gas, USDC
yields) — the venues the deployed store has data for. Run `catalyst-bt catalog`
to see what's available, and `catalyst-bt coverage <run.toml>` before a run.

## Setup

The CLI is the `catalyst-client` workspace package; run it with `uv`:

```bash
uv sync                 # once, from the repo root
uv run catalyst-bt --help
```

The service URL resolves from `--api-url`, then `$CATALYST_API_URL`, then the
deployed Fly URL — so with no flag it hits the live service. To target a local
one: `--api-url http://127.0.0.1:8080` (or `export CATALYST_API_URL=...`).

## TOML in, JSON out

These two are unrelated, and people mix them up:

- **The `run.toml` is the input** you write — the run definition. TOML is just
  the human-writable format for it.
- **`--json` is about the output** — the *result*. By default `run`/`result`
  print a readable summary table; `--json` dumps the full result as JSON (for
  `jq`/scripts), and `--out file.json` writes it to a file.

```bash
uv run catalyst-bt run examples/run.toml          # → summary table
uv run catalyst-bt run examples/run.toml --json   # → full result as JSON
uv run catalyst-bt run examples/run.toml --out r.json
```

(The graph file is also JSON, and the service speaks JSON over HTTP — but the
*only* thing you hand-write is the TOML run file.)

## A typical session

```bash
# 1. See what's available
uv run catalyst-bt catalog        # market-data series in the store
uv run catalyst-bt policies       # strict / conservative / research

# 2. Validate + check data before spending a run
uv run catalyst-bt preview  examples/hl-perp-swings.toml
uv run catalyst-bt coverage examples/hl-perp-swings.toml

# 3. Run it (submits, polls to completion, prints a summary)
uv run catalyst-bt run examples/hl-perp-swings.toml

# 4. Inspect afterwards by run id
uv run catalyst-bt status <id>
uv run catalyst-bt result <id> --json
uv run catalyst-bt events <id> --status rejected
uv run catalyst-bt list                      # run history
```

## Anatomy of a run file

```toml
graph  = "../../../strategies/graphs/g05_hl_perp_open_long.json"  # path, relative to THIS file
policy = "strict_v1"                                              # or conservative_v1 / research_v1

[config]
start    = "2026-01-01T00:00:00Z"
end      = "2026-06-01T00:00:00Z"
interval = "1h"                    # 1m | 5m | 15m | 1h | 4h | 1d

# venue -> asset -> decimal-string amount
[config.initial_portfolio.hyperliquid]
USDC = "10000"

# Optional; overrides the policy's defaults. Omit any you don't want to change.
[config.execution]
slippage_bps = "10"
gas_model    = "none"
```

## Overriding the file from the command line

Flags win over the file, so you don't need a new run file per tweak:

```bash
uv run catalyst-bt run examples/run.toml --interval 4h --policy conservative_v1
uv run catalyst-bt run examples/run.toml --start 2025-06-01T00:00:00Z --no-wait
```

## A note on data windows

Hyperliquid-native candles are capped at ~5,000 bars per series, so the available
history depends on the interval: **1h** reaches back to ~2025-12-31, while **4h**
reaches ~2024-02. If a run fails on missing required data under `strict_v1`, run
`coverage` first, then shorten the window or use a coarser interval (the
`hl-perp-roundtrip-4h.toml` example uses 4h for exactly this reason).
