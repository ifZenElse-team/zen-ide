# ADR-0013: Minimum supported Rust version is the latest stable

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The workspace manifest needs a `rust-version`. Projects choose between supporting only the latest
stable compiler, a rolling window ("latest stable minus N"), or a fixed version raised deliberately.
Zen IDE ships applications, not libraries, so downstream crates do not depend on our minimum
version. The project is unreleased and benefits from new language and standard library features
as soon as they are stable.

## Decision

1. Zen IDE supports **only the latest stable Rust release**.
2. `rust-toolchain.toml` pins the channel to `stable`, so every contributor and CI use the latest
   stable compiler with the same components and targets.
3. `rust-version` in the workspace manifest is set to the latest stable version and raised when a
   new stable version is released (a routine task), or earlier when a change needs a newer feature.
4. Nightly-only features and tools are not used in the default build or in `cargo xtask ci`.

## Consequences

- Contributors must keep their toolchain current (`rustup update`). A toolchain installed by an
  operating system package manager may lag behind and is not supported; rustup is required, also
  because the WebAssembly target must be installable.
- The policy is revisited before the first public release, when distribution packagers may need a
  rolling window.

## Alternatives considered

- **Latest stable minus N:** friendlier to Linux distributions, but not needed before the first
  public release.
- **A fixed version:** predictable, but requires deliberate bumps and delays new features.
