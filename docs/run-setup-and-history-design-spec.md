# Run Setup and Simulation History Design Spec

This spec turns the current setup and history mockups into reusable design
decisions for the Catalyst Backtest web app. It should guide the next UI pass
without turning the workbench into a graph editor or a crypto trading terminal.

Reference mockups:

- [Run Setup preflight mock](assets/run-setup-preflight-mock.png)
- [Simulation History mock](assets/simulation-history-mock.png)

## Product Intent

Run Setup is a preflight contract. A strategy author should understand exactly
what will be simulated before a run starts:

1. The graph being tested.
2. The market data used as the replay world.
3. The initial portfolio used as starting state.
4. The simulation configuration and policy assumptions.

Simulation History is the audit trail and rerun surface. It should answer:

- What has already been run for this graph?
- Which market window, data source, portfolio, and policy created the result?
- Which run should be reopened, replayed, compared, or duplicated?

## Information Architecture

The workbench navigation should grow from four pages to five:

| Page | Purpose |
| --- | --- |
| Run Setup | Configure graph, market data, portfolio, and simulation policy before running. |
| Market Replay | Overview of market candles, volume, equity, drawdown, and event rails. |
| Event Lens | Detailed event investigation with local market context, gas, funding, and costs. |
| Result Review | Portfolio outcome, equity and drawdown, cost attribution, and final state. |
| Simulation History | Search, filter, reopen, replay, duplicate, and compare prior runs. |

Run Setup keeps a small recent-runs table. Simulation History owns the full run
list.

## Run Setup Layout

Use a dense desktop workspace with a persistent top bar and workflow tabs. The
page body has a main setup column plus a right readiness rail.

### Setup Step Strip

Show four horizontal step modules above the main content:

| Step | State Signal |
| --- | --- |
| Graph | Valid, warning, invalid. |
| Market Data | Complete, partial, missing required series. |
| Portfolio | Balanced, incomplete, invalid balance. |
| Configuration | Ready, warning, invalid assumption. |

Each step should be clickable and scroll or focus the matching setup section.
The strip is not a wizard; users can move freely between sections.

### Graph Section

Graph remains read-only in v1.

Required content:

- Strategy selector.
- Graph name, hash, version, node count, edge count.
- Compact requirements table: node, kind, detail, required data.
- Clickable table rows that reveal an inline node details panel.

Do not add a full graph canvas editor. If a topology preview is added later, it
must be read-only and use the same selected-node details panel as the table.

Node details panel fields:

- Node id and kind.
- Config summary.
- Data dependencies.
- Policy assumptions affecting the node.
- Events produced in the last selected run, if available.

### Market Data Section

Market data selection is the most important setup interaction after graph
selection. The user needs to see what local data exists, choose a replay window,
and understand whether the graph's required series are covered before running.

Primary fields:

- Source: Parquet store first, then inline bundle or remote source if supported.
- Venue.
- Symbol or pair.
- Quote asset.
- Interval.
- UTC start.
- UTC end.
- Required series: candles, gas, funding, yields.

Selector behavior:

- Pick source first, then constrain venue, symbol, and interval options from the
  source catalog.
- Show available horizontal coverage before asking for start and end.
- Default start and end to the widest complete window that satisfies required
  candle and gas data.
- Allow partial optional series only with an explicit warning.
- Treat missing required series as a hard blocker.

Coverage timeline:

- Horizontal UTC scale.
- One row per required series.
- Teal segments for covered data.
- Amber gaps for partial data or missing optional series.
- Red gaps for missing required series.
- Missing funding must be explicit, not hidden behind a generic warning.

Primary actions:

- Change source.
- Preview window.
- Inspect coverage.

Scenario loading is deferred. If scenarios remain in the service catalog, they
can appear as a secondary source type later, but they should not compete with
local market-data selection in the first implementation pass.

### Initial Portfolio Section

Use an editable table once backend contracts support updates. Until then, render
read-only rows with clear disabled states.

Columns:

- Venue.
- Asset.
- Amount.
- Reference price.
- Starting value.
- Weight.
- Validation.

Validation examples:

- Missing quote price.
- Unsupported venue for action node.
- Insufficient balance for the selected graph.
- Portfolio asset unused by graph.

### Configuration Section

Required controls:

- Policy profile.
- Slippage bps.
- Max missing candles.
- Fee model.
- Gas model.
- Funding model.
- Deterministic seed.
- Timezone display, default UTC and non-editable for now.

Policy controls should use a segmented control for profile selection and compact
field groups for numeric knobs. Use toggles only for true on/off assumptions.

### Run Readiness Rail

The right rail answers whether the run can start.

Sections:

- Run readiness checklist.
- Graph requirements matched.
- Market data window.
- Portfolio validation.
- Policy and configuration summary.
- Warnings.
- Estimated samples and event count, if available.

The Run backtest button should be disabled only for hard blockers. Warnings
should remain visible after the run in Result Review and Simulation History.

## Simulation History Layout

Simulation History is a table-first screen with a details panel. It should be
fast to scan and good at reruns.

### Filters

Filter toolbar:

- Graph or strategy.
- Status.
- Market data source.
- Symbol or pair.
- Policy profile.
- UTC date range.
- Search by run id or graph hash.

Status is a segmented control. Graph, source, symbol, and policy are selects.
Search is a text input.

### Summary Strip

Use compact metric cells, not hero metrics:

- Total runs.
- Succeeded.
- Warnings.
- Failed.
- Latest run.

### Run Table

Columns:

- Run id.
- Status.
- Created.
- Graph.
- Market window.
- Source.
- Policy.
- Net PnL.
- Max drawdown.
- Events.
- Warnings.
- Actions.

Actions:

- Open result.
- Replay events.
- Duplicate setup.
- Compare, deferred until multi-run result comparison exists.

The table should support keyboard row selection and preserve the selected row
when filters change if the row remains visible.

### Selected Run Details

Details panel content:

- Readiness snapshot from run creation time.
- Market data coverage timeline.
- Initial portfolio summary.
- Cost breakdown.
- Equity and drawdown miniature.
- Gas and funding context.
- Warnings and policy assumptions.

The details panel should not require fetching every result for every row. Use
summary fields in the list endpoint, then fetch full metadata/result only for the
selected row.

## Extracted Design Tokens

Keep the current restrained light workspace. Extend the existing CSS variables
rather than adding another theme layer.

### Color Tokens

| Token | Role |
| --- | --- |
| `--cb-surface` | Page background. |
| `--cb-surface-2` | Top bar, subdued panels, table headers. |
| `--cb-surface-3` | Selected rows, inactive rails, secondary controls. |
| `--cb-border` | Default dividers and control borders. |
| `--cb-border-strong` | Focused panels and active module outlines. |
| `--cb-text` | Primary text. |
| `--cb-muted` | Labels and metadata. |
| `--cb-faint` | Axis labels, timestamps, secondary hints. |
| `--cb-blue` | Primary actions, selected navigation, active focus. |
| `--cb-teal` | Complete coverage, executed state, healthy checks. |
| `--cb-green` | Positive PnL and retained value. |
| `--cb-amber` | Missing optional data, caveats, partial coverage. |
| `--cb-red` | Failed runs, rejected actions, missing required data. |
| `--cb-violet` | Policy profile and simulation assumption accents. |

New recommended tokens:

```css
--cb-focus-ring: 0 0 0 2px oklch(0.51 0.17 245 / 0.22);
--cb-row-selected: oklch(0.955 0.018 245);
--cb-row-hover: oklch(0.972 0.01 235);
--cb-warning-bg: oklch(0.975 0.035 78);
--cb-danger-bg: oklch(0.972 0.028 25);
--cb-success-bg: oklch(0.972 0.026 178);
--cb-policy-bg: oklch(0.968 0.026 286);
```

### Spacing Tokens

Use current Mantine spacing for primitives. Add semantic layout classes:

| Token | Value | Use |
| --- | --- | --- |
| `--cb-gap-field` | `10px` | Labels and control stacks. |
| `--cb-gap-panel` | `14px` | Inside setup modules. |
| `--cb-gap-section` | `18px` | Between setup sections. |
| `--cb-rail-width` | `340px` | Readiness and selected-run details rail. |
| `--cb-control-height` | `34px` | Dense product inputs. |

### Radius And Elevation

Keep cards and controls at `6px` or Mantine `sm`. Avoid nested cards. Use
dividers, table rows, and subtle tints before shadows.

## Component Inventory

Existing components to keep:

- `SectionHeader`
- `StatusBadge`
- `DataTable`
- `MetricStrip`
- `MarketReplayChart`
- `EquityDrawdownChart`
- `CostAttribution`

New components:

| Component | Purpose |
| --- | --- |
| `SetupStepStrip` | Four setup modules with readiness state and jump links. |
| `SetupModule` | Section frame with title, state, summary, and content. |
| `GraphRequirementTable` | Read-only graph node table with selectable rows. |
| `NodeDetailsPanel` | Inline selected-node details and dependencies. |
| `MarketDataSelector` | Source, venue, symbol, interval, UTC window, and coverage selector. |
| `CoverageTimeline` | Multi-row horizontal coverage visualization. |
| `PortfolioTable` | Initial balances, weights, and validation states. |
| `ConfigurationPanel` | Policy and simulation assumption controls. |
| `RunReadinessRail` | Checklist, blockers, warnings, and run summary. |
| `HistoryFilterBar` | Filter controls for simulation history. |
| `SimulationHistoryTable` | Dense run table with selectable rows. |
| `RunDetailsRail` | Selected historical run summary and actions. |

## Library Choices

Keep the current frontend stack:

- React 19 for stateful UI composition.
- Vite for app build and local dev server.
- TypeScript for API and component contracts.
- Mantine Core and Hooks for forms, layout primitives, tabs, tables, badges,
  segmented controls, tooltips, and responsive primitives.
- Mantine Charts only for small supporting charts where Recharts would be
  unnecessarily custom.
- `lightweight-charts` for market replay candles, volume, equity, drawdown, and
  synchronized event rails.
- Recharts for cost bars and compact non-market analytical charts.
- lucide-react for icons.
- Storybook for component states and layout review.

Do not add a second component framework. Do not add a graph canvas library for
v1. If a read-only topology preview becomes necessary, evaluate React Flow only
for a non-editable preview, and keep the graph table as the primary inspection
surface.

## API Requirements

Current APIs are enough for a basic setup preview and run execution, but the new
screens need stronger list and selector contracts.

### Market Data Selection

Needed endpoints:

```text
GET /market-data/sources
GET /market-data/catalog?source=&venue=&symbol=&interval=
POST /market-data/coverage
POST /market-data/window
```

`GET /market-data/catalog` should return available horizontal coverage by
series, not just whether a requested window can be loaded.

This is the highest-priority new setup API. The setup page should be able to ask
"what local market data do I have?" before it asks the user to run a backtest.

Suggested row shape:

```json
{
  "kind": "candles",
  "source": "parquet-store",
  "venue": "base",
  "symbol": "ETH",
  "quote": "USD",
  "interval": "1h",
  "start": "2024-01-01T00:00:00Z",
  "end": "2024-01-02T07:00:00Z",
  "points": 32
}
```

### Simulation History

Needed endpoint:

```text
GET /backtests?graph_hash=&strategy_id=&status=&source=&symbol=&policy=&start=&end=&limit=&cursor=
```

The list response should include enough summary data for the table:

```json
{
  "id": "af8ceb3f",
  "graph_hash": "1ad4...",
  "strategy_id": "g04_hl_spot_ladder",
  "status": "succeeded",
  "created_at": "2024-01-02T07:10:00Z",
  "market_data": {
    "source": "parquet-store",
    "venue": "base",
    "symbol": "ETH",
    "interval": "1h",
    "start": "2024-01-01T00:00:00Z",
    "end": "2024-01-02T07:00:00Z"
  },
  "policy_profile": "strict_v1",
  "summary": {
    "pnl_usd": "3.7612",
    "return_pct": "0.3761",
    "max_drawdown_pct": "-0.1240",
    "trade_count": 1,
    "rejected_count": 0
  },
  "warning_count": 1
}
```

Continue using:

```text
GET /backtests/{id}
GET /backtests/{id}/metadata
GET /backtests/{id}/result
GET /backtests/{id}/events
```

for selected-row details.

## Accessibility And Interaction

- Every selector and action must be keyboard reachable.
- Coverage timelines need text equivalents and row-level status labels.
- Status must include text, not color alone.
- Numeric values use tabular numbers and at most four decimal places.
- All user-facing timestamps use explicit UTC labels unless the user chooses a
  different display timezone.
- Disabled run actions must name the blocker.

## Implementation Sequence

1. Add data types for setup modules, coverage spans, history rows, and selected
   run details.
2. Extract tokens into CSS variables and Mantine theme extensions.
3. Build Storybook stories for the new setup modules and history table.
4. Implement Run Setup layout with read-only controls first.
5. Add the real market data selector and coverage catalog.
6. Implement Simulation History table against `GET /backtests`.
7. Add selected-run details rail with lazy loading.
8. Defer scenario loading until local market-data selection is reliable.
9. Wire duplicate setup and rerun actions.

## Non Goals

- Graph editing.
- Order book or exchange-terminal UI.
- Live trading controls.
- Multi-run comparison charts in the first pass.
- Scenario loading in the first setup implementation pass.
- Arbitrary user-authored scenario editing in v1.
