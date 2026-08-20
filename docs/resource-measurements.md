# Resource measurements

Measured 2026-08-19 on macOS arm64 with Rust 1.96.0, Node 26.3.1, pnpm
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
| stripped release Rust executable | 27,483,392 bytes |
| stripped renderer WASM | 626,067 bytes |
| renderer source after Vite SSR transform | 61,243 bytes |
| modern initial JavaScript, raw | 47,399 bytes |
| modern initial JavaScript, gzip -9 | 16,181 bytes |
| modern initial JavaScript, Brotli quality 11 | 14,454 bytes |
| reader JavaScript artifact | none; 0 bytes and no reader script tag |
| scriptc static coverage | 241/241 statements, 100%; zero dynamic remainder |

Lazy Sources, Reader Settings, and Admin chunks are excluded from the modern
initial-bundle value. Reader functionality requires zero JavaScript.

## Runtime

| Runtime state | Actual value |
|---|---:|
| cold process start through `/health/ready` | 83.3 ms |
| idle service RSS after readiness | 204,259,328 bytes (194.80 MiB) |
| maximum service RSS across 100 sequential Solid SSR renders | 205,471,744 bytes (195.95 MiB) |
| maximum service RSS during normal RSS fixture ingestion | 208,994,304 bytes (199.31 MiB) |
| maximum service RSS during 25-link collection expansion | 209,158,144 bytes (199.47 MiB) |
| maximum sampled macOS physical footprint | 87,376,592 bytes (83.33 MiB) |
| peak macOS physical footprint since process start | 205,832,864 bytes (196.30 MiB) |
| SQLite database after both fixture imports and graceful stop | 946,176 bytes |

RSS is sampled from `ps` every 20 ms, so values are observed maxima, not a
proof that no shorter transient peak occurred. On macOS, RSS includes clean and
shared file-backed pages; `footprint` better represents process-owned memory
pressure. RSS varied from about 190 to 216 MiB across otherwise identical
runs while settled physical footprint stayed between about 76 and 99 MiB. A
manual post-start inspection found a 15.0 MiB live heap. The roughly 196 MiB
physical peak in the recorded run occurs
while Wasmtime compiles the renderer module during startup; the configured 64
MiB guest-memory cap was only 64 KiB resident. One hundred later SSR renders
added about 0.2 MiB of physical footprint. Cold startup is local wall-clock
time and includes readiness polling. Runs use local feature-hash embeddings,
extractive summaries, local ranking, no external model process, sequential
render load, and loopback fixture HTTP. These are macOS development-host
measurements, not Linux deployment numbers or sustained-load capacity claims.
Repeat on the target host with real feed volume, concurrent users, Telegram and
IMAP connections, plugins, Actions, external models, and long-running jobs
before setting production memory limits.
