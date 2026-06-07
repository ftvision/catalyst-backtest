# Strategy Dataset

This directory is the repo-level strategy repository: graph definitions live in
`graphs/`, market scenarios live in `scenarios/`, and `catalog.json` names the
strategies and scenarios that should be runnable together.

The catalog covers graphs `g01`–`g26` (the original 15 pasted examples plus the
ADR-0002 strategy-surface graphs g16–g26):

- `g01_evm_swap_buy_eth_base`
- `g02_hl_spot_buy_eth`
- `g03_hl_spot_buy_then_sell`
- `g04_hl_spot_ladder`
- `g05_hl_perp_open_long`
- `g06_hl_perp_open_close`
- `g07_hl_perp_round_trips`
- `g08_evm_yield_deposit`
- `g09_evm_yield_deposit_withdraw_partial`
- `g10_evm_yield_deposit_withdraw_all`
- `g11_evm_swap_if_below`
- `g12_evm_swap_dca_ladder`
- `g13_hl_perp_long_short_swings`
- `g14_hl_perp_add_close`
- `g15_evm_yield_add_withdraw`

The catalog also includes primitive direct-spot action examples for UI and
service smoke testing:

- `g16_direct_evm_swap_buy_eth_base`
- `g17_direct_hl_spot_buy_eth`
- `g18_direct_hl_spot_sell_eth`

It also includes strategies built on the ADR-0002 surface (data-driven sources,
derived indicators, composition, and relative sizing):

- `g19_funding_carry` — funding-rate source fans out to a long-spot + short-perp basis trade
- `g20_golden_cross` — SMA(10) vs SMA(50) crossover (derived source vs derived source)
- `g21_donchian_breakout` — price vs rolling_high/rolling_low(20)
- `g22_momentum_roc` — ROC(12) entry with an SMA(10) trend-break exit
- `g23_trend_filter_dip` — `all` combinator: buy the dip only while in an uptrend
- `g24_stop_loss` — entry plus a variable-referenced protective stop
- `g25_yield_rotation` — yield (APR) source: deposit when high, withdraw when low
- `g26_short_momentum` — short on negative ROC, cover on SMA reclaim

Derived signals need warmup history (e.g. SMA(50) needs 50 bars), so they stay
inert on the short synthetic scenarios here; their firing behavior is covered by
unit tests. The catalog run still proves every graph compiles and executes.

Run the catalog against the synthetic scenarios with:

```sh
cargo run -p catalyst-simulation-engine --example run_strategy_dataset -- strategies
```

The runner is intentionally deterministic and offline. These scenarios are not
historical market data; they are fixtures for checking that strategy graphs can
compile and execute across common price paths before wiring real providers.
