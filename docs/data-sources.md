# Data sources we actually use

This is the **live registry** of the data we fetch: each source, its exact
endpoint/tables, the script or CLI that pulls it, the store path it writes, its
history limits, and how it reaches the deployed server. For the *conceptual*
picture of where data *can* come from, see
[market-data-primer.md](market-data-primer.md); for the 1h-vs-4h interval
tradeoff and per-source reach, see [market-data-intervals.md](market-data-intervals.md).

The simulation engine never fetches raw data. The flow is always:

```
source API  ──fetch script──▶  local Parquet store  ──upload_r2──▶  R2 bucket  ──Rust loader──▶  Fly server
              (Python)         data/market-data/                   (S3)         CATALYST_STORE_ROOT
```

## Source registry

| Data | Source | Endpoint / tables | Fetcher | Store path | Provenance |
| --- | --- | --- | --- | --- | --- |
| ETH OHLC + ETH L1 gas | **Dune** | `prices.usd` (WETH/ethereum), `ethereum.blocks` (base fee) | `scripts/fetch_dune.py` | `candles/venue=ethereum/symbol=ETH`, `gas/chain=ethereum` | reference |
| Base DEX ETH price + Base L1 gas | **Dune** | `dex.trades` (WETH on base), `base.blocks` (base fee) | `scripts/fetch_dune_base.py` | `candles/venue=base/symbol=ETH` (native), `gas/chain=base` | native |
| HL perp mark + funding | **Hyperliquid `info` API** | `candleSnapshot`, `fundingHistory` | `scripts/fetch_hyperliquid_recent.py` | `candles/funding venue=hyperliquid` | native |
| Aave supply APY | **DefiLlama** | `https://yields.llama.fi/chart/{pool_id}` | `cli ingest-aave-yields` | `yields/protocol=aave/asset=<a>/chain=<c>/pool=<p>` | — |
| CEX reference candles | **Binance klines** | `/api/v3/klines` | `cli ingest-binance` | `candles/venue=<v>/symbol=<s>` | reference |

"native" = the venue's own price/feed; "reference" = a CEX/aggregate stand-in
stored under a venue label (an approximation, flagged via provenance, #38).

## How to fetch each source

All commands run from the repo root. Dune + R2 commands read secrets from a
**gitignored `.env.r2`** (template: `.env.r2.example`) — load it with
`set -a; source .env.r2; set +a`. **Never commit `.env.r2` or echo its values.**

### Dune — ETH price + ETH/Base gas

Dune is queried via reusable saved queries (created once, re-run with a
`{{start}}`/`{{end}}` window). The query IDs are passed via env so the scripts
patch the existing query instead of creating duplicates:

```bash
set -a; source .env.r2; set +a   # provides DUNE_API_KEY

# Ethereum price + gas (reuses GAS_ID / PX_ID saved queries)
GAS_ID=7669340 PX_ID=7669341 \
uv run --with httpx python scripts/fetch_dune.py \
  --start 2024-01-01T00:00:00Z --end 2026-06-07T00:00:00Z --interval 1h

# Base DEX price + Base gas (reuses BASE_GAS_ID / BASE_PX_ID)
BASE_GAS_ID=7669858 BASE_PX_ID=7669859 \
uv run --with httpx python scripts/fetch_dune_base.py \
  --start 2024-01-01T00:00:00Z --end 2026-06-07T00:00:00Z --interval 1h
```

Reusable saved-query IDs (created under the project Dune account):

| Series | 1h query id | 4h query id |
| --- | --- | --- |
| ETH gas (`GAS_ID`) | 7669340 | 7669973 |
| ETH price (`PX_ID`) | 7669341 | 7669974 |
| Base gas (`BASE_GAS_ID`) | 7669858 | 7669977 |
| Base price (`BASE_PX_ID`) | 7669859 | 7669978 |

**4h caveat:** Dune SQL hardcodes hourly buckets (`date_trunc('hour', …)`). The
`--interval 4h` flag triggers `bucketize()`, which rewrites those to an
epoch-aligned 4h floor (`from_unixtime(floor(to_unixtime(col)/14400)*14400)`) so
the aggregation actually matches the stored interval — not just the path label.

### Hyperliquid — native mark + funding

Public/keyless but **retention-limited** (rolling ~5,000 candles per interval,
~500 funding points). Only works for a recent window — see the reach table in
[market-data-intervals.md](market-data-intervals.md):

```bash
uv run --with httpx python scripts/fetch_hyperliquid_recent.py \
  --days 200 --symbol ETH --interval 1h
```

For multi-year HL-native history use `--interval 4h` (reaches ~2.3 yr); 1h only
goes back ~7 months because the old 1h candles aren't retained.

### DefiLlama — Aave yields

Identify the pool's DefiLlama UUID (from `https://yields.llama.fi/pools`), then:

```bash
# Aave V3 USDC on Base
uv run python -m catalyst_market_data.cli ingest-aave-yields \
  --root data/market-data --asset USDC --chain base --pool usdc \
  --pool-id 7e0661bf-8cf3-45e6-9424-31916d4c7b84 \
  --start 2024-01-01T00:00:00Z --end 2026-06-07T00:00:00Z
```

Known pool UUIDs:

| Pool | DefiLlama UUID | Store path |
| --- | --- | --- |
| Aave V3 USDC, Ethereum | `aa70268e-4b52-42bf-a116-608b370f9501` | `yields/protocol=aave/asset=USDC/chain=ethereum/pool=usdc` |
| Aave V3 USDC, Base | `7e0661bf-8cf3-45e6-9424-31916d4c7b84` | `yields/protocol=aave/asset=USDC/chain=base/pool=usdc` |

The yield key the graph compiler requires is `(protocol, asset, chain, pool)` —
a Base-yield graph needs the Base pool present, not just Ethereum.

### Binance — reference candles

Deep history, all intervals, but a CEX **reference** price (not venue-native).
Useful to backfill where a venue's own API can't reach:

```bash
uv run python -m catalyst_market_data.cli ingest-binance \
  --root data/market-data --venue hyperliquid --symbol ETH \
  --binance-symbol ETHUSDT --interval 4h \
  --start 2024-01-01T00:00:00Z --end 2026-06-07T00:00:00Z
```

## Publishing to R2 (so the deployed server sees it)

The local store is uploaded to Cloudflare R2 (S3-compatible); the Rust loader
reads the same key layout straight from the bucket.

```bash
set -a; source .env.r2; set +a   # AWS_ACCESS_KEY_ID/SECRET, R2_BUCKET, R2_ENDPOINT
uv run --with boto3 python scripts/upload_r2.py \
  --bucket "$R2_BUCKET" --endpoint "$R2_ENDPOINT" --prefix market-data
```

The loader resolves an `s3://` root via `AmazonS3Builder::from_env()`, which
reads these env vars on the server (set as Fly secrets):

- `CATALYST_STORE_ROOT=s3://<bucket>/market-data`
- `AWS_ENDPOINT` = the R2 S3 endpoint (note: `AWS_ENDPOINT`, **not** `AWS_ENDPOINT_URL`)
- `AWS_REGION=auto`
- `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`

## Verifying what's live

The deployed server exposes its current coverage; no redeploy is needed after an
upload (it reads R2 on request):

```bash
curl -s https://catalyst-backtest-api.fly.dev/market-data/catalog \
  | python3 -c "import sys,json; [print(i['kind'],i.get('venue',''),i.get('chain',''),i.get('symbol',''),i.get('protocol',''),i['interval'] if 'interval' in i else '',i['start'][:10],i['end'][:10],i['files']) for i in json.load(sys.stdin)['items']]"
```

## Secrets & rotation

`DUNE_API_KEY`, the R2 access keys, and the R2 account id all live only in
`.env.r2` (gitignored) and as Fly secrets. Rotate any key that has appeared in a
chat transcript or terminal scrollback. Scripts read creds from the environment
and print only store keys — never credentials.
