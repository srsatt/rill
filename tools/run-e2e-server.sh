#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test_database="${TMPDIR:-/tmp}/rill-e2e-$$.db"
e2e_port="${RILL_E2E_PORT:-3000}"
fixture_port="${RILL_E2E_FIXTURE_PORT:-3011}"

case "$e2e_port:$fixture_port" in
  *[!0-9:]*) echo "RILL_E2E_PORT and RILL_E2E_FIXTURE_PORT must be valid TCP ports" >&2; exit 2 ;;
esac

cd "$project_root"
export RILL_DATABASE_PATH="$test_database"
export RILL_BIND="127.0.0.1:$e2e_port"
export RILL_PUBLIC_BASE_URL="http://127.0.0.1:$e2e_port"
export RILL_STATIC_DIR="$project_root/ui/dist/client"
export RILL_RENDERER_WASM="$project_root/artifacts/ui-renderer.wasm"
export RILL_ADMIN_PASSWORD="rill-e2e-password"

cargo run -p rill -- --config config/development.toml admin create --username admin
RILL_FIXTURE_PORT="$fixture_port" node tools/fixture-server.mjs &
fixture_pid=$!
trap 'kill "$fixture_pid" 2>/dev/null || true' EXIT INT TERM
cargo run -p rill -- --config config/development.toml serve
