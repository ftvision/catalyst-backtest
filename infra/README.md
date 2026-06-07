# Infra

Deployment resources for the public Catalyst Backtest demo.

The first deploy target is intentionally small:

- `apps/web` deploys as a static Vite app on Cloudflare Pages.
- `crates/simulation-service` deploys as a Fly.io Docker service.
- The API reads bundled strategy/scenario fixtures from the image.
- The durable Parquet market-data store can move to `s3://...` / R2 later via
  `CATALYST_STORE_ROOT`.

## Fly.io API

The Fly config is in `infra/fly.toml`; the API image is built from
`infra/Dockerfile.api`.

From the repo root:

```bash
fly apps create catalyst-backtest-api
fly deploy --config infra/fly.toml
fly status --app catalyst-backtest-api
fly logs --app catalyst-backtest-api
```

Smoke test:

```bash
curl https://catalyst-backtest-api.fly.dev/health
curl https://catalyst-backtest-api.fly.dev/policy-profiles
curl https://catalyst-backtest-api.fly.dev/strategies
```

If the app name is taken, choose a new name, then update:

- `app` in `infra/fly.toml`
- `VITE_CATALYST_API_BASE` in `apps/web/.env.production.example`
- the Cloudflare Pages environment variable

Useful API env vars:

| Variable | Default here | Purpose |
| --- | --- | --- |
| `CATALYST_SIM_BIND` | `0.0.0.0:8080` | Public container bind address for Fly. |
| `CATALYST_STRATEGY_ROOT` | `/app/strategies` | Bundled strategy catalog path. |
| `CATALYST_SIM_WORKERS` | `2` | Queue-draining worker count. |
| `CATALYST_SIM_QUEUE` | `128` | Queue capacity before `503 queue_full`. |
| `CATALYST_STORE_ROOT` | unset | Optional Parquet store root, e.g. `s3://bucket/prefix`. |

## Cloudflare Pages Web

The Pages config is in `apps/web/wrangler.toml`.

For a CLI deploy:

```bash
cd apps/web
npm ci
VITE_CATALYST_API_BASE=https://catalyst-backtest-api.fly.dev npm run build
wrangler pages project create catalyst-backtest-web --production-branch main
wrangler pages deploy dist --project-name catalyst-backtest-web
```

For a Git-connected Cloudflare Pages project:

| Setting | Value |
| --- | --- |
| Root directory | `apps/web` |
| Build command | `npm ci && npm run build` |
| Build output directory | `dist` |
| Production env var | `VITE_CATALYST_API_BASE=https://catalyst-backtest-api.fly.dev` |

The frontend bakes `VITE_CATALYST_API_BASE` into the static bundle at build time.
If the API URL changes, rebuild and redeploy the Pages app.

## Later: R2 Market Data

The service already accepts `CATALYST_STORE_ROOT` as `s3://...`. When the demo
needs durable market-data history, create an R2 bucket with S3-compatible
credentials, upload the Parquet tree, then set the service env:

```bash
fly secrets set \
  CATALYST_STORE_ROOT=s3://catalyst-market-data \
  AWS_ACCESS_KEY_ID=... \
  AWS_SECRET_ACCESS_KEY=... \
  AWS_ENDPOINT_URL=...
```

Keep the exact bucket prefix aligned with
`docs/market-data-storage.md`.
