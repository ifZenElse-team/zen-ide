# ADR-0047: Version control reads with gitoxide and writes with the git program

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

GoGarden needs Git: file status colours, the branch widget, Update Project, Push, the Commit tool
window, diffs, the log and blame. The platform is pure Rust with no C in the default build
(ADR-0002), every crate below L6 compiles for WebAssembly (ADR-0014), and every capability is a
command reachable by AI agents (ADR-0033).

There are three ways to work with Git repositories from Rust:

- **libgit2** (`git2`): complete, but C, and its network transports bring OpenSSL or libssh2.
- **gitoxide** (`gix`): pure Rust, MIT or Apache-2.0. Reading is complete and fast: status, refs,
  revision walks, tree diffs, blame. Writing is partial: it can create references and commits, but
  not yet check out a branch over a dirty working tree with Git's safety rules, merge, rebase, or
  push; its network transports need a TLS stack and credential helpers we would have to configure.
- **The `git` program**: what users already have and trust, with their configuration, credential
  helpers, SSH agent, hooks and signing.

## Decision

1. **`zen-vcs` (L2) reads repositories with gitoxide**: the working tree status (staged, unstaged,
   unversioned and ignored files), `HEAD`, branches with their upstreams and the commits ahead and
   behind, file contents at `HEAD`, the log and blame. gitoxide is used without default features
   (no network, no parallel threads): `sha1`, `status`, `blame`, `revision`, `blob-diff`, `index`,
   `dirwalk`, `excludes` and `attributes`. A repository handle is opened per call, so the crate's
   `Repository` is `Send` and `Sync` without gitoxide's parallel features.
2. **Operations that write run the user's `git` program** as a subprocess: commit, checkout, branch
   creation, merge and deletion, rollback, fetch, pull and push. git runs non-interactively
   (`GIT_TERMINAL_PROMPT=0`, the C locale for parseable progress) with the user's configuration, so
   credentials, hooks and signing work as in a terminal. Long operations report git's progress lines.
   When git is missing, the operation fails with a message that says so; reading still works.
3. **Desktop only.** gitoxide does not compile for `wasm32-unknown-unknown`; it is a target-specific
   dependency, and in WebAssembly builds `Repository::discover` finds nothing (as `zen-terminal` has
   no PTY there). The rest of the crate (types, progress parsing, diffs) is portable.
4. **The commands are the workbench's** (`zen_workbench::vcs`, `vcs.*`), which keeps the repository's
   state in a session slot, computes the status on a worker thread and runs network operations in
   the background, one at a time, with `vcs.operation.status` for their progress. Agents may pass
   `wait: true` to get the outcome directly.
5. **Network access is explicit**: only Update Project, Fetch and Push touch remotes, and only when the
   user (or an agent) asks (AGENTS.md rule 10). Nothing is fetched in the background.

## Consequences

- No C enters the build; `cargo deny` accepts gitoxide's dependency tree.
- Users need Git installed for the operations that write.
- When gitoxide gains safe checkout and push, single operations can move from the program to the
  library without changing the commands.

## Alternatives considered

- **libgit2:** C and OpenSSL; rejected by ADR-0002.
- **gitoxide for everything:** checkout, merge, rebase and push with credentials are not complete
  yet; reimplementing Git's safety rules is a large risk.
- **The git program for everything:** simpler, but parsing porcelain output for status, log and blame
  is slower and more fragile than reading the repository directly.
