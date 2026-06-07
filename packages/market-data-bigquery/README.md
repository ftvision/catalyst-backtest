# catalyst-market-data-bigquery

Ingests historical market data from **Google BigQuery**'s public crypto datasets
(e.g. `bigquery-public-data.crypto_ethereum`) into the Catalyst Parquet store.
Good for **deep, cheap-to-free historical L1 gas** — and a cross-reference for the
Dune gas series.

## Cost

The public datasets are free to access; you pay only for **bytes scanned**, and
BigQuery includes **1 TB/month free**. The built-in gas query reads a few columns
of the `blocks` table over a date range, which is tiny — comfortably within the
free tier. (A naive scan of `transactions` is *not* — mind your columns.) You need
a GCP project with billing enabled even to use the free tier.

## How it works

The query runner is injected; the real one lazily imports `google-cloud-bigquery`
(install the `gcp` extra) and uses Application Default Credentials.

```bash
# L1 gas from the public Ethereum blocks table (base_fee_per_gas)
catalyst-ingest-bigquery gas --root data/market-data --chain ethereum \
  --start 2024-01-01T00:00:00Z --end 2024-02-01T00:00:00Z \
  --gas-units 120000 --eth-price-usd 2500 --project my-gcp-project
```

`gas_usd = avg_base_fee_wei × gas_units / 1e18 × eth_price_usd`. The **gas-price
shape is real**; the USD scaling uses a constant `--eth-price-usd` because the
public Ethereum dataset has no curated USD price (for per-hour USD, the Dune
ingester can join `prices.usd`). Treat the USD figure as an approximation.

### Prices = bring your own SQL

The public Ethereum dataset has **no curated price feed** (DEX prices require
decoding swap events, which is pool-specific). Rather than ship a fragile default,
the `prices` command runs SQL *you* supply:

```bash
catalyst-ingest-bigquery prices --root data/market-data \
  --venue ethereum --symbol ETH --interval 1h --sql-file eth_hourly.sql \
  --project my-gcp-project
```

The query must return `ts` + `open`/`high`/`low`/`close` (and optionally
`volume`). `--sql-file` also overrides the built-in gas query if you want.

## Setup

```bash
uv pip install "catalyst-market-data-bigquery[gcp]"
gcloud auth application-default login
export GOOGLE_CLOUD_PROJECT=my-gcp-project   # or pass --project
```

## Tests

```bash
uv run pytest packages/market-data-bigquery
```

Fully offline via a fake runner: gas SQL shape, base-fee→USD conversion, SQL
override, candle mapping, and store writes (no GCP, no network).
