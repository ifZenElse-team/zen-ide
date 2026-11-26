# ADR-0142: Release builds on the runner, signed and published on the release machine

- **Status:** Proposed; superseded by [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md) (releases from a protected tag), and kept as its fallback
- **Date:** 2026-10-24
- **Extends:** [ADR-0020](0020-release-server.md), [ADR-0140](0140-ci-on-a-team-mac-runner.md),
  [ADR-0141](0141-the-gate-remembers-a-pass-by-its-tree.md)

## Context

A release built the product with LTO on the release machine and loaded it for many minutes; the
owner wants the heavy builds off it. The team's Mac runner (ADR-0140) can build. But AGENTS.md
rule 12 stays: the signing key (`~/.config/zen-ide/publisher.key`) and the release server's token
(`releases.env`) never leave the release machine.

Where the secrets are used, read from `xtask/src/release.rs` and `zen-releases`: the key signs the
**manifest** (`zen-releases publish`, after uploading the archive; the manifest records the
archive's SHA-256 and size) and the **plug-in catalog** (`publish-plugin`); the token authorises
every upload (archives, the installer and its `.sha256`, the changelogs, the manifest). Everything
else needs no secret: the release binaries, the application bundles (signed ad hoc with
`codesign -`), the bundled plug-ins, the archives, the `.dmg`, the plug-in modules, the notes and
the changelogs.

## Decision

1. **Two commands.** `cargo xtask release-build <product> <version>` (or `plugin <id> <version>`)
   checks the source, runs the gate (remembered by tree), builds into a folder with
   `build.json` (kind, product, name, version, notes, commit, tree, `rustc -V`, platform, host, the
   gate's result, a plug-in's catalog fields, and every file's role, size and SHA-256), and uses no
   secret. `cargo xtask release-publish (--from <dir> | --pipeline <id> | --job <id>) [--dry-run]`
   runs on the release machine: it fetches the build, verifies every file against `build.json`, that
   the
   commit is on `origin/main` (fetched then) with the recorded tree and that the gate passed, builds
   `zen-releases` from the owner's checkout, and signs and publishes. `--dry-run` verifies and lists
   what it would send without signing or sending.
2. **One command still works.** `cargo xtask release <product> <version>`, `release-toolbox` and
   `release plugin` build into `<target>/release-build/` and publish that folder, the same code.
3. **CI.** A `release-build` job runs only in a pipeline started by hand on `main` (Run pipeline,
   `$CI_PIPELINE_SOURCE == "web"`) with `PRODUCT` and `VERSION`, on the release runner; its output
   is kept as GitLab job artifacts for 7 days. An IDE's archive is about 40 MB (RustRonin
   0.2.21: 38 MB) and Zen Toolbox's archive and installer are smaller, far under
   GitLab's 1 GB limit, so they are not kept on the runner for SSH: GitLab's artifacts need no
   access from the release machine to the runner and expire on their own. `release-publish
   --pipeline` downloads them with `glab`.
4. **Trust.** The publish trusts the runner's build as it trusts its checks: the runner is the
   team's own Mac, and the build records the commit it built, which the publish checks against
   `origin/main`. The program that reads the key is always built on the release machine.

## Consequences

- The long LTO builds of a release run on the runner; the release machine downloads and signs, in
  about a minute plus `zen-releases`' build.
- A release can still be made entirely on the release machine, so nothing depends on the runner.
- Linux and Windows releases (Docker) are unchanged for now.
