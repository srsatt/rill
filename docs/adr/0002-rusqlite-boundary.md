# ADR 0002: explicit Rusqlite boundary

Status: accepted

Rill uses Rusqlite with bundled SQLite rather than an async database abstraction.
SQLite work is synchronous by nature, so HTTP handlers move it onto Tokio's blocking
pool. A small bounded connection pool owned by `rill-db` caps concurrency and makes
WAL, foreign-key, busy-timeout, and migration policy explicit. This keeps the runtime
and generated code smaller while leaving SQL visible for review.

The tradeoff is that query mapping and migrations remain handwritten. Tests exercise
the pool and schema directly, and database calls must not run on Tokio worker threads.
