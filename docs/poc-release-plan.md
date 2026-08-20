# Rill PoC release plan

Status: release candidate ready; GitHub prerelease remains.

## Release shape

- Tag: `v0.1.0-poc.1`.
- Channel: GitHub prerelease.
- Contents: source archive and release notes. Do not publish architecture-specific binaries or a container image until CI builds and tests them on their target platforms.
- Compatibility promise: PoC only. SQLite migrations move forward; configuration and plugin APIs may still change before `v1.0.0`.

## Release blockers

No known blockers. The public `srsatt/rill` repository exists, `origin` targets
it, official AGPL-3.0 text is present, metadata versions align at `0.1.0`, and
the tracked-file audit excludes local credentials, databases, build artifacts,
and test output.

Absence of hosted CI is accepted for this source-only prerelease. Local release gates and their exact results must be included in release notes.

## Local gate result — 2026-08-19

- Formatting, Clippy, all 137 Rust tests, typechecking, renderer verification, and all 8 browser tests pass.
- ScriptC coverage is 239/239 statements static with no dynamic remainder.
- Release build, packaged `rill doctor`, and deterministic resource measurement pass.
- Current measurements are recorded in [resource-measurements.md](resource-measurements.md) and [implementation-report.md](implementation-report.md).

## Gate 1 — repository and metadata

1. Add official AGPL-3.0 license text as `LICENSE`.
2. Confirm `0.1.0` remains aligned across workspace metadata and OpenAPI.
3. Review tracked files for `.env`, databases, model artifacts, screenshots, caches, and generated secrets.
4. Configure the intended personal GitHub repository as `origin`.
5. Verify effective Git author and GitHub authentication are `srsatt` before any commit, push, tag, or release mutation.

## Gate 2 — reproducibility

Run from a clean checkout with pinned dependencies:

```bash
pnpm --dir ui install --frozen-lockfile
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
pnpm --dir ui typecheck
cargo xtask verify-renderer
cargo xtask test-e2e
cargo xtask build-release
cargo xtask measure
```

Required results:

- ScriptC coverage remains 100% static with no dynamic remainder.
- Reader routes contain no client JavaScript.
- Browser suite has no console errors, failed assets, horizontal overflow, hover movement, or hydration replacement.
- `rill doctor` succeeds against packaged executable, renderer WASM, static assets, config, and a fresh database.
- Updated executable, renderer, client bundle, startup, and RSS measurements are recorded in `docs/resource-measurements.md` and `docs/implementation-report.md`.

## Gate 3 — release commit

1. Update implementation report with final typography, theme, performance, and measurement evidence.
2. Commit only reviewed source, tests, and documentation on `main`.
3. Push `main`; verify remote commit equals local `HEAD`.
4. Create and push annotated tag `v0.1.0-poc.1` from that exact commit.

## Gate 4 — GitHub prerelease

Create prerelease notes containing:

- local-first Rust/SQLite architecture and static Solid/ScriptC renderer;
- supported RSS/Atom, Telegram preview, IMAP, streams, Reader mode, feedback, local ranking, and generic Favorite actions;
- exact verification and resource results;
- deployment/build links;
- PoC limitations: no API/config stability promise, no automated key rotation, and real Telegram/IMAP/vendor/deployment checks remain environment-specific.

Attach no locally built binary unless its target platform was tested. GitHub's source archives are sufficient for this prerelease.

## Gate 5 — post-release verification

1. Verify GitHub release points to the annotated tag and intended commit.
2. Download the source archive into a clean temporary directory and run `cargo xtask build-release`.
3. Confirm README, deployment guide, security model, license, and release notes render correctly on GitHub.
4. Record release URL, tag, commit, and final gate results in the implementation report.

Release is complete only after remote tag, GitHub prerelease, clean-archive build, and documentation checks all pass.
