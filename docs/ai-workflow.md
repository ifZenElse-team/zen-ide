# Working with AI agents

> **Last updated:** 2026-11-26

Zen IDE is developed mostly with AI coding agents. This document describes how to work so that
**any agent** (Claude Code, Codex, Cursor, GitHub Copilot, Gemini CLI, Aider or another) and **any
developer** can pick up a task, do it correctly and hand it over cleanly. Nothing here depends on a
particular tool.

## 1. How agents find their instructions

| File | Read by |
|---|---|
| `AGENTS.md` | The canonical instructions. Read natively by Codex, Cursor, GitHub Copilot, Aider and others |
| `CLAUDE.md` → `AGENTS.md` | Symlink for Claude Code |
| `GEMINI.md` → `AGENTS.md` | Symlink for Gemini CLI |

There is **one source of truth**: `AGENTS.md`. Never create tool-specific instruction files with
different content. If a tool needs a file in another place, add a symlink or a one-line pointer to
`AGENTS.md`.

Nested `AGENTS.md` files may be added inside a crate when that crate has rules of its own (for
example `crates/lang/rust/AGENTS.md`). They add to the root file and never contradict it.

## 2. Principles for an agent-friendly repository

1. **Everything an agent needs is in the repository.** Decisions live in `docs/adr/`, plans in
   `ROADMAP.md`, conventions in `AGENTS.md` and `CONTRIBUTING.md`. A chat conversation is not a
   source of truth until its outcome is written down here.
2. **Explicit beats clever.** Prefer obvious names, small modules, and one way of doing a thing.
   Agents copy the patterns they see; make sure the patterns are good.
3. **Machines check the rules.** A rule that matters is enforced by `cargo xtask ci`
   (formatting, lints, layering, licences, tests), so neither an agent nor a reviewer has to
   remember it.
4. **Every crate explains itself.** Each crate has a crate-level doc comment (`//!`) that states
   its responsibility, its layer, what it must not do, and where to start reading.
5. **Small, verifiable steps.** A task should fit in one pull request that a reviewer can
   understand in one sitting.

## 3. The task format

Tasks (GitHub issues) are written so that an agent can execute them without guessing.
Every task contains:

- **Goal:** one or two sentences on the outcome, not the method.
- **Context:** links to the relevant ADRs, documents, crates and prior pull requests.
- **Scope:** what is included and, explicitly, what is not.
- **Acceptance criteria:** a checklist that can be verified, preferably by tests or commands.
- **Verification:** the exact commands that prove the task is done.

## 4. The working loop

1. **Read.** `AGENTS.md`, the task, the ADRs and documents it links, and the code it touches.
2. **Plan.** For anything larger than a small fix, write a short plan (in the pull request
   description) before changing code: which files, which commands, which tests.
3. **Implement** in small commits, following the patterns already in the code.
4. **Verify.** Run the quick checks locally — `cargo xtask ci --skip test` (format, Clippy, layers,
   comments, translations, changelogs, deny, WebAssembly: a minute or two) and `cargo test -p <crate>` for
   every crate you changed — and let CI run the full `cargo xtask ci` on the pull request. Add or
   update tests for every behaviour change. Never weaken, skip or delete a test to make a change
   pass.
5. **Document.** Update every document the change makes wrong, in the same pull request. Write an
   ADR if the change is a significant decision ([ADR-0001](adr/0001-record-architecture-decisions.md)).
6. **Hand over.** Open a pull request that says what changed, why, how it was verified,
   and anything left open.

## 5. Definition of done

A change is done when:

- [ ] the acceptance criteria are met;
- [ ] the pull request's pipeline (`cargo xtask ci` in CI) is green;
- [ ] new behaviour is covered by tests;
- [ ] the code has no comments ([ADR-0194](adr/0194-comments.md)), and affected documents are updated;
- [ ] a change has its release notes in a file of its own in
  [changelog/unreleased/](../changelog/unreleased/README.md), and a user-visible one is
  described in the user guide, [docs/guide/](guide/README.md), the website's documentation;
- [ ] a new capability is a command reachable by AI agents ([ADR-0033](adr/0033-ai-access-to-every-api.md));
- [ ] it works in **every** Zen IDE: shared behaviour in the platform, a language, tool or framework
  as a plug-in that any IDE can install; a product only decides what it bundles
  ([ADR-0050](adr/0050-products-are-plugin-bundles.md));
- [ ] no rule in `AGENTS.md` is broken, and any deviation is backed by a new ADR;
- [ ] the pull request description explains the change and its verification.

### Running several agents on one machine

An agent can work through a headless IDE over MCP, `rustronin mcp --headless --no-restore .`
(RustRonin 0.2.21 or newer): it opens the project without the tabs a windowed RustRonin left
open, which an agent has no use for (ADR-0139).

Advice, not configuration: nothing in the repository forces these.

- **A target folder per worktree** (`CARGO_TARGET_DIR=<repo>-<worktree>-target`), so that agents
  never wait for each other's Cargo lock or rebuild each other's artefacts.
- **About half the cores per build** (`CARGO_BUILD_JOBS=6` on a 10-core Mac, roughly `ncpu / 2`), so
  that two builds at once leave the machine responsive.
- **Incremental builds on** (Cargo's default for the dev profile) when the disk allows; turn them
  off (`CARGO_INCREMENTAL=0`) only when space runs short. `CARGO_PROFILE_DEV_DEBUG=0` and
  `CARGO_PROFILE_TEST_DEBUG=0` save space and link time when nobody needs a debugger.
- **The gate once per tree:** `cargo xtask ci` remembers a pass of a clean tree, so run it on a
  committed tree and a second run (or a release command) costs nothing.
- **macOS:** add the terminal and the agent's application to System Settings → Privacy & Security
  → Developer Tools, so that every new test and build binary is not checked by `syspolicyd` on its
  first start; and add the target folders to Spotlight's privacy list, so that they are not indexed.

## 6. Responsibility

An agent writes code; **a person is responsible for it.** The developer who opens a pull request
has read and understood every line, vouches for its provenance (see the clean-room rules in
[ADR-0002](adr/0002-open-source-clean-room-pure-rust.md) and [ADR-0280](adr/0280-clean-room-process.md)) and signs off the commits under the
Developer Certificate of Origin. Reviewers review agent-written code with the same care as any other.

## 7. Things agents must never do

- Build a capability for one product only (in `apps/` or in a way only one IDE can use): it belongs
  in the platform or in a plug-in for every IDE (ADR-0050).
- Copy or recall the code, assets or texts of another product, read another IDE's source, or work
  from observation records instead of our behaviour specification (ADR-0280).
- Name another IDE, editor or developer tool as something we follow, compare with or look like, in
  code, comments, identifiers, docs, the user guide, changelogs, commit messages, pull requests or
  issues, or say our work is "as in" or "like" one, or matches one. The only exceptions are user-visible
  import and compatibility features and the legal notices (AGENTS.md, golden rule 14).
- Add a dependency without checking its licence and the dependency policy.
- Disable lints, tests or checks to get a green build.
- Change an accepted ADR instead of writing a new one.
- Commit secrets, credentials, personal data or machine-specific files.
- Write anything in the repository in a language other than English.
- Start an IDE with folders or files (`gogarden <path>`, `rustronin <path>`, a test or script that
  does) without a throwaway `ZEN_DATA_DIR`: it would open them in the person's running IDE and bring
  it to the front ([ADR-0111](adr/0111-hand-files-to-the-running-ide.md)). Add `ZEN_HANDOFF_ONLY=1`
  where no window may open.
