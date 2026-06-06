# Web App

Static ES-module prototype for the Catalyst Backtest workbench.

The v1 surface is a separate backtest workbench with four connected views:

- Run Setup: graph summary, run configuration, policy profile, data coverage, assumptions.
- Market Replay: market data, equity, drawdown, and event overlays for overview analysis.
- Event Lens: detailed event analysis with historical data evidence, gas, fees, costs, and policy reasons.
- Result Review: headline outcome, equity/drawdown charts, final portfolio, timeline.

The UI is organized as:

```text
src/
  api/                 future backend client seams
  data/                mock contract-shaped payloads
  design-system/       tokens and reusable UI components
  pages/               page composition modules
  main.js              app shell and lightweight interactions
```

Serve locally from this folder:

```bash
python3 -m http.server 4173
```

Then open `http://localhost:4173`.

Backend integration targets:

- existing: `POST /backtests`, `GET /backtests/{id}`, `GET /backtests/{id}/result`, `GET /backtests/{id}/events`
- planned: issue #33 workbench APIs for preview, coverage, policy profiles, run history, and paginated events
