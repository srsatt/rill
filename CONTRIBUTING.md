# Contributing

Use Rust 1.96 and the pnpm version pinned in `ui/package.json`. Keep changes
inside existing crate boundaries; add a crate only for a real capability or
trust boundary. Do not add a production JavaScript runtime or external state
service.

Before submitting:

```bash
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui typecheck
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo xtask test-e2e
cargo xtask build-release
```

Renderer changes must keep `scriptc coverage` at 100% static. Add migrations as
new numbered files; never edit an applied migration because checksums are
enforced. Tests must use fixtures rather than real credentials. Never commit
databases, `.env`, session material, action headers, mail passwords, Telegram
codes, or the master key.

Use Conventional Commit-style concise subjects when practical. Document a
decision in `docs/adr/` when it changes runtime topology, trust boundaries,
public contracts, or persistence ownership.
