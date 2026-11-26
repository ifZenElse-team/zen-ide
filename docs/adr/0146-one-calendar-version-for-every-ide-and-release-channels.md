# ADR-0146: One calendar version for every IDE (release channels deferred)

- **Status:** Proposed (the scheme: the owner, 2026-10-25; channels deferred by the owner the same day)
- **Date:** 2026-10-25
- **Changes:** the IDEs' versions in [ADR-0020](0020-release-server.md); the release commands of
  [ADR-0142](0142-release-builds-on-the-runner-published-on-the-release-machine.md)

## Context

Each IDE had its own semantic version: GoGarden 0.38.49, RustRonin 0.2.25, WebWave 0.1.15 on the
same day, with the same changes, since they are bundles of one platform and one set of plug-ins
(ADR-0050). The numbers said nothing to users and made every release three releases.

The owner decided (2026-10-25) that every IDE has **one version**, a calendar version as Ubuntu's,
`YY.M.N`; Zen Toolbox and plug-ins keep their own versions. Release channels (Preview, Beta,
Release Candidate, Stable) were first designed in as well, then **deferred** the same day: every
release is implicitly a preview for now, and how to organise channels is decided later, in an ADR of
its own. Nothing of them is shown or published until then.

## Decision

1. **Version.** The IDEs share one version, `YY.M.N`: the year's last two digits, the month
   without a leading zero, and the release's number within the month, from 1 (`26.10.1`, `26.10.2`,
   `26.11.1`, `27.1.1`). Every IDE is released at each release, under that version, even one
   without changes of its own, so the versions never diverge. A calendar version is a valid
   semantic version and greater than every earlier IDE version (26 > 0), so installed IDEs and Zen
   Toolbox update from 0.38.49, 0.2.25 and 0.1.15 as they are. The version is shown alone:
   "26.10.1".
2. **The next version is this month's next.** `cargo xtask release-version` prints it: `YY.M` of
   today in local time (the owner's, on the release machine and on the release runner; `TZ` sets
   another) and the month's highest published IDE number plus one, read from the public manifest.
   `release ides` and `release-build ides` refuse any other version, so that a typo (`27.10.1`,
   `26.12.1` in October, a skipped number) cannot block every later release; `--allow-version`
   releases another on purpose, which must still be greater than every IDE version published.
3. **All or nothing.** The IDEs are built together (`release-build ides`, one CI job) and published
   together: `release-publish` takes the IDEs' builds only as the whole set, every IDE once, at one
   calendar version and commit, and verifies them all before publishing any. If publishing stops
   midway, it says which IDEs are published; running it again publishes them all, the published
   ones once more at the same version, which the release server allows.
4. **Changelogs.** Each IDE keeps its own file, and an IDE release is the same heading in each,
   `## 26.10.1 - 2026-10-25`; `cargo xtask changelog` checks that the IDEs' files list the same
   calendar versions. The notes stay per IDE, as each IDE's What's new shows its own. One shared
   section was rejected: the website, Zen Toolbox and the release commands read one file per
   product, and the IDEs' notes differ.
5. **The manifest is unchanged.** IDE releases are published on the `stable` channel, as before;
   Zen Toolbox's channel setting (`toolbox.channel`, `stable` or `nightly`) is unchanged.
6. **Zen Toolbox and plug-ins** keep their own semantic versions. A plug-in states the IDE versions
   it works with against the calendar version, by year and month ("Zen IDE 26.10 and later"); the
   ABI version is what enforces it (docs/plugins.md).

## Channels, for the later ADR

What was learned while designing them, for the ADR that adds them:

- The channel belongs to the release, not the version: a semantic pre-release tag
  (`26.10.1-preview`) sorts below `26.10.1` and makes a later Stable of the same build look lower.
- Zen Toolbox up to 0.3.15 and IDEs up to 0.38.49, 0.2.25 and 0.1.15 offer only releases whose
  channel is exactly `stable`, and the manifest refuses unknown fields; listing a preview also on
  `stable` for them makes a version's channel ambiguous (which listing is the real one), and
  promoting a release to a more mature channel then needs its own rule.

## Consequences

- Users see one number for the family; a release is one command.
- An IDE without changes of its own still gets a release, and a download, at each release.
