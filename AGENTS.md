# Zen IDE: entry point for AI agents

This file is the starting point for any AI agent (and any new contributor) working in this
repository, whatever the tool: Codex, Cursor, GitHub Copilot and Aider read `AGENTS.md` directly;
`CLAUDE.md` (Claude Code) and `GEMINI.md` (Gemini CLI) are symlinks to it. Never create a
tool-specific instruction file with different content. Keep this file short and current: it links
to the details instead of repeating them. How to work with agents: [docs/ai-workflow.md](docs/ai-workflow.md).

> **Read this first — how the product is built ([ADR-0050](docs/adr/0050-products-are-plugin-bundles.md)):**
> every capability beyond the platform is a **plug-in that works in every Zen IDE at once**; a
> product (GoGarden, RustRonin, WebWave, DevopsDaruma, TestTotami...) is only a **ready-made bundle** of plug-ins
> for one kind of work. **Basic web (HTML, CSS, JavaScript, TypeScript) is in every IDE** as a base
> function; everything else is installed when needed. Never build a feature for one product only:
> shared behaviour goes into the platform (`crates/platform`), capabilities into plug-ins.
>
> **Releases are made by the maintainers** ([ADR-0171](docs/adr/0171-releases-from-a-protected-tag-through-gitlab.md)):
> a release is a protected tag `v<YY.M.N>` on `main`, built, staged and checked by the release
> pipeline and made live by the owner. The signing key and the release server's token are protected
> CI variables, never files in the repository. A contributor's work ends with a merged pull
> request: never create a release tag or run `cargo xtask release*`.

## 1. What this project is

Zen IDE is a clean-room, open-source family of IDEs in pure Rust, made by the ifzenelse team. All
IDEs share one engine, the **Zen Platform**, and one set of plug-ins; each IDE is a bundle of those
plug-ins for one kind of work, with its own branding.
The first IDE released is **GoGarden** (Go, [ADR-0026](docs/adr/0026-gogarden-first.md)); RustRonin (Rust) is offered in Zen Toolbox since 0.2.0 (2026-10-21), and Zen IDE itself is developed through it. WebWave (the web) has its app, not released yet. JvmJizo (Java and the JVM) is released with the other IDEs from 26.10.2, a preview not announced yet ([ADR-0147](docs/adr/0147-jvmjizo-app-and-jdks.md)). The full product list is in [docs/naming.md](docs/naming.md). Since 26.10.1 every IDE is released together under one calendar version, `YY.M.N` SqlScroll (SQL and databases) and DevopsDaruma (DevOps) are released with the other IDEs from 26.11.8.
([ADR-0146](docs/adr/0146-one-calendar-version-for-every-ide-and-release-channels.md)); Zen Toolbox and plug-ins keep their own versions.

**Current state:** the IDEs have our own editor, language servers (problems, completion,
navigation, rename, formatting), Git, run configurations and debugging, themes, the terminal,
plug-ins and MCP for AI agents; Zen Toolbox installs and updates the IDEs. Every release is built
and checked for macOS; Linux and Windows builds are made on request. The gate is `cargo xtask ci`,
run by CI on every pull request. Tasks are GitHub issues (section 5).

## 2. Golden rules

1. **English only.** Code, comments, identifiers, commit messages, pull request descriptions,
   issues and documentation are written in English, without exception
   ([ADR-0009](docs/adr/0009-english-only.md)).
2. **Clean room.** Code is written only from our own behaviour specification, public
   specifications and public documentation; observing another product is a task of its own, and its
   records stay outside the repository. Never copy or recall the code, icons, images, texts, keymap or
   settings files of another product, and never read another IDE's source, whatever its licence.
   ([ADR-0002](docs/adr/0002-open-source-clean-room-pure-rust.md), [ADR-0280](docs/adr/0280-clean-room-process.md))
3. **Pure Rust.** No C or C++ in the default build. `unsafe` is forbidden outside a crate that is
   explicitly listed as an exception in the layering rules, with a documented reason. The only
   exception is `zen-macos` (`crates/platform/macos`): the macOS application hooks without a safe
   binding (asking before a system quit), in one module, every block with `// SAFETY:`
   ([ADR-0079](docs/adr/0079-zen-macos-unsafe-exception.md)).
4. **Layering is law.** The platform never depends on a language module; nothing below the UI layer
   depends on a UI toolkit; apps are thin. The layers are checked by `cargo xtask layers` once the
   workspace exists. ([ADR-0004](docs/adr/0004-monorepo-and-layering.md))
5. **Everything is a command, and every command is reachable by AI** ([ADR-0033](docs/adr/0033-ai-access-to-every-api.md)). Every user-visible action is a command with a stable id and typed,
   serialisable parameters. The UI, CLI, MCP server and plug-ins dispatch commands; they never call
   feature code directly. ([ADR-0007](docs/adr/0007-everything-is-a-command.md))
6. **Decisions are written down.** A change that affects architecture, dependencies policy, file
   formats or public behaviour needs an ADR in [docs/adr/](docs/adr/). An ADR is never edited after
   it is accepted; a new ADR supersedes it.
7. **Docs move with code.** A change that makes a document wrong updates that document in the same
   pull request. Every change users can see also updates the user guide
   ([docs/guide/](docs/guide/README.md)), which is the documentation on the website.
8. **The UI follows our behaviour specification.** Read [ADR-0280](docs/adr/0280-clean-room-process.md)
   and the specification ([docs/behaviour.md](docs/behaviour.md)) before any UI work. The default
   keymap, the action and menu names users search for and the import of other products' files stay
   compatible; icons, texts, colours and layouts are our own.
9. **No user-visible text in code.** Every string shown to people is a Fluent message used through
   `zen_i18n::t!`, written in English and Russian in the same change (`<crate>/locales/<lang>/*.ftl`);
   `cargo xtask i18n` checks it. ([ADR-0024](docs/adr/0024-localisation.md))
10. **No telemetry, no accounts, no network by default.** The IDE works offline. Any network access
   (updates, toolchain downloads) is explicit, visible and can be turned off.
11. **Plug-ins first, for every IDE.** A new language, tool or framework is a plug-in that works in
   every Zen IDE; a product only bundles plug-ins and is never the only place a capability exists.
   Basic web (HTML, CSS, JavaScript, TypeScript) is in the base set of every IDE. Features every IDE
   needs (the editor, Git, run, language servers, settings) live in the platform.
   ([ADR-0050](docs/adr/0050-products-are-plugin-bundles.md), [ADR-0036](docs/adr/0036-bundled-plugins.md))
12. **Releases are the maintainers'.** A release is the tag `v<YY.M.N>` on `main`, created by a
   maintainer; its pipeline builds every platform, stages everything on the release server, checks
   the staged downloads, and waits for the owner's click on the manual `release` job, which makes
   it live ([ADR-0171](docs/adr/0171-releases-from-a-protected-tag-through-gitlab.md)). The signing
   key and the release server's token exist only as protected, masked CI variables and with the
   maintainers; never copy them anywhere else. Every release is for macOS and is checked after
   publishing; Linux and Windows builds are made on request
   ([docs/releasing.md](docs/releasing.md)).

13. **No comments in code.** Rust code has no `//`, `/* */`, `///` or `//!` comments and no
   commented-out code; names and types say what code is, and a decision goes into `docs/`. The
   exceptions are `// SAFETY:`, a lone one-line `// WHY:` where no test can hold the reason, and
   the docs of types deriving `JsonSchema` or clap, whose text is data agents and `--help` show.
   A rule that protects behaviour is a test named after it, not a comment. `cargo xtask comments`
   (a CI step) enforces it.
   ([ADR-0194](docs/adr/0194-comments.md))

14. **Never name another product.** Code, comments, identifiers, docs, the user guide, changelogs,
   commit messages, pull requests and issues never name another IDE, editor or developer tool as
   something we follow, compare with or look like, and never say our work is "as in" or "like" one, or matches one: say what our product does. The only exceptions are user-visible import
   and compatibility features (settings, keymap, `.idea` and theme import, the compatibility names
   of ADR-0280) and the legal notices (`TRADEMARKS.md`, `NOTICE`, `ATTRIBUTION.md`).
   `cargo xtask legal` checks it against the maintainers' word list (`ZEN_LEGAL_WORDS`; without it the
   names are not checked and the step says so). ([ADR-0280](docs/adr/0280-clean-room-process.md))

## 3. Naming

| Level | Name | Used as |
|---|---|---|
| Team | ifzenelse | GitHub organisation, copyright holder |
| Family | Zen IDE | Product family, this repository (`zen-ide`) |
| Platform | Zen Platform | The shared engine; crates are named `zen-*` |
| Core | Zen Core | The command engine and session (`zen-core`) |
| Products | RustRonin, GoGarden, ... | Branded applications in `apps/` |
| Installer | Zen Toolbox | Installs, updates and launches the IDEs |

Rules and the full list: [docs/naming.md](docs/naming.md).

## 4. Where things live

```text
README.md            public overview
ROADMAP.md           phases and milestones
docs/README.md       documentation index
docs/development.md  toolchain setup and everyday commands
docs/architecture.md platform design (target state)
docs/naming.md       product, crate and binary names
docs/behaviour.md    the UI's behaviour specification and status (ADR-0280)
docs/ai-workflow.md  how to work with AI agents: task format, loop, definition of done
docs/adr/            architecture decision records (numbered, immutable once accepted)
crates/platform/     Zen Platform crates (zen-*); rules in its README
crates/lang/         language modules (zen-lang-*), built-in modules of every IDE (ADR-0071); rules in its README
plugins/             plug-ins (zen-plugin-*), WebAssembly modules; docs/plugins.md
drivers/             database driver programs (zen-db-*) that plug-ins ship; docs/database-drivers.md
apps/                applications (IDEs, zen-toolbox, zen-cli); rules in its README
xtask/               build automation: `cargo xtask`
```

Lints are set once in the root `Cargo.toml` (`[workspace.lints]`, `unsafe_code = "forbid"`); every
crate opts in with `[lints] workspace = true`. Two kinds of crate copy the lints with
`unsafe_code = "deny"` instead: a plug-in crate, so that its ABI exports can use
`#[unsafe(no_mangle)]` (ADR-0031), and `zen-macos`, whose one macOS module allows `unsafe` for the
Objective-C runtime (ADR-0079).

## 5. Working in this repository

- **Tasks are GitHub issues**, and changes are pull requests: one issue, one branch from the latest
  `main`, one pull request. Say on the issue that you take it before you start, and never take an
  issue someone else is working on. Details in [CONTRIBUTING.md](CONTRIBUTING.md).
- **Sign off every commit** (`git commit -s`): the `Signed-off-by` line is the Developer Certificate
  of Origin, your statement that you may contribute the change under the project's licences.
- **The gate is `cargo xtask ci`** ([docs/development.md](docs/development.md)): CI runs it on
  every pull request, and a pull request merges only when it passes. Before you push, run the quick
  checks locally, `cargo xtask ci --skip test` (format, Clippy, layers, comments, legal,
  translations, changelogs, deny, WebAssembly: a minute or two), and `cargo test -p <crate>` for
  every crate you changed.
- **Agents may work through a Zen IDE**: `rustronin mcp --headless --no-restore .` is a headless
  RustRonin over MCP (ADR-0084, ADR-0085). Prefer its commands to the shell where they are cheaper
  (usages, rename, problems after an edit, tests, `editor.text.replace`, ranged
  `get_document_text`), and say in the pull request where it fell short.
- **Never start an IDE with folders or files on a person's data folder** (ADR-0111): `rustronin
  <path>` or `gogarden <path>` without `ZEN_DATA_DIR` hands the path to the person's running IDE
  and brings it to the front. Every start an agent, test or script makes sets a throwaway
  `ZEN_DATA_DIR`, and `ZEN_HANDOFF_ONLY=1` when it must never open a window; headless IDEs
  (`--headless`, `mcp --headless`) never take such starts unless run with `--accept-open`.
- Commit messages: imperative mood, a subject of at most 72 characters, optional scope prefix
  (`docs: ...`, `core: ...`, `lang-rust: ...`). Details in [CONTRIBUTING.md](CONTRIBUTING.md).
- Toolchain: the latest stable Rust through rustup, edition 2024 ([ADR-0013](docs/adr/0013-msrv-latest-stable.md)).
  Run `rustup update` before starting work.
