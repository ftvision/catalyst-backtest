# catalyst-market-data-dune

Ingests historical market data from **[Dune Analytics](https://dune.com)** into
the Catalyst Parquet store. Dune is a blockchain *analytics* platform — decoded
on-chain SQL tables plus a curated `prices.usd` feed — so it's a good source for
**deep historical gas and prices** to backfill (and to cross-reference against
other vendors). It is **not** a live feed: results lag and execution is
credit-metered, so we only use it for offline ingestion, never on a run path.

## How it works

You author a query on Dune and pass its numeric **`query_id`**. The client
executes it, polls to completion, and maps the result rows into the store. Column
names are configurable, so your query doesn't have to match ours.

```bash
# gas — the query returns ts + gas_usd
catalyst-ingest-dune gas --root data/market-data --chain ethereum \
  --query-id 1234567 --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z

# prices — the query returns ts + open/high/low/close[/volume]
catalyst-ingest-dune prices --root data/market-data \
  --venue ethereum --symbol ETH --interval 1h --query-id 7654321 \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z
```

Your query receives `start` / `end` (ISO strings) as Dune **query parameters**;
add more with repeated `--param key=value`. Map columns with `--ts-col`,
`--gas-col`, `--open-col`, etc.

## Auth

Set `DUNE_API_KEY` (or pass `--api-key`). The free tier is limited and large
historical queries consume credits — mind your query's scan size.

## Library use

```python
from catalyst_market_data_core import ParquetStore, http_transport
from catalyst_market_data_dune import DuneClient, ingest_gas

client = DuneClient(api_key, http_transport())
ingest_gas(ParquetStore("data/market-data"), client, chain="ethereum",
           query_id=1234567, start=start, end=end)
```

## Tests

```bash
uv run pytest packages/market-data-dune
```

Fully offline via a fake transport: execute→poll→results flow, API-key headers,
parameter passing, timestamp parsing, gas/candle mapping, and store writes.
