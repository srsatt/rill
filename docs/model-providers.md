# Model providers

`rill-model-api` defines async `EmbeddingProvider`, `SummaryProvider`, and
`RecommendationProvider` traits. Every provider returns a stable provider,
model, and version identity; derived records also store input checksums so model
changes do not silently reuse stale output.

When no model slots are configured, Rill uses deterministic local feature-hash embeddings, concise
extractive summaries, and a local ranker. They require no model server and make
all core flows usable offline. Recommendation provider errors, invalid IDs, or
empty output are logged and replaced by the local result. Summary failure leaves
the source content visible. Optional `models.embedding`, `models.summary`, and
`models.collection_parser` entries use the built-in OpenAI-compatible HTTP
adapter. Summary responses contain both a concise summary and bounded topic
tags. Requests receive only bounded extracted `body_text`; raw or sanitized HTML
is never included in the summary/topic prompt. `models.recommendation` uses
OpenAI-compatible chat JSON for providers named `ollama`, `openai`, `claude`,
`gemini`, or `openai-compatible`; other provider names use the
bounded Rill rank/feedback HTTP contract. Each endpoint supports API-key environment variables, request and
response limits, batching, timeouts, bounded retries, and circuit breaking.

Example:

```toml
[models.summary]
base_url = "http://127.0.0.1:11434/v1/"
provider = "ollama"
model = "gemma4:e4b"
version = "operator-pinned"
api_key_env = "RILL_MODEL_API_KEY"
ca_certificate_path = "/etc/rill/model-ca.crt"
timeout_seconds = 30
maximum_request_bytes = 524288
maximum_response_bytes = 4194304
maximum_batch_items = 128
retries = 2
circuit_failure_threshold = 3
circuit_cooldown_seconds = 30
```

`ca_certificate_path` adds one PEM trust anchor only to that provider's HTTPS
client. Use it for a private model gateway without changing system trust.

The admin global-settings page manages three live slots: `embedding`, `ranking`,
and `text_parse`. The text-parse slot powers both summaries and collection
parsing. When TOML configures a slot, deployment remains the owner of its URL,
provider kind, API-key environment variable, CA certificate, request limits,
retry policy, and circuit breaker. The admin page can override only the model
and version; SQLite stores only that identity. Reset removes the identity
override and immediately restores the TOML model. A later deployment can change
connection details without erasing the selected model.

When TOML does not configure a slot, the admin UI can create the complete HTTP
connection instead. It provides OpenAI, Claude, Gemini, Ollama, and custom
OpenAI-compatible presets, one encrypted token field, and a live health test.
API keys use encrypted secret storage and are never returned. Existing complete
overrides automatically inherit a newly deployment-managed connection while
retaining their model identity.

The development config uses Ollama `embeddinggemma:latest` for embeddings and
`gemma4:e4b` for enrichment, collection parsing, and ranking.

Ranking first computes Rill's local freshness, coverage, affinity, feedback,
and semantic score. A configured ranking endpoint receives the bounded
candidate set and reranks it. OpenAI-compatible providers use
`POST <base_url>/chat/completions`; custom providers use
`POST <base_url>/rank`. Provider failure falls back to the local order. Changing
or resetting the ranking slot invalidates cached recommendation runs immediately.

For another protocol, implement the relevant trait in a focused crate and keep
strict limits/validation in that adapter. Do not give a provider raw
user/session identifiers:
recommendation requests use a stable hashed user key. Never send private bodies
to a remote provider without an explicit operator/user policy.

Local HTTP fixtures prove bearer authentication, transient retry, non-retryable
4xx handling, response bounds, identity persistence, collection URL validation,
and fallback. Real credentials and vendor-service validation remain
deployment-specific.
