#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

FLY_APP_NAME="${FLY_APP_NAME:-catalyst-backtest-api}"
FLY_CONFIG="${FLY_CONFIG:-$ROOT_DIR/infra/fly.toml}"
CATALYST_API_URL="${CATALYST_API_URL:-https://${FLY_APP_NAME}.fly.dev}"
CREATE_FLY_APP="${CREATE_FLY_APP:-0}"
SMOKE_TEST="${SMOKE_TEST:-1}"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

require_cmd curl

FLY_CMD="${FLY_CMD:-}"
if [[ -z "$FLY_CMD" ]]; then
  if command -v fly >/dev/null 2>&1; then
    FLY_CMD="fly"
  elif command -v flyctl >/dev/null 2>&1; then
    FLY_CMD="flyctl"
  else
    echo "Missing required command: fly or flyctl" >&2
    exit 1
  fi
else
  require_cmd "$FLY_CMD"
fi

if [[ -n "${GITHUB_ACTIONS:-}" && -z "${FLY_API_TOKEN:-}" ]]; then
  echo "Missing FLY_API_TOKEN for GitHub Actions deploy." >&2
  exit 1
fi

if [[ "$CREATE_FLY_APP" == "1" ]]; then
  create_args=(apps create "$FLY_APP_NAME")
  if [[ -n "${FLY_ORG:-}" ]]; then
    create_args+=(--org "$FLY_ORG")
  fi
  "$FLY_CMD" "${create_args[@]}" || echo "Fly app may already exist: $FLY_APP_NAME"
fi

deploy_args=(deploy --config "$FLY_CONFIG" --app "$FLY_APP_NAME")
if [[ "${FLY_DEPLOY_REMOTE_ONLY:-0}" == "1" ]]; then
  deploy_args+=(--remote-only)
fi

"$FLY_CMD" "${deploy_args[@]}"

if [[ "$SMOKE_TEST" == "1" ]]; then
  curl --fail --silent --show-error "$CATALYST_API_URL/health"
  echo
  curl --fail --silent --show-error "$CATALYST_API_URL/policy-profiles" >/dev/null
  curl --fail --silent --show-error "$CATALYST_API_URL/strategies" >/dev/null
  echo "API smoke checks passed: $CATALYST_API_URL"
fi
