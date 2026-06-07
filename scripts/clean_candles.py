"""Clean OHLC outliers from an *already-stored* candle series, in place.

Re-cleans existing Parquet candles without re-querying the source (no Dune
credits). Reads the whole series so the rolling-median test spans file
boundaries, then rewrites the affected partition files. By default it also
**repairs** implausible high/low wicks down to the candle body (open/close),
which is safe when open/close are trusted; pass ``--no-repair-wicks`` to remove
wick-bad candles instead. A quality report is recorded in ``_quality.json``.

    uv run python scripts/clean_candles.py \
        --root data/market-data --venue base --symbol ETH --interval 1h

Removed candles become honest gaps (re-ingest with the fixed query to refill).
"""

from __future__ import annotations

import argparse
from collections import defaultdict

import pyarrow as pa
import pyarrow.parquet as pq

from catalyst_contracts import Candle
from catalyst_market_data_core import ParquetStore, filter_candle_outliers
from catalyst_market_data_core.store import _CANDLE_COLS, _ts_schema


def _row_to_candle(row: dict) -> Candle:
    return Candle(
        ts=row["ts"],
        open=str(row["open"]),
        high=str(row["high"]),
        low=str(row["low"]),
        close=str(row["close"]),
        volume=str(row["volume"]) if row.get("volume") is not None else None,
    )


def _candle_row(c: Candle) -> dict:
    return {
        "ts": c.ts,
        "open": c.open,
        "high": c.high,
        "low": c.low,
        "close": c.close,
        "volume": c.volume,
    }


def main() -> int:
    ap = argparse.ArgumentParser(prog="clean_candles")
    ap.add_argument("--root", default="data/market-data")
    ap.add_argument("--venue", required=True)
    ap.add_argument("--symbol", required=True)
    ap.add_argument("--interval", required=True)
    ap.add_argument("--tolerance", type=float, default=0.5)
    ap.add_argument("--window", type=int, default=11)
    ap.add_argument(
        "--repair-wicks",
        action=argparse.BooleanOptionalAction,
        default=True,
        help="Collapse implausible high/low wicks to the body (default) vs remove the candle.",
    )
    ap.add_argument(
        "--wick-tolerance",
        type=float,
        default=0.2,
        help="In repair mode, max wick extension past the body before it's collapsed (0.2=20%%).",
    )
    ap.add_argument("--dry-run", action="store_true", help="Report only; don't rewrite files.")
    args = ap.parse_args()

    store = ParquetStore(args.root)
    base = store._candle_dir(args.venue, args.symbol, args.interval)
    files = sorted(base.glob("*.parquet"))
    if not files:
        print(f"no candles under {base}")
        return 1

    all_candles = [_row_to_candle(r) for f in files for r in pq.ParquetFile(f).read().to_pylist()]
    kept, report = filter_candle_outliers(
        all_candles,
        window=args.window,
        tolerance=args.tolerance,
        repair_wicks=args.repair_wicks,
        wick_tolerance=args.wick_tolerance,
    )

    print(
        f"{args.venue}/{args.symbol}/{args.interval}: {report.total} candles, "
        f"removed {report.outliers_removed} ({len(report.affected_ranges)} ranges), "
        f"repaired {report.wicks_repaired} wicks ({len(report.repaired_ranges)} ranges)"
    )

    if args.dry_run:
        print("(dry run; no files changed)")
        return 0

    quality_key = f"{args.venue}/{args.symbol}/{args.interval}"
    orig = {c.ts: (c.high, c.low) for c in all_candles}
    kept_by_ts = {c.ts: c for c in kept}
    removed_ts = set(orig) - set(kept_by_ts)

    # Partition dates touched by a removal or a wick repair.
    touched: set[str] = {ts.date().isoformat() for ts in removed_ts}
    for ts, c in kept_by_ts.items():
        if orig[ts] != (c.high, c.low):
            touched.add(ts.date().isoformat())

    if not touched:
        print("nothing to change")
        store.set_quality("candles", quality_key, report.as_dict())
        return 0

    # Rewrite each touched partition from the kept (possibly repaired) candles.
    by_date: dict[str, list[Candle]] = defaultdict(list)
    for c in kept:
        by_date[c.ts.date().isoformat()].append(c)
    schema = _ts_schema(_CANDLE_COLS)
    for date in touched:
        path = base / f"{date}.parquet"
        day = sorted(by_date.get(date, []), key=lambda c: c.ts)
        if not day:
            path.unlink(missing_ok=True)
            continue
        table = pa.Table.from_pylist([_candle_row(c) for c in day], schema=schema)
        pq.write_table(table, path)

    store.set_quality("candles", quality_key, report.as_dict())
    print(f"done; rewrote {len(touched)} partition(s); quality under candles/{quality_key}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
