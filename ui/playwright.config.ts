import { defineConfig } from "@playwright/test";

const port = Number(process.env.RILL_E2E_PORT ?? "3000");
if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error("RILL_E2E_PORT must be a valid TCP port");
const fixturePort = Number(process.env.RILL_E2E_FIXTURE_PORT ?? "3011");
if (!Number.isInteger(fixturePort) || fixturePort < 1 || fixturePort > 65_535) throw new Error("RILL_E2E_FIXTURE_PORT must be a valid TCP port");

export default defineConfig({
  testDir: "tests",
  timeout: 30_000,
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure"
  },
  webServer: {
    command: `RILL_E2E_PORT=${port} RILL_E2E_FIXTURE_PORT=${fixturePort} sh tools/run-e2e-server.sh`,
    cwd: "..",
    url: `http://127.0.0.1:${port}/health/ready`,
    reuseExistingServer: false,
    timeout: 180_000
  }
});
