"""Historical L1 gas from BigQuery's public Ethereum dataset.

`bigquery-public-data.crypto_ethereum.blocks` carries `base_fee_per_gas` (wei) per
block since EIP-1559. We bucket it to hourly averages and convert to a USD cost
per on-chain action:

    gas_usd = base_fee_wei * gas_units / 1e18 * eth_price_usd

`eth_price_usd` is a flag (the public Ethereum dataset has no curated USD price),
so treat the USD figure as an approximation — the *gas-price* shape is real, the
USD scaling is a constant. (For per-hour USD, cross-reference the Dune ingester,
which can join `prices.usd`.)
"""

from __future__ import annotations

from datetime import UTC, datetime
from decimal import Decimal

from catalyst_contracts.market_data import GasPoint
from catalyst_market_data_core import ParquetStore

from .runner import QueryRunner

DEFAULT_DATASET = "bigquery-public-data.crypto_ethereum"


def _ts_literal(ts: datetime) -> str:
    return ts.astimezone(UTC).strftime("%Y-%m-%d %H:%M:%S")


def gas_sql(start: datetime, end: datetime, dataset: str = DEFAULT_DATASET) -> str:
    """Hourly average base fee (wei) over ``[start, end]`` from the blocks table."""

    return (
        "SELECT TIMESTAMP_TRUNC(timestamp, HOUR) AS ts, "
        "AVG(base_fee_per_gas) AS base_fee_wei\n"
        f"FROM `{dataset}.blocks`\n"
        f"WHERE timestamp BETWEEN TIMESTAMP('{_ts_literal(start)}') "
        f"AND TIMESTAMP('{_ts_literal(end)}')\n"
        "  AND base_fee_per_gas IS NOT NULL\n"
        "GROUP BY ts\nORDER BY ts"
    )


def _ts(value: object) -> datetime:
    if isinstance(value, datetime):
        return value if value.tzinfo else value.replace(tzinfo=UTC)
    parsed = datetime.fromisoformat(str(value).replace("Z", "+00:00").replace(" UTC", "+00:00"))
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=UTC)


def _gas_usd(base_fee_wei: object, gas_units: int, eth_price_usd: Decimal) -> str:
    value = Decimal(str(base_fee_wei)) * Decimal(gas_units) / Decimal(10**18) * eth_price_usd
    return str(value)


def fetch_gas(
    runner: QueryRunner,
    *,
    start: datetime,
    end: datetime,
    gas_units: int,
    eth_price_usd: Decimal | str,
    dataset: str = DEFAULT_DATASET,
    sql: str | None = None,
    ts_col: str = "ts",
    base_fee_col: str = "base_fee_wei",
) -> list[GasPoint]:
    eth_price = Decimal(str(eth_price_usd))
    rows = runner(sql or gas_sql(start, end, dataset))
    return [
        GasPoint(ts=_ts(r[ts_col]), gas_usd=_gas_usd(r[base_fee_col], gas_units, eth_price))
        for r in rows
    ]


def ingest_gas(store: ParquetStore, runner: QueryRunner, *, chain: str, **kwargs) -> int:
    return store.write_gas(chain, fetch_gas(runner, **kwargs))


__all__ = ["gas_sql", "fetch_gas", "ingest_gas", "DEFAULT_DATASET"]
