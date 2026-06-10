# Production readiness — gap analysis & roadmap

An honest assessment of the distance between the current system and a
**production-ready backtesting platform**, and the order in which to close it.
Tracked by the production-readiness epic (#131).

## Where we are

This is a well-architected, deterministic **MVP/foundation**, not a toy. The
parts that are hardest to retrofit later already exist:

- **Deterministic Rust run path** with a clean language boundary
  ([ADR 0001](adr/0001-language-boundary.md)) — reproducible by construction.
- **Schema-contracted data shapes** (JSON Schema → Rust serde + Python Pydantic,
  guarded by round-trip fixtures) — no drift between layers.
- **Real test discipline** — ~166 Rust tests + Python suites +
  conformance/golden/fixtures.
- **Data-quality tooling** — provenance, outlier/wick cleaning, and a
  cross-reference validator ([market-data-construction.md](market-data-construction.md)).
- **Honest cost modeling** — fees, gas, funding, slippage (incl. AMM impact),
  liquidations.

The gaps are about **trust, breadth, and operations** — not architecture.

## Gaps by dimension

### 1. Correctness & execution realism — *blocks trust*
A backtester's first job is believable P&L. There is an open cluster of
correctness bugs in fill timing, accrual, valuation, and margin
(#115–#124). Until these are fixed the reported numbers are wrong, so this is
the prerequisite for everything else.

### 2. Performance analytics — minimal
The reporter produces only `starting/final value, pnl, return%, max_drawdown,
trade_count, rejected_count`. Missing the table-stakes risk/return metrics:
**Sharpe, Sortino, Calmar, volatility, CAGR/annualized return, win rate, profit
factor, turnover, exposure**, rolling metrics, and trade-level analytics
(avg win/loss, holding period, MAE/MFE).

### 3. Research methodology — absent
You can run *one* backtest; you cannot yet *research* a strategy. Missing
**walk-forward, out-of-sample splits, parameter optimization/sweeps, Monte Carlo
/ bootstrap**, and overfitting controls (deflated Sharpe, PBO). This is the
difference between "ran a sim" and "have evidence."

### 4. Data breadth & depth — single-asset
The store holds **only ETH** across 3 venues at 1h/4h. Production needs a
**multi-asset universe**, **point-in-time universe membership**
(survivorship/listing bias), a **corporate-actions analog** (token migrations,
rebases, redenominations), and ideally finer/L2 data for realistic fills. The QA
tooling is good but coverage has holes (HL funding ~74%, #80).

### 5. Persistence & ops — in-memory
Run results live in an in-memory index (`Mutex<HashMap>`) and are **lost on
restart**. No durable result/history store, run artifacts, or idempotency. Plus
the usual service needs: **auth, multitenancy, rate limiting, observability
(metrics/tracing/structured logs), CI/CD gates, backups, alerting.**

### 6. Scale — full-in-memory, single-run
`load_bundle` reads the whole window into in-memory `Vec` bundles — fine for one
ETH series, but won't scale to a large universe × multi-year × fine intervals, or
to many concurrent runs. No streaming/chunked engine, no distributed/parallel
sweep execution.

### 7. Live-trading parity — none
Backtest-only. Production-grade systems share the *same execution code* between
sim and live (and offer paper trading) so backtest results actually predict live
behavior. Without this, sim/live divergence is unbounded. This is the real
institutional differentiator.

### 8. Instrument & risk coverage
Has spot, perps, yield. Missing **options, LP/IL (#127), lending/borrowing,
cross-margin**, and portfolio-level **risk limits** (exposure/leverage caps,
drawdown stops, borrow availability/cost for shorts). Liquidation realism is
partial (#117, #120).

## Roadmap (prioritized)

| Tier | Goal | Scope |
| --- | --- | --- |
| **0 — Trustworthy** | The numbers are right | Fix the correctness cluster #115–#124 (fill booking time, accrual-on-gaps, equity valuation, margin cap, look-ahead) with regression tests |
| **1 — Credible research tool** | Can actually evaluate a strategy | Performance/risk metrics · walk-forward + parameter sweep + OOS · durable result/history store · multi-asset data + point-in-time universe |
| **2 — Production service** | Reliable & operable at scale | Observability, auth, CI/CD, backups · scale the engine (streaming/parallel sweeps) · richer instruments + portfolio risk limits |
| **3 — Institutional** | Predicts live, robust to overfitting | Live/paper execution parity · options/LP/borrow · overfitting controls (deflated Sharpe, PBO) · L2/orderbook fills + latency model |

Tiers 0–1 are the gap between *demo* and *a tool a strategist would trust*; tiers
2–3 are the gap between *trusted tool* and *platform*.

## Issue index

**Tier 0 — correctness (must-fix):**
- #116 next_open market orders deferred to fill+book on the fill bar (no phantom entry-bar P&L) — **fixed**
- #117 leveraged perp loss not capped at posted margin
- #118 accrual uses static `interval_secs` vs a gapped tick grid
- #119 inconsistent price lookups misvalue equity/funding & reject sizing on gaps
- #120 liquidation realism (close-only marking, no maintenance margin)
- #122 same-bar fills under non-`next_open` price selection — **decided convention** (trade-on-close, kept + per-run warning)
- #123 Python policy contract drifts from Rust
- #124 resting limit orders don't reserve balance
- #115 non-stable yield positions mis-accounted

**Tier 1 — research-tool capabilities (to be split into issues):**
- Performance/risk metrics (Sharpe, Sortino, Calmar, vol, CAGR, win rate, turnover, exposure, rolling)
- Research harness: walk-forward, out-of-sample, parameter sweep, Monte Carlo/bootstrap, overfitting controls
- Durable result/history persistence (replace the in-memory run index)
- Multi-asset data + point-in-time universe (survivorship/listing bias)
- #129 QA validator: independent reference (Binance/Chainlink)

**Tier 2–3 — fidelity & platform (filed / backlog):**
- #121 modeling-fidelity backlog (yield compounding, yield policy gate, pct_position, cooldown/TIF)
- #125 stablecoin depeg · #126 net-liquidation equity · #127 LP/IL valuation · #128 multi-quote/FX
- #80 Hyperliquid funding ingester + retry/backoff
- Live/paper execution parity (not yet filed)
