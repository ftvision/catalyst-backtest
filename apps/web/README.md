# Web App

React, Vite, TypeScript, Mantine, and Storybook frontend for the Catalyst
Backtest workbench.

The v1 surface is a separate backtest workbench with four connected views:

- Run Setup: graph summary, run configuration, policy profile, data coverage, assumptions.
- Market Replay: market data, equity, drawdown, and event overlays for overview analysis.
- Event Lens: detailed event analysis with historical data evidence, gas, fees, costs, and policy reasons.
- Result Review: headline outcome, equity/drawdown charts, final portfolio, timeline.

The UI is organized as:

```text
src/
  api/                 backend client seams
  components/          reusable UI, chart, metric, status, and table components
  data/                mock contract-shaped payloads
  pages/               page composition modules
  stories/             Storybook checks for pages and charts
  App.tsx              workbench shell, workflow tabs, global actions
  theme.ts             Mantine theme tokens
```

Install and run from this folder:

```bash
npm install
npm run dev
```

Then open `http://127.0.0.1:4173`.

The frontend connects to the Rust simulation service at
`http://127.0.0.1:8080` by default. Override it with:

```bash
VITE_CATALYST_API_BASE=http://127.0.0.1:8080 npm run dev
```

Seed the Parquet market-data store, then run the service from the repo root:

```bash
uv run python -m catalyst_market_data.cli ingest-binance \
  --root data/market-data --venue base --symbol ETH \
  --binance-symbol ETHUSDT --interval 1h \
  --start 2024-01-01T00:00:00Z --end 2024-01-02T07:00:00Z

uv run python -m catalyst_market_data.cli ingest-gas \
  --root data/market-data --chain base --constant 0.02 --interval 1h \
  --start 2024-01-01T00:00:00Z --end 2024-01-02T07:00:00Z
```

```bash
CATALYST_STORE_ROOT=data/market-data cargo run -p catalyst-simulation-service
```

If the service has no `CATALYST_STORE_ROOT`, the workbench falls back to a small
inline demo bundle so the UI remains inspectable.

Inspect UI states in Storybook:

```bash
npm run storybook
```

Build checks:

```bash
npm run typecheck
npm run build
npm run build-storybook
```

Backend integration targets:

- `POST /backtests`
- `GET /backtests/{id}`
- `GET /backtests/{id}/result`
- `GET /backtests/{id}/events`
- `GET /backtests/{id}/metadata`
- `GET /backtests?graph_hash=...`
- `POST /backtests/preview`
- `GET /market-data/catalog`
- `POST /market-data/coverage`
- `POST /market-data/window`
- `GET /policy-profiles`
