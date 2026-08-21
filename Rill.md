# Rill

## Build a Resource-Efficient, Self-Hosted Personalized News Reader

You are a senior staff software architect and implementation engineer specializing in Rust, WebAssembly, SolidJS, information retrieval, recommender systems, public-web ingestion, Telegram bots, authentication, and resource-efficient self-hosted software.

Build the complete service described below in the current repository.

The working project name is **Rill**.

Do not merely produce a design document or scaffold. Deliver a working vertical product with migrations, tests, frontend assets, production builds, local development tooling, deployment documentation, realistic fixtures, and measured resource usage.

When a technical detail is not explicitly specified, choose the simplest robust solution that preserves the architectural boundaries below.

Document significant decisions as ADRs.

Do not stop to ask questions unless a required external credential is genuinely necessary for a manual integration test.

---

# 1. Product Goal

Rill is a self-hosted personalized information reader.

It aggregates content from:

- RSS and Atom feeds
- Public Telegram channels through their web previews, with an optional binding bot
- Email newsletters through IMAP
- Future external source plugins

It transforms a potentially overwhelming amount of incoming information into a much smaller set of useful stories.

The application must:

- Normalize heterogeneous source items into a common domain model.
- Detect digest-style and collection-style source items.
- Expand newsletters, Telegram roundups, and other collections into independent feed candidates.
- Preserve the original collection and curator provenance.
- Extract and sanitize article content.
- Deduplicate identical links.
- Cluster semantically equivalent coverage into stories.
- Preserve all source variants and curator relationships inside a story.
- Generate concise article summaries.
- Rank stories per user.
- Learn from explicit feedback.
- Learn content preference separately from source, curator, and publisher preference.
- Support `Like`, `Dislike`, and `Favorite`.
- Support configurable user actions triggered by application events.
- Support user-defined tab-based streams such as `Germany`, `AI`, `Software`, or `Science`, alongside built-in `All`.
- Provide a modern hydrated interface for phones and computers.
- Provide a much simpler e-reader interface that works without JavaScript.
- Support multiple users, roles, sessions, reader devices, and device revocation.
- Pair e-readers using a short one-time code that becomes an HttpOnly device-session cookie.
- Run primarily as one resource-efficient Rust service with SQLite.
- Keep model inference outside the main process behind explicit provider interfaces.
- Support sandboxed future source plugins through WebAssembly.

The system is a **modular monolith**, not a collection of microservices.

The deployment goal is:

```text
one Rust process
one SQLite database
one UI renderer WASM module
optional source/action WASM plugins
external model endpoints
static browser assets
```

Do not require:

- Postgres
- Redis
- Kafka
- RabbitMQ
- NATS
- external job queues
- vector databases
- Node.js in production
- an embedded JavaScript runtime in production

Development-time Node tooling is allowed for the Solid frontend build.

---

# 2. Architectural Philosophy

Follow these principles throughout the implementation.

1. Own the domain logic, outsource commodity inference.
2. Keep external boundaries explicit and versioned.
3. Use SQLite until a demonstrated workload proves it insufficient.
4. Prefer pure functions for normalization, collection parsing, clustering, scoring, and rendering inputs.
5. Keep source-specific behavior outside the core domain.
6. Preserve provenance instead of flattening information into one source string.
7. Store raw user feedback events so aggregates can be rebuilt.
8. Never mix embeddings produced by incompatible model versions.
9. Make jobs idempotent and retryable.
10. Make the e-reader experience functional with HTML and CSS alone.
11. Treat browser hydration as progressive enhancement.
12. Keep private-source data isolated between users.
13. Avoid infrastructure introduced only because it is fashionable.
14. Avoid giant files and frontend monoliths.
15. Split files that grow beyond roughly 500 lines unless they are generated.
16. Introduce traits only at real architectural boundaries.
17. Do not build framework-shaped abstractions around trivial application code.
18. Do not use panicking error handling in request, job, source, plugin, or model-provider paths.
19. Generated files must live in clearly marked generated directories.
20. Production must remain understandable by one engineer reading the repository.
21. Resource usage is an explicit product constraint.
22. Deterministic parsing should precede LLM-assisted parsing wherever practical.
23. External models may improve decisions but must not become the only path through which the product works.

---

# 3. Non-Negotiable Stack

## Backend

Use:

- Rust stable
- Tokio
- Axum
- Tower
- SQLite in WAL mode
- SQLx or Rusqlite
- Serde
- Tracing
- Reqwest or an equivalent maintained HTTP client
- Wasmtime for sandboxed WebAssembly execution

Choose SQLx or Rusqlite and explain the decision in an ADR.

## Frontend

Use:

- SolidJS core
- TypeScript
- TSX
- Vite
- Solid compiler

Do not use:

- React
- Next.js
- Vue
- Svelte
- Marko
- SolidStart as the production application server
- browser-side Rust UI frameworks

The same Solid TSX component source should be usable for:

- server-side rendering
- browser hydration

## Persistence

Use:

- one SQLite database file
- migrations committed to the repository
- WAL mode
- foreign keys
- FTS5

## Production runtime

Production should consist primarily of:

```text
rill
rill.db
ui-renderer.wasm
static/
plugins/
config.toml
```

No Node, Bun, Deno, V8, QuickJS, or other JavaScript runtime may be required by normal production operation.

---

# 4. High-Level Architecture

The target processing pipeline is:

```text
 RSS       Telegram       Email       Plugins
  │           │             │            │
  └───────────┴───────┬─────┴────────────┘
                      ▼
                  INGESTION
                      │
                      ▼
                 RAW ITEMS
                      │
                      ▼
                NORMALIZATION
                      │
                      ▼
              COLLECTION DETECTION
                  ┌───┴────┐
                  │        │
              ordinary   digest/
                item     collection
                  │        │
                  │        ▼
                  │    EXPANSION
                  │    ┌───┼───┐
                  │    ▼   ▼   ▼
                  │   A   B   C
                  └────┬───┴───┘
                       ▼
              exact deduplication
                       │
                       ▼
              content extraction
                 ┌─────┴─────┐
                 ▼           ▼
             embedding    summary
                 │           │
                 └─────┬─────┘
                       ▼
              semantic clustering
                       │
                       ▼
                     STORY
            variants + curator provenance
                       │
                       ▼
                  USER STREAMS
                       │
                       ▼
               candidate scoring
                       │
                       ▼
           external recommendation
                       │
                       ▼
                diversity/explore
                       │
                       ▼
          per-user representative
                       │
             ┌─────────┴─────────┐
             ▼                   ▼
        modern UI            reader UI
        SSR+hydrate           SSR-first
             │
       👍   👎   ★
                 │
                 ▼
           generic Actions
```

---

# 5. Repository Structure

Create a Rust workspace approximately like:

```text
/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── README.md
├── ARCHITECTURE.md
├── SECURITY.md
├── CONTRIBUTING.md
├── docs/
│   └── adr/
├── config/
│   ├── example.toml
│   └── development.toml
├── crates/
│   ├── app/
│   ├── domain/
│   ├── contracts/
│   ├── db/
│   ├── auth/
│   ├── jobs/
│   ├── ingestion/
│   ├── collection-expansion/
│   ├── extraction/
│   ├── dedup/
│   ├── streams/
│   ├── recommendation/
│   ├── model-providers/
│   ├── source-api/
│   ├── source-rss/
│   ├── source-telegram/
│   ├── source-email/
│   ├── plugin-host/
│   ├── renderer-host/
│   ├── actions/
│   └── web/
├── ui/
│   ├── package.json
│   ├── pnpm-lock.yaml
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── src/
│   │   ├── components/
│   │   ├── pages/
│   │   ├── modern/
│   │   ├── reader/
│   │   ├── shared/
│   │   ├── modern-server.tsx
│   │   ├── modern-client.tsx
│   │   ├── reader-server.tsx
│   │   └── reader-client.tsx
│   ├── generated/
│   └── tests/
├── wit/
│   ├── source-plugin/
│   └── action-plugin/
├── plugins/
│   ├── sdk-rust/
│   └── examples/
├── migrations/
├── tests/
│   ├── fixtures/
│   ├── integration/
│   └── e2e/
├── tools/
│   └── xtask/
└── deploy/
    ├── systemd/
    └── docker/
```

These are compile-time/module boundaries, not independent network services.

---

# 6. Solid SSR Through `scriptc` and WASI

This architecture is a core experiment and must be implemented first.

The Solid server-side renderer runs as an AOT-compiled WASI module inside the Rust process.

## 6.1 Build pipeline

Use:

```text
             Solid TSX source
                    │
          ┌─────────┴─────────┐
          ▼                   ▼
   Solid client build    Solid SSR build
          │                   │
          ▼                   ▼
 browser JS assets       bundled TS/JS
                              │
                              ▼
                           scriptc
                              │
               SCRIPTC_CC=zigcc
               SCRIPTC_TARGET=wasm32-wasi
                              │
                              ▼
                       ui-renderer.wasm
                              │
                              ▼
                         Rust / Wasmtime
```

The expected build invocation is equivalent to:

```bash
SCRIPTC_CC=zigcc \
SCRIPTC_TARGET=wasm32-wasi \
scriptc build renderer.ts -o ui-renderer.wasm
```

Use `scriptc coverage` in CI.

The renderer must remain fully statically compiled.

Do not enable the `--dynamic` QuickJS island.

---

# 7. Renderer Architecture

The Rust application owns:

- routing
- authentication
- authorization
- database access
- stream selection
- story ranking
- action execution
- model calls
- HTTP headers
- cache policy
- CSRF
- page data loading

The renderer owns only presentation.

Conceptually:

```rust
let result = renderer.render(RenderRequest {
    template: "modern-feed",
    props,
    render_id,
    locale,
    asset_manifest,
    csrf_token,
})?;
```

The public Rust abstraction should resemble:

```rust
pub trait Renderer {
    fn render(
        &self,
        request: &RenderRequest,
    ) -> Result<RenderResponse, RenderError>;
}
```

The host-side application must not care about the exact transport used to invoke the WASI module.

---

# 8. WASI Renderer ABI

`scriptc` produces a WASI Preview 1 standalone module.

Implement a narrow versioned request/response protocol.

Prefer direct exported functions if the generated module supports them cleanly.

Otherwise use a small WASI stdin/stdout protocol:

```text
stdin:
single JSON or length-delimited RenderRequest

stdout:
single JSON RenderResponse
```

Encapsulate this transport entirely inside `renderer-host`.

Do not let the rest of the application depend on stdin/stdout details.

Example request:

```json
{
  "version": 1,
  "template": "modern-feed",
  "mode": "modern",
  "locale": "en",
  "renderId": "request-specific-id",
  "props": {},
  "assets": {},
  "csrfToken": "..."
}
```

Example response:

```json
{
  "version": 1,
  "status": 200,
  "headHtml": "...",
  "bodyHtml": "...",
  "hydrationState": {}
}
```

---

# 9. Renderer Sandbox

The renderer must be deliberately capability-poor.

Do not provide:

- network access
- sockets
- application database access
- process spawning
- environment inheritance
- application filesystem access

Prefer:

```text
stdin       controlled request
stdout      controlled response
stderr      captured diagnostics
env         empty
filesystem  no preopens
network     unavailable
```

Apply limits for:

- execution time
- Wasmtime fuel
- memory
- input size
- output size

A renderer trap must produce a normal server error and must not terminate the Rust process.

The renderer must be deterministic for equivalent inputs.

---

# 10. Solid Hydration

Use the same TSX components for server and browser.

The server and client builds must share:

- template identifiers
- component structure
- serializable props
- hydration markers
- stable render IDs

Hydration state must be serialized safely.

Do not interpolate unescaped JSON directly into executable JavaScript.

Use a non-executable JSON script element or equivalent safe mechanism.

Implement a browser test that:

1. requests an SSR page
2. stores the initial DOM
3. runs the client bundle
4. hydrates
5. fails on hydration mismatch
6. fails on unexpected root replacement
7. verifies interactive controls

---

# 11. Resource Goals

Measure separately:

- Rust executable size
- renderer WASM size
- modern browser bundle
- reader browser bundle
- idle RSS
- RSS under render load
- RSS during ingestion

Targets:

```text
renderer WASM:
< 1 MiB uncompressed
stretch goal < 500 KiB

modern initial JS:
< 50 KiB Brotli
excluding lazy admin bundles

reader initial JS:
< 10 KiB Brotli
core reader functionality must require 0 JS

core service idle RSS:
< 128 MiB
excluding external model processes

normal operation:
< 256 MiB
excluding external model processes
```

These are goals, not excuses to fake measurements.

Report actual numbers.

---

# 12. Core Domain Model

Distinguish clearly between:

## Source Definition

A kind of source:

```text
RSS
Telegram
IMAP
WASM Plugin
```

## Source Instance

A configured connection belonging to:

- a user
- the installation
- an administrator-managed shared scope

Includes:

- configuration
- encrypted credentials
- cursor
- health
- plugin identity where applicable

## Raw Source Item

The result directly produced by a connector.

Include:

```text
stable external ID
source instance
original URL
title
text
HTML
author
publication time
edit time
media metadata
external links
source metadata
visibility scope
content checksum
```

## Collection Parent

A raw item detected as a digest or link collection.

Examples:

- newsletter issue
- Telegram roundup
- weekly recommendations post
- RSS roundup article

The parent remains stored even if only its children are shown in the feed.

## Collection Entry

A derived candidate extracted from a parent collection.

Include:

```text
parent raw item ID
target URL
title hint
commentary
author hint
publication hint
ordinal
confidence
extraction method
curator
```

## Document Variant

A normalized representation of one publisher or curator's coverage.

Include:

```text
canonical URL
original URL
source
curator
publisher
title
clean text
sanitized HTML
language
publication time
update time
embedding reference
story ID
quality signals
visibility scope
```

## Story

A semantic grouping of equivalent or substantially equivalent coverage.

Include:

```text
story ID
anchor embedding
first publication
latest publication
variant IDs
coverage count
language
manual merge/split metadata
```

## User-specific Story State

Store separately:

```text
read state
hidden state
explicit feedback
favorite state
recommendation score
selected representative
last impression
reader progress when available
```

Never store one globally selected representative on the Story as if all users prefer the same source.

---

# 13. Multi-User Model

Support multiple users from the beginning.

Roles:

```text
admin
user
```

Reader devices are restricted sessions, not separate users.

Rules:

- Every user-specific query must be scoped by user ID.
- Shared public sources may share normalized documents and story clusters.
- Private Telegram and private email content must not leak across users.
- Private items may only deduplicate inside a compatible visibility scope.
- Never reveal the existence of a private source through cross-user clustering.
- Source credentials must be encrypted at rest.
- The encryption master key must live outside SQLite.
- Secrets are never returned to browsers after creation.

---

# 14. SQLite Database

Use:

- WAL
- foreign keys
- busy timeout
- explicit migrations
- bounded connection pool
- transactions for state transitions
- FTS5
- maintenance jobs

Create tables or equivalent normalized structures for at least:

```text
users
password_credentials
sessions
device_sessions
pairing_codes
audit_events

source_definitions
source_instances
source_subscriptions
source_cursors
source_health
encrypted_secrets

raw_items

collection_expansions
collection_entries

documents
document_curators
stories
story_memberships
media
canonical_urls

embedding_records
summaries

model_providers
recommendation_runs
recommendation_scores

feedback_events
user_story_state
source_affinity_events

streams
stream_rules
user_stream_state

action_definitions
action_triggers
action_executions
action_attempts

jobs
job_attempts

plugin_installations
plugin_permissions
```

The schema must support:

```text
one collection parent -> many derived entries

one document -> many curators

one curator -> many documents
```

Do not encode these relationships only inside opaque JSON metadata.

Store embeddings compactly as binary floating-point data or another documented compact representation.

Every embedding record must include:

```text
provider
model
model version
dimension
input checksum
entity type
entity ID
creation time
```

Never compare incompatible vector identities.

---

# 15. Internal Background Job Queue

Implement a SQLite-backed job queue inside the Rust process.

Do not add an external broker.

Jobs require:

```text
type
JSON payload
state
priority
scheduled time
attempt count
lease owner
lease expiry
last error
idempotency key
retry policy
dead-letter state
```

Required jobs include:

```text
PollSource
ParseEmail
NormalizeRawItem

DetectCollection
ExpandCollection
ParseCollectionWithProvider
ProcessDerivedItem

ExtractArticle
ResolveCanonicalUrl
GenerateEmbedding
GenerateSummary
ClusterStory

InvalidateRecommendations
EvaluateStreamCandidates
SubmitRecommendationFeedback

ExecuteAction
RetryAction

RecomputeAffinity
ReembedContent

CleanupSessions
CleanupPairingCodes
DatabaseMaintenance
```

The exact collection-job split may be simplified if a smaller idempotent pipeline is cleaner.

Workers must have bounded concurrency.

A process crash must not permanently lose leased work.

Set a configurable maximum collection expansion fan-out.

---

# 16. Source Connector Boundary

Create one internal Rust abstraction for source implementations.

Conceptually:

```rust
#[async_trait]
pub trait SourceConnector: Send + Sync {
    fn kind(&self) -> SourceKind;

    fn metadata(&self) -> ConnectorMetadata;

    fn config_schema(&self) -> serde_json::Value;

    async fn validate(
        &self,
        context: &ConnectorContext,
        config: &serde_json::Value,
    ) -> Result<ValidationResult, ConnectorError>;

    async fn poll(
        &self,
        context: &ConnectorContext,
        config: &serde_json::Value,
        cursor: Option<&serde_json::Value>,
        limit: usize,
    ) -> Result<SourceBatch, ConnectorError>;
}
```

Core ingestion must not know about Telegram-specific, IMAP-specific, RSS-specific, or Wasmtime-specific types.

All connectors produce the same `RawSourceItem` domain contract.

---

# 17. RSS and Atom

Implement native Rust RSS and Atom support.

Requirements:

- maintained XML/feed parser
- ETag
- Last-Modified
- conditional requests
- bounded redirects
- per-host timeout
- response-size limits
- compression
- GUID and Atom ID handling
- stable identity fallback
- publication timestamps
- feed-provided summaries/content
- OPML import/export
- shared or user-owned feeds
- per-feed polling intervals
- enable/disable state

Never parse XML using regular expressions.

RSS entries themselves may be digest-style items and must be eligible for Collection Detection.

---

# 18. Telegram as a First-Class Source

Telegram is not an RSS bridge.

Fetch public channel preview HTML through a bounded connector. Parse it into the
same generic source-item structure as RSS and email. One channel is fetched and
parsed once; per-user subscriptions control visibility.

The Bot API is a control plane, not the content transport. Use teloxide for one
long-polling bot that binds a Telegram identity to a Rill account and accepts a
forwarded public-channel message or an explicit `@username` as subscription
input. Private and protected channels are unsupported.

---

# 19. Telegram Binding

Modern UI flow:

```text
create one-time deep link
    ↓
open bot and consume token
    ↓
bind Telegram identity
    ↓
forward public-channel post or send @username
    ↓
create/reuse shared source and user subscription
```

Store only a hash of the short-lived binding token. Encrypt the admin-managed
bot token, never return it, and never log either token.

---

# 20. Telegram Item Model

Preserve:

```text
channel username
message ID
message text
media metadata
published time
forward origin
external URLs
canonical t.me URL
parser version
```

Support:

- bounded initial backfill
- bounded pagination cursor
- recent edit overlap
- media-group coalescing
- idempotent updates
- per-channel parser and source health

Do not infer deletion from disappearance on a rolling preview page.

Do not download large media by default.

---

# 21. Curator vs Publisher

Telegram frequently acts as a curator rather than publisher.

Represent independently:

```text
curator = Telegram channel
publisher = linked website
```

Example:

```text
@excellent_linux_channel
   │
   │ commentary
   ▼
LWN article
```

These are distinct information relationships.

When a Telegram message links to an article:

- preserve Telegram commentary
- create/extract the external document
- cluster both when appropriate
- never discard the Telegram post merely because an external URL exists

User affinity for curator and publisher must be modeled independently.

---

# 22. Email Newsletters

Implement native IMAP ingestion.

Support:

- per-user accounts
- selected folders
- UID-based cursor
- MIME decoding
- multipart selection
- HTML sanitization
- text fallback
- sender metadata
- list metadata
- Message-ID identity
- link extraction
- configurable mark-as-read behavior

Default behavior must not modify or delete mail.

Treat newsletter sender and linked publisher independently when appropriate.

Email newsletters are expected to be a major source of collections and must feed directly into Collection Detection.

---

# 23. Collection Detection and Expansion

Rill must treat **digest-style source items and link collections as first-class input**.

A single source item may contain multiple independently valuable stories.

Examples:

- email newsletter containing several recommended articles
- Telegram post containing a curated list of links
- “5 interesting things this week”
- source plugin returning a collection
- RSS article whose content is a link roundup

Each useful linked entry must be able to become an independent feed candidate.

---

# 24. Collection Processing Model

The pipeline is:

```text
Source Item
    │
    ▼
Normalization
    │
    ▼
Collection Detection
    │
    ├── Single
    │      │
    │      ▼
    │ normal processing
    │
    └── Collection
           │
           ▼
       Expansion
        ┌──┼──┐
        ▼  ▼  ▼
        A  B  C
        │  │  │
        └──┼──┘
           ▼
 normal Rill pipeline
```

Every expanded child independently undergoes:

- URL canonicalization
- extraction
- summary generation
- embedding
- exact deduplication
- semantic clustering
- stream filtering
- recommendation
- Like
- Dislike
- Favorite
- Actions

After expansion, downstream components should treat children similarly to ordinary incoming items.

---

# 25. Preserve Collection Parents

Do not destroy the source digest.

Store relationships:

```text
Parent Source Item
    ├── Derived Entry A
    ├── Derived Entry B
    └── Derived Entry C
```

A derived entry retains:

```text
parent_raw_item_id
source_instance_id
curator identity
parent title
parent URL
child ordinal
target URL
anchor text
title hint
surrounding commentary
extraction method
confidence
```

The original parent remains available for:

- provenance
- debugging
- administration
- optional display

---

# 26. Collection Curator Semantics

The parent source acts as curator for the derived items.

Example Telegram message:

```text
Interesting links today:

1. SQLite on WASM is getting faster
   https://example.com/sqlite-wasm

2. A new LLM reranker
   https://example.org/reranker
```

Produces:

```text
Candidate A

curator:
@telegram_channel

publisher:
example.com

curator commentary:
SQLite on WASM is getting faster
```

and:

```text
Candidate B

curator:
@telegram_channel

publisher:
example.org

curator commentary:
A new LLM reranker
```

The same model applies to newsletters:

```text
curator:
newsletter identity

publisher:
linked external site
```

Feedback may affect:

```text
content preference
curator affinity
publisher affinity
```

using configurable weights.

---

# 27. Collection Detection

Do not assume that every item containing multiple URLs is a digest.

Ordinary articles contain:

- navigation
- references
- author links
- social links
- advertising
- related stories
- unsubscribe links

Create a dedicated collection-detection stage.

Detection may use:

- source type
- HTML structure
- number of meaningful links
- repeated title/description/link structures
- numbered lists
- bulleted lists
- headings
- Telegram formatting/entities
- newsletter structures
- source-specific rules
- optional model classification

Conceptually:

```rust
enum ItemShape {
    Single,
    Collection(CollectionDescriptor),
}
```

Expand only when confidence crosses a configurable threshold or a manual/source override forces expansion.

---

# 28. Deterministic Collection Parsing First

Use deterministic extraction before an LLM.

Use:

- DOM structure
- HTML links
- Telegram URL entities
- Markdown-like structure
- MIME content
- paragraphs
- headings
- list structure
- text/link proximity

These should work without a model:

```text
1. Foo
   https://foo.example/a

2. Bar
   https://bar.example/b
```

and:

```text
• Foo
  useful commentary
  https://foo.example

• Bar
  commentary
  https://bar.example
```

and repeated newsletter cards.

---

# 29. Collection Entry Candidate

Use a structure similar to:

```rust
struct CollectionEntryCandidate {
    url: Url,
    title_hint: Option<String>,
    commentary: Option<String>,
    author_hint: Option<String>,
    published_at_hint: Option<DateTime>,
    ordinal: usize,
    confidence: f32,
}
```

Do not reduce collection parsing to a list of naked URLs.

Example:

```html
<h3>SQLite gets a new query planner</h3>
<p>A surprisingly deep change worth reading.</p>
<a href="https://example.com/article">Read article</a>
```

should preserve:

```text
title_hint:
SQLite gets a new query planner

commentary:
A surprisingly deep change worth reading.

url:
https://example.com/article
```

---

# 30. Optional Model-Assisted Collection Parsing

Provide an optional external boundary:

```rust
#[async_trait]
pub trait CollectionParserProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;

    async fn parse_collection(
        &self,
        request: CollectionParseRequest,
    ) -> Result<CollectionParseResponse, ModelError>;
}
```

Use it for ambiguous cases where deterministic parsing cannot reliably infer:

- whether the item is a collection
- which title belongs to which URL
- which commentary belongs to which URL

The model receives bounded cleaned content.

It returns structured entries only.

Example:

```json
{
  "isCollection": true,
  "entries": [
    {
      "url": "https://example.com/a",
      "titleHint": "New SQLite planner",
      "commentary": "The interesting part is the new cost model.",
      "confidence": 0.96
    }
  ]
}
```

The model must never invent URLs.

Every model-returned URL must exist in the source item and be validated deterministically.

---

# 31. Collection Link Filtering

Reject obvious non-content links:

```text
unsubscribe
privacy
terms
newsletter settings
login
signup
advertising
social profiles
share buttons
tracking URLs
tracking pixels
image CDN assets
mailto:
tel:
javascript:
```

Detect repeated site chrome.

Support:

- global exclusion rules
- source-specific exclusion rules
- hostname/path rules

Do not hard-code one newsletter provider into the domain model.

---

# 32. Derived Item Identity

A derived item requires deterministic identity.

Use approximately:

```text
parent item identity
+
normalized target URL
```

Reprocessing the same newsletter or Telegram message must not create duplicates.

If the target already exists through:

```text
RSS
another newsletter
Telegram
another plugin
```

normal exact deduplication should converge on the same document where appropriate.

But all curator provenance must remain.

---

# 33. Multi-Curator Provenance

A document may have multiple discovery paths.

Example:

```text
Document:
https://example.com/great-article

Seen via:
├── RSS directly
├── Telegram @channel_a
├── Telegram @channel_b
└── Email Newsletter X
```

Preserve all of these.

Do not flatten them to:

```text
source = "RSS"
```

This information matters for:

- recommendation
- curator affinity
- debugging
- representative selection
- coverage UI

---

# 34. Parent Display Policy

Support:

```text
children_only
parent_and_children
parent_only
```

Default for confidently detected link roundups:

```text
children_only
```

The parent remains stored even when not shown.

Avoid default UI like:

```text
Weekly roundup
Article A
Article B
Article C
```

when the useful entities are the children.

---

# 35. Telegram Collection Support

Telegram collection parsing is mandatory.

Example:

```text
Interesting things today:

• New Rust async work
  https://example.com/rust

• Small local rerankers are getting good
  https://example.com/reranker

• Weird SQLite tricks
  https://example.com/sqlite
```

must produce three independent candidates.

Preserve:

```text
Telegram account
channel
message ID
message URL
parent commentary
per-link commentary
publication time
```

Use Telegram entities for URL extraction when available.

Do not unnecessarily reparse rendered text.

---

# 36. Email Newsletter Collection Support

Newsletter expansion is mandatory.

Support repeated structures such as:

```text
heading
description
button/link
```

Use sanitized MIME-derived HTML when available.

Preserve:

```text
newsletter sender
newsletter identity
Message-ID
subject
parent timestamp
per-entry commentary
```

Do not rely only on the plain-text fallback when usable structured HTML exists.

---

# 37. Collection Expansion and Summaries

A summary of the parent digest does not replace summaries of derived articles.

Each child proceeds through normal extraction and receives its own summary.

Keep distinct:

```text
curator_commentary
generated_summary
```

Example:

```text
Newsletter
    │
    ├── Article A
    │      └── generated summary A
    │
    ├── Article B
    │      └── generated summary B
    │
    └── Article C
           └── generated summary C
```

---

# 38. Collection Expansion and Streams

Children participate independently in streams.

One newsletter can yield:

```text
Article A -> Germany
Article B -> AI
Article C -> Software
```

Do not assign children to streams merely based on their parent.

Stream matching happens after expansion.

---

# 39. Collection Expansion and Feedback

Explicit feedback applies to the derived story, not its siblings.

If the user dislikes Article B, do not automatically dislike A and C.

The curator may still receive a weighted negative affinity event.

Do not interpret one disliked child as:

```text
user dislikes entire newsletter
```

Use accumulated Bayesian evidence.

---

# 40. Collection Debugging

Users/admins with access should be able to inspect:

```text
Detected shape:
Collection

Confidence:
0.93

Derived entries:
1. ...
2. ...
3. ...

Ignored links:
unsubscribe
social profile
tracking URL
```

Support manual:

```text
re-run detection
force expansion
mark as not a collection
```

Manual overrides should persist across reprocessing.

---

# 41. WebAssembly Source Plugins

Provide a future-facing source plugin ABI using the WebAssembly Component Model and WIT.

Built-in RSS, Telegram, and Email remain native Rust implementations.

Plugins should support:

- metadata
- configuration schema
- configuration validation
- polling
- opaque cursor
- health information
- host logging
- access to explicitly assigned secrets
- controlled outbound HTTP capability

Plugins must not receive:

- arbitrary filesystem access
- arbitrary environment access
- application database access
- other users' secrets
- unrestricted network access

Raw TCP support is not required in v1.

---

# 42. Source Plugin WIT

Design a versioned WIT interface approximately like:

```wit
package rill:source-plugin@1.0.0;

interface source {
    record plugin-metadata {
        id: string,
        name: string,
        version: string,
        description: string,
    }

    record raw-item {
        external-id: string,
        url: option<string>,
        title: option<string>,
        text: option<string>,
        html: option<string>,
        author: option<string>,
        published-at: option<string>,
        updated-at: option<string>,
        metadata-json: string,
    }

    record batch {
        items: list<raw-item>,
        next-cursor-json: option<string>,
        has-more: bool,
    }

    metadata: func() -> plugin-metadata;

    config-schema: func() -> string;

    validate-config: func(
        config-json: string
    ) -> result<string, string>;

    poll: func(
        config-json: string,
        cursor-json: option<string>,
        limit: u32
    ) -> result<batch, string>;
}
```

Refine exact WIT where needed.

Generate typed Rust bindings.

Ship:

- one example plugin
- Rust plugin SDK
- authoring documentation

Keep the design compatible with possible future TypeScript-to-WASM plugins.

Plugin-produced raw items must also pass through Collection Detection.

---

# 43. Plugin Administration

Admin UI must support:

```text
install
inspect manifest
show component hash
show requested permissions
enable
disable
configure source instance
show health
show failures
remove
```

Apply limits:

- memory
- fuel
- execution duration
- output size

Plugins must fail in isolation.

---

# 44. Content Extraction

Implement bounded article extraction.

Requirements:

- HTTP timeout
- byte limits
- redirect limits
- canonical URL extraction
- tracking-parameter cleanup
- URL normalization
- title
- author
- publication date
- main body text
- sanitized main HTML
- image metadata
- content checksum

Do not execute webpage JavaScript.

Do not require Chromium or another headless browser.

Detect obvious unsupported:

- login pages
- paywalls
- challenge pages

Do not mistake navigation or cookie banners for article body.

---

# 45. Exact Deduplication

Check in roughly this order:

```text
same source external ID
same normalized canonical URL
same content checksum
same explicitly linked external URL
same Telegram message identity
```

Exact processing must be idempotent.

A direct RSS item and the same article discovered inside three separate collections should converge on one underlying document where visibility permits.

Provenance must remain many-to-one.

---

# 46. Semantic Story Clustering

After embeddings exist:

- compare only compatible vectors
- use a configurable recent time window
- default approximately 72 hours
- consider language
- consider publication time
- use anchor-based clustering
- preserve similarity evidence
- preserve every variant
- support manual merge/split

Avoid naive transitive union-find where:

```text
A ~ B
B ~ C
```

causes:

```text
A ~ C
```

without sufficient direct evidence.

A high-confidence anchor-based approach is preferred.

---

# 47. Representative Variant

A story contains multiple variants.

Do not permanently select one universal representative.

Select a representative **per user and per request**.

Consider:

```text
curator affinity
publisher affinity
source affinity
original-source preference
content completeness
extraction quality
freshness
direct readability
stable canonical URL
```

If several outlets publish the same story, prefer the variant from the source the user has learned to trust more.

Other variants and curator paths remain accessible under a coverage disclosure.

---

# 48. External Model Architecture

Provide four independent provider abstractions where appropriate:

```text
EmbeddingProvider
SummaryProvider
RecommendationProvider
CollectionParserProvider
```

CollectionParserProvider is optional and only necessary for ambiguous collections.

Do not assume providers use the same vendor or model.

Example deployment:

```text
embeddings:
local embedding HTTP server

summaries:
remote LLM API

recommendations:
small local reranker

collection parsing:
same remote LLM API
```

Core domain logic must not care.

---

# 49. Embedding Provider

Conceptual trait:

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;

    async fn embed(
        &self,
        input: &[EmbeddingInput],
    ) -> Result<Vec<EmbeddingOutput>, ModelError>;

    async fn health(
        &self,
    ) -> Result<ModelHealth, ModelError>;
}
```

Provide:

- generic HTTP provider
- OpenAI-compatible provider where practical
- deterministic fake provider for tests

Support:

- batching
- timeouts
- retries
- circuit breaking
- size limits

---

# 50. Article Summaries

Every readable document should eventually receive a concise machine-generated summary.

The goal is not generic summarization.

The summary should answer:

> Is this worth opening and reading?

Target approximately two concise sentences.

Use one common content policy for every source:

- preserve the source language unless source instructions explicitly request translation
- keep already-short, readable source text verbatim instead of degrading it through paraphrase
- never expose raw JSON, HTML, XML, serialized metadata, opaque identifiers, or access keys
- state the human-readable meaning of machine content, or use a brief original-link placeholder when no meaning can be recovered

Prefer:

- concrete facts
- key claims
- useful numbers
- meaningful consequences
- what is actually new

Avoid:

```text
This article discusses recent developments...
```

Example:

```text
Germany plans to shift more federal IT procurement toward open-source
software, initially targeting office and collaboration tools.
The important part is the proposed migration timeline and procurement rules.
```

---

# 51. Summary Provider

Conceptual trait:

```rust
#[async_trait]
pub trait SummaryProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;

    async fn summarize(
        &self,
        request: SummaryRequest,
    ) -> Result<SummaryResponse, ModelError>;

    async fn health(
        &self,
    ) -> Result<ModelHealth, ModelError>;
}
```

Input may include:

```text
title
source
author
publication time
canonical URL
clean article text
language
curator commentary when applicable
```

Do not send raw HTML unless a provider explicitly requires it.

Bound input length.

When needed, truncate or select chunks deterministically.

---

# 52. Summary Persistence

Store:

```text
summary text
provider
model
model version
input checksum
creation time
```

Do not reuse a stale summary if the article content checksum changed.

Summary generation is asynchronous.

A summary failure must never hide the article.

Fallback order:

```text
feed-provided summary
collection curator commentary when useful
extracted description
first useful paragraph
title
```

---

# 53. Recommendation Provider

Conceptual trait:

```rust
#[async_trait]
pub trait RecommendationProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;

    async fn rank(
        &self,
        request: RankRequest,
    ) -> Result<RankResponse, ModelError>;

    async fn submit_feedback(
        &self,
        events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError>;

    async fn health(
        &self,
    ) -> Result<ModelHealth, ModelError>;
}
```

Rank input should support:

```text
user/model key
candidate stories
candidate summaries
embedding references or embeddings
source features
curator features
publisher features
freshness
coverage
recent explicit feedback
recent topic exposure
active stream
stream ranking instruction
requested result count
UI mode
```

Response includes:

```text
ranked IDs
scores
model identity
request ID
optional score features
```

The external recommender is not the source of truth for feedback.

Rill stores feedback first and retries provider delivery asynchronously.

---

# 54. Deterministic Fallback Ranker

The feed must remain useful when the external recommender is unavailable.

Implement a deterministic local fallback using cached information.

Potential features:

```text
positive embedding affinity
negative embedding affinity
source affinity
curator affinity
publisher affinity
freshness
coverage
topic fatigue
diversity
exploration
```

No local model inference is required for the fallback.

Persist a score breakdown for debugging.

---

# 55. Feedback

Primary actions:

```text
Like
Dislike
Favorite
```

They represent explicit preference.

Feedback replacement must correctly supersede previous feedback.

Changing:

```text
Like -> Dislike
```

must not leave both contributions active.

Store raw events.

---

# 56. Content Preference

Feedback effects:

```text
Like:
positive semantic signal

Dislike:
negative semantic signal

Favorite:
stronger positive semantic signal
```

For the fallback model, positive and negative preference centroids may be derived from cached story embeddings.

Do not make an implicit skip equivalent to Dislike.

Explicit feedback dominates weak behavioral signals.

---

# 57. Source, Curator, and Publisher Affinity

Use Bayesian-smoothed affinity.

Initial prior may resemble:

```text
alpha = 2
beta = 2
```

Example weights:

```text
Like:
positive += 1

Favorite:
positive += 2

Dislike:
negative += 1
```

Use time decay at scoring time.

Do not destructively rewrite raw history.

For a direct RSS article:

```text
publisher/source contribution = 1.0
```

For Telegram or newsletter collection links:

```text
curator contribution = 1.0
publisher contribution = 0.25
```

Preserve enough event information to change formulas later.

One negative child from a collection must not make the curator globally undesirable immediately.

---

# 58. Streams

Streams are first-class user-defined views over the available story pool.

Examples:

```text
All
Germany
AI
Software
Science
Local
Long Reads
```

A story can appear in more than one stream.

Streams are visible as tabs in the modern interface.

Reader mode exposes equivalent simple navigation.

---

# 59. Stream Model

Store approximately:

```text
id
user_id
name
slug
optional icon
position
enabled
filter definition
semantic description
ranking instruction
ranking configuration
created_at
updated_at
```

`All` is the built-in complete stream.

Users may create arbitrary additional streams.

---

# 60. Stream Filters

Initially support composable deterministic filters:

```text
include/exclude sources
include/exclude curators
include/exclude publishers
language
text query
topic labels
publication age
minimum coverage
read state
favorite state
```

Also support semantic matching.

---

# 61. Semantic Stream Definition

A stream may have a natural-language semantic description.

Example:

```text
Name:
AI

Description:
Important developments in LLMs, AI agents, model infrastructure,
AI research, developer tooling around AI, and major model releases.
Avoid generic corporate funding news unless it has technical significance.
```

Embed this definition using the configured embedding provider.

The stream embedding is a feature, not the whole ranking system.

Respect embedding model identity.

---

# 62. Stream Ranking Instructions

Streams may also define natural-language ranking guidance.

Example:

```text
Germany

Prefer:
- meaningful political changes
- legislation affecting residents
- important Berlin and Brandenburg developments
- infrastructure
- economic policy

Deprioritize:
- routine party statements
- political horse-race coverage
- celebrity news
```

Example:

```text
AI

Prefer:
- model architecture
- inference
- agents
- developer tools
- open-source releases
- significant research

Deprioritize:
- generic funding announcements
- superficial product marketing
```

Providers that understand natural-language context receive these instructions.

The deterministic fallback ignores unsupported free-form instructions safely.

---

# 63. Stream Pipeline

Filtering and ranking are separate.

```text
all eligible stories
        │
        ▼
deterministic stream filter
        │
        ▼
semantic stream candidate scoring
        │
        ▼
candidate set
        │
        ▼
user recommendation model
        │
        ▼
stream-specific adjustments
        │
        ▼
diversity
        │
        ▼
exploration
        │
        ▼
visible stream
```

Do not create one recommendation model per stream by default.

Streams provide context to the same user preference model.

Children derived from one collection may independently enter completely different streams.

---

# 64. Stream Membership Storage

Do not permanently materialize:

```text
story × user × stream
```

for every combination.

Membership may be cached.

Invalidate appropriately when:

```text
stream definition changes
story embedding changes
embedding model changes
subscriptions change
story changes significantly
```

---

# 65. Recommendation Pipeline

For a requested stream:

```text
eligible unread stories
        │
        ├── authorization
        ├── visibility
        ├── subscription filtering
        ├── stream filtering
        ├── freshness/retention
        └── cheap candidate scoring
                  │
                  ▼
             candidate cap
                  │
                  ▼
         external recommender
                  │
                  ▼
       deterministic post-process
        ├── diversity
        ├── topic fatigue
        ├── exploration
        └── policy
                  │
                  ▼
      per-user representative
```

Requirements:

- external model failure cannot break the feed
- bound candidate count
- short recommendation cache
- invalidate on explicit feedback
- invalidate on significant new stories
- invalidate on stream changes
- reserve small exploration capacity
- do not show multiple near-identical stories
- allow future non-personal `must know` scoring

---

# 66. Diversity

A recommendation system that returns:

```text
AI
AI
AI
AI
AI
```

is not acceptable even if every story is individually relevant.

Use a simple deterministic diversity pass such as maximal marginal relevance.

Conceptually:

```text
nextScore =
    relevance
    -
    lambda * similarity_to_selected_items
```

Deduplication and diversity solve different problems.

Deduplication handles the same event.

Diversity handles thematic fatigue.

---

# 67. Generic User Actions

Do not hard-code any third-party bookmark or read-later service into the core domain.

Implement a generic user-configurable **Action** system.

Actions can react to events.

Initial mandatory event:

```text
story.favorite
```

Design for future events:

```text
story.like
story.dislike
story.read
story.opened
story.matches_stream
story.received
```

Do not implement every future trigger unless necessary.

---

# 68. Favorite Semantics

Favorite means:

```text
persistent local Favorite state
+
strong positive recommendation signal
+
strong positive source/curator/publisher signal
+
optional configured actions
```

Interaction:

```text
Favorite
   │
   ├── persist locally transactionally
   │
   └── enqueue matching actions
              │
              ├── Action A
              ├── Action B
              └── ...
```

External action failure must not roll back Favorite.

---

# 69. Action Model

Store:

```text
action_definitions
action_triggers
action_executions
action_attempts
```

Configured action:

```text
id
user
name
kind
enabled
configuration
encrypted secrets
created
updated
```

---

# 70. HTTP Action

Implement one useful generic built-in action:

```text
HTTP request
```

Configuration:

```text
URL
HTTP method
headers
secret references
safe request template
timeout
retry policy
```

Event payload should expose structured data such as:

```json
{
  "event": "story.favorite",
  "story": {
    "id": "...",
    "title": "...",
    "summary": "...",
    "url": "...",
    "source": "...",
    "curator": "...",
    "publishedAt": "..."
  }
}
```

Do not implement arbitrary JavaScript templating.

Use a small safe declarative mechanism.

Apply:

- SSRF protection
- timeout
- response-size limits
- retries
- exponential backoff
- idempotency key
- secret redaction

---

# 71. Future Action Plugins

Keep actions architecturally compatible with future WASM plugins.

Conceptually:

```text
Source plugin:
external system -> Rill item

Action plugin:
Rill event -> external side effect
```

A complete action-plugin implementation is optional for v1.

The boundary should be explicit enough to add later.

---

# 72. Authentication

Implement local authentication.

Support:

```text
username or email
password
admin-created users
optional invitation codes
disabled users
roles
session revocation
password change
audit trail
```

Passwords:

```text
Argon2id
```

Never ship a default password.

Provide a bootstrap command for the first administrator.

---

# 73. Browser Sessions

Use opaque random session tokens.

Store only token hashes.

Cookie:

```text
__Host-rill_session
```

Properties:

```text
Secure
HttpOnly
SameSite=Lax or stricter
Path=/
no Domain
```

Rotate after login and privilege changes.

Support:

```text
logout
revoke session
revoke all sessions
```

---

# 74. CSRF

All unsafe cookie-authenticated requests require CSRF protection.

Use:

- CSRF token validation
- SameSite cookies
- Origin or Referer validation where applicable

Reader HTML forms must remain CSRF-safe without JavaScript.

---

# 75. Rate Limiting

Protect:

```text
password login
pairing-code generation
pairing-code attempts
Telegram login
invitation redemption
sensitive admin endpoints
```

Use simple in-process limits where sufficient.

Persist security-relevant state when restart bypass would be dangerous.

---

# 76. Reader Pairing

Reader pairing is a core feature.

A logged-in modern user chooses:

```text
Pair reader
```

The server generates a short one-time code.

Requirements:

```text
unambiguous uppercase alphabet
approximately ABCD-EFGH
at least 40 bits entropy
single use
default expiration 10 minutes
maximum attempts
bound to user
bound to reader scope
stored hashed
never logged
```

A QR code may point to the pairing page but must not contain a long-lived session token.

---

# 77. Reader Login

Reader visits:

```text
/reader/pair
```

Plain SSR HTML.

User enters pairing code.

In one transaction:

```text
consume pairing code
create device session
generate random device token
store only token hash
set cookie
redirect 303 to /reader
```

Cookie:

```text
__Host-rill_reader
```

Properties:

```text
Secure
HttpOnly
SameSite=Strict when practical, otherwise Lax
Path=/
no Domain
long-lived but revocable
default 180 days
```

Never place the reader token in:

```text
URL
HTML
JavaScript
localStorage
logs
browser history
```

Pairing responses require:

```text
Cache-Control: no-store
```

---

# 78. Reader Session Permissions

Reader session may:

```text
view feed
change stream
open story
mark read/unread
Like
Dislike
Favorite
execute Favorite-triggered actions
logout itself
```

Reader session may not:

```text
manage users
access admin
view credentials
configure model providers
configure Telegram
install plugins
create another reader session
```

Modern UI lists paired devices and supports immediate revocation.

Store:

```text
label
created time
last used
user-agent summary
optional privacy-aware IP summary
```

---

# 79. Modern Frontend

Modern UI targets desktop and mobile.

Use Solid SSR + hydration.

Required areas:

```text
personalized streams
story reader
coverage / alternative sources
curator provenance
search
favorites
history
source management
RSS / OPML
Telegram accounts
Telegram channels
email sources
stream management
action configuration
user settings
reader devices
recommendation explanation
collection-expansion debugging
admin dashboard
users
model providers
plugins
jobs
source health
audit log
```

Admin areas should be route-level lazy chunks.

Do not put the entire admin application into the initial feed bundle.

---

# 80. Stream Tabs

Primary modern navigation:

```text
All   Germany   AI   Software   +
```

Each stream has a stable route:

```text
/stream/all
/stream/germany
/stream/ai
```

Do not store selected stream only in browser state.

SSR remains authoritative.

Hydration makes navigation pleasant.

---

# 81. Modern Story Cards

At minimum show:

```text
title
generated summary
selected source
optional curator
publication time
coverage count
estimated reading time when available
Like
Dislike
Favorite
```

Where applicable, distinguish:

```text
curator commentary
generated summary
```

Allow coverage expansion showing:

- alternative publishers
- alternative curators
- original digest provenance

Explicit actions should update optimistically after hydration.

---

# 82. Reader Frontend

Reader mode is a separate UI entrypoint.

It is not simply the modern layout with a narrow CSS breakpoint.

Requirements:

- server rendered
- fully usable without JavaScript
- no infinite scroll
- explicit pagination
- large controls
- high contrast
- grayscale safe
- no animation
- no sticky overlays
- minimal layout shifts
- system fonts
- images disabled by default
- text-first
- small HTML pages
- minimal CSS
- no client router
- no localStorage requirement
- no browser-side WASM requirement

Routes:

```text
/reader
/reader/stream/:slug
/reader/page/:number
/reader/story/:id
/reader/pair
/reader/settings
/reader/logout
```

Use normal HTML forms and HTTP 303 redirects.

---

# 83. Reader Streams

Reader stream navigation may be:

```text
All | Germany | AI | Software
```

or an equivalent narrow-screen layout.

Changing stream must work without JavaScript.

Remember last selected stream server-side per reader session.

---

# 84. Rendering Page DTOs

Rust loads and authorizes data.

The renderer receives explicit page models.

For example:

```rust
enum PageModel {
    ModernFeed(ModernFeedPage),
    ModernStory(ModernStoryPage),
    ModernSettings(SettingsPage),
    AdminSources(AdminSourcesPage),
    AdminCollection(AdminCollectionPage),
    ReaderFeed(ReaderFeedPage),
    ReaderStory(ReaderStoryPage),
    ReaderPair(ReaderPairPage),
    Error(ErrorPage),
}
```

Generate TypeScript contracts from Rust or from a shared schema.

Do not hand-maintain two divergent sets of types.

Page models must not contain:

- secrets
- session tokens
- database handles
- executable code

---

# 85. JSON API

Expose versioned JSON endpoints under:

```text
/api/v1
```

Cover:

```text
auth
current user
sessions
reader pairing
reader devices
streams
feed
stories
feedback
read state
sources
RSS
OPML
Telegram auth
Telegram channels
email sources
collection detection/debugging
model providers
actions
plugins
jobs
health
admin users
recommendation debug
```

Provide equivalent HTML form endpoints for reader operations.

Generate OpenAPI documentation.

Never expose encrypted secret blobs.

---

# 86. Security

Implement:

- strict validation
- request body limits
- outbound body limits
- HTML sanitization
- URL scheme validation
- SSRF protection
- configurable private-network fetch policy
- encrypted secrets
- hashed sessions
- CSRF
- secure cookies
- rate limiting
- plugin capability limits
- Wasmtime memory/fuel limits
- output escaping
- safe hydration serialization
- CSP
- X-Content-Type-Options
- Referrer-Policy
- clickjacking protection
- appropriate cache headers

Do not log:

```text
passwords
Telegram bot tokens
Telegram binding tokens
cookies
session tokens
reader pairing codes
API keys
private email bodies by default
private Telegram bodies by default
```

Write a threat model in `SECURITY.md`.

---

# 87. Observability

Use structured tracing.

Provide:

```text
/health/live
/health/ready
```

Provide Prometheus-compatible metrics on configurable admin/local endpoint.

Measure:

```text
source poll latency
source errors
new item count

collection detection count
collection expansion count
collection parser model usage
collection fan-out
collection parse failures

extraction errors
embedding latency
summary latency
recommendation latency

job queue depth
job retries

cluster size

renderer latency
renderer traps
renderer memory

authentication failures
pairing failures
action failures
```

Use request and job correlation IDs.

Admin UI should provide a useful operational overview without requiring Grafana.

---

# 88. Configuration

Use TOML plus environment overrides.

Support configuration for:

```text
HTTP bind
public base URL
database path
static assets
renderer WASM path
plugin directory
master encryption key source
cookies
sessions
reader session lifetime
pairing expiration

source polling
fetch limits

collection detection threshold
collection expansion maximum fan-out
collection parent display default
collection parser provider

job concurrency

embedding provider
summary provider
recommendation provider

Telegram API credentials
IMAP defaults

plugin limits
renderer limits

logging
metrics
```

Secrets should come from:

```text
environment
restricted files
future OS secret providers
```

Do not require plaintext secrets inside main configuration.

Validate configuration on startup.

Fail with actionable errors.

---

# 89. Deployment

Deliver:

```text
one stripped Rust executable
renderer WASM
static assets
migrations
example config
systemd unit
optional small container
backup documentation
restore documentation
```

Typical layout:

```text
/opt/rill/
├── rill
├── ui-renderer.wasm
├── static/
├── plugins/
└── config.toml

/var/lib/rill/
├── rill.db
└── secrets/
```

Provide commands similar to:

```text
rill serve
rill migrate
rill admin create
rill sessions revoke
rill plugins inspect
rill doctor
rill backup
```

Production container must not contain frontend build tooling.

---

# 90. Build Tooling

Create `cargo xtask` commands:

```text
cargo xtask build-ui
cargo xtask build-renderer
cargo xtask verify-renderer
cargo xtask build-release
cargo xtask test-e2e
cargo xtask measure
```

`build-renderer` should:

```text
generate TS contracts
build Solid SSR entry
bundle it
run scriptc coverage
fail if dynamic execution is required
compile wasm32-wasi
run a Rust smoke test
record artifact size
```

`build-ui` produces hashed browser assets and an asset manifest.

---

# 91. Rust Unit Tests

Cover at least:

```text
URL canonicalization
source identity
Telegram identity

collection detection
collection candidate extraction
collection link exclusion
collection child deterministic identity
collection manual overrides

feedback replacement
Bayesian affinity
time decay
representative selection

exact dedup
anchor clustering

stream filtering
diversity reranking

job leasing

session hashing
pairing expiration
pairing replay
authorization
```

---

# 92. Collection-Specific Tests

## Telegram

Test:

```text
formatted three-link roundup
plain-text three-link roundup
single ordinary link
post with many incidental links
edited roundup
same URL repeated twice
```

## Email

Test:

```text
HTML newsletter with repeated cards
plain-text newsletter
newsletter with unsubscribe/social links
newsletter linking same article as RSS
multiple entries from same publisher
```

## Deduplication

Verify:

```text
RSS article
+
same article linked from Telegram digest
+
same article linked from email digest
```

becomes one underlying document/story where visibility permits while preserving all curator relationships.

## Feedback

Feedback on one child:

- affects that child's semantic signal
- affects curator affinity with configured weight
- affects publisher affinity with configured weight
- does not apply explicit feedback to siblings

## Streams

Children from one parent collection must be able to independently appear in different streams.

---

# 93. Integration Tests

Cover:

```text
RSS -> story -> feed

Telegram fixture -> story
Telegram roundup -> multiple independent stories

email fixture -> story
newsletter -> multiple independent stories

summary generation
embedding generation
semantic clustering

recommendation failure fallback

Favorite -> Action
Action retry

multi-user isolation
private-source isolation

plugin resource limits

application restart with pending jobs

collection reprocessing remains idempotent
```

---

# 94. Renderer Tests

Cover:

```text
every page template
escaping malicious strings
large feed
deterministic output
timeout/fuel exhaustion
memory limit
invalid JSON
unknown template
hydration-state escaping
```

---

# 95. Browser Tests

Cover:

```text
modern login
SSR hydration
stream navigation
Like
Dislike
Favorite

reader pairing
reader cookie creation
device revocation
reader without JavaScript

admin permissions

collection expansion debugging UI

responsive modern UI
zero hydration console errors
```

---

# 96. Security Tests

Cover:

```text
CSRF
origin validation
cookie flags
session rotation
rate limits
pairing brute force
pairing replay
SSRF
plugin capability denial
secret redaction

model collection parser cannot inject URLs absent from source
private collection provenance cannot leak between users
```

---

# 97. Resource Tests

Provide a repeatable measurement command.

Report:

```text
release binary size
renderer WASM size
modern JS size
reader JS size
cold startup
idle RSS
RSS after 100 renders
RSS during source ingestion
RSS during large collection expansion
SQLite database size after fixture import
```

Do not report estimates as measurements.

---

# 98. Development Environment

Provide one-command local development.

Include:

```text
fixture RSS feeds

recorded Telegram fixture events
Telegram collection fixtures

email fixtures
newsletter collection fixtures

fake embedding provider
fake summary provider
fake recommendation provider
fake collection parser provider
fake HTTP Action target

hot-reload modern UI
automatic renderer rebuild

temporary SQLite database
seed user creation
database reset command
```

Development may require:

- Node
- Vite
- Solid compiler
- scriptc
- Zig

Production must not.

---

# 99. Documentation

Write:

```text
README.md
ARCHITECTURE.md
SECURITY.md
CONTRIBUTING.md
ADRs

plugin authoring guide
model-provider guide

Telegram setup guide
reader pairing guide
Actions guide
streams guide

collection detection and expansion guide

backup/restore guide
resource measurement guide
troubleshooting guide
```

Explicitly document:

```text
Source
Curator
Publisher

Raw Item
Collection Parent
Collection Entry

Document Variant
Story

Stream
Summary

Feedback
Favorite
Action

Recommendation
Representative Variant
```

---

# 100. Implementation Order

Keep the repository runnable after every phase.

## Phase 0: Renderer Proof

Implement first:

```text
Solid TSX page
Solid SSR compilation
Solid client compilation
scriptc coverage
scriptc wasm32-wasi output
Wasmtime Rust host
SSR request
browser hydration
resource measurements
```

Do not continue until this works without an embedded JavaScript engine.

## Phase 1: Foundation

```text
workspace
configuration
SQLite
migrations

users
auth
sessions
reader pairing

modern shell
reader shell
```

## Phase 2: Basic Ingestion

```text
job queue
RSS
normalization

collection detection
deterministic collection expansion

content extraction
exact dedup

search
```

## Phase 3: Intelligence

```text
embedding provider
summary provider

optional collection parser provider

summary generation
semantic clustering

stream filtering

recommendation provider
fallback ranker

feedback
affinity
diversity
```

## Phase 4: Telegram and Email

```text
Telegram login
Telegram channel selection
backfill
live updates
Telegram roundup expansion

IMAP
newsletter expansion

curator/publisher model
multi-curator provenance
```

## Phase 5: Actions and Plugins

```text
HTTP Action
Action trigger system

source plugin WIT
plugin host
example plugin

admin UI
```

## Phase 6: Hardening

```text
security review
resource tests
browser tests
deployment
documentation
cleanup
```

---

# 101. Definition of Done

The implementation is complete only when all of the following are true.

## Runtime and rendering

1. A fresh clone can build documented production artifacts.
2. Production starts without Node, Bun, Deno, V8, QuickJS, Postgres, Redis, or a vector database.
3. Solid SSR code compiles through `scriptc` to `wasm32-wasi`.
4. The production renderer runs inside Wasmtime.
5. `scriptc coverage` shows no required dynamic QuickJS execution.
6. Rust invokes the renderer through a stable internal Renderer abstraction.
7. Solid browser hydration succeeds without mismatch.
8. Reader UI functions with JavaScript disabled.

## Authentication

9. An administrator can create a user.
10. A user can authenticate.
11. A user can pair an e-reader using a one-time code.
12. The e-reader receives a Secure HttpOnly device cookie.
13. Reader sessions can be revoked immediately.

## Sources

14. RSS ingestion works.
15. Public Telegram ingestion is fetched once per channel and remains visible only to subscribed users; the optional bot binding flow can add subscriptions.
16. Email newsletter ingestion works through IMAP.
17. A WASM source plugin runs under capability and resource limits.

## Collection expansion

18. A Telegram post containing several curated links can become several independent feed entries.
19. An email newsletter containing several curated links can become several independent feed entries.
20. Each derived entry preserves parent and curator provenance.
21. Each derived entry independently passes through extraction, summary, embedding, deduplication, clustering, stream matching, and recommendation.
22. Directly discovered and digest-discovered versions of the same article deduplicate correctly.
23. Deduplication never destroys curator provenance.
24. Per-link commentary remains separate from generated summaries.
25. Feedback on one collection child does not explicitly affect siblings.
26. Children from one collection may appear independently in different streams.
27. Unsubscribe, social, tracking, and navigation links do not become stories.
28. Reprocessing a parent does not duplicate children.
29. Collection fan-out is bounded.
30. Collection detection and expansion are visible and debuggable.
31. Manual collection overrides persist.

## Deduplication and recommendation

32. Exact duplicates collapse.
33. Semantic duplicates cluster into one Story.
34. Every source variant remains inspectable.
35. Multi-curator provenance remains inspectable.
36. Representative selection responds to learned source, curator, and publisher affinity.
37. External recommendation failure produces a usable fallback feed.
38. Private user sources do not leak between accounts.

## Summaries and streams

39. Readable articles receive concise model-generated summaries.
40. Summary failure does not hide content.
41. Users can create streams such as `Germany` and `AI`.
42. Streams have stable URLs.
43. Streams work in modern and reader interfaces.
44. Stream navigation works without JavaScript on the reader.
45. Streams can combine deterministic and semantic filtering.

## Feedback and actions

46. Like affects semantic and affinity signals positively.
47. Dislike affects semantic and affinity signals negatively.
48. Favorite acts as a stronger positive signal.
49. Favorite is independent of external services.
50. User-configured Actions can react to Favorite.
51. Action execution is asynchronous and retryable.

## Quality

52. Unit, integration, browser, renderer, and security tests pass.
53. Actual resource measurements are documented.
54. No core feature exists only as a TODO or mock.
55. There is no unexplained multi-thousand-line frontend monolith.

---

# 102. Final Implementation Report

After implementation, provide:

1. The resulting architecture.
2. Important modules and files.
3. Exact Solid -> scriptc -> WASI -> Wasmtime build flow.
4. Exact collection-detection and collection-expansion pipeline.
5. How Telegram and newsletter roundups are represented.
6. How multi-curator provenance is preserved.
7. Actual executable, WASM, and browser bundle sizes.
8. Actual memory measurements.
9. Development commands.
10. Test commands.
11. Production commands.
12. Which external integrations were tested against fixtures.
13. Which integrations require real credentials for final validation.
14. Remaining limitations.
15. Significant ADR decisions.

Do not claim that a test passed unless it was actually run.

Do not claim a resource target was achieved unless it was actually measured.

When encountering an unexpected implementation obstacle, preserve the core architecture rather than silently replacing it with a heavier runtime or additional infrastructure.
