# Troubleshooting

- `rill doctor` fails: verify config paths, file permissions, renderer WASM,
  static manifest, SQLite directory ownership, and all required environment
  variables.
- `health/live` works but `health/ready` returns 503: SQLite cannot lease a
  connection or execute `SELECT 1`; inspect ownership, disk, pool exhaustion,
  and migration errors.
- Login form says origin rejected: `http.public_base_url` must exactly match the
  browser scheme, host, and port. Do not place a proxy on a different public
  origin without updating it.
- A source never advances: inspect `source_health`, its cursor, and queued/dead
  jobs. Rill retains the cursor on failed batches.
- LAN feed or Action is rejected: production SSRF policy blocks private,
  loopback, link-local, and non-routable DNS answers. Enable private networks
  only when the whole Rill instance may reach trusted LAN endpoints.
- Telegram misses messages: confirm the account/channel subscription and API
  credentials. Live listeners reconnect with backoff; run the source poll to
  catch up from durable state.
- Plugin cannot enable: grant every permission it requested, with explicit HTTP
  hosts or owner-matched secret IDs. Check plugin health for traps/limits.
- Renderer traps: rebuild with `cargo xtask build-renderer`, confirm 100% static
  coverage, then run `cargo test -p rill-renderer-host`.
- Browser UI changes but reader does not: reader is separately rendered and has
  no hydration bundle; add equivalent links/forms to its TSX page.
