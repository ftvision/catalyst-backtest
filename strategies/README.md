# Strategy Dataset

This directory is the repo-level strategy repository: graph definitions live in
`graphs/`, market scenarios live in `scenarios/`, and `catalog.json` names the
strategies and scenarios that should be runnable together.

The catalog mirrors all 15 pasted graph examples:

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

Run the catalog against the synthetic scenarios with:

```sh
cargo run -p catalyst-simulation-engine --example run_strategy_dataset -- strategies
```

The runner is intentionally deterministic and offline. These scenarios are not
historical market data; they are fixtures for checking that strategy graphs can
compile and execute across common price paths before wiring real providers.
