# ADR 0001: Compile Solid SSR to WASI through scriptc

- Status: accepted
- Date: 2026-08-17

## Context

Rill needs shared Solid TSX without a production JavaScript runtime. Solid's
official server bundle currently reaches `seroval`; current scriptc marks that
module unsupported for fully static compilation. `--dynamic` would embed
QuickJS and violate Rill's runtime constraints.

## Decision

Use official Solid compiler in both builds and official `solid-js` in browser.
Alias server-only helpers to a small statically compilable adapter covering
exact helpers emitted by Rill's component set. Require:

- 100% `scriptc coverage`
- no `--dynamic`
- renderer determinism and escaping tests
- browser hydration and root-preservation tests
- Wasmtime capability and resource limits
- build-time Wasmtime AOT compilation after stripping; runtime loads only the
  trusted, architecture-matched serialized module

Strip non-runtime WASM custom sections after compilation. Preserve every runtime
section and rerun renderer tests against stripped artifact.

## Consequences

Rill has no embedded JavaScript engine, and renderer startup performs no Wasm
translation or code generation. Shared TSX remains source of truth.
Adding unsupported Solid SSR features requires explicit adapter work and fails
coverage/tests rather than silently adding a runtime.
