# ADR-0140: CI on merge requests, on the team's own Mac runner

- **Status:** Proposed; decision 7 (no secrets on the runner) superseded by [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md)
- **Date:** 2026-10-24
- **Extends:** [ADR-0010](0010-hosting.md); the owner's decision of 2026-10-16 to switch GitLab CI off

## Context

On 2026-10-16 the owner switched GitLab CI/CD off: `cargo xtask ci`, run locally, became the only
gate. On the release machine (M5, 10 cores) the gate takes 8–12 minutes, most of it the tests, and
with several agents working at once it ran many times a day on the same machine, which was then
slow for everything else. A second Mac is now available (M2, 8 GB,
4 + 4 cores, no fan), always on, reachable over SSH, without the release
signing key.

Measured on it (2026-10-24): the gate from a clean build in 6.6 minutes, and after a change to
`zen-ui` (a typical merge request) in 9.7 minutes; the largest process took 3.1 GB. That is as fast
as the release machine under load, so it keeps up with the merge requests of a day (about ten, at most
a few an hour).

## Decision

1. **CI runs on merge requests only**, in two stages of the same command as locally: the fast
   checks (`cargo xtask ci --skip test`, about two minutes), then the tests
   (`cargo xtask ci --only test`) only once they pass.
   No pipelines for branches without a merge request and none for a push to `main`, which only
   receives merged merge requests.
2. **On the team's Mac runner.** A GitLab runner registered for this project only, with a tag
   of its own, a shell executor (no containers: the gate tests a macOS application), and
   `concurrent = 1`: one gate at a time, the rest wait in GitLab's queue. GitLab's shared runners
   are switched off for the project, so a job never runs elsewhere or uses paid minutes.
3. **Warm builds.** The runner keeps one checkout (`GIT_STRATEGY: fetch`) and builds into
   `CARGO_TARGET_DIR` outside it, which `git clean` keeps; builds are incremental and use six jobs.
   The toolchain comes from `rust-toolchain.toml` through rustup on the machine, and `cargo-deny`
   is installed there.
4. **Stale pipelines are cancelled.** Jobs are interruptible and a new push to a merge request
   cancels its running pipeline.
5. **CI is the gate** (the owner, 2026-10-25). A merge request merges only with a green pipeline
   (the project's "Pipelines must succeed"), and the full gate no longer runs on the developer's
   machine: before pushing, only the quick checks (`cargo xtask ci --skip test` and the tests of the
   crates changed). When the runner is down for long, the owner may switch the setting off for a
   while and merge on a local `cargo xtask ci`.
6. **Nightly on `main`.** A pipeline schedule runs the whole gate `NIGHTLY_RUNS` times in a row
   (5 by default) with `--force`, so a test that fails now and then shows up, and a security
   advisory published after a remembered pass ([ADR-0141](0141-the-gate-remembers-a-pass-by-its-tree.md))
   is seen within a day. The job fails if any run failed; GitLab mails the schedule's owner.
7. **No secrets on the runner.** It reads the repository with a deploy key that cannot push; it
   holds neither the release signing key nor the release server's token, and releases stay on the
   release machine (AGENTS.md rule 12).

## Consequences

- The gate leaves the release machine for merge requests; agents still run it locally before pushing.
- The runner's machine is a single point: if it is off, merge requests wait or fall back to the
  local gate. Its setup (Homebrew `gitlab-runner`, a launch daemon running as the CI user,
  `~/ci/runner/config.toml`) is described in docs/development.md.
- Linux and Windows are not tested in CI; their builds stay on request (AGENTS.md rule 12).
