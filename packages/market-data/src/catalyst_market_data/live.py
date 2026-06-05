"""Live market data source abstractions.

Network access is *injected* via a :class:`Transport`, so request building and
response parsing are unit-testable offline (and the package never forces a real
HTTP client on consumers). The default transport refuses to make calls, which
keeps fixture-backed runs honest.

- :class:`HyperliquidSource` implements the real Hyperliquid ``info`` API shapes
  for candles and funding.
- :class:`CallableGasSource` / :class:`CallableYieldSource` are thin abstractions
  that normalize whatever a provided fetch callable returns (Base RPC gas, Aave
  subgraph yields, etc. are wired in by the caller).
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any, Callable, Protocol

from catalyst_contracts import Candle
from catalyst_contracts.market_data import FundingPoint, GasPoint, YieldPoint

HYPERLIQUID_INFO_URL = "https://api.hyperliquid.xyz/info"


class Transport(Protocol):
    """Minimal HTTP transport: returns parsed JSON for a POST request."""

    def post(self, url: str, body: dict[str, Any]) -> Any: ...


class NetworkDisabledTransport:
    """Default transport that refuses to make network calls."""

    def post(self, url: str, body: dict[str, Any]) -> Any:  # noqa: ARG002
        raise RuntimeError(
            "network is disabled; inject a real Transport to fetch live data, "
            "or use FixtureSource for offline bundles"
        )


def _ms_to_iso(ms: int) -> str:
    return datetime.fromtimestamp(ms / 1000, tz=UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _to_ms(ts: datetime) -> int:
    return int(ts.timestamp() * 1000)


class HyperliquidSource:
    """Hyperliquid spot/perp candles and funding via the ``info`` endpoint."""

    name = "hyperliquid"

    def __init__(
        self,
        start: datetime,
        end: datetime,
        interval: str,
        transport: Transport | None = None,
        url: str = HYPERLIQUID_INFO_URL,
    ) -> None:
        self._start = start
        self._end = end
        self._interval = interval
        self._transport = transport or NetworkDisabledTransport()
        self._url = url

    def candles(self, venue: str, symbol: str) -> list[Candle]:  # noqa: ARG002
        body = {
            "type": "candleSnapshot",
            "req": {
                "coin": symbol,
                "interval": self._interval,
                "startTime": _to_ms(self._start),
                "endTime": _to_ms(self._end),
            },
        }
        rows = self._transport.post(self._url, body) or []
        return [
            Candle(
                ts=_ms_to_iso(int(row["t"])),
                open=str(row["o"]),
                high=str(row["h"]),
                low=str(row["l"]),
                close=str(row["c"]),
                volume=str(row["v"]) if "v" in row else None,
            )
            for row in rows
        ]

    def funding(self, venue: str, symbol: str) -> list[FundingPoint]:  # noqa: ARG002
        body = {
            "type": "fundingHistory",
            "coin": symbol,
            "startTime": _to_ms(self._start),
            "endTime": _to_ms(self._end),
        }
        rows = self._transport.post(self._url, body) or []
        return [
            FundingPoint(ts=_ms_to_iso(int(row["time"])), rate=str(row["fundingRate"]))
            for row in rows
        ]

    def gas(self, chain: str) -> list[GasPoint]:  # noqa: ARG002
        return []  # Hyperliquid carries no EVM gas

    def yields(self, protocol, asset, chain, pool) -> list[YieldPoint]:  # noqa: ARG002
        return []


class CallableGasSource:
    """Normalizes gas data from an injected ``chain -> [(ts, gas_usd)]`` fetcher."""

    name = "evm-gas"

    def __init__(self, fetch: Callable[[str], list[tuple[str, str]]]) -> None:
        self._fetch = fetch

    def candles(self, venue, symbol) -> list[Candle]:  # noqa: ARG002
        return []

    def funding(self, venue, symbol) -> list[FundingPoint]:  # noqa: ARG002
        return []

    def gas(self, chain: str) -> list[GasPoint]:
        return [GasPoint(ts=ts, gas_usd=str(v)) for ts, v in self._fetch(chain)]

    def yields(self, protocol, asset, chain, pool) -> list[YieldPoint]:  # noqa: ARG002
        return []


class CallableYieldSource:
    """Normalizes yield data from an injected fetcher.

    The fetcher is called as ``fetch(protocol, asset, chain, pool)`` and returns
    ``[(ts, apr)]``.
    """

    name = "aave-yields"

    def __init__(self, fetch: Callable[[str, str, str, str | None], list[tuple[str, str]]]) -> None:
        self._fetch = fetch

    def candles(self, venue, symbol) -> list[Candle]:  # noqa: ARG002
        return []

    def funding(self, venue, symbol) -> list[FundingPoint]:  # noqa: ARG002
        return []

    def gas(self, chain) -> list[GasPoint]:  # noqa: ARG002
        return []

    def yields(self, protocol: str, asset: str, chain: str, pool: str | None) -> list[YieldPoint]:
        return [
            YieldPoint(ts=ts, apr=str(v)) for ts, v in self._fetch(protocol, asset, chain, pool)
        ]


class CompositeSource:
    """Routes each data kind to a dedicated source (HL candles, EVM gas, ...)."""

    name = "composite"

    def __init__(self, *, candles=None, funding=None, gas=None, yields=None) -> None:
        self._candles = candles
        self._funding = funding
        self._gas = gas
        self._yields = yields

    def candles(self, venue, symbol) -> list[Candle]:
        return self._candles.candles(venue, symbol) if self._candles else []

    def funding(self, venue, symbol) -> list[FundingPoint]:
        return self._funding.funding(venue, symbol) if self._funding else []

    def gas(self, chain) -> list[GasPoint]:
        return self._gas.gas(chain) if self._gas else []

    def yields(self, protocol, asset, chain, pool) -> list[YieldPoint]:
        return self._yields.yields(protocol, asset, chain, pool) if self._yields else []


__all__ = [
    "Transport",
    "NetworkDisabledTransport",
    "HyperliquidSource",
    "CallableGasSource",
    "CallableYieldSource",
    "CompositeSource",
    "HYPERLIQUID_INFO_URL",
]
