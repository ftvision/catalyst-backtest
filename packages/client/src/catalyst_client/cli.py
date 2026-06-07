"""``catalyst-bt`` — command-line client for the Catalyst backtest service.

A backtest setup is *graph + config + policy + (optional) market data*. The graph
lives in its own JSON file; everything else lives in a ``run.toml`` that
references it (see :mod:`catalyst_client.config`). Market data is normally left
out so the service loads it from its own store for the run's window; pass
``--market-data`` to send an inline bundle instead.

Examples::

    catalyst-bt run run.toml --wait
    catalyst-bt run run.toml --start 2024-01-01Z --interval 4h --out result.json
    catalyst-bt preview run.toml
    catalyst-bt catalog
    catalyst-bt coverage run.toml
    catalyst-bt result <id> --json
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Optional

import typer

from .api import ApiError, BacktestFailed, CatalystClient
from .config import load_run
from .render import console, render_catalog, render_coverage, render_preview, render_summary

app = typer.Typer(
    name="catalyst-bt",
    help="Command-line client for the Catalyst backtest service.",
    no_args_is_help=True,
    add_completion=False,
)

ApiUrl = typer.Option(
    None, "--api-url", help="Service base URL (env: CATALYST_API_URL).", envvar="CATALYST_API_URL"
)


def _overrides(
    start: str | None, end: str | None, interval: str | None, policy: str | None, graph: str | None
) -> dict[str, Any]:
    return {"start": start, "end": end, "interval": interval, "policy": policy, "graph": graph}


def _fail(exc: ApiError) -> None:
    console.print(f"[red]error[/red] [{exc.status_code} {exc.code}] {exc.message}")
    if exc.extra:
        console.print(exc.extra)
    raise typer.Exit(code=1)


def _emit(data: Any, out: Path | None) -> None:
    """Write JSON to ``out`` if given, else pretty-print to stdout."""
    if out is not None:
        out.write_text(json.dumps(data, indent=2))
        console.print(f"[green]wrote[/green] {out}")
    else:
        console.print_json(data=data)


@app.command()
def run(
    run_file: Path = typer.Argument(..., exists=True, readable=True, help="Path to a run.toml."),
    api_url: Optional[str] = ApiUrl,
    start: Optional[str] = typer.Option(None, help="Override config.start."),
    end: Optional[str] = typer.Option(None, help="Override config.end."),
    interval: Optional[str] = typer.Option(None, help="Override config.interval (1m..1d)."),
    policy: Optional[str] = typer.Option(None, help="Override policy profile."),
    graph: Optional[str] = typer.Option(None, help="Override the graph path."),
    market_data: Optional[Path] = typer.Option(
        None, help="Inline market-data bundle JSON (else store-loaded)."
    ),
    wait: bool = typer.Option(
        True, "--wait/--no-wait", help="Poll to completion (else just print the run id)."
    ),
    poll_interval: float = typer.Option(1.0, help="Seconds between status polls."),
    timeout: float = typer.Option(300.0, help="Max seconds to wait for completion."),
    out: Optional[Path] = typer.Option(None, help="Write the full result JSON here."),
    raw: bool = typer.Option(
        False, "--json", help="Print the full result JSON instead of a summary."
    ),
) -> None:
    """Submit a backtest from a run.toml and (by default) wait for the result."""
    spec = load_run(
        run_file,
        overrides=_overrides(start, end, interval, policy, graph),
        market_data_path=market_data,
    )
    body = spec.body()
    try:
        with CatalystClient(api_url) as client:
            if not wait:
                run_id = client.submit(body)
                console.print(f"queued [cyan]{run_id}[/cyan]")
                return
            run_id = client.submit(body)
            console.print(f"queued [cyan]{run_id}[/cyan] — polling...")
            client.wait(
                run_id,
                poll_interval=poll_interval,
                timeout=timeout,
                on_update=lambda rec: console.print(f"  status: {rec.get('status')}"),
            )
            result = client.result(run_id)
    except BacktestFailed as exc:
        _fail(exc)
    except ApiError as exc:
        _fail(exc)

    if out is not None or raw:
        _emit(result, out)
    if out is None:
        render_summary(result)


@app.command()
def preview(
    run_file: Path = typer.Argument(..., exists=True, readable=True, help="Path to a run.toml."),
    api_url: Optional[str] = ApiUrl,
    policy: Optional[str] = typer.Option(None, help="Override policy profile."),
) -> None:
    """Validate the graph and show data requirements + resolved policy (no run)."""
    spec = load_run(run_file, overrides=_overrides(None, None, None, policy, None))
    try:
        with CatalystClient(api_url) as client:
            data = client.preview(spec.graph, spec.policy_profile)
    except ApiError as exc:
        _fail(exc)
    render_preview(data)


@app.command()
def coverage(
    run_file: Path = typer.Argument(..., exists=True, readable=True, help="Path to a run.toml."),
    api_url: Optional[str] = ApiUrl,
    start: Optional[str] = typer.Option(None, help="Override config.start."),
    end: Optional[str] = typer.Option(None, help="Override config.end."),
    interval: Optional[str] = typer.Option(None, help="Override config.interval."),
) -> None:
    """Show per-series market-data coverage for the run's window."""
    spec = load_run(run_file, overrides=_overrides(start, end, interval, None, None))
    try:
        with CatalystClient(api_url) as client:
            data = client.coverage(spec.graph, spec.start, spec.end, spec.interval)
    except ApiError as exc:
        _fail(exc)
    render_coverage(data)


@app.command()
def catalog(api_url: Optional[str] = ApiUrl) -> None:
    """List the market-data series available in the service's store."""
    try:
        with CatalystClient(api_url) as client:
            data = client.catalog()
    except ApiError as exc:
        _fail(exc)
    render_catalog(data)


@app.command()
def policies(api_url: Optional[str] = ApiUrl) -> None:
    """List the available policy profiles."""
    try:
        with CatalystClient(api_url) as client:
            data = client.policy_profiles()
    except ApiError as exc:
        _fail(exc)
    console.print_json(data=data)


@app.command()
def strategies(
    strategy_id: Optional[str] = typer.Argument(
        None, help="Fetch one strategy's graph; omit to list."
    ),
    api_url: Optional[str] = ApiUrl,
    save: Optional[Path] = typer.Option(None, help="Save the fetched graph JSON here."),
) -> None:
    """List bundled strategies, or fetch one's graph."""
    try:
        with CatalystClient(api_url) as client:
            if strategy_id is None:
                console.print_json(data=client.strategies())
                return
            data = client.strategy(strategy_id)
    except ApiError as exc:
        _fail(exc)
    if save is not None:
        save.write_text(json.dumps(data.get("graph", data), indent=2))
        console.print(f"[green]wrote[/green] {save}")
    else:
        console.print_json(data=data)


@app.command(name="list")
def list_runs(
    api_url: Optional[str] = ApiUrl,
    graph_hash: Optional[str] = typer.Option(None, help="Filter history by graph hash."),
) -> None:
    """List prior backtest runs (optionally filtered by graph hash)."""
    try:
        with CatalystClient(api_url) as client:
            console.print_json(data=client.list_backtests(graph_hash))
    except ApiError as exc:
        _fail(exc)


@app.command()
def status(run_id: str, api_url: Optional[str] = ApiUrl) -> None:
    """Show the status record of a run."""
    try:
        with CatalystClient(api_url) as client:
            console.print_json(data=client.status(run_id))
    except ApiError as exc:
        _fail(exc)


@app.command()
def result(
    run_id: str,
    api_url: Optional[str] = ApiUrl,
    out: Optional[Path] = typer.Option(None, help="Write the full result JSON here."),
    raw: bool = typer.Option(False, "--json", help="Print full JSON instead of a summary."),
) -> None:
    """Fetch a completed run's result (summary, or full JSON with --json/--out)."""
    try:
        with CatalystClient(api_url) as client:
            data = client.result(run_id)
    except ApiError as exc:
        _fail(exc)
    if out is not None or raw:
        _emit(data, out)
    if out is None and not raw:
        render_summary(data)


@app.command()
def events(
    run_id: str,
    api_url: Optional[str] = ApiUrl,
    type: Optional[str] = typer.Option(None, "--type", help="Filter by event type."),
    node_id: Optional[str] = typer.Option(None, help="Filter by node id."),
    event_status: Optional[str] = typer.Option(None, "--status", help="executed | rejected."),
    limit: int = typer.Option(100, help="Page size."),
    cursor: int = typer.Option(0, help="Page offset."),
) -> None:
    """Fetch a run's events (filterable, paginated)."""
    try:
        with CatalystClient(api_url) as client:
            data = client.events(
                run_id, type=type, node_id=node_id, status=event_status, cursor=cursor, limit=limit
            )
    except ApiError as exc:
        _fail(exc)
    console.print_json(data=data)


def main() -> None:
    app()


if __name__ == "__main__":
    main()
