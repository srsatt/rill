#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
snapshot="$project_root/ui/tests/screenshots/rill.db"
temporary=$(mktemp -d "${TMPDIR:-/tmp}/rill-screenshot-db.XXXXXX")
cleanup() {
  rm -f "$temporary/rill.db" "$temporary/rill.db-shm" "$temporary/rill.db-wal" "$temporary/snapshot.db"
  rmdir "$temporary" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

cd "$project_root"
export RILL_DATABASE_PATH="$temporary/rill.db"
export RILL_ADMIN_PASSWORD="rill-e2e-password"
export RILL_MASTER_KEY="CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk"

cargo run -p rill -- --config config/development.toml admin create --username admin
cargo run -p rill -- --config config/development.toml dev-seed --user admin
cargo run -p rill -- --config config/development.toml backup "$temporary/snapshot.db"
mv "$temporary/snapshot.db" "$snapshot"
printf 'Created %s\n' "$snapshot"
