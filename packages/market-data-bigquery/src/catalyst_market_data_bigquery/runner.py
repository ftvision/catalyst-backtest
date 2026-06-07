"""Injected query seam for BigQuery.

A ``QueryRunner`` runs a SQL string and returns the result rows as dicts:

    runner(sql) -> list[dict]

The real runner lazily imports ``google-cloud-bigquery`` (install the ``gcp``
extra) and uses Application Default Credentials, so tests inject a fake runner
and never touch the network or GCP. We build full SQL strings (timestamps are
formatted as `TIMESTAMP('...')` literals from values we control), so the runner
itself stays a trivial sql->rows callable.
"""

from __future__ import annotations

from typing import Callable

QueryRunner = Callable[[str], list[dict]]


def bigquery_runner(project: str | None = None) -> QueryRunner:
    """A real runner backed by google-cloud-bigquery (imported lazily)."""

    try:
        from google.cloud import bigquery
    except ImportError as exc:  # pragma: no cover - exercised only without the extra
        raise RuntimeError(
            "google-cloud-bigquery is not installed; install the 'gcp' extra "
            "(uv pip install 'catalyst-market-data-bigquery[gcp]')"
        ) from exc

    client = bigquery.Client(project=project)

    def _run(sql: str) -> list[dict]:
        return [dict(row) for row in client.query(sql).result()]

    return _run


def network_disabled(sql: str) -> list[dict]:  # noqa: ARG001
    raise RuntimeError("no query runner configured; pass one (e.g. bigquery_runner(project))")


__all__ = ["QueryRunner", "bigquery_runner", "network_disabled"]
