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
- `POST /market-data/coverage`
- `GET /policy-profiles`
