# Final implementation report

## PoC completion update — 2026-08-19

### TL;DR

Rill now ranks locally with a durable bounded logistic model, preserves typed original/discussion links from RSS and Atom, and can deliver a Favorite through a configurable JSON HTTP action. The final UI has consistent cards and typography, responsive feed controls, tabbed settings, stable account layout, and a persistent light/dark theme picker. Reader remains JavaScript-free, all touch targets meet the 44px requirement, and the seven-icon static Lucide sprite retains 100% ScriptC coverage. Local verification and the live Karakeep proof are complete.

### Architecture changes

- Raw feedback and embeddings remain authoritative. A coalesced `RefitPreferenceModel` job fits L2-regularized binary logistic regression after five new labeled events, uses at most 500 recent mixed-class examples, stores versioned coefficients atomically, and invalidates ranking caches. Ranking stays deterministic when the model is absent, incompatible, corrupt, or under-labeled; otherwise it blends 75% deterministic score with 25% preference probability. Provider-controlled ranking and feedback are disabled.
- `RawSourceItem` links now use bounded `{url, relation, title, ordinal}` values. RSS `<comments>` and Atom `rel="replies"` become normalized persisted document links without hostname rules. Story views render distinct “Open original” and “Discussion” actions while Favorite payloads retain the canonical article URL.
- HTTP actions accept an optional structured JSON body template and per-user private headers. The settings form sends the value once, the server encrypts it through the existing secret store, and views return only a redacted configured state. Local Favorite persistence commits before delivery is queued; failed delivery cannot undo it.
- A dedicated durable `ExecuteAction` queue consumer prevents slow model enrichment from blocking user-triggered Favorite delivery. General job concurrency remains bounded by configuration.
- Reader title, metadata, and summary use a compact semantic type scale and AA contrast. Shared spacing/control tokens, responsive controls, explicit emoji feedback labels, a favicon, and seven official Lucide symbols cover the visual audit without adding client JavaScript to Reader.

### HTTP action template contract

Allowed placeholders are `${event}`, `${eventId}`, `${story.id}`, `${story.title}`, `${story.summary}`, `${story.url}`, `${story.source}`, `${story.curator}`, `${story.publishedAt}`, and `${story.relatedLinks}`. An exact placeholder may resolve to a JSON array or `null`; placeholders embedded inside strings must resolve to scalars. Unknown, unterminated, unsafe, or oversized input is rejected. Omitting the template preserves the original full event JSON body.

### Verification evidence

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test --workspace --all-targets`: 137 tests across 26 suites passed.
- `pnpm --dir ui typecheck` and `pnpm --dir ui build`: passed; renderer 241/241 statements static.
- `cargo xtask verify-renderer`: stripped WASM 626,067 bytes; 6 resource-limit tests and 12 renderer view/determinism/escaping tests passed.
- Playwright: 8/8 passed on isolated ports, including JavaScript-disabled Reader pairing, responsive navigation, theme persistence, typed original/discussion links, encrypted per-user headers, stream drag/reorder, account-menu scroll behavior, and admin/model tabs.
- `cargo xtask build-release`, packaged `rill doctor`, and `cargo xtask measure`: passed.
- Before/after screenshots, accessibility trees, annotations, and objective measurements are ignored runtime artifacts linked from [visual-audit.md](visual-audit.md).

### Live Karakeep evidence

On 2026-08-19 the generic action was created through `/settings/readers` as `POST https://keep.srsatt.dev/api/v1/bookmarks`, using the documented JSON template and an encrypted `Authorization` header. The UI confirmed that the custom body was configured and private headers were encrypted; no token value entered logs or this report. The current form accepts that value directly as a per-user secret instead of asking for a server environment-variable name.

The normal Favorite UI delivered story `892844e2-77db-4e5e-98e4-80282dd48d12`, whose canonical URL is `http://127.0.0.1:3014/article/visual-audit-11`. Execution `5d416b66-b7cb-4e1e-86f1-3d7619f6ef85` succeeded on attempt 1 with a 2xx response. Repeating the same Favorite delivery created execution `359c4b14-d926-44fa-ba57-8c28b56db87b`, which also succeeded on attempt 1 with a 2xx response.

An authenticated Karakeep query over all bookmarks found exactly one exact-URL match before and after the replay. The surviving bookmark ID is `bkdpxw4pimmzcrks416735li`; it remains favourited at `http://127.0.0.1:3014/article/visual-audit-11` and was intentionally left in place.

### Known limitations and exact follow-up experiments

- The preference model is deliberately binary and compact. After at least 50 mixed like/dislike labels, snapshot the same candidate sets and compare deterministic versus blended ranking with AUC and nDCG@10; change the 25% blend only if both improve.
- Benchmark refit wall time and peak RSS with 100, 500, and 1,000 recent examples. Raise the 500-example cap only if the larger window improves held-out AUC and remains below a 250ms local refit target.
- Keep attention/vector-native experiments in the sibling `rill-experiments` directory. Reconsider product integration only if a frozen-data comparison beats the logistic baseline on nDCG@10 while preserving bounded memory and restart determinism.

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
            -> Wasmtime AOT -> architecture-matched ui-renderer.cwasm
Rust workspace -> thin-LTO stripped rill executable
```

The 2026-08-20 verification compiled 241/241 renderer statements statically with no
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

## Measured AOT release (2026-08-20)

| Measurement | Actual value |
|---|---:|
| release executable | 24,922,160 bytes |
| renderer compiler-input WASM | 623,400 bytes |
| renderer AOT module | 2,252,544 bytes |
| deployed executable + renderer | 27,174,704 bytes |
| modern initial JS | 47,226 raw / 14,421 Brotli bytes |
| reader JS | no artifact or script tag; 0 bytes |
| cold startup to ready | 29.7 ms |
| idle RSS | 18,825,216 bytes (17.95 MiB) |
| max RSS after 100 SSR renders | 20,873,216 bytes (19.91 MiB) |
| max RSS during RSS ingestion | 25,509,888 bytes (24.33 MiB) |
| max RSS during 25-link expansion | 25,509,888 bytes (24.33 MiB) |
| max sampled macOS physical footprint | 8,405,544 bytes (8.02 MiB) |
| peak macOS physical footprint | 8,405,544 bytes (8.02 MiB) |
| fixture SQLite database | 782,336 bytes |

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
creation and polling, typed story links, a UI-created Favorite action and delivered
body, Story Like/Dislike/Favorite requests, and stream create/edit/delete. Renderer tests exercised every template, malicious strings,
safe hydration JSON, a 50-story page, deterministic output, unknown templates,
invalid guest JSON, and input/output/fuel/time/memory limits.

## Credentials and remaining limitations

Real Telegram bot traffic, live public-preview stability, real IMAP servers,
remote model vendors, third-party plugins,
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
