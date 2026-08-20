# Resource measurements

Measured 2026-08-20 on macOS arm64 with Rust 1.96.0, Node 26.3.1, pnpm
10.33.0, Zig 0.16.0, scriptc 0.0.32, Solid 1.9.14, and Wasmtime 47.0.3.
The release profile uses thin LTO, one codegen unit, panic abort, and symbol
stripping.

Run the repeatable measurement from a completed release build:

```bash
cargo xtask build-release
cargo xtask measure
```

`build-release` compiles the stripped renderer WASM into an
architecture-matched Wasmtime serialized module. Rill memory-maps that trusted
artifact and performs no renderer translation or code generation at startup.
The compiler-input `.wasm` is not a deployment artifact.

`measure` uses a new temporary SQLite database, the release executable, the
compiled renderer/static assets, and the deterministic local fixture server.
It creates one admin, renders `/login` 100 times, imports the normal RSS
fixture, imports a 25-link roundup at the configured fan-out limit, confirms a
fixture story is searchable, stops Rill through SIGTERM, and deletes only its
own temporary directory.

## Before and after

The baseline and AOT rows were collected in this worktree with the same command
and workload immediately before and after the change.

| Measurement | Runtime compilation | AOT renderer | Change |
|---|---:|---:|---:|
| stripped release executable | 27,533,200 bytes | 24,922,160 bytes | -9.5% |
| deployed executable + renderer | 28,156,600 bytes | 27,174,704 bytes | -3.5% |
| cold process start through `/health/ready` | 87.5 ms | 29.7 ms | -66.1% |
| idle service RSS after readiness | 226,148,352 bytes (215.67 MiB) | 18,825,216 bytes (17.95 MiB) | -91.7% |
| maximum RSS across 100 sequential Solid SSR renders | 227,328,000 bytes (216.80 MiB) | 20,873,216 bytes (19.91 MiB) | -90.8% |
| maximum RSS during normal RSS fixture ingestion | 231,079,936 bytes (220.38 MiB) | 25,509,888 bytes (24.33 MiB) | -89.0% |
| maximum RSS during 25-link collection expansion | 231,079,936 bytes (220.38 MiB) | 25,509,888 bytes (24.33 MiB) | -89.0% |
| maximum sampled macOS physical footprint | 107,807,488 bytes (102.81 MiB) | 8,405,544 bytes (8.02 MiB) | -92.2% |
| peak macOS physical footprint since process start | 195,347,152 bytes (186.30 MiB) | 8,405,544 bytes (8.02 MiB) | -95.7% |

The raw stripped renderer is 623,400 bytes. Its architecture-specific AOT
artifact is 2,252,544 bytes. Removing unused Wasmtime defaults saves 2,611,040
bytes from the executable; the larger AOT artifact leaves the deployed native
runtime plus renderer 981,896 bytes smaller overall.

## Where RSS is spent after AOT

| Runtime phase | Observed peak RSS | Increase over idle |
|---|---:|---:|
| ready and idle | 17.95 MiB | - |
| 100 sequential SSR renders | 19.91 MiB | 1.95 MiB |
| RSS fixture ingestion | 24.33 MiB | 6.38 MiB |
| 25-link collection expansion | 24.33 MiB | 6.38 MiB |

The old startup compilation accounted for almost all observed peak memory: AOT
removed 177.65 MiB from the measured physical peak. Later SSR added 1.95 MiB
RSS over idle; ingestion and collection work peaked 6.38 MiB over idle.

The runtime executable still includes Cranelift for safe validation and
compilation of untrusted source-plugin Components during admin installation.
Loading user-supplied serialized native artifacts would bypass Wasmtime's
sandbox and is not safe. Removing that remaining compiler requires a separate
trusted compiler/signing boundary; it is not needed to remove renderer startup
compilation.

RSS is sampled from `ps` every 20 ms, so values are observed maxima, not a
proof that no shorter transient peak occurred. On macOS, RSS includes clean and
shared file-backed pages; `footprint` better represents process-owned memory
pressure. Cold startup is local wall-clock time and includes readiness polling.
Runs use local feature-hash embeddings,
extractive summaries, local ranking, no external model process, sequential
render load, and loopback fixture HTTP. These are macOS development-host
measurements, not Linux deployment numbers or sustained-load capacity claims.
Repeat on the target host with real feed volume, concurrent users, Telegram and
IMAP connections, plugins, Actions, external models, and long-running jobs
before setting production memory limits.
