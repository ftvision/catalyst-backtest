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

## Venue-native prices + provenance (#38)

Binance candles are a deep, reliable **reference** (CEX spot), but a proxy — not
the venue's own price. For realism, ingest **venue-native** prices and label them
so results can tell native from reference:

- **Hyperliquid perps** trade on HL's own mark — author a Dune query over HL data
  and store it under `--venue hyperliquid` with `--provenance native`.
- **Base swaps** fill against a DEX pool — query `dex.trades` (Uniswap/Aerodrome)
  for hourly OHLC and store under `--venue base --provenance native`.

```bash
catalyst-ingest-dune prices --root data/market-data \
  --venue hyperliquid --symbol ETH --interval 1h --provenance native \
  --query-id <HL_MARK_QUERY_ID> --start ... --end ...

catalyst-ingest-dune prices --root data/market-data \
  --venue base --symbol ETH --interval 1h --provenance native \
  --query-id <BASE_DEX_QUERY_ID> --start ... --end ...
```

`--provenance` (default `native`) is written to the store's `_provenance.json`
manifest; the Rust loader reads it so each candle series' provider records
`native`/`reference`. Binance ingestion records `reference` automatically. Keep
Binance as a labeled fallback where native data is thin.

Example Base-DEX hourly OHLC (Uniswap v3 WETH/USDC) — author on Dune, then pass
its query id:

```sql
SELECT date_trunc('hour', block_time) AS ts,
       (array_agg(amount_usd / token_bought_amount ORDER BY block_time ASC))[1]  AS open,
       max(amount_usd / token_bought_amount)                                     AS high,
       min(amount_usd / token_bought_amount)                                     AS low,
       (array_agg(amount_usd / token_bought_amount ORDER BY block_time DESC))[1] AS close
FROM dex.trades
WHERE blockchain = 'base' AND token_bought_symbol = 'WETH'
  AND amount_usd > 0 AND token_bought_amount > 0
  AND block_time >= TIMESTAMP '{{start}}' AND block_time < TIMESTAMP '{{end}}'
GROUP BY 1 ORDER BY 1
```

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
