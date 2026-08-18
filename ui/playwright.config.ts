import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "tests",
  timeout: 30_000,
  use: {
    baseURL: "http://127.0.0.1:3000",
    trace: "retain-on-failure"
  },
  webServer: {
    command: "sh tools/run-e2e-server.sh",
    cwd: "..",
    url: "http://127.0.0.1:3000/health/ready",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000
  }
});
