"""One-off: create the gas + prices queries on Dune, run them, write the store.

Reads DUNE_API_KEY from the environment. Creates two saved queries (so they're
reusable), executes each for a small window, and writes results to the Parquet
store via the real ingester path.
"""

from __future__ import annotations

import os
import sys
from datetime import UTC, datetime

import httpx

from catalyst_market_data_core import ParquetStore, http_transport
from catalyst_market_data_dune import DuneClient, ingest_candles, ingest_gas

API = "https://api.dune.com/api/v1"
KEY = os.environ["DUNE_API_KEY"]
ROOT = "data/market-data"
START = datetime(2024, 1, 1, tzinfo=UTC)
END = datetime(2024, 1, 8, tzinfo=UTC)

GAS_SQL = """
WITH gas AS (
  SELECT date_trunc('hour', time) AS ts, avg(base_fee_per_gas) AS base_fee_wei
  FROM ethereum.blocks
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

PRICES_SQL = """
SELECT date_trunc('hour', minute) AS ts,
       (array_agg(price ORDER BY minute ASC))[1]  AS open,
       max(price)                                 AS high,
       min(price)                                 AS low,
       (array_agg(price ORDER BY minute DESC))[1] AS close
FROM prices.usd
WHERE blockchain = 'ethereum' AND symbol = 'WETH'
  AND minute >= TIMESTAMP '{{start}}' AND minute < TIMESTAMP '{{end}}'
GROUP BY 1
ORDER BY 1
"""

PARAMS = [
    {"key": "start", "type": "datetime", "value": "2024-01-01 00:00:00"},
    {"key": "end", "type": "datetime", "value": "2024-01-08 00:00:00"},
]


def upsert_query(name: str, sql: str, existing: str | None) -> int:
    """PATCH an existing query's SQL if its id is given, else create a public one."""
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


def main() -> int:
    store = ParquetStore(ROOT)
    client = DuneClient(KEY, http_transport(), poll_interval=3.0, max_polls=60)

    gas_id = upsert_query("catalyst: eth L1 gas (hourly)", GAS_SQL, os.environ.get("GAS_ID"))
    px_id = upsert_query("catalyst: eth hourly OHLC", PRICES_SQL, os.environ.get("PX_ID"))

    print("running gas query ...")
    n_gas = ingest_gas(store, client, chain="ethereum_dune", query_id=gas_id, start=START, end=END)
    print(f"  wrote {n_gas} gas points -> chain=ethereum_dune")

    print("running prices query ...")
    n_px = ingest_candles(
        store, client, venue="ethereum", symbol="ETH", interval="1h",
        query_id=px_id, start=START, end=END,
    )
    print(f"  wrote {n_px} candles -> venue=ethereum/symbol=ETH")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
