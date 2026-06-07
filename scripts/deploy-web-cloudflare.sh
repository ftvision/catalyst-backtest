#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

WEB_DIR="${WEB_DIR:-$ROOT_DIR/apps/web}"
FLY_APP_NAME="${FLY_APP_NAME:-catalyst-backtest-api}"
VITE_CATALYST_API_BASE="${VITE_CATALYST_API_BASE:-https://${FLY_APP_NAME}.fly.dev}"
CF_PAGES_PROJECT="${CF_PAGES_PROJECT:-catalyst-backtest-web}"
CF_PAGES_BRANCH="${CF_PAGES_BRANCH:-main}"
CF_PAGES_PRODUCTION_BRANCH="${CF_PAGES_PRODUCTION_BRANCH:-main}"
CF_PAGES_STABLE_URL="${CF_PAGES_STABLE_URL:-https://${CF_PAGES_PROJECT}.pages.dev}"
CREATE_CF_PAGES_PROJECT="${CREATE_CF_PAGES_PROJECT:-0}"
WRANGLER_CMD="${WRANGLER_CMD:-wrangler}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

require_cmd npm

read -r -a wrangler_cmd <<< "$WRANGLER_CMD"
require_cmd "${wrangler_cmd[0]}"

if [[ -n "${GITHUB_ACTIONS:-}" && -z "${CLOUDFLARE_API_TOKEN:-}" ]]; then
  echo "Missing CLOUDFLARE_API_TOKEN for GitHub Actions deploy." >&2
  exit 1
fi

if [[ -n "${GITHUB_ACTIONS:-}" && -z "${CLOUDFLARE_ACCOUNT_ID:-}" ]]; then
  echo "Missing CLOUDFLARE_ACCOUNT_ID for GitHub Actions deploy." >&2
  exit 1
fi

cd "$WEB_DIR"
npm ci
VITE_CATALYST_API_BASE="$VITE_CATALYST_API_BASE" npm run build

if [[ "$CREATE_CF_PAGES_PROJECT" == "1" ]]; then
  "${wrangler_cmd[@]}" pages project create "$CF_PAGES_PROJECT" \
    --production-branch "$CF_PAGES_PRODUCTION_BRANCH" \
    || echo "Cloudflare Pages project may already exist: $CF_PAGES_PROJECT"
fi

"${wrangler_cmd[@]}" pages deploy dist \
  --project-name "$CF_PAGES_PROJECT" \
  --branch "$CF_PAGES_BRANCH"

echo "Stable Pages URL: $CF_PAGES_STABLE_URL"
echo "Wrangler may also print a hash-prefixed preview URL; use the stable URL for sharing."
