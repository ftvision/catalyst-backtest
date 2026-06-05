# UI/UX Questions

The first sketch leaned too hard into a generic graph workbench. Before designing
screens, we should answer these workflow questions.

> **Resolved (issue #12).** These questions are now answered and the first
> workflow + screen information architecture are documented in
> [web-app-workflow.md](web-app-workflow.md). In short: a **separate backtest
> workbench** (not embedded, no graph editing in v1) whose dominant surface is
> **result review with visible assumptions**, for a **strategy author** asking
> *"would this graph have made money, and why?"*. The sections below are kept as
> the original framing that led to those decisions.

## Product Context

- Is this an internal tool or a user-facing app?
- Is the UI embedded inside Catalyst, or does it live as a separate backtesting
  product?
- Does the user arrive with a graph already built?
- Does the user need to edit the graph here?
- Should this feel like a builder, an analysis notebook, or an execution audit
  tool?

## Primary User

Possible first users:

- strategy author validating a Catalyst graph
- engineer debugging graph execution semantics
- investor/researcher comparing strategies
- product user checking whether a suggested graph would have worked

Each user wants a different first screen.

## Primary Job

Which one is the product's main job?

- "Would this graph have made money?"
- "Why did this graph behave this way?"
- "Which assumptions matter most?"
- "How does this compare to buy-and-hold?"
- "Can I trust this graph before I run it live?"

## Result UX Requirements

Every result should show:

- final portfolio
- total return
- max drawdown
- equity curve
- trade/action log
- rejected actions
- fees, gas, funding, and yield breakdown
- assumptions used
- data coverage and fallbacks

## Likely Better First Screen

Instead of starting with a large graph canvas, a better first screen may be:

```text
Backtest Run Setup

Top: graph identity + validation status
Left: compact graph summary / node list
Center: run configuration and initial portfolio
Right: data requirements and assumption summary
Bottom: run history for this graph
```

Then after a run:

```text
Backtest Result

Top: headline outcome and key caveats
Center: equity curve + drawdown
Right: final portfolio and assumptions
Bottom: event timeline / trades / rejected actions
```

