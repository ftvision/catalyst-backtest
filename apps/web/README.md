# Web App

Placeholder for the eventual TypeScript web application.

The first workflow is now decided (issue #12): a **separate backtest workbench**
with two screens — **Run Setup** and **Result Review** — whose dominant surface
is result review with visible assumptions. See
[docs/web-app-workflow.md](../../docs/web-app-workflow.md) for the workflow, the
screen information architecture, and the ready-to-file UI implementation issue.

The frontend framework and app shell are intentionally **not** chosen yet; that
is the first task of the UI implementation issue. The only hard constraint: the
app consumes the existing JSON contracts (`schemas/`) and the `backtest-api`
endpoints (`POST /backtests`, `GET /backtests/{id}[/result|/events]`).
