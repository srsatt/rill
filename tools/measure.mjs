import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const binary = join(root, "target/release/rill");
const rendererWasm = join(root, "artifacts/ui-renderer.wasm");
const renderer = join(root, "artifacts/ui-renderer.cwasm");
const staticDir = join(root, "ui/dist/client");
const rendererSource = join(root, "ui/dist/renderer/renderer.js");
for (const path of [binary, renderer, join(staticDir, ".vite/manifest.json")]) {
  if (!existsSync(path)) throw new Error(`missing ${path}; run cargo xtask build-release first`);
}

const temporary = mkdtempSync(join(tmpdir(), "rill-measure-"));
const database = join(temporary, "rill.db");
const serviceOrigin = "http://127.0.0.1:3021";
const env = {
  ...process.env,
  RILL_DATABASE_PATH: database,
  RILL_BIND: "127.0.0.1:3021",
  RILL_PUBLIC_BASE_URL: serviceOrigin,
  RILL_STATIC_DIR: staticDir,
  RILL_RENDERER_WASM: renderer,
  RILL_MASTER_KEY: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
  RILL_ADMIN_PASSWORD: "rill-measurement-password"
};
let fixture;
let service;

function compressed(path) {
  const input = readFileSync(path);
  return {
    raw: input.length,
    gzip: gzipSync(input, { level: 9 }).length,
    brotli: brotliCompressSync(input, {
      params: { [constants.BROTLI_PARAM_QUALITY]: 11 }
    }).length
  };
}

function runRill(args) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ["--config", "config/development.toml", ...args], {
      cwd: root,
      env,
      stdio: ["ignore", "pipe", "pipe"]
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("exit", (code, signal) => {
      if (code === 0) resolve(stdout.trim());
      else reject(new Error(`rill ${args.join(" ")} failed (${code ?? signal}): ${stderr.trim()}`));
    });
  });
}

function rssBytes(pid) {
  const result = spawnSync("ps", ["-o", "rss=", "-p", String(pid)], { encoding: "utf8" });
  if (result.status !== 0) return 0;
  const kibibytes = Number(result.stdout.trim());
  return Number.isFinite(kibibytes) ? kibibytes * 1024 : 0;
}

function physicalFootprint(pid) {
  if (process.platform !== "darwin") return null;
  const result = spawnSync(
    "footprint",
    ["-p", String(pid), "-f", "bytes", "--noCategories"],
    { encoding: "utf8" }
  );
  if (result.status !== 0) return null;
  const current = Number(result.stdout.match(/phys_footprint:\s+(\d+) B/)?.[1]);
  const peak = Number(result.stdout.match(/phys_footprint_peak:\s+(\d+) B/)?.[1]);
  return Number.isFinite(current) && Number.isFinite(peak) ? { current, peak } : null;
}

async function delay(milliseconds) {
  await new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitFor(url, timeoutMilliseconds = 15_000) {
  const deadline = Date.now() + timeoutMilliseconds;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return;
      lastError = new Error(`${url} returned ${response.status}`);
    } catch (error) {
      lastError = error;
    }
    await delay(25);
  }
  throw lastError ?? new Error(`timed out waiting for ${url}`);
}

async function sampleFor(pid, milliseconds, initial = 0) {
  let maximum = Math.max(initial, rssBytes(pid));
  const deadline = Date.now() + milliseconds;
  while (Date.now() < deadline) {
    maximum = Math.max(maximum, rssBytes(pid));
    await delay(20);
  }
  return maximum;
}

async function addAndPoll(name, url) {
  const created = await runRill([
    "sources", "add-rss", "--user", "admin", "--name", name, "--url", url
  ]);
  const sourceId = created.match(/created RSS source ([0-9a-f-]+)/)?.[1];
  if (!sourceId) throw new Error(`could not parse source ID from: ${created}`);
  await runRill(["sources", "poll", sourceId]);
}

async function stop(child, signal = "SIGTERM") {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  child.kill(signal);
  await Promise.race([once(child, "exit"), delay(5_000)]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL");
    await once(child, "exit");
  }
}

try {
  const manifest = JSON.parse(readFileSync(join(staticDir, ".vite/manifest.json"), "utf8"));
  const modernPath = join(staticDir, manifest["src/modern-client.tsx"].file);
  const modern = compressed(modernPath);
  const readerEntry = manifest["src/reader-client.tsx"];
  const reader = readerEntry
    ? compressed(join(staticDir, readerEntry.file))
    : { raw: 0, gzip: 0, brotli: 0 };

  fixture = spawn("node", ["tools/fixture-server.mjs"], {
    cwd: root,
    env,
    stdio: ["ignore", "ignore", "pipe"]
  });
  await waitFor("http://127.0.0.1:3011/health");
  await runRill(["admin", "create", "--username", "admin"]);

  const startedAt = performance.now();
  service = spawn(binary, ["--config", "config/development.toml", "serve"], {
    cwd: root,
    env,
    stdio: ["ignore", "ignore", "pipe"]
  });
  await waitFor(`${serviceOrigin}/health/ready`);
  const coldStartMilliseconds = performance.now() - startedAt;
  const idleRssBytes = await sampleFor(service.pid, 1_000);
  const footprintSamples = [physicalFootprint(service.pid)].filter(Boolean);

  let renderRssBytes = idleRssBytes;
  for (let index = 0; index < 100; index += 1) {
    const response = await fetch(`${serviceOrigin}/login`);
    if (!response.ok) throw new Error(`/login render ${index + 1} returned ${response.status}`);
    await response.arrayBuffer();
    renderRssBytes = Math.max(renderRssBytes, rssBytes(service.pid));
  }
  footprintSamples.push(physicalFootprint(service.pid));

  const ingestionStartRss = rssBytes(service.pid);
  await addAndPoll("Measured RSS", "http://127.0.0.1:3011/rss.xml");
  const ingestionRssBytes = await sampleFor(service.pid, 3_000, ingestionStartRss);
  footprintSamples.push(physicalFootprint(service.pid));

  const collectionStartRss = rssBytes(service.pid);
  await addAndPoll("Measured large roundup", "http://127.0.0.1:3011/large-rss.xml");
  const collectionRssBytes = await sampleFor(service.pid, 5_000, collectionStartRss);
  footprintSamples.push(physicalFootprint(service.pid));

  const search = await runRill(["search", "--user", "admin", "Germany"]);
  if (search === "[]") throw new Error("fixture ingestion produced no searchable story");

  await stop(service);
  service = undefined;
  const databaseBytes = statSync(database).size;

  console.log(`release executable: ${statSync(binary).size} bytes`);
  console.log(`renderer compiler-input WASM: ${statSync(rendererWasm).size} bytes`);
  console.log(`renderer AOT module: ${statSync(renderer).size} bytes`);
  console.log(`deployed native runtime + renderer: ${statSync(binary).size + statSync(renderer).size} bytes`);
  console.log(`renderer Vite source: ${statSync(rendererSource).size} bytes`);
  console.log(`modern initial JS: ${modern.raw} raw, ${modern.gzip} gzip-9, ${modern.brotli} Brotli bytes`);
  console.log(`reader JS: ${reader.raw} raw, ${reader.gzip} gzip-9, ${reader.brotli} Brotli bytes`);
  console.log(`cold startup to readiness: ${coldStartMilliseconds.toFixed(1)} ms`);
  console.log(`idle RSS: ${idleRssBytes} bytes`);
  console.log(`maximum RSS across 100 sequential SSR renders: ${renderRssBytes} bytes`);
  console.log(`maximum service RSS during RSS fixture ingestion: ${ingestionRssBytes} bytes`);
  console.log(`maximum service RSS during 25-link collection expansion: ${collectionRssBytes} bytes`);
  const footprints = footprintSamples.filter(Boolean);
  if (footprints.length > 0) {
    console.log(`maximum sampled physical footprint: ${Math.max(...footprints.map(({ current }) => current))} bytes`);
    console.log(`peak physical footprint since process start: ${Math.max(...footprints.map(({ peak }) => peak))} bytes`);
  }
  console.log(`SQLite database after fixture import: ${databaseBytes} bytes`);
} finally {
  await stop(service).catch(() => {});
  await stop(fixture).catch(() => {});
  rmSync(temporary, { recursive: true, force: true });
}
