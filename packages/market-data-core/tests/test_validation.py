"""Cross-reference validation: invariants, deviation stats, and the pass gate."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta

from catalyst_contracts import Candle
from catalyst_market_data_core import check_ohlc_invariants, compare_to_reference


def _c(i: int, o: str, h: str, low: str, cl: str) -> Candle:
    ts = datetime(2024, 1, 1, tzinfo=UTC) + timedelta(hours=i)
    return Candle(ts=ts, open=o, high=h, low=low, close=cl, volume="1")


def _series(n: int, base: float = 2000.0) -> list[Candle]:
    out = []
    for i in range(n):
        p = base + i
        out.append(_c(i, str(p), str(p + 1), str(p - 1), str(p + 0.5)))
    return out


def test_invariants_flag_zero_low_and_bad_order() -> None:
    candles = [
        _c(0, "2000", "2010", "1990", "2005"),  # ok
        _c(1, "2000", "2010", "0", "2005"),  # zero low
        _c(2, "2000", "1990", "1980", "2005"),  # high < close (order)
    ]
    viol = check_ohlc_invariants(candles)
    kinds = sorted(v.kind for v in viol)
    assert kinds == ["non_positive", "ohlc_order"]


def test_clean_series_passes_against_identical_reference() -> None:
    ref = _series(30)
    target = _series(30)
    report = compare_to_reference(
        target, ref, venue="base", symbol="ETH", interval="1h", reference_label="ethereum/ETH"
    )
    assert report.passed
    assert report.compared == 30
    assert report.missing_reference == 0
    assert report.deviations["close"].max_pct == 0.0


def test_inflated_high_is_flagged_against_reference() -> None:
    ref = _series(30)
    target = _series(30)
    # Inflate one candle's high ~5% above the reference (the DEX wick artifact).
    bad = target[10]
    target[10] = Candle(ts=bad.ts, open=bad.open, high="2115", low=bad.low, close=bad.close, volume="1")
    report = compare_to_reference(
        target, ref, venue="base", symbol="ETH", interval="1h", reference_label="ethereum/ETH",
        tolerance=0.02,
    )
    assert not report.passed
    assert report.deviations["high"].over_tolerance == 1
    assert report.worst[0]["field"] == "high"


def test_missing_reference_timestamps_are_counted_not_scored() -> None:
    ref = _series(20)
    target = _series(30)  # 10 extra timestamps beyond the reference
    report = compare_to_reference(
        target, ref, venue="base", symbol="ETH", interval="1h", reference_label="ethereum/ETH"
    )
    assert report.compared == 20
    assert report.missing_reference == 10
    assert report.passed
