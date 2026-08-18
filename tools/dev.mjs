import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, watch } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const temporary = mkdtempSync(join(tmpdir(), "rill-dev-"));
const database = process.env.RILL_DEV_DATABASE_PATH ?? join(temporary, "rill.db");
const env = {
  ...process.env,
  RILL_DATABASE_PATH: database,
  RILL_BIND: "127.0.0.1:3000",
  RILL_PUBLIC_BASE_URL: "http://127.0.0.1:3000",
  RILL_STATIC_DIR: join(root, "ui/dist/client"),
  RILL_RENDERER_WASM: join(root, "artifacts/ui-renderer.wasm"),
  RILL_MASTER_KEY: process.env.RILL_MASTER_KEY ?? "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
  RILL_ADMIN_PASSWORD: process.env.RILL_ADMIN_PASSWORD ?? "rill-development-password",
  RILL_DEV_RELOAD: "1",
};

function run(program, args) {
  const result = spawnSync(program, args, { cwd: root, env, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function buildUi() {
  run("cargo", ["xtask", "generate-contracts"]);
  run("pnpm", ["--dir", "ui", "build"]);
}

buildUi();
run("cargo", ["run", "-p", "rill", "--", "--config", "config/development.toml", "admin", "create", "--username", "admin"]);

const fixture = spawn("node", ["tools/fixture-server.mjs"], { cwd: root, env, stdio: "inherit" });
run("cargo", ["run", "-p", "rill", "--", "--config", "config/development.toml", "dev-seed", "--user", "admin"]);
let server;
let rebuilding = false;
let queuedUi = false;
let timer;

function startServer() {
  server?.kill("SIGTERM");
  server = spawn("cargo", ["run", "-p", "rill", "--", "--config", "config/development.toml", "serve"], {
    cwd: root, env, stdio: "inherit",
  });
}

async function rebuild(uiChanged) {
  if (rebuilding) { queuedUi ||= uiChanged; return; }
  rebuilding = true;
  if (uiChanged) buildUi();
  startServer();
  rebuilding = false;
  if (queuedUi) { queuedUi = false; await rebuild(true); }
}

function schedule(uiChanged) {
  clearTimeout(timer);
  timer = setTimeout(() => void rebuild(uiChanged), 150);
}

for (const [path, ui] of [["ui/src", true], ["ui/public", true], ["crates", true], ["migrations", false], ["config", false]]) {
  watch(join(root, path), { recursive: true }, () => schedule(ui));
}

const stop = () => {
  server?.kill("SIGTERM");
  fixture.kill("SIGTERM");
  if (!process.env.RILL_DEV_DATABASE_PATH) rmSync(temporary, { recursive: true, force: true });
  process.exit(0);
};
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
startServer();
process.stdout.write("Rill dev: http://127.0.0.1:3000 (admin / rill-development-password)\n");
