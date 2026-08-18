# Source plugin authoring

Rill source plugins implement `rill:source-plugin@1.0.0` from [`crates/plugin-host/wit/source-plugin.wit`](../crates/plugin-host/wit/source-plugin.wit). The ABI uses JSON strings for evolvable domain payloads while WIT still gives typed functions, results, cursors, and capability imports.

Use [`sdk/source-plugin`](../sdk/source-plugin) for the Rust data types and JSON helpers. [`plugins/example-rust`](../plugins/example-rust) is the authoring template. [`plugins/example-static/component.wat`](../plugins/example-static/component.wat) is a hand-authored, runnable component used by host acceptance tests.

## Capabilities

Plugins receive no WASI context, filesystem, environment, raw sockets, database, or ambient secrets. Optional imports are granted per installation:

- `http`: constrained to an explicit hostname allowlist; redirects and response bytes remain bounded by the host fetch policy.
- `secret:<name>`: maps one logical name to one encrypted secret owned by the source user.

All requested permissions appear in the manifest. An administrator must grant each one before enabling the plugin. Plugin logs redact exact secret values returned by the host.

## Resource limits

Every call receives a fresh store with configured memory, fuel, epoch timeout, output, component, and HTTP-response limits. A trap or invalid result updates plugin/source health and is retried by the durable job queue without affecting other sources.

## Payloads

`metadata` returns the SDK `Metadata` JSON. `config-schema` returns JSON Schema. `validate-config` returns normalized configuration JSON. `poll` returns SDK `Batch` JSON. Every item then enters the same collection detection, extraction, deduplication, summary, embedding, clustering, and recommendation pipeline as native sources.
