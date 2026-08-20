import { defineConfig } from "@playwright/test";

const port = Number(process.env.RILL_E2E_PORT ?? "3012");

export default defineConfig({
  testDir: ".",
  timeout: 30_000,
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `sh -c 'set -eu
test_root=$(mktemp -d "\${TMPDIR:-/tmp}/rill-e2e-isolated.XXXXXX")
test -n "$test_root"
test_database="$test_root/rill.db"
export RILL_DATABASE_PATH="$test_database"
export RILL_BIND="127.0.0.1:${port}"
export RILL_PUBLIC_BASE_URL="http://127.0.0.1:${port}"
export RILL_STATIC_DIR="$PWD/ui/dist/client"
export RILL_RENDERER_WASM="$PWD/artifacts/ui-renderer.cwasm"
export RILL_ADMIN_PASSWORD="rill-e2e-password"
cargo run -p rill -- --config config/development.toml admin create --username admin
exec cargo run -p rill -- --config config/development.toml serve'`,
    cwd: "../..",
    url: `http://127.0.0.1:${port}/health/ready`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
