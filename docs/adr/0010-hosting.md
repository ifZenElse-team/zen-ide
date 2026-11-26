# ADR-0010: Hosting on GitLab, moving to GitHub

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The ifzenelse team hosts its projects on GitLab under the `ifzenelse` group, one subgroup per
product. Zen IDE is meant to become a public open-source project, and most of the open-source Rust
community works on GitHub.

## Decision

- During the design and early development phases the repository is hosted privately on GitLab,
  with `main` as the default branch and GitLab CI.
- Before the first public release, the project moves to GitHub as its primary home. Repository
  conventions are kept host-neutral so that the move is mechanical: plain Markdown, CI logic in
  `cargo xtask` rather than in CI-specific scripts, and no dependence on host-only features.

## Consequences

- The CI configuration file is a thin wrapper around `cargo xtask ci` and is rewritten once for
  GitHub Actions.
- Issue and merge request templates exist in `.gitlab/` now and will be ported to `.github/`.
- Links in documentation are relative wherever possible.

## Alternatives considered

- **GitHub from the start:** the team's infrastructure and habits are on GitLab, and the project is
  not ready to be public.
