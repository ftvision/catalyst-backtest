"""QA check: validate a constructed candle series against an independent reference.

Loads a target series and a reference series from the Parquet store and reports
per-candle deviation (open/high/low/close) plus internal OHLC invariants. Exits
non-zero if the series fails (an invariant violation, or any field deviating from
the reference beyond the tolerance) so it can gate CI or a post-ingest step.

    # validate Base ETH against the ethereum reference (both already in the store)
    uv run python scripts/validate_market_data.py \
        --venue base --symbol ETH --interval 1h \
        --ref-venue ethereum --ref-symbol ETH

    # against an independent Binance reference you ingested as venue=binance:
    #   uv run python -m catalyst_market_data.cli ingest-binance \
    #       --root data/market-data --venue binance --symbol ETH \
    #       --binance-symbol ETHUSDT --interval 1h --start ... --end ...
    # then --ref-venue binance

See docs/market-data-construction.md for why we validate this way.
"""

from __future__ import annotations

import argparse
import json
from datetime import UTC, datetime

from catalyst_market_data_core import ParquetSource, ParquetStore, compare_to_reference


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(UTC)


def main() -> int:
    ap = argparse.ArgumentParser(prog="validate_market_data")
    ap.add_argument("--root", default="data/market-data")
    ap.add_argument("--venue", required=True)
    ap.add_argument("--symbol", required=True)
    ap.add_argument("--interval", required=True)
    ap.add_argument("--ref-venue", required=True, help="Reference series venue (e.g. ethereum, binance)")
    ap.add_argument("--ref-symbol", default=None, help="Reference symbol (defaults to --symbol)")
    ap.add_argument("--start", type=_dt, default=datetime(2024, 1, 1, tzinfo=UTC))
    ap.add_argument("--end", type=_dt, default=datetime(2027, 1, 1, tzinfo=UTC))
    ap.add_argument("--tolerance", type=float, default=0.02, help="Max allowed deviation (0.02=2%)")
    ap.add_argument("--write", action="store_true", help="Record the report in _validation.json")
    args = ap.parse_args()

    ref_symbol = args.ref_symbol or args.symbol
    src = ParquetSource(args.root, args.start, args.end, args.interval)
    target = src.candles(args.venue, args.symbol)
    reference = src.candles(args.ref_venue, ref_symbol)

    if not target:
        print(f"no candles for {args.venue}/{args.symbol}/{args.interval}")
        return 2
    if not reference:
        print(f"no reference candles for {args.ref_venue}/{ref_symbol}/{args.interval}")
        return 2

    ref_label = f"{args.ref_venue}/{ref_symbol}"
    report = compare_to_reference(
        target,
        reference,
        venue=args.venue,
        symbol=args.symbol,
        interval=args.interval,
        reference_label=ref_label,
        tolerance=args.tolerance,
    )

    status = "PASS" if report.passed else "FAIL"
    print(f"[{status}] {args.venue}/{args.symbol}/{args.interval} vs {ref_label}")
    print(
        f"  candles={report.total} compared={report.compared} "
        f"missing_ref={report.missing_reference} invariant_violations={report.invariant_violations}"
    )
    print(f"  field    median%   p99%     max%    over_{args.tolerance:g}")
    for f in ("open", "high", "low", "close"):
        d = report.deviations[f]
        print(f"  {f:<6} {d.median_pct:8.3f} {d.p99_pct:8.3f} {d.max_pct:8.3f}   {d.over_tolerance}")
    if report.worst and not report.passed:
        print("  worst deviations:")
        for w in report.worst[:8]:
            print(
                f"    {w['ts']}  {w['field']}: ours={w['ours']:.2f} "
                f"ref={w['reference']:.2f}  dev={w['deviation_pct']:.2f}%"
            )

    if args.write:
        store = ParquetStore(args.root)
        path = store.root / "_validation.json"
        manifest = {}
        if path.exists():
            try:
                manifest = json.loads(path.read_text())
            except ValueError:
                manifest = {}
        manifest[f"candles/{args.venue}/{args.symbol}/{args.interval}"] = report.as_dict()
        store.root.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(manifest, indent=2, sort_keys=True))
        print(f"  recorded -> {path}")

    return 0 if report.passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
