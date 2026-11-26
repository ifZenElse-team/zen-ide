# Changelogs

Every release of every product and plug-in, newest first, one file each:

- `gogarden.md`, `toolbox.md`, ... — a product, named by its id (`cargo xtask release <id>`);
- `plugins/<id>.md` — a plug-in from `plugins/<id>`.

The repository's own changes (code, docs, tooling) are in [../CHANGELOG.md](../CHANGELOG.md), one
section per release; these files describe releases, for users.

Notes not released yet are files of their own in [unreleased/](unreleased/README.md), one per pull
request; `cargo xtask changelog gather <YY.M.N>` moves them into the release's sections here and in
`CHANGELOG.md` before the tag (ADR-0188). A pull request never edits a release's section: the
newest one is out already.

## Format

```markdown
# GoGarden changelog

Intro.

## 0.9.0 - 2026-10-20

### Added

- Find in Files (`⇧⌘F`).

### Fixed

- The caret no longer jumps after **Undo**.
```

- A release is a level-2 heading `## <major.minor.patch> - <YYYY-MM-DD>`, added at the top, above
  the previous release, by `cargo xtask changelog gather` for the IDEs and Zen Toolbox, and by hand
  for a plug-in's new version. Each version appears once.
- Its body, up to the next level-2 heading, is the release notes: Zen Toolbox and the IDE show it
  as "What's new". Use `###` subsections (`Added`, `Changed`, `Fixed`) and lists, in English, and
  describe what users notice, not the code.
- `cargo xtask release` reads the notes from here and refuses to release without them; for
  plug-ins too. The gate (`cargo xtask ci`, step `changelog`) checks every file's format, that
  each plug-in's current version has its section, and the files of `unreleased/`.
- `cargo xtask changelog --json <file>` writes all changelogs as JSON for the website; every
  `cargo xtask release` publishes it (`/releases/changelog.json`), and
  `https://ide.ifzenelse.space/changelog.html` shows it, linked from Zen Toolbox's What's new:
  `{ "schema": 1, "changelogs": [{ "kind": "product" | "plugin", "id", "name", "releases": [{ "version",
  "date", "notes" }] }] }`.

Releases before 2026-10-17 were recovered from the release server's notes, word for word as users
saw them; plug-in releases before then had no notes and were written from the history.
