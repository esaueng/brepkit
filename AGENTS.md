# brepkit agent guide

## What this repository is

brepkit is a layered Rust workspace for an exact B-Rep modeling kernel, with
native geometry, topology, modeling, file-I/O, rendering, and WebAssembly
crates. This repository is a production fork: its delta adds geometry and
import/export hardening, additional Rust/WASM contracts, fork-owned CI and
release automation, and a committed WASM package consumed directly from Git.

## Build and verify

Run commands from the repository root. `rust-toolchain.toml` selects the normal
toolchain; CI separately checks the declared Rust 1.88 MSRV and installs
`cargo-nextest` for the test jobs.

Baseline for every change:

```bash
cargo build --workspace --all-features
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --workspace --no-fail-fast
cargo test --workspace --doc
cargo nextest run -p brepkit-operations --features perf-counters -E 'test(scaling_)'
./scripts/check-boundaries.sh
```

Change-specific gates:

```bash
# Documentation
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features
mdbook build book
./scripts/check-doc-paths.sh

# Manifests, workspace crates, dependencies, or release metadata
rustup run 1.88.0 cargo check --workspace --all-features  # needs the 1.88 toolchain installed locally
./scripts/check-versions.sh
cargo check --manifest-path fuzz/Cargo.toml --bins
cargo publish --workspace --dry-run  # uses crates.io; CI definition verified, not run in the agent-doc audit

# WASM API, bindings, build tooling, or package contents
cargo test --manifest-path xtask/Cargo.toml
cargo clippy -p brepkit-wasm --target wasm32-unknown-unknown --no-default-features -- -D warnings
cargo test -p brepkit-wasm --no-default-features
cargo xtask wasm-build --skip-opt  # needs wasm-pack + Node; rewrites crates/wasm/pkg; CI definition verified, not run in the agent-doc audit
cd crates/wasm/pkg && npm pack --dry-run
```

CI also gates 60% line coverage, software Vulkan rendering, cargo-deny,
RustSec, cargo-machete, Taplo, secret scanning, fuzz-target compilation, and
Linux/macOS/Windows tests. The SemVer Check is advisory and is deliberately
excluded from `CI Pass`.

CI baseline verified 2026-08-11: current `main` is red only because the Windows
runner cannot list `brepkit-render::adapter_required` (`0xc0000135`, missing
module); all other blocking jobs passed. Recheck untouched `main` before
attributing that inherited runner failure to a change.

## Enforced workspace boundaries

`scripts/check-boundaries.sh`, run by the `Layer Boundaries` CI job, checks
normal `[dependencies]`; dev-dependencies are intentionally exempt.

- L0: `math` and `sketch` have no workspace dependencies.
- L1: `geometry` and `topology` may depend on `math`.
- L2: `algo`, `blend`, `heal`, `check`, and `offset` may depend on `math`,
  `topology`, and `geometry`.
- L3: `operations` composes the lower layers. `io` and `render` may depend on
  `math`, `topology`, and `operations`.
- L4: `wasm` may depend on every non-render workspace crate. `render` is a
  leaf; no other regular workspace dependency may point to it.

## Invariants that are easy to miss

- Lengths are millimetres and angles are radians. Scalars carry no unit tag or
  conversion; scale coordinates, dimensions, deflections, and linear
  tolerances together at an application boundary.
- `Tolerance::new()` is scale-aware: linear `1e-7`, angular `1e-12`, relative
  `1e-10`. Use the tolerance API; do not replace exact-geometry checks with
  raw float equality or a looser preview tolerance.
- A geometry regression is not disproved by successful tessellation. Assert
  closed/valid topology, expected volume, preserved analytic surface kinds,
  and STEP round-trip behavior whenever the affected path crosses STEP I/O.
- Solid-wide traversal must include cavity shells. Use
  `brepkit_topology::explorer::solid_faces`; iterate shell-by-shell only when
  the operation is explicitly per-shell.
- Workspace lints deny unsafe code, `unwrap`, and panic. Test modules use
  narrowly scoped allows; do not broaden them into production code.
- `crates/wasm/pkg` is generated but committed. Never hand-edit it. The xtask
  builds bundler and Node targets, merges them, validates the package, and runs
  `scripts/test-wasm-smoke.mjs`; the main-branch publish workflow refreshes the
  committed package after merges. Exclude local generated churn unless the
  task explicitly updates the artifact.
- `cargo check --manifest-path fuzz/Cargo.toml --bins` can refresh stale local
  workspace-version entries in `fuzz/Cargo.lock`. Inspect the diff and do not
  include that churn unless dependencies are part of the task.

## Adding a user-visible operation

The compiler does not catch missing JavaScript or batch parity.

1. Implement and export the native operation in `brepkit-operations`, with a
   native regression that checks the geometric contract above.
2. Add the public method to the matching module under `crates/wasm/src/bindings/`.
   Use `#[wasm_bindgen(js_name = "camelCase")]`, existing input validators, and
   the typed handle converters.
3. If the operation belongs in batch execution, add its dispatch arm and batch
   contract test in `bindings/batch.rs`.
4. Add WASM contract coverage, run the WASM gates, and inspect generated package
   changes rather than editing them.

## Fork maintenance

Treat `origin` as this production fork and `upstream` as the source project.
Locate the fork delta instead of guessing at it:

```bash
git rev-list --left-right --count upstream/main...origin/main
git log --oneline upstream/main..origin/main
git diff --stat upstream/main...origin/main
```

Sync on a dedicated branch by fetching both remotes and merging
`upstream/main`; recent syncs preserve upstream ancestry rather than rebasing
or rewriting it. Resolve source conflicts first, retain fork regressions for
behavior that differs, run the full matrix, then rebuild the committed WASM
package. Do not hand-resolve generated WASM binaries.

Outside a scoped task, avoid opportunistic edits to the current sync hot spots:
`.github/workflows/ci.yml`, `.github/workflows/publish.yml`,
`crates/algo/src/builder/face_splitter/`, `crates/blend/src/fillet_builder*`,
`crates/operations/src/blend_ops.rs`, and `crates/wasm/pkg/`. The latest
upstream sync and its follow-up preservation commits touched these areas.

Local Husky hooks run format/Clippy plus optional Taplo and cargo-machete;
pre-push delegates the full suite to CI. Commitlint checks conventional commits
only when the npm development dependencies are installed and otherwise skips.
