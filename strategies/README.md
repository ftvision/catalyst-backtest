# Strategy Dataset

This directory is the repo-level strategy repository: graph definitions live in
`graphs/`, market scenarios live in `scenarios/`, and `catalog.json` names the
strategies and scenarios that should be runnable together.

The first catalog entries mirror the pasted graph examples:

- `g01_evm_swap_buy_eth_base`
- `g02_hl_spot_buy_eth`
- `g03_hl_spot_buy_then_sell`
- `g04_hl_spot_ladder`
- `g05_hl_perp_open_long`

Run the catalog against the synthetic scenarios with:

```sh
cargo run -p catalyst-simulation-engine --example run_strategy_dataset -- strategies
```

The runner is intentionally deterministic and offline. These scenarios are not
historical market data; they are fixtures for checking that strategy graphs can
compile and execute across common price paths before wiring real providers.
