# Final implementation report

## Resulting architecture

Rill is one Tokio/Axum Rust process with a bounded rusqlite pool, WAL SQLite,
durable leased jobs, a capability-poor Wasmtime Preview 1 renderer, and a
separate Wasmtime Component Model source-plugin host. Rust owns routing,
authorization, persistence, jobs, network policy, final HTML assembly, and safe
hydration serialization. Shared Solid TSX owns modern and reader presentation.

Important entry points:

- `crates/app`: CLI, HTTP server, worker lifecycle, maintenance, metrics.
- `crates/auth`, `crates/db`, `crates/jobs`, `crates/secrets`: durable platform
  boundaries.
- `crates/source-*`, `crates/ingestion`, `crates/collection-expansion`,
  `crates/extraction`, `crates/dedup`: source-to-document pipeline.
- `crates/model-api`, `crates/intelligence`: provider adapters, summaries,
  embeddings, clustering, streams, affinity, ranking, and cache invalidation.
- `crates/actions`, `crates/plugin-host`: asynchronous Favorite integrations
  and capability-limited third-party sources.
- `crates/contracts`, `crates/renderer-host`, `ui/src`: Rust-generated page
  contracts, renderer ABI/limits, Solid SSR, hydration, and reader forms.
- `migrations`, `config`, `deploy`, `fixtures`, and `docs`: persistence,
  operations, deterministic integration data, and operator guidance.

[ARCHITECTURE.md](../ARCHITECTURE.md) contains vocabulary and flow details.

## Exact Solid to Wasmtime build flow

`cargo xtask build-release` performs:

```text
Rust contracts -> ts-rs declarations -> ui/generated/render-contract.ts
Solid TSX -> Vite client build -> hashed browser assets
Solid TSX -> Vite SSR build -> dist/renderer/renderer.js
renderer.js -> scriptc coverage --npm-static auto
renderer.js -> SCRIPTC_CC=zigcc + SCRIPTC_TARGET=wasm32-wasi
            -> ui-renderer.wasm -> custom-section stripping
Rust workspace -> thin-LTO stripped rill executable
```

The 2026-08-17 build compiled 148/148 renderer statements statically with no
dynamic remainder. At runtime `rill-renderer-host` writes one versioned JSON
request to controlled WASI stdin, invokes `_start` under epoch, fuel, memory,
input, and output limits, reads controlled stdout, and validates the versioned
JSON response. The guest receives no inherited environment, preopened files,
network, database, or process capability. Production contains no JavaScript
engine.

## Collection pipeline and roundup representation

Every native/plugin connector emits bounded `RawSourceItem` values with stable
external identity. Deterministic collection detection scores structured cards
and meaningful links, filters repeated/tracking/social/navigation/unsubscribe
links, applies source/manual overrides, and caps fan-out. An optional remote
collection parser may classify only URLs present in the source; invented URLs
are rejected.

Accepted parents persist `collection_expansions` plus ordered
`collection_entries`. Stable parent-and-URL identity makes reprocessing
idempotent. Each child keeps parent raw-item ID, collection-entry ID, curator,
title/author hints, and per-link commentary. Children independently run through
article extraction, normalization, summary, embedding, exact deduplication,
semantic clustering, stream filtering, and recommendation.

Telegram roundups preserve channel/account/message/update metadata. Email
roundups preserve mailbox/message/MIME and List-Unsubscribe metadata. Both use
the same parent/entry/derived-item representation after connector output.
Direct and roundup discoveries may converge on one document/story, while
many-to-many `document_curators`, raw occurrences, and collection entries keep
every curator path. Generated summary text never overwrites curator commentary.

## Measured release

| Measurement | Actual value |
|---|---:|
| release executable | 26,000,208 bytes |
| renderer WASM | 298,144 bytes |
| modern initial JS | 19,971 raw / 6,489 Brotli bytes |
| reader JS | 0 functional bytes; empty 1-byte artifact, no reader script tag |
| cold startup to ready | 57.4 ms |
| idle RSS | 94,863,360 bytes (90.47 MiB) |
| max RSS after 100 SSR renders | 95,928,320 bytes (91.48 MiB) |
| max RSS during RSS ingestion | 99,106,816 bytes (94.52 MiB) |
| max RSS during 25-link expansion | 99,155,968 bytes (94.56 MiB) |
| fixture SQLite database | 847,872 bytes |

Exact method and scope: [resource-measurements.md](resource-measurements.md).

## Commands

One-command development after dependency installation:

```bash
pnpm --dir ui install --frozen-lockfile
node tools/dev.mjs
```

The coordinator generates contracts, builds renderer/client artifacts, creates
a temporary database and development admin, starts fixtures and Rill, watches
Rust/UI/config/migration inputs, rebuilds affected assets, and restarts Rill.
Set `RILL_DEV_DATABASE_PATH` to retain a development database.

Verification and measurement:

```bash
pnpm --dir ui typecheck
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask test-e2e
cargo xtask build-release
cargo xtask measure
```

Production:

```bash
target/release/rill --config /etc/rill/config.toml doctor
target/release/rill --config /etc/rill/config.toml migrate
target/release/rill --config /etc/rill/config.toml backup /srv/backup/rill.db
target/release/rill --config /etc/rill/config.toml serve
```

## Validation scope

Local deterministic fixtures exercised RSS/Atom parsing and live HTTP fetch,
article extraction, direct and roundup ingestion, 25-link bounded fan-out,
HTML/plain MIME newsletters, Telegram message/edit/delete/media state,
OpenAI-compatible embedding/summary/collection responses, external ranking,
HTTP feedback/Action endpoints, and provider retry/output bounds. The example
WASM component was installed and polled in the real Component Model host; a
trapping component remained isolated and updated health.

Playwright exercised real login, in-place Solid hydration without console
errors/root replacement, JavaScript-disabled reader pairing/cookie/feed, RSS
creation and polling, Story Like/Dislike/Favorite requests, and stream
create/edit/delete. Renderer tests exercised every template, malicious strings,
safe hydration JSON, a 50-story page, deterministic output, unknown templates,
invalid guest JSON, and input/output/fuel/time/memory limits.

## Credentials and remaining limitations

Real Telegram bot traffic, live public-preview stability, real IMAP servers,
remote model vendors, remote Action receivers, third-party plugins,
reverse-proxy TLS, container startup, restore drills, and target-host sustained
load require real credentials or deployment infrastructure and are not claimed
as validated. Admins can switch encrypted global embedding, ranking, and
text-parse provider settings live; vendor-specific behavior remains
deployment-specific. Automatic master key rotation is not implemented.
Resource numbers are short macOS arm64 local measurements, not deployment
capacity claims.

## Significant decisions

- ADR 0001: Solid SSR compiled by scriptc into a capability-poor WASI module;
  unsupported dynamic JavaScript is a build failure.
- ADR 0002: synchronous rusqlite remains behind bounded connections and
  blocking-pool HTTP boundaries instead of adding an async database service.
- SQLite leases/idempotency provide the durable worker and retry model.
- Story clustering preserves all variants/provenance; representative selection
  is affinity-aware and replaceable without changing Story identity.
- Favorites are local durable state; external Actions are asynchronous effects.
