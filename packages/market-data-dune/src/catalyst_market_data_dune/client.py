"""Minimal Dune Analytics API client (saved-query execution).

Runs a **saved query** by id: execute -> poll status -> fetch result rows. The
HTTP transport is injected (see ``catalyst_market_data_core.http_transport``) so
the flow is testable offline, and `sleep` is injectable so tests don't wait.

Dune is research/analytics data (decoded on-chain tables, curated `prices.usd`,
etc.), not a live feed — results lag and execution is credit-metered. We use it
to backfill the durable Parquet store, not on any run path.
"""

from __future__ import annotations

import time
from typing import Any, Callable

from catalyst_market_data_core import Transport, network_disabled

DUNE_API = "https://api.dune.com/api/v1"

_DONE = "QUERY_STATE_COMPLETED"
_FAILED = {"QUERY_STATE_FAILED", "QUERY_STATE_CANCELLED", "QUERY_STATE_EXPIRED"}


class DuneClient:
    """Execute a saved Dune query and return its result rows."""

    def __init__(
        self,
        api_key: str,
        transport: Transport | None = None,
        *,
        sleep: Callable[[float], None] | None = None,
        poll_interval: float = 2.0,
        max_polls: int = 120,
    ) -> None:
        self._key = api_key
        self._t = transport or network_disabled
        self._sleep = sleep or time.sleep
        self._poll_interval = poll_interval
        self._max_polls = max_polls

    def _headers(self) -> dict:
        return {"X-Dune-API-Key": self._key}

    def run_query(self, query_id: int, params: dict | None = None) -> list[dict[str, Any]]:
        """Execute saved query ``query_id`` with ``params`` and return its rows."""

        started = self._t(
            "POST",
            f"{DUNE_API}/query/{query_id}/execute",
            headers=self._headers(),
            json={"query_parameters": params or {}},
        )
        execution_id = started.get("execution_id")
        if not execution_id:
            raise RuntimeError(f"Dune execute returned no execution_id: {started}")

        for _ in range(self._max_polls):
            status = self._t(
                "GET", f"{DUNE_API}/execution/{execution_id}/status", headers=self._headers()
            )
            state = status.get("state")
            if state == _DONE:
                break
            if state in _FAILED:
                raise RuntimeError(f"Dune query {query_id} ended in state {state}")
            self._sleep(self._poll_interval)
        else:
            raise RuntimeError(f"Dune query {query_id} did not complete in time")

        results = self._t(
            "GET", f"{DUNE_API}/execution/{execution_id}/results", headers=self._headers()
        )
        rows = results.get("result", {}).get("rows")
        if rows is None:
            raise RuntimeError(f"Dune results missing rows: {results}")
        return rows


__all__ = ["DuneClient", "DUNE_API"]
