# Notes not released yet

Every pull request writes its release notes here, as a file of its own named after its issue or
branch: `changelog/unreleased/<name>.md`, for example `find-in-files.md` ([ADR-0188](../../docs/adr/0188-release-notes-and-decision-records-in-files-of-their-own.md)).
No pull request edits the top of `CHANGELOG.md` or of a product's changelog any more, so they no
longer conflict there, and no note lands in a release that is already out.

```markdown
## ides

### Fixed

- **F4** opens the file from the Commit window and the diff viewer.

## repository

### Changed

- `cargo xtask changelog gather` moves the notes of `changelog/unreleased/` into a release.
```

- A level-2 heading says where the notes go: `ides` (every IDE's changelog, the usual case for the
  platform and the plug-ins bundled with every IDE), one IDE (`gogarden`, `rustronin`, `webwave`,
  `jvmjizo`, `sqlscroll`, `devopsdaruma`), `toolbox` (Zen Toolbox) or `repository` (`CHANGELOG.md`: code, docs, tooling).
- A level-3 heading is the section: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed` or
  `Security`. Under it, the notes as a Markdown list, in English. Product notes describe what users
  notice; repository notes may name commands, files and ADRs.
- A plug-in's notes stay in its own changelog, `changelog/plugins/<id>.md`, under the version its
  `Cargo.toml` gives (the gate checks that section).

`cargo xtask ci` (step `changelog`) checks every file here, and that `CHANGELOG.md`'s
`[Unreleased]` holds no notes of its own. Before a release, a maintainer runs
`cargo xtask changelog gather <YY.M.N>` (add `--toolbox <version>` when Zen Toolbox is released
too, and `--date <YYYY-MM-DD>` for another day than today): it writes the section
`## <YY.M.N> - <date>` into every IDE's changelog with the notes for `ides` and for that IDE, Zen
Toolbox's section, and `## [<YY.M.N>] - <date>` into `CHANGELOG.md`, then removes the files it
emptied, in one pull request before the release ([docs/releasing.md](../../docs/releasing.md)).
When the version's section is there already and not released (written by hand, or gathered for an
attempt that failed), the notes are added to it under their `###` sections and its date stays; a
released version (one the published manifest on the release server lists) is refused, so a
release's notes never change once it is out.

What waits here can be read at any time: `cargo xtask changelog preview` prints it by product and
section, and on GitHub the draft release "Next release" holds the same text, written again after
every merge into `main`.
