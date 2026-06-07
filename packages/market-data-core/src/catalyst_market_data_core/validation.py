"""Validate a constructed candle series against an independent reference.

Our store holds *constructed* market data — OHLC we derive from a chosen
methodology (see ``docs/market-data-construction.md``). No price is ground truth,
so the way to trust a series is **agreement with an independent construction**:
for a fungible asset like ETH, arbitrage pins cross-venue prices within a fraction
of a percent, so a clean reference (e.g. Binance klines, or our ethereum
``prices.usd`` series) is an excellent baseline.

This module provides two checks:

- :func:`check_ohlc_invariants` — internal sanity (no reference needed):
  positivity and OHLC ordering. Catches zero-lows and impossible bars.
- :func:`compare_to_reference` — per-candle deviation of close/high/low vs a
  reference series at the same timestamps. Catches the kind of one-sided wick
  artifact DEX-trade construction can introduce.
"""

from __future__ import annotations

import statistics
from dataclasses import dataclass, field
from datetime import datetime

from catalyst_contracts import Candle


@dataclass
class InvariantViolation:
    ts: datetime
    kind: str  # "non_positive" | "ohlc_order"


def check_ohlc_invariants(candles: list[Candle]) -> list[InvariantViolation]:
    """Every candle must have positive prices and obey low ≤ open,close ≤ high."""
    out: list[InvariantViolation] = []
    for c in candles:
        try:
            o, h, low_, cl = float(c.open), float(c.high), float(c.low), float(c.close)
        except (TypeError, ValueError):
            out.append(InvariantViolation(c.ts, "non_positive"))
            continue
        if min(o, h, low_, cl) <= 0:
            out.append(InvariantViolation(c.ts, "non_positive"))
        elif not (low_ <= min(o, cl) and h >= max(o, cl) and h >= low_):
            out.append(InvariantViolation(c.ts, "ohlc_order"))
    return out


@dataclass
class FieldDeviation:
    """Deviation stats for one OHLC field vs the reference (percent)."""

    field: str
    over_tolerance: int = 0
    median_pct: float = 0.0
    p99_pct: float = 0.0
    max_pct: float = 0.0

    def as_dict(self) -> dict:
        return {
            "field": self.field,
            "over_tolerance": self.over_tolerance,
            "median_pct": round(self.median_pct, 4),
            "p99_pct": round(self.p99_pct, 4),
            "max_pct": round(self.max_pct, 4),
        }


@dataclass
class ValidationReport:
    venue: str
    symbol: str
    interval: str
    reference: str
    tolerance: float
    total: int = 0
    compared: int = 0
    missing_reference: int = 0
    invariant_violations: int = 0
    deviations: dict[str, FieldDeviation] = field(default_factory=dict)
    worst: list[dict] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        if self.invariant_violations:
            return False
        return all(d.over_tolerance == 0 for d in self.deviations.values())

    def as_dict(self) -> dict:
        return {
            "venue": self.venue,
            "symbol": self.symbol,
            "interval": self.interval,
            "reference": self.reference,
            "tolerance": self.tolerance,
            "total": self.total,
            "compared": self.compared,
            "missing_reference": self.missing_reference,
            "invariant_violations": self.invariant_violations,
            "passed": self.passed,
            "deviations": {k: v.as_dict() for k, v in self.deviations.items()},
            "worst": self.worst,
        }


def _pct(a: float, b: float) -> float | None:
    return abs(a - b) / b if b else None


def _percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    s = sorted(values)
    return s[min(len(s) - 1, int(len(s) * q))]


def compare_to_reference(
    candles: list[Candle],
    reference: list[Candle],
    *,
    venue: str,
    symbol: str,
    interval: str,
    reference_label: str,
    tolerance: float = 0.02,
    worst_n: int = 10,
) -> ValidationReport:
    """Compare open/high/low/close to a reference series at matching timestamps.

    Deviation is ``|ours - ref| / ref``. ``tolerance`` (0.02 = 2%) is the max
    allowed; on a fungible asset, arbitrage keeps a clean series well within it,
    so anything over flags our construction. Timestamps absent from the reference
    are counted but not scored.
    """
    report = ValidationReport(
        venue=venue,
        symbol=symbol,
        interval=interval,
        reference=reference_label,
        tolerance=tolerance,
        total=len(candles),
    )
    report.invariant_violations = len(check_ohlc_invariants(candles))

    ref_by_ts = {c.ts: c for c in reference}
    fields = ("open", "high", "low", "close")
    devs: dict[str, list[float]] = {f: [] for f in fields}
    scored: list[dict] = []

    for c in candles:
        r = ref_by_ts.get(c.ts)
        if r is None:
            report.missing_reference += 1
            continue
        report.compared += 1
        row_dev = {}
        for f in fields:
            d = _pct(float(getattr(c, f)), float(getattr(r, f)))
            if d is not None:
                devs[f].append(d)
                row_dev[f] = d
        worst_field = max(row_dev, key=row_dev.get) if row_dev else None
        if worst_field:
            scored.append(
                {
                    "ts": c.ts.isoformat().replace("+00:00", "Z"),
                    "field": worst_field,
                    "ours": float(getattr(c, worst_field)),
                    "reference": float(getattr(r, worst_field)),
                    "deviation_pct": round(row_dev[worst_field] * 100, 3),
                }
            )

    for f in fields:
        vals = devs[f]
        report.deviations[f] = FieldDeviation(
            field=f,
            over_tolerance=sum(1 for v in vals if v > tolerance),
            median_pct=statistics.median(vals) * 100 if vals else 0.0,
            p99_pct=_percentile(vals, 0.99) * 100,
            max_pct=max(vals) * 100 if vals else 0.0,
        )

    scored.sort(key=lambda r: r["deviation_pct"], reverse=True)
    report.worst = scored[:worst_n]
    return report


__all__ = [
    "check_ohlc_invariants",
    "compare_to_reference",
    "InvariantViolation",
    "FieldDeviation",
    "ValidationReport",
]
