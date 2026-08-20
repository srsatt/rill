import { defineConfig } from "@playwright/test";

const port = Number(process.env.RILL_SCREENSHOT_PORT ?? "3012");
const fixturePort = Number(process.env.RILL_SCREENSHOT_FIXTURE_PORT ?? "3013");

export default defineConfig({
  testDir: "tests/screenshots",
  timeout: 30_000,
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    colorScheme: "light",
    viewport: { width: 1440, height: 1000 },
  },
  webServer: {
    command: `RILL_E2E_DATABASE_SNAPSHOT=ui/tests/screenshots/rill.db RILL_E2E_PORT=${port} RILL_E2E_FIXTURE_PORT=${fixturePort} sh tools/run-e2e-server.sh`,
    cwd: "..",
    url: `http://127.0.0.1:${port}/health/ready`,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
