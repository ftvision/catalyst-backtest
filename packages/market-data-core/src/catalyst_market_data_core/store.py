"""Parquet-backed historical market-data store and a ``MarketDataSource`` over it.

This is the durable *series store* — the source of truth for deep history —
which sits upstream of the per-run ``BundleCache``. Data is laid out as Parquet
with Hive-style partitioning so reads prune by date and project columns:

    <root>/candles/venue=<v>/symbol=<s>/interval=<i>/<YYYY-MM-DD>.parquet
    <root>/funding/venue=<v>/symbol=<s>/<YYYY-MM-DD>.parquet
    <root>/gas/chain=<c>/<YYYY-MM-DD>.parquet
    <root>/yields/protocol=<p>/asset=<a>/chain=<c>/pool=<pool>/<YYYY-MM-DD>.parquet

Decimal/quantity columns are stored as **strings** to preserve precision and
match the contract directly (the storage-schema contract; a future revision may
switch to Parquet ``Decimal128``). Timestamps are stored as UTC ``timestamp[us]``.

The same layout is what the Rust loader reads directly (issue #29); keeping it
documented here is the cross-language storage contract.
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Iterable

import pyarrow as pa
import pyarrow.parquet as pq

from catalyst_contracts import Candle
from catalyst_contracts.market_data import FundingPoint, GasPoint, LiquidityPoint, YieldPoint

# Column schemas per series (ts + decimal-string value columns).
_CANDLE_COLS = ["ts", "open", "high", "low", "close", "volume"]
_FUNDING_COLS = ["ts", "rate"]
_GAS_COLS = ["ts", "gas_usd"]
_YIELD_COLS = ["ts", "apr"]
_LIQUIDITY_COLS = ["ts", "reserve_base", "reserve_quote"]


def _date(ts: datetime) -> str:
    return ts.date().isoformat()


def _ts_schema(value_cols: list[str]) -> pa.Schema:
    fields = [pa.field("ts", pa.timestamp("us", tz="UTC"))]
    fields += [pa.field(c, pa.string()) for c in value_cols if c != "ts"]
    return pa.schema(fields)


class ParquetStore:
    """Writes/reads the partitioned Parquet store. Writes merge by timestamp."""

    def __init__(self, root: str | Path) -> None:
        self.root = Path(root)

    # --- provenance manifest (#38) ---
    #
    # A small sidecar (`<root>/_provenance.json`) recording, per series, whether
    # its data is venue-`native` (the venue's own price/feed), a `reference`
    # proxy (e.g. a CEX price stored under another venue), or `derived`. The Rust
    # loader reads it to label provider metadata so results can tell them apart.

    def _provenance_path(self) -> Path:
        return self.root / "_provenance.json"

    def read_provenance(self) -> dict[str, str]:
        path = self._provenance_path()
        if not path.exists():
            return {}
        try:
            data = json.loads(path.read_text())
            return data if isinstance(data, dict) else {}
        except (ValueError, OSError):
            return {}

    def set_provenance(self, kind: str, key: str, provenance: str) -> None:
        """Record provenance for a series, keyed as ``"<kind>/<key>"`` (e.g.
        ``"candles/hyperliquid/ETH"``)."""
        manifest = self.read_provenance()
        manifest[f"{kind}/{key}"] = provenance
        self.root.mkdir(parents=True, exist_ok=True)
        self._provenance_path().write_text(json.dumps(manifest, indent=2, sort_keys=True))

    # --- quality manifest ---
    #
    # A sidecar (`<root>/_quality.json`) recording, per series, the result of
    # ingestion-time data cleaning (outliers removed, method, affected ranges).
    # Keyed like provenance, as ``"<kind>/<key>"``.

    def _quality_path(self) -> Path:
        return self.root / "_quality.json"

    def read_quality(self) -> dict[str, dict]:
        path = self._quality_path()
        if not path.exists():
            return {}
        try:
            data = json.loads(path.read_text())
            return data if isinstance(data, dict) else {}
        except (ValueError, OSError):
            return {}

    def set_quality(self, kind: str, key: str, report: dict) -> None:
        """Record a data-quality report for a series, keyed ``"<kind>/<key>"``."""
        manifest = self.read_quality()
        manifest[f"{kind}/{key}"] = report
        self.root.mkdir(parents=True, exist_ok=True)
        self._quality_path().write_text(json.dumps(manifest, indent=2, sort_keys=True))

    # --- partition paths ---

    def _candle_dir(self, venue: str, symbol: str, interval: str) -> Path:
        return (
            self.root / "candles" / f"venue={venue}" / f"symbol={symbol}" / f"interval={interval}"
        )

    def _funding_dir(self, venue: str, symbol: str) -> Path:
        return self.root / "funding" / f"venue={venue}" / f"symbol={symbol}"

    def _gas_dir(self, chain: str) -> Path:
        return self.root / "gas" / f"chain={chain}"

    def _liquidity_dir(self, venue: str, symbol: str) -> Path:
        return self.root / "liquidity" / f"venue={venue}" / f"symbol={symbol}"

    def _yield_dir(self, protocol: str, asset: str, chain: str, pool: str | None) -> Path:
        return (
            self.root
            / "yields"
            / f"protocol={protocol}"
            / f"asset={asset}"
            / f"chain={chain}"
            / f"pool={pool or '_none'}"
        )

    # --- writes (grouped by date, merged with existing by ts) ---

    def _write_rows(self, base: Path, cols: list[str], rows: list[dict]) -> None:
        by_date: dict[str, list[dict]] = {}
        for row in rows:
            by_date.setdefault(_date(row["ts"]), []).append(row)
        base.mkdir(parents=True, exist_ok=True)
        for date, day_rows in by_date.items():
            path = base / f"{date}.parquet"
            merged: dict[datetime, dict] = {}
            if path.exists():
                # Read the file directly (not the dataset API) so Hive partition
                # columns from the path aren't injected as extra fields.
                existing = pq.ParquetFile(path).read().to_pylist()
                for r in existing:
                    merged[r["ts"]] = r
            for r in day_rows:
                merged[r["ts"]] = r
            ordered = [merged[k] for k in sorted(merged)]
            table = pa.Table.from_pylist(ordered, schema=_ts_schema(cols))
            pq.write_table(table, path)

    def write_candles(
        self, venue: str, symbol: str, interval: str, candles: Iterable[Candle]
    ) -> int:
        rows = [
            {
                "ts": c.ts,
                "open": c.open,
                "high": c.high,
                "low": c.low,
                "close": c.close,
                "volume": c.volume,
            }
            for c in candles
        ]
        self._write_rows(self._candle_dir(venue, symbol, interval), _CANDLE_COLS, rows)
        return len(rows)

    def write_funding(self, venue: str, symbol: str, points: Iterable[FundingPoint]) -> int:
        rows = [{"ts": p.ts, "rate": p.rate} for p in points]
        self._write_rows(self._funding_dir(venue, symbol), _FUNDING_COLS, rows)
        return len(rows)

    def write_gas(self, chain: str, points: Iterable[GasPoint]) -> int:
        rows = [{"ts": p.ts, "gas_usd": p.gas_usd} for p in points]
        self._write_rows(self._gas_dir(chain), _GAS_COLS, rows)
        return len(rows)

    def write_yields(
        self, protocol: str, asset: str, chain: str, pool: str | None, points: Iterable[YieldPoint]
    ) -> int:
        rows = [{"ts": p.ts, "apr": p.apr} for p in points]
        self._write_rows(self._yield_dir(protocol, asset, chain, pool), _YIELD_COLS, rows)
        return len(rows)

    def write_liquidity(self, venue: str, symbol: str, points: Iterable[LiquidityPoint]) -> int:
        rows = [
            {"ts": p.ts, "reserve_base": p.reserve_base, "reserve_quote": p.reserve_quote}
            for p in points
        ]
        self._write_rows(self._liquidity_dir(venue, symbol), _LIQUIDITY_COLS, rows)
        return len(rows)

    # --- range read (partition-pruned by date, window-filtered by ts) ---

    def _read_window(self, base: Path, start: datetime, end: datetime) -> list[dict]:
        if not base.exists():
            return []
        start_date, end_date = start.date(), end.date()
        rows: list[dict] = []
        for path in sorted(base.glob("*.parquet")):
            try:
                file_date = datetime.fromisoformat(path.stem).date()
            except ValueError:
                continue
            if file_date < start_date or file_date > end_date:
                continue  # partition pruning
            for row in pq.ParquetFile(path).read().to_pylist():
                if start <= row["ts"] <= end:
                    rows.append(row)
        rows.sort(key=lambda r: r["ts"])
        return rows

    def coverage(self, base: Path) -> tuple[datetime, datetime] | None:
        """(min_ts, max_ts) available for a series, or None if absent."""
        if not base.exists():
            return None
        lo = hi = None
        for path in sorted(base.glob("*.parquet")):
            col = pq.ParquetFile(path).read(columns=["ts"]).column("ts").to_pylist()
            if not col:
                continue
            lo = min(col) if lo is None else min(lo, min(col))
            hi = max(col) if hi is None else max(hi, max(col))
        return (lo, hi) if lo is not None else None


class ParquetSource:
    """A ``MarketDataSource`` reading the Parquet store for a fixed window/interval."""

    name = "parquet-store"

    def __init__(self, root: str | Path, start: datetime, end: datetime, interval: str) -> None:
        self._store = ParquetStore(root)
        self._start = start
        self._end = end
        self._interval = interval

    def candles(self, venue: str, symbol: str) -> list[Candle]:
        rows = self._store._read_window(
            self._store._candle_dir(venue, symbol, self._interval), self._start, self._end
        )
        return [Candle(**r) for r in rows]

    def funding(self, venue: str, symbol: str) -> list[FundingPoint]:
        rows = self._store._read_window(
            self._store._funding_dir(venue, symbol), self._start, self._end
        )
        return [FundingPoint(**r) for r in rows]

    def gas(self, chain: str) -> list[GasPoint]:
        rows = self._store._read_window(self._store._gas_dir(chain), self._start, self._end)
        return [GasPoint(**r) for r in rows]

    def yields(self, protocol: str, asset: str, chain: str, pool: str | None) -> list[YieldPoint]:
        rows = self._store._read_window(
            self._store._yield_dir(protocol, asset, chain, pool), self._start, self._end
        )
        return [YieldPoint(**r) for r in rows]


__all__ = ["ParquetStore", "ParquetSource"]
