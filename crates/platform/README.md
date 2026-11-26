# Zen Platform crates

The shared engine of every Zen IDE. Each crate lives in its own directory here, is named
`zen-<area>` (directory: `<area>`), and belongs to exactly one layer. The planned crates and their
layers are listed in [docs/architecture.md](../../docs/architecture.md), section 3.

Rules (ADR-0004 and ADR-0014, enforced by `cargo xtask layers`; the registry is `xtask/src/layers.rs`):

- A crate depends only on crates in lower layers.
- No crate here depends on a language module in `crates/lang/`.
- Nothing below L6 depends on a UI toolkit or windowing crate.
- Everything below L6 compiles for `wasm32-unknown-unknown`.

`unsafe` (ADR-0002, decision 5): every crate here forbids it through the workspace lints, except
the one listed exception, **`zen-macos`** (L0, [ADR-0079](../../docs/adr/0079-zen-macos-unsafe-exception.md)):
the macOS application hooks that have no safe binding, today `applicationShouldTerminate:` so that
a system quit asks first. It copies the workspace lints with `unsafe_code = "deny"`, allows
`unsafe` only in its macOS module `src/appkit.rs`, and every block there has a `// SAFETY:`
comment. Adding `unsafe` anywhere else, or a second exception, needs a new ADR.

A new crate must:

1. use the workspace package fields and `[lints] workspace = true`;
2. start with a crate-level `//!` doc comment stating its responsibility, its layer, what it must
   not do, and where to start reading;
3. be registered with its layer in `xtask/src/layers.rs`, and pass `cargo xtask layers`.
