# ADR-0141: The quality gate remembers a pass by its tree

- **Status:** Proposed
- **Date:** 2026-10-24

## Context

`cargo xtask ci` is the only gate (CI is off, ADR-0010 and the owner, 2026-10-16), and every release
command runs it first. Its test step takes 4.5–12 minutes. The release of 2026-10-24 ran four release
commands on one unchanged tree and so ran the whole gate four times, about 20 minutes of a fully
loaded Mac for nothing; an agent that runs the gate, commits and runs it again pays twice too.

## Decision

1. When the worktree is clean (`git status --porcelain` is empty, untracked files included), a
   passed gate is recorded as `<target directory>/gate-passed/<tree hash>`, the file holding the
   `rustc -V` it passed with. The tree hash is `git rev-parse HEAD^{tree}`: it covers every
   committed file, `Cargo.lock` included, and is the same for any commit with the same content.
2. `cargo xtask ci` on a clean tree whose record holds the current `rustc -V` prints that the tree
   already passed and runs nothing. `cargo xtask ci --force` runs it anyway and records it again.
3. A worktree with changes always runs the gate and records nothing; so does a worktree that
   changed while the gate ran.
4. The release commands (`gate()` in `xtask/src/release.rs`) use the same record.
5. `--skip <steps>` and `--only <steps>` run some of the gate's steps (CI's fast first stage and its
   test stage, ADR-0140); such a run is never skipped by the record and never recorded.

## Consequences

- Several release commands on one tree, or a gate run again after committing what passed, cost
  seconds instead of minutes.
- The record lives in the target directory: cleaning it, or another target directory, runs the gate
  again. Nothing else (cargo-deny's advisory database, the operating system, environment
  variables) is part of the key: a security advisory published after the pass is seen on the next
  change of the tree or with `--force`.
