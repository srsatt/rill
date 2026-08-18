# Rill architecture

Rill is a modular Rust monolith backed by a bounded SQLite pool. Crate
boundaries isolate domain, persistence, sources, ingestion, intelligence,
actions, plugins, rendering, and HTTP without adding runtime services.

## Data and job flow

```text
RSS/Atom     Telegram public HTML IMAP mail     WASM Component
    \                |               |               /
                  RawSourceItem
                        |
       collection detect + bounded child expansion
                        |
         normalized Document + curator provenance
                        |
         exact dedup -> embedding -> anchor clustering
                        |
      summary -> stream filters -> local/external ranking
                        |
           modern Solid UI / reader Solid UI
```

`rill-jobs` leases durable SQLite jobs with recovery, backoff, dead-letter
state, concurrency limits, and idempotency keys. A source cursor advances only
with a successful batch. Public Telegram channels use the same durable polling
path and one shared source serves all subscribed users.

## Important crates

- `rill-db`: migrations and bounded connections with WAL, foreign keys, busy
  timeout, and migration checksum enforcement.
- `rill-auth`: Argon2id users, hashed opaque sessions, CSRF, roles, audit
  events, one-time reader pairing, and immediate revocation.
- `rill-source-*`: native RSS, IMAP, Telegram, and shared bounded HTTP
  contracts. DNS answers are validated and pinned on every redirect hop.
- `rill-ingestion`, `rill-collection-expansion`, `rill-extraction`, and
  `rill-dedup`: normalized ingestion, roundup children, sanitized article
  extraction, visibility-safe convergence, and provenance.
- `rill-model-api` and `rill-intelligence`: provider identities, summaries,
  compact f32 embeddings, direct-anchor clustering, feedback state, affinity,
  stream filters, ranking, diversity, and local fallback.
- `rill-actions`: encrypted HTTP headers and asynchronous Favorite triggers.
- `rill-plugin-host`: Wasmtime Component Model host with explicit HTTP and
  named-secret capabilities plus per-call fuel, memory, time, and output caps.
- `rill-contracts`, `rill-renderer-host`, and `rill-app`: typed page DTOs,
  stable renderer boundary, routes, worker lifecycle, and security headers.

## Renderer boundary

`rill-renderer-host` exposes one synchronous `Renderer` trait. The Wasmtime
implementation passes a versioned JSON request on WASI stdin and reads a
versioned response on stdout. It inherits no environment, files, network,
database, or subprocess capability. Every call has input, output, fuel, memory,
and epoch timeout limits; traps become typed HTTP failures.

Rust owns authorization, data loading, headers, CSP, hydration serialization,
and final document assembly. Shared Solid TSX owns presentation. Modern pages
hydrate using the same render ID and props. Reader pages use ordinary links and
forms and contain no script element.

## Core vocabulary

- A **Source** fetches items. A **Curator** selected or forwarded an item. A
  **Publisher** authored the underlying article; these identities are not
  collapsed.
- A **Raw Item** is connector output. A **Collection Parent** is a roundup; its
  **Collection Entries** retain per-link title/commentary and parent context.
- A **Document Variant** is one normalized article occurrence. A **Story**
  clusters variants while keeping each variant inspectable. The
  **Representative Variant** is selected using quality and learned affinity.
- A **Stream** combines deterministic filters with an optional semantic vector
  and ranking instruction. A **Summary** is provider-versioned derived data.
- **Feedback** replaces explicit like/dislike state. **Favorite** is independent
  durable state and a stronger positive signal. An **Action** reacts
  asynchronously to a Favorite.
- A **Recommendation** is a persisted ordered run. Provider failure retains the
  deterministic local ranking.

Architectural decisions live in [docs/adr](docs/adr).
