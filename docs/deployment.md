# Deployment, backup, and restore

## Production artifacts

Build on the deployment architecture:

```bash
pnpm --dir ui install --frozen-lockfile
cargo xtask build-release
cargo xtask measure
```

Install `target/release/rill` as `/opt/rill/rill`,
`artifacts/ui-renderer.cwasm` as `/opt/rill/ui-renderer.cwasm`, and
`ui/dist/client` as `/opt/rill/static`. Copy `config/example.toml` to
`/etc/rill/config.toml`, create `/var/lib/rill`, and install
`deploy/systemd/rill.service`. The service expects a restricted
`/etc/rill/rill.env` containing at least `RILL_MASTER_KEY`; add Telegram API
variables only when enabled.

```bash
sudo install -d -o rill -g rill /opt/rill/static /var/lib/rill /etc/rill
sudo systemctl daemon-reload
sudo systemctl enable --now rill
curl --fail http://127.0.0.1:3000/health/ready
```

Use `deploy/Caddyfile.example` for TLS termination. Keep `secure_cookies = true`
when the public origin is HTTPS. `rill doctor` validates config, assets,
renderer instantiation, and database access before rollout.

The optional `deploy/Containerfile` packages already-built, architecture-matched
artifacts. The precompiled renderer must match the deployment architecture and
remain immutable while Rill runs. The image is intentionally not a
frontend/Rust build environment.

```bash
cargo xtask build-release
docker build -f deploy/Containerfile -t rill:alpha .
```

The alpha publishes source only. Publish a registry image after CI builds and
tests both `linux/amd64` and `linux/arm64`; a locally packaged image is useful
for self-hosting but is not a portable release artifact.

## Backup

SQLite WAL files cannot be copied independently. Prefer Rill's consistent
online backup command, which refuses to overwrite an existing output:

```bash
rill --config /etc/rill/config.toml backup /srv/backup/rill.db
sqlite3 /srv/backup/rill.db "PRAGMA integrity_check;"
```

SQLite's `.backup` command while the service is running, or a filesystem copy
while the service is fully stopped, are acceptable operator alternatives.

Back up `/etc/rill/config.toml`, installed plugin artifacts if managed outside
SQLite, and the master key through a separate encrypted secret backup. Restrict
all copies. The database contains ciphertext but still contains private article
metadata and bodies.

## Restore

1. Stop Rill and preserve the current database separately.
2. Restore the database to a new file on the same filesystem.
3. Run `PRAGMA integrity_check` and restore the matching master key.
4. Set owner/mode, atomically rename the file to `/var/lib/rill/rill.db`, and
   remove stale `-wal`/`-shm` files only while Rill is stopped.
5. Run `rill --config /etc/rill/config.toml migrate` and `doctor`, then start the
   service and verify `/health/ready`, login, one source poll, and decryption of
   one configured credential.

When one deployment has multiple browser hostnames, keep the canonical URL in
`http.public_base_url` and list the other exact origins in `http.trusted_origins`.

Changing the master key without re-encrypting every secret makes encrypted
Telegram, email, plugin, and Action credentials unreadable. Automated key
rotation is not yet implemented.
