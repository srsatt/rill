# Resource measurements

Measured 2026-08-17 on macOS arm64 with Rust 1.96.0, Node 26.3.1, pnpm
10.33.0, Zig 0.16.0, scriptc 0.0.32, Solid 1.9.14, and Wasmtime 47.0.3.
The release profile uses thin LTO, one codegen unit, panic abort, and symbol
stripping.

Run the repeatable measurement from a completed release build:

```bash
cargo xtask build-release
cargo xtask measure
```

`measure` uses a new temporary SQLite database, the release executable, the
compiled renderer/static assets, and the deterministic local fixture server.
It creates one admin, renders `/login` 100 times, imports the normal RSS
fixture, imports a 25-link roundup at the configured fan-out limit, confirms a
fixture story is searchable, stops Rill through SIGTERM, and deletes only its
own temporary directory.

## Artifacts

| Artifact | Actual value |
|---|---:|
| stripped release Rust executable | 26,000,208 bytes |
| stripped renderer WASM | 298,144 bytes |
| renderer source after Vite SSR transform | 27,486 bytes |
| modern initial JavaScript, raw | 19,971 bytes |
| modern initial JavaScript, gzip -9 | 7,175 bytes |
| modern initial JavaScript, Brotli quality 11 | 6,489 bytes |
| reader JavaScript artifact | 1 raw byte; empty and never referenced by reader HTML |
| scriptc static coverage | 148/148 statements, 100%; zero dynamic remainder |

Lazy Sources, Reader Settings, and Admin chunks are excluded from the modern
initial-bundle value. Reader functionality requires zero JavaScript.

## Runtime

| Runtime state | Actual value |
|---|---:|
| cold process start through `/health/ready` | 57.4 ms |
| idle service RSS after readiness | 94,863,360 bytes (90.47 MiB) |
| maximum service RSS across 100 sequential Solid SSR renders | 95,928,320 bytes (91.48 MiB) |
| maximum service RSS during normal RSS fixture ingestion | 99,106,816 bytes (94.52 MiB) |
| maximum service RSS during 25-link collection expansion | 99,155,968 bytes (94.56 MiB) |
| SQLite database after both fixture imports and graceful stop | 847,872 bytes |

RSS is sampled from `ps` every 20 ms, so values are observed maxima, not a
proof that no shorter transient peak occurred. Cold startup is local wall-clock
time and includes readiness polling. Runs use local feature-hash embeddings,
extractive summaries, local ranking, no external model process, sequential
render load, and loopback fixture HTTP. These are macOS development-host
measurements, not Linux deployment numbers or sustained-load capacity claims.
Repeat on the target host with real feed volume, concurrent users, Telegram and
IMAP connections, plugins, Actions, external models, and long-running jobs
before setting production memory limits.
