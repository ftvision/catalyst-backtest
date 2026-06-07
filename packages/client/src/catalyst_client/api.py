"""Typed HTTP client over the Catalyst backtest service.

Wraps the service endpoints (run lifecycle + workbench setup) with a small,
synchronous :class:`CatalystClient`. Errors come back as the service's
``{"error": {code, message}, ...}`` envelope and are raised as :class:`ApiError`.
"""

from __future__ import annotations

import os
import time
from typing import Any, Callable

import httpx

DEFAULT_BASE_URL = "https://catalyst-backtest-api.fly.dev"
TERMINAL_STATUSES = frozenset({"succeeded", "failed"})


class ApiError(RuntimeError):
    """A non-2xx response from the service, carrying the parsed error envelope."""

    def __init__(
        self, status_code: int, code: str, message: str, extra: dict[str, Any] | None = None
    ):
        self.status_code = status_code
        self.code = code
        self.message = message
        self.extra = extra or {}
        super().__init__(f"[{status_code} {code}] {message}")


class BacktestFailed(ApiError):
    """The run reached a terminal ``failed`` status."""


def _raise_for_status(resp: httpx.Response) -> None:
    if resp.is_success:
        return
    code, message, extra = "http_error", resp.text, {}
    try:
        body = resp.json()
        if isinstance(body, dict) and isinstance(body.get("error"), dict):
            code = body["error"].get("code", code)
            message = body["error"].get("message", message)
            extra = {k: v for k, v in body.items() if k != "error"}
    except ValueError:
        pass
    raise ApiError(resp.status_code, code, message, extra)


class CatalystClient:
    """Synchronous client for the backtest service.

    ``base_url`` falls back to ``$CATALYST_API_URL`` then the deployed Fly URL.
    Use as a context manager to close the underlying connection pool.
    """

    def __init__(
        self,
        base_url: str | None = None,
        *,
        timeout: float = 30.0,
        transport: httpx.BaseTransport | None = None,
    ):
        self.base_url = (base_url or os.environ.get("CATALYST_API_URL") or DEFAULT_BASE_URL).rstrip(
            "/"
        )
        self._http = httpx.Client(base_url=self.base_url, timeout=timeout, transport=transport)

    def __enter__(self) -> "CatalystClient":
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def close(self) -> None:
        self._http.close()

    # --- low-level ---

    def _get(self, path: str, **params: Any) -> dict[str, Any]:
        resp = self._http.get(path, params={k: v for k, v in params.items() if v is not None})
        _raise_for_status(resp)
        return resp.json()

    def _post(self, path: str, body: dict[str, Any]) -> httpx.Response:
        resp = self._http.post(path, json=body)
        _raise_for_status(resp)
        return resp

    # --- health ---

    def health(self) -> dict[str, Any]:
        return self._get("/health")

    # --- run lifecycle (async) ---

    def submit(self, body: dict[str, Any]) -> str:
        """Enqueue a backtest; returns the run id (202 Accepted)."""
        return self._post("/backtests", body).json()["id"]

    def status(self, run_id: str) -> dict[str, Any]:
        return self._get(f"/backtests/{run_id}")

    def result(self, run_id: str) -> dict[str, Any]:
        return self._get(f"/backtests/{run_id}/result")

    def metadata(self, run_id: str) -> dict[str, Any]:
        return self._get(f"/backtests/{run_id}/metadata")

    def events(
        self,
        run_id: str,
        *,
        type: str | None = None,
        node_id: str | None = None,
        status: str | None = None,
        cursor: int = 0,
        limit: int = 100,
    ) -> dict[str, Any]:
        return self._get(
            f"/backtests/{run_id}/events",
            type=type,
            node_id=node_id,
            status=status,
            cursor=cursor,
            limit=limit,
        )

    def list_backtests(self, graph_hash: str | None = None) -> dict[str, Any]:
        return self._get("/backtests", graph_hash=graph_hash)

    def wait(
        self,
        run_id: str,
        *,
        poll_interval: float = 1.0,
        timeout: float = 300.0,
        on_update: Callable[[dict[str, Any]], None] | None = None,
    ) -> dict[str, Any]:
        """Poll until the run is terminal; return its final status record.

        Raises :class:`BacktestFailed` if it ends ``failed``, or :class:`TimeoutError`
        if ``timeout`` elapses first.
        """
        deadline = time.monotonic() + timeout
        last_status: str | None = None
        while True:
            record = self.status(run_id)
            current = record.get("status")
            if on_update is not None and current != last_status:
                on_update(record)
            last_status = current
            if current in TERMINAL_STATUSES:
                if current == "failed":
                    raise BacktestFailed(
                        422, "backtest_failed", record.get("error") or "run failed"
                    )
                return record
            if time.monotonic() >= deadline:
                raise TimeoutError(
                    f"backtest {run_id} not finished after {timeout}s (status={current})"
                )
            time.sleep(poll_interval)

    def run(
        self,
        body: dict[str, Any],
        *,
        poll_interval: float = 1.0,
        timeout: float = 300.0,
        on_update: Callable[[dict[str, Any]], None] | None = None,
    ) -> dict[str, Any]:
        """Submit, wait for completion, and return the full result."""
        run_id = self.submit(body)
        if on_update is not None:
            on_update({"id": run_id, "status": "queued"})
        self.wait(run_id, poll_interval=poll_interval, timeout=timeout, on_update=on_update)
        return self.result(run_id)

    # --- low-level synchronous engine call (no persistence) ---

    def simulate(self, body: dict[str, Any]) -> dict[str, Any]:
        """Run the engine inline and return the raw trace (blocks until done)."""
        return self._post("/simulate", body).json()

    # --- workbench setup ---

    def preview(self, graph: dict[str, Any], policy_profile: str = "strict_v1") -> dict[str, Any]:
        return self._post(
            "/backtests/preview", {"graph": graph, "policy": {"profile": policy_profile}}
        ).json()

    def coverage(
        self, graph: dict[str, Any], start: str, end: str, interval: str
    ) -> dict[str, Any]:
        body = {"graph": graph, "start": start, "end": end, "interval": interval}
        return self._post("/market-data/coverage", body).json()

    def catalog(self) -> dict[str, Any]:
        return self._get("/market-data/catalog")

    def policy_profiles(self) -> dict[str, Any]:
        return self._get("/policy-profiles")

    def strategies(self) -> dict[str, Any]:
        return self._get("/strategies")

    def strategy(self, strategy_id: str) -> dict[str, Any]:
        return self._get(f"/strategies/{strategy_id}")
