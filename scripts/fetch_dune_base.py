"""Fetch venue-native Base data from Dune: gas (base.blocks) + DEX ETH price.

Creates/updates two public Dune queries and writes:
- gas    -> chain=base               (base L1 fee -> USD via ETH price)
- candles-> venue=base/symbol=ETH    (Base DEX WETH price, provenance=native)

Reads DUNE_API_KEY from the env. Reuses query ids via BASE_GAS_ID / BASE_PX_ID.
"""

from __future__ import annotations

import argparse
import os
import sys
from datetime import UTC, datetime

import httpx

from catalyst_market_data_core import ParquetStore, http_transport
from catalyst_market_data_dune import DuneClient, ingest_candles, ingest_gas

API = "https://api.dune.com/api/v1"
KEY = os.environ["DUNE_API_KEY"]
ROOT = "data/market-data"

# Base L1 gas (base.blocks base fee) converted to USD via the ETH price feed.
BASE_GAS_SQL = """
WITH gas AS (
  SELECT date_trunc('hour', time) AS ts, avg(base_fee_per_gas) AS base_fee_wei
  FROM base.blocks
  WHERE time >= TIMESTAMP '{{start}}' AND time < TIMESTAMP '{{end}}'
  GROUP BY 1
),
px AS (
  SELECT date_trunc('hour', minute) AS ts, avg(price) AS eth_usd
  FROM prices.usd
  WHERE blockchain = 'ethereum' AND symbol = 'WETH'
    AND minute >= TIMESTAMP '{{start}}' AND minute < TIMESTAMP '{{end}}'
  GROUP BY 1
)
SELECT gas.ts AS ts, CAST(gas.base_fee_wei * 120000 / 1e18 * px.eth_usd AS double) AS gas_usd
FROM gas JOIN px ON gas.ts = px.ts
ORDER BY gas.ts
"""

# Native Base DEX WETH price: hourly OHLC from dex.trades on Base.
#
# Data-quality filters (so scam/illiquid rows never enter the store):
#   - canonical Base WETH contract address, NOT token_bought_symbol = 'WETH'
#     (the symbol is attacker-controllable; scam tokens spoof it).
#   - amount_usd >= 500: dust trades produce wildly wrong amount/price ratios.
#   - >= 5 trades per bucket: thin buckets aren't a reliable price.
#   - per-trade price within +/-10% of the bucket median: a single bad trade
#     can't set open/close/high/low. (The wick-repair pass at ingest time then
#     collapses any high/low still beyond ~2% of the body; see ingest_candles.
#     Base dex.trades carries a systematic ~4.5% upper-wick artifact that this
#     two-stage filter removes — verified against the ethereum reference feed.)
BASE_WETH = "0x4200000000000000000000000000000000000006"
BASE_DEX_SQL = """
WITH raw AS (
  SELECT block_time,
         date_trunc('hour', block_time) AS ts,
         amount_usd / token_bought_amount AS price
  FROM dex.trades
  WHERE blockchain = 'base'
    AND token_bought_address = {weth}
    AND amount_usd >= 500 AND token_bought_amount > 0
    AND block_time >= TIMESTAMP '{{{{start}}}}' AND block_time < TIMESTAMP '{{{{end}}}}'
),
stats AS (
  SELECT ts, approx_percentile(price, 0.5) AS med, count(*) AS n
  FROM raw GROUP BY ts
),
clean AS (
  SELECT r.ts, r.block_time, r.price
  FROM raw r JOIN stats s ON r.ts = s.ts
  WHERE s.n >= 5 AND r.price BETWEEN s.med * 0.9 AND s.med * 1.1
)
SELECT ts,
       (array_agg(price ORDER BY block_time ASC))[1]  AS open,
       max(price)                                     AS high,
       min(price)                                     AS low,
       (array_agg(price ORDER BY block_time DESC))[1] AS close
FROM clean
GROUP BY ts
ORDER BY ts
""".format(weth=BASE_WETH)

PARAMS = [
    {"key": "start", "type": "datetime", "value": "2024-01-01 00:00:00"},
    {"key": "end", "type": "datetime", "value": "2024-02-01 00:00:00"},
]


def upsert_query(name: str, sql: str, existing: str | None) -> int:
    h = {"X-Dune-API-Key": KEY}
    if existing:
        resp = httpx.patch(
            f"{API}/query/{existing}", headers=h,
            json={"name": name, "query_sql": sql, "parameters": PARAMS}, timeout=60,
        )
        if resp.status_code >= 400:
            sys.exit(f"update query {existing} failed: HTTP {resp.status_code} {resp.text}")
        print(f"updated query {name!r}: id={existing}")
        return int(existing)
    resp = httpx.post(
        f"{API}/query", headers=h,
        json={"name": name, "query_sql": sql, "is_private": False, "parameters": PARAMS}, timeout=60,
    )
    if resp.status_code >= 400:
        sys.exit(f"create query {name!r} failed: HTTP {resp.status_code} {resp.text}")
    qid = resp.json()["query_id"]
    print(f"created query {name!r}: id={qid}")
    return qid


def _dt(value: str) -> datetime:
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(UTC)


_INTERVAL_SECS = {"1h": 3600, "4h": 14400, "1d": 86400}


def bucketize(sql: str, interval: str) -> str:
    """Rewrite hourly `date_trunc` buckets to an epoch-aligned floor at `interval`."""
    secs = _INTERVAL_SECS[interval]
    if secs == 3600:
        return sql
    for col in ("time", "minute", "block_time"):
        sql = sql.replace(
            f"date_trunc('hour', {col})",
            f"from_unixtime(floor(to_unixtime({col})/{secs})*{secs})",
        )
    return sql


def main() -> int:
    ap = argparse.ArgumentParser(prog="fetch_dune_base")
    ap.add_argument("--start", type=_dt, default=datetime(2024, 1, 1, tzinfo=UTC))
    ap.add_argument("--end", type=_dt, default=datetime(2024, 2, 1, tzinfo=UTC))
    ap.add_argument("--interval", default="1h")
    args = ap.parse_args()

    store = ParquetStore(ROOT)
    client = DuneClient(KEY, http_transport(), poll_interval=3.0, max_polls=180)

    gas_id = upsert_query(
        f"catalyst: base L1 gas ({args.interval})", bucketize(BASE_GAS_SQL, args.interval),
        os.environ.get("BASE_GAS_ID"),
    )
    px_id = upsert_query(
        f"catalyst: base DEX eth price ({args.interval})", bucketize(BASE_DEX_SQL, args.interval),
        os.environ.get("BASE_PX_ID"),
    )

    print(f"running base gas {args.start:%Y-%m-%d}..{args.end:%Y-%m-%d} ({args.interval}) ...")
    n_gas = ingest_gas(store, client, chain="base", query_id=gas_id, start=args.start, end=args.end)
    print(f"  wrote {n_gas} gas points -> chain=base")

    print("running base DEX price ...")
    n_px = ingest_candles(
        store, client, venue="base", symbol="ETH", interval=args.interval,
        query_id=px_id, start=args.start, end=args.end,
        filter_outliers=True, repair_wicks=True, wick_tolerance=0.02,
    )
    store.set_provenance("candles", "base/ETH", "native")
    q = store.read_quality().get(f"candles/base/ETH/{args.interval}", {})
    print(
        f"  wrote {n_px} candles -> venue=base/symbol=ETH/{args.interval} [native]"
        f"  (removed {q.get('outliers_removed', 0)} outliers)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
