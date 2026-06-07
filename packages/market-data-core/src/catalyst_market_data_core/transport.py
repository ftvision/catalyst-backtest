"""Injected HTTP transport shared by REST-based ingestion vendors.

Vendor ingesters take a ``Transport`` so fetching is testable offline (pass a
fake), with a real one created lazily from ``httpx`` only when network is needed.
The default transport refuses to make calls, so a misconfigured ingester fails
loudly instead of hitting the network unexpectedly.

A ``Transport`` is a single callable covering both GET and POST:

    transport(method, url, *, headers=None, params=None, json=None) -> parsed JSON
"""

from __future__ import annotations

from typing import Any, Callable

Transport = Callable[..., Any]


def http_transport(timeout: float = 60.0) -> Transport:
    """A real HTTP transport backed by httpx (imported lazily)."""

    import httpx

    def _request(
        method: str,
        url: str,
        *,
        headers: dict | None = None,
        params: dict | None = None,
        json: Any | None = None,
    ) -> Any:
        resp = httpx.request(method, url, headers=headers, params=params, json=json, timeout=timeout)
        resp.raise_for_status()
        return resp.json()

    return _request


def network_disabled(*args: Any, **kwargs: Any) -> Any:  # noqa: ARG001
    raise RuntimeError("no transport configured; pass one (e.g. http_transport())")


__all__ = ["Transport", "http_transport", "network_disabled"]
