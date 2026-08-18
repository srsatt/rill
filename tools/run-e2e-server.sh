#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test_database="${TMPDIR:-/tmp}/rill-e2e-$$.db"

cd "$project_root"
export RILL_DATABASE_PATH="$test_database"
export RILL_BIND="127.0.0.1:3000"
export RILL_PUBLIC_BASE_URL="http://127.0.0.1:3000"
export RILL_STATIC_DIR="$project_root/ui/dist/client"
export RILL_RENDERER_WASM="$project_root/artifacts/ui-renderer.wasm"
export RILL_ADMIN_PASSWORD="rill-e2e-password"

cargo run -p rill -- --config config/development.toml admin create --username admin
node tools/fixture-server.mjs &
fixture_pid=$!
trap 'kill "$fixture_pid" 2>/dev/null || true' EXIT INT TERM
cargo run -p rill -- --config config/development.toml serve
