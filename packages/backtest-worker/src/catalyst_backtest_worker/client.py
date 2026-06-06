"""Clients for the Rust simulation service.

The HTTP transport is injected so the worker can be tested offline and never
forces a particular HTTP library on callers. A `CallableSimulationClient` adapts
any function into a client (used in tests and in-process wiring).
"""

from __future__ import annotations

from typing import Any, Callable, Protocol

from catalyst_contracts import MarketDataBundle, SimulationTrace

DEFAULT_SIMULATE_PATH = "/simulate"


class SimulationClient(Protocol):
    """Runs a simulation and returns the resulting trace."""

    def simulate(
        self,
        *,
        graph: dict,
        config: dict,
        policy: dict,
        market_data: MarketDataBundle,
    ) -> SimulationTrace: ...


def _build_payload(graph: dict, config: dict, policy: dict, market_data: MarketDataBundle) -> dict:
    return {
        "graph": graph,
        "config": config,
        "policy": policy,
        "market_data": market_data.model_dump(mode="json", exclude_none=True),
    }


class CallableSimulationClient:
    """Adapts ``fn(payload: dict) -> trace dict`` into a `SimulationClient`."""

    def __init__(self, fn: Callable[[dict], dict]) -> None:
        self._fn = fn

    def simulate(self, *, graph, config, policy, market_data) -> SimulationTrace:
        payload = _build_payload(graph, config, policy, market_data)
        return SimulationTrace.model_validate(self._fn(payload))


class NetworkDisabledError(RuntimeError):
    """Raised when an HTTP client is used without a real transport."""


def _disabled_transport(url: str, body: dict) -> Any:  # noqa: ARG001
    raise NetworkDisabledError(
        "no HTTP transport configured; inject one or use CallableSimulationClient"
    )


class HttpSimulationClient:
    """Posts to the simulation service over an injected transport.

    ``transport(url, json_body) -> parsed JSON`` keeps the HTTP library a caller
    concern (and tests offline).
    """

    def __init__(
        self,
        base_url: str,
        transport: Callable[[str, dict], Any] | None = None,
        path: str = DEFAULT_SIMULATE_PATH,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.path = path
        self._transport = transport or _disabled_transport

    @property
    def url(self) -> str:
        return f"{self.base_url}{self.path}"

    def simulate(self, *, graph, config, policy, market_data) -> SimulationTrace:
        payload = _build_payload(graph, config, policy, market_data)
        response = self._transport(self.url, payload)
        return SimulationTrace.model_validate(response)


__all__ = [
    "SimulationClient",
    "CallableSimulationClient",
    "HttpSimulationClient",
    "NetworkDisabledError",
]
