"""Candle outlier detection for ingestion-time data cleaning.

DEX-trade-derived candles (e.g. Base ETH from ``dex.trades``) can carry absurd
prices — scam tokens mislabeled ``WETH``, dust trades, or bad ``amount`` values —
producing highs in the trillions or lows of zero. We **remove** such candles
rather than clamp them: clamping invents a price the market never printed, which
would corrupt a simulation. A removed candle becomes an honest gap (surfaced by
the existing gap detection) and is recorded in the quality report.

The test is robustness-based: a candle is an outlier if any of its OHLC values
deviates from the *rolling median of neighboring closes* by more than a
tolerance. The rolling median ignores the candle itself, so a lone spike can't
hide inside its own window.
"""

from __future__ import annotations

import statistics
from dataclasses import dataclass, field
from datetime import UTC, datetime

from catalyst_contracts import Candle


@dataclass
class CandleQualityReport:
    """What the outlier filter did to a candle series."""

    total: int = 0
    kept: int = 0
    outliers_removed: int = 0
    wicks_repaired: int = 0
    method: str = ""
    tolerance: float = 0.0
    window: int = 0
    # Inclusive [first_ts, last_ts] ISO ranges of consecutively removed candles.
    affected_ranges: list[list[str]] = field(default_factory=list)
    # Inclusive ranges of candles whose high/low wick was collapsed to the body.
    repaired_ranges: list[list[str]] = field(default_factory=list)

    def as_dict(self) -> dict:
        return {
            "total": self.total,
            "kept": self.kept,
            "outliers_removed": self.outliers_removed,
            "wicks_repaired": self.wicks_repaired,
            "method": self.method,
            "tolerance": self.tolerance,
            "window": self.window,
            "affected_ranges": self.affected_ranges,
            "repaired_ranges": self.repaired_ranges,
        }


def _f(value: object) -> float | None:
    try:
        out = float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None
    return out


def _iso(ts: datetime) -> str:
    if ts.tzinfo is None:
        return ts.isoformat() + "Z"
    return ts.astimezone(UTC).isoformat().replace("+00:00", "Z")


def filter_candle_outliers(
    candles: list[Candle],
    *,
    window: int = 11,
    tolerance: float = 0.5,
    repair_wicks: bool = False,
    wick_tolerance: float = 0.2,
) -> tuple[list[Candle], CandleQualityReport]:
    """Clean candles against the rolling median of neighboring closes.

    ``window`` is the centered neighbor count for the median (the candle itself
    is excluded). ``tolerance`` is the max allowed relative deviation (0.5 = 50%);
    normal volatility stays well inside it while trillion-dollar highs and zero
    lows fall far outside.

    A candle whose **body** (open or close) is invalid or beyond ``tolerance`` is
    always **removed** — the body is untrustworthy. For the **wicks** (high/low):

    - ``repair_wicks=False`` (default): a candle with an out-of-band wick is also
      removed.
    - ``repair_wicks=True``: if only the wick is bad, it is **collapsed to the
      body** — high → max(open, close), low → min(open, close), reusing the exact
      open/close decimal strings so no new price is invented. A wick is bad if it
      is out-of-band vs the median *or* extends past the body by more than
      ``wick_tolerance`` (0.2 = 20%; the body is trusted, so an implausibly long
      wick is a bad print). Use this when open/close are validated (e.g. against a
      reference feed).

    Returns the kept candles (input order) and a :class:`CandleQualityReport`.
    """
    ordered = sorted(candles, key=lambda c: c.ts)
    closes = [_f(c.close) for c in ordered]
    half = max(1, window // 2)

    report = CandleQualityReport(
        total=len(ordered),
        method="rolling_median_deviation" + ("_with_wick_repair" if repair_wicks else ""),
        tolerance=tolerance,
        window=window,
    )

    kept: list[Candle] = []
    removed_run: list[datetime] = []
    repaired_run: list[datetime] = []

    def flush_removed() -> None:
        if removed_run:
            report.affected_ranges.append([_iso(removed_run[0]), _iso(removed_run[-1])])
            removed_run.clear()

    def flush_repaired() -> None:
        if repaired_run:
            report.repaired_ranges.append([_iso(repaired_run[0]), _iso(repaired_run[-1])])
            repaired_run.clear()

    def out_of_band(v: float | None) -> bool:
        if v is None or v <= 0:
            return True
        return med is not None and med > 0 and abs(v - med) / med > tolerance

    for i, candle in enumerate(ordered):
        lo = max(0, i - half)
        hi = min(len(ordered), i + half + 1)
        neighbors = [
            closes[j] for j in range(lo, hi) if j != i and closes[j] is not None and closes[j] > 0
        ]
        med = statistics.median(neighbors) if neighbors else None

        o, h, low_, c = _f(candle.open), _f(candle.high), _f(candle.low), _f(candle.close)
        body_bad = out_of_band(o) or out_of_band(c)

        # Body extremes (trusted in repair mode); a wick is bad if out-of-band vs
        # the median, or — in repair mode — it extends past the body too far.
        body_hi = max(o, c) if o is not None and c is not None else None
        body_lo = min(o, c) if o is not None and c is not None else None
        hi_bad = out_of_band(h)
        lo_bad = out_of_band(low_)
        if repair_wicks and not body_bad:
            if h is not None and body_hi and h > body_hi * (1 + wick_tolerance):
                hi_bad = True
            if low_ is not None and body_lo and low_ < body_lo * (1 - wick_tolerance):
                lo_bad = True
        wick_bad = hi_bad or lo_bad

        if body_bad or (wick_bad and not repair_wicks):
            report.outliers_removed += 1
            removed_run.append(candle.ts)
            flush_repaired()
            continue

        flush_removed()
        if wick_bad:  # repair_wicks is True and body is sound
            # Collapse only the bad side to the body, reusing exact open/close.
            update: dict[str, str] = {}
            if hi_bad:
                update["high"] = candle.open if (o or 0) >= (c or 0) else candle.close
            if lo_bad:
                update["low"] = candle.open if (o or 0) <= (c or 0) else candle.close
            candle = candle.model_copy(update=update)
            report.wicks_repaired += 1
            repaired_run.append(candle.ts)
        else:
            flush_repaired()
        kept.append(candle)
    flush_removed()
    flush_repaired()

    report.kept = len(kept)
    return kept, report


__all__ = ["filter_candle_outliers", "CandleQualityReport"]
