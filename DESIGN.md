# Design

## Theme

Light, focused analytical workspace for a strategy author working at a desktop monitor in daylight or office lighting, trying to validate a graph without losing the trail of assumptions.

## Color System

Use OKLCH tokens. The palette is restrained: tinted neutral surfaces, a rare blue primary action color, teal/green for executed and healthy states, amber for caveats, red for rejected or failed states, and violet only for policy or data category accents.

## Typography

Use a native system UI stack for speed and product familiarity. Keep typography fixed, not viewport-fluid. Use tabular numbers for metrics, tables, balances, and chart labels.

## Layout

The app uses a persistent shell with top context, tabbed workflow navigation, and a dense workspace. Group information with alignment, spacing, dividers, and subtle surface tints before borders or shadows. Avoid nested cards.

## Components

Core components include app shell, tabs, buttons, metric cells, status pills, segmented controls, field groups, data tables, timeline rows, assumption rows, coverage meters, chart panels, and empty/error states. Components should be reusable before page composition.

## Motion

Motion is state feedback only: tab transitions, selected row emphasis, hover/focus changes, and small progress/coverage changes. Respect reduced-motion preferences.

## Implementation Notes

The v1 UI composes four workbench pages: Run Setup, Market Replay, Event Lens, and Result Review. Market Replay provides the overview of historical market data, equity, drawdown, and event overlays. Event Lens provides detailed event analysis with market evidence, gas, costs, profile comparisons, and policy reasons. It should be contract-aware and ready to connect to `POST /backtests`, `GET /backtests/{id}/result`, `GET /backtests/{id}/events`, and the planned workbench APIs from issue #33.
