"""Candle outlier filtering: spikes out, normal volatility kept, metadata right."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta

from catalyst_contracts import Candle
from catalyst_market_data_core import filter_candle_outliers


def _candle(i: int, *, open_=None, high=None, low=None, close=None, base="2000") -> Candle:
    ts = datetime(2024, 1, 1, tzinfo=UTC) + timedelta(hours=i)
    o = open_ or base
    return Candle(ts=ts, open=o, high=high or base, low=low or base, close=close or base, volume="1")


def test_keeps_clean_series() -> None:
    candles = [_candle(i, base=str(2000 + i)) for i in range(30)]
    kept, report = filter_candle_outliers(candles)
    assert report.outliers_removed == 0
    assert report.kept == 30
    assert report.affected_ranges == []


def test_removes_trillion_high_and_zero_low() -> None:
    candles = [_candle(i, base="2000") for i in range(30)]
    candles[10] = _candle(10, high="8250671076904", base="2000")  # absurd wick
    candles[20] = _candle(20, low="0", base="2000")  # zero low
    kept, report = filter_candle_outliers(candles)
    assert report.outliers_removed == 2
    kept_ts = {c.ts for c in kept}
    assert candles[10].ts not in kept_ts
    assert candles[20].ts not in kept_ts
    assert report.method == "rolling_median_deviation"
    assert len(report.affected_ranges) == 2


def test_keeps_normal_volatility() -> None:
    # A real 20% move with tolerance 0.5 must survive.
    candles = [_candle(i, base="2000") for i in range(30)]
    candles[15] = _candle(15, open_="2000", high="2400", low="2000", close="2400")
    kept, report = filter_candle_outliers(candles, tolerance=0.5)
    assert report.outliers_removed == 0
    assert candles[15].ts in {c.ts for c in kept}


def test_consecutive_outliers_group_into_one_range() -> None:
    candles = [_candle(i, base="2000") for i in range(30)]
    for i in (12, 13, 14):
        candles[i] = _candle(i, high="999999", base="2000")
    _, report = filter_candle_outliers(candles)
    assert report.outliers_removed == 3
    assert len(report.affected_ranges) == 1


def test_report_as_dict_is_json_friendly() -> None:
    candles = [_candle(i, base="2000") for i in range(15)]
    candles[7] = _candle(7, high="500000", base="2000")
    _, report = filter_candle_outliers(candles)
    d = report.as_dict()
    assert d["outliers_removed"] == 1
    assert set(d) >= {
        "total",
        "kept",
        "outliers_removed",
        "wicks_repaired",
        "method",
        "tolerance",
        "window",
        "affected_ranges",
        "repaired_ranges",
    }


def test_repair_wicks_collapses_bad_high_to_body() -> None:
    # Body (open/close) is sound; only the high wick is absurd.
    candles = [_candle(i, base="2000") for i in range(30)]
    candles[10] = _candle(10, open_="2000", high="900000", low="2000", close="2010")
    kept, report = filter_candle_outliers(candles, repair_wicks=True)
    assert report.outliers_removed == 0
    assert report.wicks_repaired == 1
    assert report.kept == 30  # candle is kept, not dropped
    fixed = next(c for c in kept if c.ts == candles[10].ts)
    assert fixed.high == "2010"  # collapsed to max(open, close), exact string
    assert fixed.low == "2000"


def test_repair_wicks_catches_moderate_wick_within_median_band() -> None:
    # A 30% wick is inside the 50% median band but well past the body (20%):
    # the body-relative check must still collapse it.
    candles = [_candle(i, base="2000") for i in range(30)]
    candles[10] = _candle(10, open_="2000", high="2600", low="2000", close="2010")
    kept, report = filter_candle_outliers(candles, repair_wicks=True, wick_tolerance=0.2)
    assert report.wicks_repaired == 1
    fixed = next(c for c in kept if c.ts == candles[10].ts)
    assert fixed.high == "2010"


def test_repair_wicks_still_removes_bad_body() -> None:
    # If open/close themselves are bad, the candle is removed even in repair mode.
    candles = [_candle(i, base="2000") for i in range(30)]
    candles[10] = _candle(10, open_="900000", high="900000", low="2000", close="2000")
    kept, report = filter_candle_outliers(candles, repair_wicks=True)
    assert report.outliers_removed == 1
    assert candles[10].ts not in {c.ts for c in kept}
