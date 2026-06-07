"""Render service responses as rich tables for the terminal."""

from __future__ import annotations

from typing import Any

from rich.console import Console
from rich.table import Table

console = Console()


def _kv_table(title: str, rows: list[tuple[str, Any]]) -> Table:
    table = Table(title=title, show_header=False, title_justify="left", title_style="bold")
    table.add_column("field", style="cyan", no_wrap=True)
    table.add_column("value")
    for key, value in rows:
        table.add_row(key, "" if value is None else str(value))
    return table


def render_summary(result: dict[str, Any]) -> None:
    """Print the headline summary + cost breakdown of a completed result."""
    summary = result.get("summary", {})
    rows = [
        ("starting value (USD)", summary.get("starting_value_usd")),
        ("final value (USD)", summary.get("final_value_usd")),
        ("PnL (USD)", summary.get("pnl_usd")),
        ("return %", summary.get("return_pct")),
        ("max drawdown %", summary.get("max_drawdown_pct")),
        ("trades", summary.get("trade_count")),
        ("rejected", summary.get("rejected_count")),
    ]
    console.print(_kv_table("Backtest summary", rows))

    costs = result.get("costs") or {}
    if costs:
        cost_rows = [
            ("fees (USD)", costs.get("total_fees_usd")),
            ("gas (USD)", costs.get("total_gas_usd")),
            ("funding (USD)", costs.get("total_funding_usd")),
            ("yield (USD)", costs.get("total_yield_usd")),
        ]
        console.print(_kv_table("Costs", cost_rows))

    warnings = (result.get("metadata") or {}).get("warnings") or []
    for warning in warnings:
        console.print(f"[yellow]warning:[/yellow] {warning}")


def render_catalog(catalog: dict[str, Any]) -> None:
    """Print the available market-data series and their spans."""
    items = catalog.get("items", [])
    table = Table(title=f"Market-data catalog ({catalog.get('source', '?')})", title_style="bold")
    for col in (
        "kind",
        "venue",
        "chain",
        "symbol",
        "protocol",
        "interval",
        "start",
        "end",
        "files",
    ):
        table.add_column(col, no_wrap=True)
    for item in items:
        table.add_row(
            str(item.get("kind", "")),
            str(item.get("venue", "")),
            str(item.get("chain", "")),
            str(item.get("symbol", "")),
            str(item.get("protocol", "")),
            str(item.get("interval", "")),
            str(item.get("start", ""))[:10],
            str(item.get("end", ""))[:10],
            str(item.get("files", "")),
        )
    console.print(table)
    for warning in catalog.get("warnings", []) or []:
        console.print(f"[yellow]warning:[/yellow] {warning}")


def render_coverage(coverage: dict[str, Any]) -> None:
    """Print per-series coverage for a requested window."""
    series = coverage.get("coverage") or []
    table = Table(title="Market-data coverage", title_style="bold")
    for col in ("series", "complete", "% complete", "points", "first", "last", "missing"):
        table.add_column(col, no_wrap=True)
    for entry in series:
        label = " ".join(
            str(entry[k]) for k in ("kind", "venue", "chain", "protocol", "symbol") if entry.get(k)
        )
        table.add_row(
            label,
            _flag(entry.get("complete")),
            str(entry.get("completeness_pct", "")),
            str(entry.get("points", "")),
            str(entry.get("start", ""))[:16],
            str(entry.get("end", ""))[:16],
            str(entry.get("missing", "")),
        )
    console.print(table)
    for warning in coverage.get("warnings", []) or []:
        console.print(f"[yellow]warning:[/yellow] {warning}")


def render_preview(preview: dict[str, Any]) -> None:
    """Print graph validity, data requirements, and resolved policy."""
    valid = preview.get("valid")
    valid_text = "[green]yes[/green]" if valid else "[red]no[/red]"
    console.print(f"graph_hash: [cyan]{preview.get('graph_hash', '?')}[/cyan]  valid: {valid_text}")
    if not valid:
        console.print(f"[red]error:[/red] {preview.get('error', 'invalid graph')}")
        return

    reqs = preview.get("data_requirements") or {}
    # data_requirements is a dict keyed by series kind; each value is a list of
    # {venue/chain/symbol/...} entries (plus scalar fields like lookback_bars).
    rows: list[tuple[str, str]] = []
    for kind, value in reqs.items():
        if isinstance(value, list):
            for entry in value:
                detail = (
                    ", ".join(f"{k}={v}" for k, v in entry.items())
                    if isinstance(entry, dict)
                    else str(entry)
                )
                rows.append((kind, detail))
        elif value:  # scalar like lookback_bars (skip 0/empty)
            rows.append((kind, str(value)))
    if rows:
        table = Table(title="Data requirements", title_style="bold")
        table.add_column("kind", style="cyan", no_wrap=True)
        table.add_column("detail")
        for kind, detail in rows:
            table.add_row(kind, detail)
        console.print(table)

    for warning in preview.get("warnings", []) or []:
        console.print(f"[yellow]warning:[/yellow] {warning}")


def _flag(value: Any) -> str:
    if value is True:
        return "[green]yes[/green]"
    if value is False:
        return "[red]no[/red]"
    return ""
