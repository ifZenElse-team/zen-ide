# ADR-0188: Release notes and decision records in files of their own, so merge requests stop conflicting

- **Status:** Proposed
- **Date:** 2026-10-31
- **Extends:** [ADR-0146](0146-one-calendar-version-for-every-ide-and-release-channels.md) (one
  calendar version for every IDE), [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md)
  (releases from a protected tag), [ADR-0001](0001-record-architecture-decisions.md) (decision
  records)

## Context

On 2026-10-22, 44 of 46 open merge requests conflicted with `main`, and of the 9 that did not, no
more than two could merge one after the other. Every merge request wrote its notes at the top of
`changelog/gogarden.md`, `changelog/rustronin.md` and `CHANGELOG.md`, and added a row at the end of
the ADR table in `docs/README.md`: the same few lines in every branch. Each merge broke every other
merge request. Notes also landed in versions already released (0.38.33, 0.2.8), because the top
section of a product's changelog is the last release once it is out, and ADR numbers collided (two
0089, 0094 taken twice; on 2026-10-31 two open branches still both use 0184).

`CHANGELOG.md` had grown to one `[Unreleased]` section of about 2,500 lines that was never cut into
releases. The release commands (`cargo xtask release*`, the tag's pipeline `cargo xtask release-ci`)
read a version's notes from its section `## <version> - <date>` in each product's changelog; the
tag's pipeline cannot commit, so that section must be on `main` before the tag.

## Decision

1. **Notes not released yet are files of their own.** Every merge request writes its notes as
   `changelog/unreleased/<task>.md` (or named after its branch): level-2 headings name where they
   go (`ides` for every IDE's changelog, one IDE's id, `toolbox`, or `repository` for
   `CHANGELOG.md`), level-3 headings the section (`Added`, `Changed`, `Deprecated`, `Removed`,
   `Fixed`, `Security`), and the notes are a Markdown list under it. A plug-in's notes stay in its
   own changelog under the version its manifest gives, as before.
2. **A release gathers them.** `cargo xtask changelog gather <YY.M.N> [--toolbox <version>]
   [--date <YYYY-MM-DD>]` writes the section `## <YY.M.N> - <date>` at the top of every IDE's
   changelog (the notes for `ides` and for that IDE; it refuses when one IDE would have none),
   Zen Toolbox's section when asked, and `## [<YY.M.N>] - <date>` under `CHANGELOG.md`'s
   `[Unreleased]`, then removes the files it emptied. It writes nothing when anything is wrong.
   A version that is out is never written to: one with a release tag `v<YY.M.N>`, or a section
   the changelog has at the newest release tag (Zen Toolbox's versions too). A section that is
   there and not released yet (notes merged straight into it before this decision, or an earlier
   gather of the same version) is the newest one and takes the notes after its own, under their
   `###` sections, keeping its date; so gathering again before the tag is safe. A maintainer runs
   it in one merge request before the tag (docs/releasing.md, step 2); the release commands then
   read the sections as before, and say to run it when a section is missing. The tag's pipeline
   (`cargo xtask release-ci build`, ADR-0171) refuses, building nothing, a commit whose
   `changelog/unreleased/` still holds notes for the release (Zen Toolbox's only when the tag
   releases it) or whose IDE changelogs lack the version's section: notes merged between the
   gather and the tag are never silently left out.
3. **The gate checks them.** `cargo xtask ci` (step `changelog`) refuses a notes file that is
   badly named, has text outside a target and a section, an unknown target or section, or an empty
   section, and refuses notes written under `CHANGELOG.md`'s `[Unreleased]`, which only points to
   the folder.
4. **The records are the ADR index.** The table in `docs/README.md` keeps the records up to
   ADR-0179 as it stood and is not extended; `docs/adr/` lists the records by number, and
   `cargo xtask adr list` prints every record's title and status from the records themselves.
5. **ADR numbers come from every branch, and the gate refuses clashes.** `cargo xtask adr next`
   fetches and names the next number used by no file in any branch's history (and lists numbers two
   records took). The gate refuses two records under one number, a record whose heading names
   another number or that has no status, and a number that `origin/main` gives another record,
   when that branch is known where the gate runs. A merge request that loses the race takes the
   next free number before it merges.
6. **The migration.** `CHANGELOG.md`'s `[Unreleased]` held every change up to 26.10.4, whose tag
   was on `main`'s last commit with notes; what it held at the tag `v26.10.4`, and only that,
   becomes the section `## [26.10.4] and earlier - 2026-10-27` unchanged (a test compares it with
   the tag). Notes merged after the tag are not part of it: they belong to the next release. The products' changelogs had no notes after their last release. Released sections
   are not touched. Open merge requests move their notes once (the merge request of this decision
   says how).

## Consequences

- Merge requests no longer touch the same lines for their notes or a new ADR: each adds files of
  its own, so they merge one after another without a rebase for that.
- No note lands in a release that is out: notes merged after the gather wait in
  `changelog/unreleased/` for the next release.
- Releasing has one more step on `main` (the gather's merge request), and its notes may need an
  edit for users there, since they come from many merge requests.
- Notes in a gathered section are ordered by section and then by file name, not by merge time.
- An ADR number can still be taken twice by branches that start the same day; the later merge
  request sees it in the gate and renumbers.
- The status of a record lives in its own `Status` line, the only place the index reads it from.

## Alternatives considered

- **Generating the ADR table into `docs/README.md` and checking it in the gate.** Every new record
  would still change the same last lines of the table, so merge requests would still conflict.
- **Taking ADR numbers at merge time** (a placeholder number in the branch). Links between records,
  code and docs would have to be rewritten when merging, by hand or by a tool, and a merge request
  would no longer be what was reviewed.
- **Gathering notes in the tag's pipeline.** The pipeline cannot commit to `main`, so the released
  notes would not be in the repository's changelogs, and the website's changelogs would miss them.
- **A `Unreleased` section in every product's changelog.** Every merge request would still edit its
  first lines.
