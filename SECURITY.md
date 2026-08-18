# Rill security model

## Trust boundaries and threats

The Rust process and authorized SQLite data are trusted. Renderers, source
plugins, remote feeds, websites, mail servers, Telegram, action endpoints,
browser input, and model providers are untrusted. Main threats are credential
theft, session fixation, CSRF, SSRF/DNS rebinding, cross-user data leakage,
malicious HTML, unbounded inputs, plugin escape/resource exhaustion, replayed
Actions, and secret disclosure through logs or APIs.

## Implemented controls

- Passwords use Argon2id with fresh salts. Browser/device tokens have 256 random
  bits; only SHA-256 hashes are stored. Pairing codes have 40 bits, expire, are
  attempt-limited, and are atomically single-use.
- Production cookies use `__Host-`, Secure, HttpOnly, SameSite, Path `/`, and no
  Domain. Unsafe authenticated requests require an exact same-origin Origin or
  Referer plus a session-bound CSRF token. Login and pairing are rate-limited.
- Responses set same-origin CSP/form actions, `Referrer-Policy: same-origin`,
  nosniff, frame denial, and no-store for secret-bearing/auth responses. The
  same-origin referrer policy is deliberate: Chromium otherwise serializes
  ordinary same-origin form POST origins as `null`, defeating strict origin
  validation.
- HTML extraction uses an allowlist sanitizer. Hydration JSON escapes
  HTML-significant characters. Reader pages contain no JavaScript.
- Outbound HTTP permits only HTTP(S), rejects credentials, resolves DNS before
  each request/redirect, rejects private and non-routable answers unless
  explicitly enabled, and pins approved addresses. Non-GET redirects preserve
  method only for 307/308 and cannot change host, protecting Action headers.
- Secret rows use XChaCha20-Poly1305 with random nonces and authenticated
  owner/purpose/record metadata. The master key comes from a named environment
  variable and is never stored in SQLite. API views redact Action headers and
  encrypted blobs.
- Visibility scopes participate in document convergence, candidate selection,
  clustering, and search. Tests prove private documents do not cross users.
- Renderer WASM receives only versioned JSON over WASI stdio and no filesystem,
  environment, network, database, or process access.
- Source plugins receive no WASI. The only imports are bounded logging, exact
  named-secret grants, and HTTP GET to explicit host grants. A fresh store
  enforces component size, memory, fuel, epoch timeout, output, and HTTP limits.
  Traps update health and cannot stop other sources.
- HTTP Actions are POST/PUT/PATCH only, keep headers encrypted, send fixed
  structured payloads and idempotency keys, and run through durable bounded
  retries. Favorite commits before Action enqueue or delivery.

Rill does not log passwords, Telegram login values/sessions, cookies, session
tokens, pairing codes, API keys, IMAP passwords, or private item bodies by
default. Plugin logs redact exact secret values returned during that invocation.

## Operational responsibilities

Protect the master key separately from the database; both are required for a
usable restore. Terminate public TLS at a hardened reverse proxy, keep Rill on a
private loopback/LAN address, set `secure_cookies = true`, keep
`allow_private_networks = false` unless a source genuinely requires LAN access,
restrict config/environment file permissions, and patch dependencies regularly.

Real Telegram, IMAP, HTTP Action, and third-party plugin credentials are not in
the repository. Validate those integrations in the target environment. Backup,
restore, and rotation procedures are in [docs/deployment.md](docs/deployment.md).
