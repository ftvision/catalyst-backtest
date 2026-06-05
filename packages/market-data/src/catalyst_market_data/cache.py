"""Local filesystem cache for normalized market data bundles.

Cache layout (under ``root``, default ``./data/market-data``)::

    <root>/<key>.json

where ``<key>`` is a stable hash of the requested range, interval, and the
series keys present in the bundle. This keeps fetched data local so repeat runs
of the same graph/range avoid refetching.
"""

from __future__ import annotations

import hashlib
import json
from datetime import datetime
from pathlib import Path

from catalyst_contracts import MarketDataBundle

DEFAULT_CACHE_ROOT = Path("data/market-data")


def bundle_key(*, start: datetime, end: datetime, interval: str, requirements: object) -> str:
    """Deterministic cache key for a planned bundle."""

    payload = json.dumps(
        {
            "start": start.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "end": end.strftime("%Y-%m-%dT%H:%M:%SZ"),
            "interval": interval,
            "requirements": requirements,
        },
        sort_keys=True,
        default=str,
    )
    return hashlib.sha256(payload.encode()).hexdigest()[:16]


class BundleCache:
    """Reads/writes ``MarketDataBundle`` JSON files under a cache root."""

    def __init__(self, root: str | Path = DEFAULT_CACHE_ROOT) -> None:
        self.root = Path(root)

    def _path(self, key: str) -> Path:
        return self.root / f"{key}.json"

    def get(self, key: str) -> MarketDataBundle | None:
        path = self._path(key)
        if not path.exists():
            return None
        return MarketDataBundle.model_validate_json(path.read_text())

    def put(self, key: str, bundle: MarketDataBundle) -> Path:
        self.root.mkdir(parents=True, exist_ok=True)
        path = self._path(key)
        path.write_text(bundle.model_dump_json(indent=2))
        return path


__all__ = ["BundleCache", "bundle_key", "DEFAULT_CACHE_ROOT"]
