# Contributing to Zen IDE

Thank you for your interest in Zen IDE. This guide explains how to contribute, whether you write
code yourself or with an AI agent.

## Before you start

1. Read [README.md](README.md) and [AGENTS.md](AGENTS.md): what the project is and its rules.
2. Read [docs/ai-workflow.md](docs/ai-workflow.md): how work is planned, done and verified. It
   applies to every contribution, with or without an agent.
3. Skim the [decision records](docs/adr/) relevant to the area you want to change.

## Ground rules

- **English only**, everywhere in the repository ([ADR-0009](docs/adr/0009-english-only.md)).
- **Clean room.** Code is written only from our own behaviour specification, public specifications
  and public documentation. Do not copy or recall the code, assets, texts or configuration files of
  another product, do not read another IDE's source whatever its licence, and do not ask an agent to
  reproduce them ([ADR-0002](docs/adr/0002-open-source-clean-room-pure-rust.md),
  [ADR-0280](docs/adr/0280-clean-room-process.md)). The default keymap, the action names users search
  for and import features stay compatible; icons, texts, colours and layouts are our own.
- **Never name another product.** Code, comments, identifiers, docs, the user guide, changelogs,
  commit messages, pull requests and issues do not name another IDE, editor or developer tool as
  something we follow, compare with or look like, and do not say our work is "as in" or "like" one, or matches one: say what our product does. The only exceptions are user-visible import and compatibility features and the legal
  notices (`TRADEMARKS.md`, `NOTICE`, `ATTRIBUTION.md`) ([ADR-0280](docs/adr/0280-clean-room-process.md)).
- **No comments in code**, no commented-out code; decisions go into `docs/`, rules that protect
  behaviour into tests named after them. Only `// SAFETY:`, a lone one-line `// WHY:` where no test
  can hold the reason, and the docs of command schema and clap types stay ([ADR-0194](docs/adr/0194-comments.md)).
  `cargo xtask comments` checks it.
- **Respect the layering.** See [docs/architecture.md](docs/architecture.md) and
  [ADR-0004](docs/adr/0004-monorepo-and-layering.md).
- **Significant decisions need an ADR** ([ADR-0001](docs/adr/0001-record-architecture-decisions.md)).

## Finding something to work on

Tasks are GitHub issues. An issue that is ready to be picked up has a goal, scope and acceptance
criteria. If you want to work on something without an issue, open one first so that the approach
can be agreed before you invest time.

## Workflow

1. Say on the issue that you take it, then create a branch (in your fork) from the latest `main`: `<type>/<short-description>`, for example `feat/editor-multi-caret` or
   `fix/vfs-watcher-rename`. One issue, one branch, one pull request; several people work at once,
   so never work on an issue someone else has taken.
2. Make your change in focused commits.
3. Run the quick checks locally: `cargo xtask ci --skip test` (format, Clippy, layers, comments, legal, translations,
   changelogs, deny, WebAssembly: a minute or two; the legal step looks for other products' names only
   when `ZEN_LEGAL_WORDS` names the maintainers' word list, and says so when it does not) and `cargo test -p <crate>` for every crate you
   changed. The full gate, `cargo xtask ci` (formatting, Clippy, tests, the layering check,
   translations, changelogs, the licence and advisory check, and the WebAssembly build of the core),
   runs in CI on your pull request, which merges only once it is green. Setup, including the one extra tool it
   needs, is in [docs/development.md](docs/development.md). A change adds its release notes as a
   file of its own, `changelog/unreleased/<name>.md` ([changelog/unreleased/](changelog/unreleased/README.md)):
   never edit `CHANGELOG.md` or the top of a product's changelog, which is a release that is out. A
   plug-in's new version adds its section to `changelog/plugins/<id>.md`. A new ADR takes the
   number `cargo xtask adr next` names ([ADR-0188](docs/adr/0188-release-notes-and-decision-records-in-files-of-their-own.md)).
4. Open a pull request and link the issue; it is merged into `main` after review.
   Releases are made by the maintainers, from `main` only ([docs/releasing.md](docs/releasing.md)).
5. Address review comments by pushing new commits; they are squashed or rebased on merge.

## Commit messages

```text
<scope>: <imperative summary, at most 72 characters>

<Body: what and why, wrapped at 100 characters. Optional for trivial changes.>

Signed-off-by: Your Name <you@example.com>
```

- **Scope** is the area: a crate without the `zen-` prefix (`core`, `text`, `ui`, `workbench`,
  `lang-go`), an app (`go-garden`, `toolbox`), or `plugins`, `release`, `site`, `docs`, `ci`,
  `xtask`, `deps`, `project`. Several scopes are joined with commas (`workbench, ui: ...`).
- Use the imperative mood: "Add multi-caret paste", not "Added" or "Adds".
- If an agent co-wrote the change, a `Co-Authored-By:` trailer is welcome but not required.

## Developer Certificate of Origin

Every commit must be signed off (`git commit -s`), certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you have the right to
submit the contribution under the project's licence. For AI-assisted work, signing off means you
have reviewed the change and are responsible for it.

## Licence

By contributing, you agree that your contribution is dual-licensed under the MIT License and the
Apache License 2.0, as described in [README.md](README.md#license), without any additional terms.
