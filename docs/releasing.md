# Releasing

> **Last updated:** 2026-11-28 · **Decisions:** [ADR-0020](adr/0020-release-server.md),
> [ADR-0022](adr/0022-toolbox-updates-itself.md), [ADR-0146](adr/0146-one-calendar-version-for-every-ide-and-release-channels.md),
> [ADR-0171](adr/0171-releases-from-a-protected-tag-through-gitlab.md), [ADR-0188](adr/0188-release-notes-and-decision-records-in-files-of-their-own.md),
> [ADR-0300](adr/0300-releases-from-github.md), [ADR-0301](adr/0301-releases-start-from-a-release-branch.md)

> **Releases are made by the maintainers.** This page documents their tooling: the release
> workflow runs on GitHub-hosted runners, with the signing key and the release server's token held
> as secrets of the environment `release`. A contributor never needs any of it: a change ends with
> its pull request.

> **A release is a push to the branch `release`, and the owner approves it twice** (ADR-0301).
> Every IDE is released together under one calendar version `YY.M.N` (ADR-0146), which nobody names
> by hand: it is the calendar's next unpublished version.
>
> ```sh
> git fetch origin && V=$(cargo xtask release-version)   # 26.11.11: this month, the next number
> # The notes first (ADR-0188): on a branch from main, then merge it.
> git switch -c release/notes-$V origin/main
> cargo xtask changelog gather $V            # add --toolbox <version> when Zen Toolbox is released too
> git commit -am "changelog: the notes of $V" && git push -u origin HEAD   # its pull request, merged
> git fetch origin && git switch --detach origin/main
> cargo xtask release-ci plan                # on the merged main: what it releases; refuses what a release would
> git push origin origin/main:refs/heads/release   # the release: `release` moves forward to that commit
> ```
>
> The workflow **Release** builds every IDE for macOS, Linux and Windows (and Zen Toolbox and the
> catalog's plug-ins when they have a new version), asks for the first approval, stages it all on the
> release server, checks every staged download and writes a draft GitHub release; it asks for the
> second approval, makes the release live, and only then creates the tag `v<YY.M.N>` on the released
> commit and publishes the GitHub release. A maintainer's machine stays the fallback (below).

## Releasing from the branch `release`

The branch `release` always points at a commit of `main` and only moves forward; no commit is ever
made on it. Moving it is the request for a release: the workflow **Release**
(`.github/workflows/release.yml`) runs on every push to it. A pushed tag starts nothing.

Move it only when the owner asks, to a commit of `main` after the pull request of
`cargo xtask changelog gather <YY.M.N>` is merged (step 2 of the checklist below). Only the owner
(an organisation admin) can push to it, and only forward (the branch ruleset, below). The plan
refuses, building nothing, a commit that is not on `origin/main`, notes still in
`changelog/unreleased/` (Zen Toolbox's count only when Toolbox is released), an IDE changelog without
the version's section, and a run whose ref or environment is not what a release needs. Fix the
cause on `main`, gather and merge, and move `release` again.

**What counts as released.** A version is released when the published manifest on the release
server lists it. `cargo xtask release-version`, `cargo xtask changelog gather` and the plan read
that, and nothing reads tags: the tag `v<YY.M.N>` is created by the last job, after the release is
live, so a failed attempt leaves no tag and nothing to delete. The same version stays the next one,
`gather` adds the fixes' notes to its section, and the retry releases a newer commit of `main` at
that version. Do not gather while a release runs: the version is fixed by its plan, and notes
gathered into it then would not be in it.

**Retrying.** If a job fails, nothing is live (until `live`) and there is no tag. Either re-run the
failed jobs of the run (Actions → the run → Re-run failed jobs), when the cause was the machine or
the network; or fix the cause on `main` and move `release` forward to the new commit, which starts
a new run. A run whose commit `release` no longer points at refuses to stage or go live, so an old
run cannot release over a newer one. Runs of the workflow wait for each other (one at a time).

### The jobs

The builds are the jobs of `.github/workflows/release-parts.yml`, which the nightly dry run (below)
calls too. They read no secret and name no environment.

| Job | Runner | Secrets | What it does |
|---|---|---|---|
| `plan` | Linux | none | `cargo xtask release-ci matrix`: what the release is (every IDE; Zen Toolbox and the catalog's plug-ins when their version is not published), written as the jobs below, and its version, fixed for the whole run; refuses, building nothing, notes that are not gathered, an IDE changelog without the version's section, a version that is not the calendar's next, a commit that is not on `origin/main`, a run that is not of the protected branch `release`, and an environment `release` that does not exist, has no required reviewers or does not allow the branch `release` (read with the job's token, `actions: read`) |
| `gate` | macOS | none | `cargo xtask release-ci gate`: the source check and the full quality gate (`cargo xtask ci`), once; its pass is `release-gate.json` with the commit, the tree and the compiler |
| `macos` (3 jobs) | macOS, Apple silicon | none | `cargo xtask release-ci part <ide> macos`: the application bundles, two IDEs in a job (the second reuses what the first compiled) |
| `cross` (12 jobs, 14 with Zen Toolbox) | Linux | none | `cargo xtask release-ci part <ide|toolbox> <linux|windows>`: one product for one platform, cross-built as on the Mac (ADR-0150) |
| `extras` (only when there is something) | macOS | none | Zen Toolbox for macOS (`part toolbox macos`) and the catalog's plug-ins with their driver programs for every platform (`part plugins all`) |
| `join` | Linux | none | `cargo xtask release-ci join`: takes every part only if its files match its `build.json`, its commit, tree and compiler are those of the gate's pass, and the parts are exactly the release (every IDE once for each platform, all or none); lays them out as one release (`release-build/`, with the website and `release-plan.json`) and keeps that as the workflow artefact `release` for 7 days |

A part is a release build with every check of one (the source check, the version, the notes) but
the gate, which it records as `deferred` in `build.json`; nothing publishes a build whose gate is
not `passed`, and only `join` turns `deferred` into `passed`, with the gate job's pass for the same
tree. That is how the gate runs once for twenty builds. The jobs are split so that the release
takes about as long as its slowest job: a clean macOS build of one IDE took 61 minutes on a hosted
runner (2026-11-26) and a Linux or Windows one 17 to 18, and at most five macOS jobs run at a time
there, which is why the six bundles are three jobs, beside the gate and `extras`. The gate took
about 65 minutes on a hosted Mac (2026-11-27).

No release build restores a cache: every part and the gate compile from nothing. `plan` and `join`,
which build only `xtask`, restore the cache the workflows of `main` save and never save one; a
cache a pull request's run wrote cannot reach them, because GitHub keeps a cache within the ref
that wrote it and the default branch, and no workflow here saves a cache from a pull request
(`ci.yml`).

The version's month is the month in UTC on GitHub (`TZ: UTC`), fixed when the plan runs.

**What stops a release before it is built.** The plan checks everything a release needs that a job
without the secrets can see (the row of `plan` above), so a push that cannot be released fails in a
few minutes instead of after the builds. The secrets themselves cannot be checked there: GitHub
gives an environment's secrets only to a job that names the environment, which asks the reviewers
first, and the job's token cannot list them. A third approval at the start was not wanted, so the
first step of `stage` and of `live` checks that `ZEN_PUBLISHER_KEY`, `ZEN_RELEASES_TOKEN` and
`ZEN_RELEASES_SERVER` are set, before anything is compiled, signed or sent, and prints only which
are missing. To know beforehand, run **Release credentials** (below) before moving `release`.

### The nightly dry run

Every night at 03:17 UTC, and by hand (Actions → **Release dry run** → Run workflow), the workflow
**Release dry run** (`.github/workflows/release-dry-run.yml`) runs the builds of a release on the
latest commit of `main`, as a release of it would be made now: the plan, every build of every
product for every platform, and the join, with the version the calendar gives
(`cargo xtask release-version`). So a release meets no problem of its builds for the first time on
release day.

A dry run is not a release, and differs from one only here:

- **The notes.** It tolerates notes still waiting in `changelog/unreleased/` and a changelog without
  the version's section (the usual state between releases): the plan says what a release would
  refuse ("A release of this commit would be refused now: ..."), and a build without its section
  carries the notes of a trial build.
- **The gate.** The full gate is not run: the workflow **Tests** runs every test on a hosted Mac
  each night (01:00 UTC) and **CI** runs the gate without tests on Linux after every merge, so a dry
  run runs only the gate without its tests on macOS (`cargo xtask ci --skip test`), for what is
  checked on macOS alone (Clippy of the macOS code). Every build records the gate as `skipped`, as a
  trial build does; `join --dry-run` takes only such parts, checks them as a release's join does
  (each file against its `build.json`; every part of the commit and compiler of the join's own
  checkout; exactly the release) and leaves them `skipped`. Nothing publishes a build whose gate is
  not `passed`, and `release-ci stage`, `verify`, `notes`, `live`, `build` and `gate` refuse
  `--dry-run`.
- **Nothing outside the run.** It names no environment, reads no secret, and its token can only
  read; it signs nothing and sends nothing. It keeps the parts as workflow artefacts for one day
  (the join needs them) and the build records (`dry-run-records`: `release-plan.json` and every
  `build.json`) for seven; the joined release itself is not kept, since nothing may use it.

It costs about what the builds of a release cost (three macOS jobs, an `extras` job when Zen
Toolbox or a plug-in has a new version, the gate without tests on macOS, and twelve or fourteen
Linux jobs), on hosted minutes that are free for a public repository. It starts two hours after
**Tests**, so the two do not compete for the five macOS runners that run at once.

**A failure** makes the run red: Actions → **Release dry run** → the run → the failed job's log.
GitHub mails the failure to the person who last changed the workflow's `cron` line, and shows it on
the repository's Actions page. A newer run cancels a dry run still going (one at a time). GitHub
turns scheduled workflows of a public repository off after 60 days without activity in it.

```sh
cargo xtask release-ci plan --dry-run          # on any checkout: what a dry run would build now
```

The steps run as in the workflow: `release-ci matrix --dry-run`, `release-ci part <ide|toolbox|plugins>
<platform> [v<YY.M.N>] --dry-run` and `release-ci join [v<YY.M.N>] --dry-run` (without a version,
the calendar's next).

### After `join`: stage, verify, release

| Job | Runner | Secrets | Token | What it does |
|---|---|---|---|---|
| `stage` | Linux | the environment `release` | read | After the first approval: checks that the secrets are set (above); compiles `xtask` and `zen-releases` without the secrets; then one step with them runs `cargo xtask release-ci stage v<version>`: refuses a build of another commit than the run's, a run whose commit `release` no longer points at, and a version published already; uploads every file (stored, listed nowhere), signs and stages the manifest, the catalog, the changelogs and the website under `v<version>-p<run id>`. Its record is the artefact `stage` |
| `verify` | macOS | none | read | `cargo xtask release-ci verify`: every staged file downloaded from its public address and compared with the build, `codesign --verify` for each macOS application, the staged documents and website. Its checks are the artefact `verify` |
| `draft` | Linux | none | `contents: write` | `cargo xtask release-ci notes` writes the release's text (each product's notes, a link to every file with its SHA-256, what was checked); `gh release create --draft --target <commit>` makes the draft release `v<version>` for the released commit, or rewrites the draft an earlier attempt made. A draft creates no tag |
| `live` | Linux | the environment `release` | read | After the second approval, once the secrets are checked as in `stage`: `cargo xtask release-ci live`, which refuses as `stage` does when `release` has moved, then makes the stage live in one request |
| `publish` | Linux | none | `contents: write` | Creates the tag `v<version>` on the released commit (or finds it there already; a tag on another commit stops it), then publishes the draft as the latest release |

So the button is the approval of `live`: Actions → the run → **Review deployments** → `release` →
Approve. Before approving, read the draft release (Releases → the draft named `Zen IDE <version>`),
open the staged website (the `stage` job prints its address) and look at the `verify` job. The
same environment guards `stage`, so the run asks for an approval twice: once before the secrets are
used at all, once before the release is live. Rejecting the first approval stops the release with
nothing sent; the run's artefact `release` can still be published from a maintainer's machine
(below). No job holds both a release secret and a token that can write to the repository.

If `publish` fails after `live` (the tag ruleset refused the tag, for example), the release is live
and only the tag and the GitHub release are missing: fix the cause and re-run `publish`, or have an
organisation admin create the tag on the released commit
(`git push origin <commit>:refs/tags/v<version>`) and re-run it.

`cargo xtask` checks again what the repository's settings already enforce (`xtask/src/ci_host.rs`):
the secrets are refused unless the run is of the protected branch `release` (`GITHUB_REF` is
`refs/heads/release`, `GITHUB_REF_TYPE` is `branch`, `GITHUB_REF_NAME` is `release`, and
`GITHUB_REF_PROTECTED` is `true`), never in a pull request's run, on another branch or on a tag;
every step of a release refuses a run that is not of `release`; `stage` and `live` release only the
run's own commit (`GITHUB_SHA`), and only while `release` points at it. A CI that is neither GitHub
Actions nor a maintainer's machine (a `CI` variable without `GITHUB_ACTIONS`, or with `GITLAB_CI`
too) is refused the secrets and the release altogether; a maintainer's machine, where no CI
variable is set, works as before.

### The repository's settings

The owner keeps these, under Settings:

- **The branch ruleset `release`** (Rules → Rulesets → New branch ruleset): target the branch
  `release`; rules **Restrict creations**, **Restrict updates**, **Restrict deletions** and **Block
  force pushes**; bypass list: Organization admin, always. So only the owner moves `release`, only
  forward, and GitHub reports the branch as protected (`GITHUB_REF_PROTECTED`).
- **The environment `release`** (Environments → `release`): the required reviewer; deployment
  branches and tags: the branch `release` only; the secrets `ZEN_PUBLISHER_KEY` and
  `ZEN_RELEASES_TOKEN`, and the variable `ZEN_RELEASES_SERVER` (`https://ide.ifzenelse.space`).
- **The tag ruleset "release tags"** (`refs/tags/v*`): **Restrict updates**, **Restrict deletions**
  and **Block force pushes**, bypass Organization admin; not **Restrict creations**, so that the
  `publish` job can create the tag with the workflow's token (ADR-0301, decision 10). A release tag
  can then be created by anyone with write access, but never moved or deleted, and it starts
  nothing; `publish` refuses a tag that is not on the released commit.
- **The variable `ZEN_RELEASE_FROM_GITHUB`** (Secrets and variables → Actions → Variables) is not
  read any more; delete it.

**Check the secrets without releasing:** Actions → **Release credentials** → Run workflow, from the
branch `release` (`.github/workflows/release-credentials.yml`). After the approval it runs
`cargo xtask release-ci credentials`: `zen-releases check-credentials` signs an empty manifest with
the key and has the server check it with the token (`PUT /api/v1/check/manifest`, which stores
nothing), and prints only `ok`, or which of the two the server refused. Run it after the secrets
change, and before a release when in doubt.

**GitHub builds, a maintainer's machine publishes** (the fallback):

```sh
cargo xtask release-publish --run <run id> [--dry-run] [--stage <name>]
```

It downloads the run's artefact `release` with `gh run download` (the GitHub CLI, signed in; into
`<target>/release-fetch/run-<id>/`), and publishes each group of `release-plan.json` as
`release-publish --from` does: every file checked against `build.json`, the commit on `origin/main`
with the recorded tree, the gate passed, every IDE for every platform or none. `--from <dir>` takes
such a folder too. The website is not part of this path; it goes live with a staged release or
with a deployment of the release server. Neither creates a tag: create it on the released commit by
hand (`git push origin <commit>:refs/tags/v<version>`) after the release is live.

## On a maintainer's machine (the fallback)

```sh
V=$(cargo xtask release-version)
cargo xtask release ides $V                  # build, sign and publish every IDE at once
```

`release-publish --stage <name>` stages instead of publishing, and `zen-releases release-stage
--stage <name>` makes such a stage live, as the workflow does. Zen Toolbox and plug-ins keep
their own versions (`cargo xtask release-toolbox`, `cargo xtask release plugin <id> <version>`; a
plug-in's first catalog release is always made this way).

Every release command first checks that it runs on `main`, the branch `release` or a detached
checkout of a commit of `main` (as in CI), with no uncommitted changes and a commit that is already
on `origin/main` (`ZEN_RELEASE_ANY_BRANCH=1` allows another branch in an emergency), then runs the
full quality gate (`cargo xtask ci`), and refuses to release if either fails. A tree that already
passed the gate with the same toolchain is not run again
([ADR-0141](adr/0141-the-gate-remembers-a-pass-by-its-tree.md)). On a maintainer's machine the
secrets are `~/.config/zen-ide/publisher.key` and `releases.env`; they are never copied anywhere but
into the environment `release` above.

## Two steps: build, then publish on a maintainer's machine

A release is a **build**, which needs no secret, and a **publish**, which needs the signing key
(`~/.config/zen-ide/publisher.key`) and the release server's token (`releases.env`), both only on
a maintainer's machine ([ADR-0142](adr/0142-release-builds-on-the-runner-published-on-the-release-machine.md)).

**Build** (on a maintainer's machine; the workflow builds the same in parts):

```sh
cargo xtask release-build ides 26.10.1 [--allow-version] [--out <dir>]   # every IDE
cargo xtask release-build toolbox 0.3.16 [--out <dir>]
cargo xtask release-build plugin json 0.1.2 [--out <dir>]
cargo xtask release-build plugin duckdb 0.1.0 --platform all [--out <dir>]   # with its driver programs
```

It checks the source as a release does (a commit of `main` on `origin/main`, nothing uncommitted;
a detached checkout of such a commit is fine), runs the gate unless the tree already passed it
([ADR-0141](adr/0141-the-gate-remembers-a-pass-by-its-tree.md)), builds, and writes into
`<dir>/<product>-<version>/` (`<target>/release-build/<product>-<version>/` without `--out`; the
IDEs into `ides-<version>/<id>-<version>/`, one folder each):

| Release | Files |
|---|---|
| IDE | `<id>-<version>-macos-<arch>.tar.gz` (the application with its bundled plug-ins and zen-cli) |
| Zen Toolbox | `zen-toolbox-<version>-macos-<arch>.tar.gz`, `Zen-Toolbox-<version>-macos-<arch>.dmg` and its `.sha256` |
| Plug-in | `zen-plugin-<id>-<version>.wasm`, and `zen-db-<driver>-<version>-<platform>` (`.exe` on Windows) for each database driver program |

and in each, `changelog.json` (the website's changelogs) and `build.json`: the kind, product, name,
version, release notes, commit and tree, `rustc -V`, platform, host, the gate's result, what a
plug-in's catalog entry says, and every file's role, size and SHA-256.

A plug-in that declares database drivers with a crate in `drivers/<driver>` gets their programs
built too ([ADR-0191](adr/0191-driver-programs-published-with-their-plugins.md)), one per platform
`--platform` asks for (`macos`, this Mac's, by default; `release plugin` builds only that one, and the
release workflow builds `all`). A macOS program is signed ad hoc and checked with `codesign --verify`.
The build writes each program's address (`artifacts/plugin-<id>/<version>/<file>`, relative to the
catalog), SHA-256 and size into the catalog entry it publishes; publishing checks that the two agree,
uploads the programs, then signs the catalog, and the release server refuses a catalog whose
programs it does not store as listed. `--platform` changes nothing for a plug-in without them.

**A trial build** proves that a product builds and packs on a machine, and is never published:

```sh
cargo xtask release-build gogarden 0.0.0 --trial [--platform macos|linux|windows|all] [--out <dir>]
```

It builds one product (an IDE or `toolbox`) with the same build, bundling, licence notices and
packing as a release, and skips what makes a build a release: the source check (a commit of `main`
on `origin/main`), the quality gate, the version check against the published releases and the
release notes of the changelog (the notes are "A trial build: not a release."). `build.json`
records the gate as `skipped`, and `release-publish` refuses a build whose gate did not pass, so a
trial build cannot be signed or sent. The workflow **Release build (trial)**
(`.github/workflows/release-build.yml`, started by hand: Actions → Release build (trial) → Run
workflow, with the product and a version) runs it on GitHub-hosted runners for macOS on Apple
silicon, Linux and Windows (the last two on a Linux runner, cross-built as on the Mac), with no
secret, and keeps the folders as workflow artefacts for three days.

**Publish** (only on a maintainer's machine):

```sh
cargo xtask release-publish --run <GitHub run id> [--dry-run]   # or --from <dir>
```

It downloads the run's artefact `release` with `gh` (into `<target>/release-fetch/`), or takes a
folder of builds (`--from`), finds every build in it (the IDEs' release has one per IDE: all are verified before any is
published, and they must be every IDE once, at one calendar version and commit; one IDE's build alone
is refused), checks every file's size and
SHA-256 against `build.json`, that the commit is on `origin/main`
(fetched now) with the recorded tree and that the gate passed, builds `zen-releases` from this
checkout (the program that reads the key is never one built elsewhere), then publishes: the
update archive (upload, then the manifest signed with the key) or the plug-in (the signed
catalog), the installer and its checksum, and the changelogs. `--dry-run` does all the checks and
lists what it would sign and send, sending nothing. Then check the release as below.

## Versions

The IDEs share one **calendar version**, `YY.M.N` ([ADR-0146](adr/0146-one-calendar-version-for-every-ide-and-release-channels.md)):
the year's last two digits, the month without a leading zero, and the release's number within the
month from 1 (`26.10.1`, `26.10.2`, `26.11.1`, `27.1.1`). Every IDE is released at every IDE release,
even one without changes of its own. `cargo xtask release-version` prints the next version (`YY.M` of
today, in UTC, and the month's highest published number plus one, read from the public manifest or
`ZEN_RELEASES`). "Today" is in local time on a maintainer's machine and in UTC in the release
workflow (`TZ` sets another). `release ides` and `release-build ides` refuse, building nothing, any other
version: one not of this month (a typo such as `27.10.1`, or `26.12.1` in October), or one that
skips a number. `--allow-version` builds another on purpose (never in the release workflow); it
must still be a calendar version greater than every IDE version published.

IDE releases are published on the `stable` channel, as before; release channels (Preview, Beta,
Release Candidate, Stable) are deferred to a later ADR. Every release is a preview for now, and the
version is shown alone.

If publishing stops midway (the network, the server), `release-publish` says which IDEs were
published already: run the same command again, and it publishes them all, the published ones once
more at the same version, which is allowed. The IDEs' versions never stay apart.

Zen Toolbox keeps its own semantic version (0.3.x), released on `stable`; each plug-in keeps its own
too, and states the IDE versions it works with against the calendar version (`26.10` and later; see
[plugins.md](plugins.md)). A release build gets its version from `ZEN_BUILD_VERSION`; a build without
it is a development build, which never offers or performs a self-update.

## Release notes

Every release carries notes in Markdown, which Zen Toolbox shows as "What's new" and the website
lists under "What's new" (the subset is described in [release-manifest.md](release-manifest.md)).
They are written in English, for users, by every pull request as a file of its own in
[changelog/unreleased/](../changelog/unreleased/README.md) (ADR-0188); before releasing,
`cargo xtask changelog gather <YY.M.N> [--toolbox <version>]` moves them to the top of the
product's file in [changelog/](../changelog/README.md) (`changelog/gogarden.md`,
`changelog/toolbox.md`; a plug-in's are written in `changelog/plugins/<id>.md` with its version),
as a section `## <version> - <YYYY-MM-DD>`, and the repository's into `CHANGELOG.md`; an IDE release as the same section
`## <YY.M.N> - <YYYY-MM-DD>` in each IDE's file (`cargo xtask changelog` checks that the
IDEs' files list the same calendar versions; each has its own notes). The format and its checks
are described in [changelog/README.md](../changelog/README.md). After publishing, every release also uploads all
changelogs for the website (`/releases/changelog.json`, shown at `/changelog.html`).

- The release commands read the section when no notes are given, and refuse to release when it is
  missing or empty; plug-in releases need their section too. Notes given as the last argument take
  precedence.

## Zen Toolbox (macOS)

```sh
cargo xtask release-toolbox 0.1.1        # notes from `## 0.1.1` in changelog/toolbox.md
cargo xtask release-toolbox 0.1.1 "- One line of Markdown"
```

This builds the release binary, assembles `Zen Toolbox.app`, publishes the update artifact (signed
with `~/.config/zen-ide/publisher.key`) to the release server, and builds and uploads the installer
`Zen-Toolbox-<version>-macos-<arch>.dmg`. Installed copies offer the update within 30 minutes, or at
once from the settings menu (Check for updates).

The installer is not notarised: there is no Apple Developer ID yet, so a `.dmg` downloaded in a
browser is blocked by Gatekeeper on first launch, and it is not in the signed manifest, so only HTTPS
covers it and its checksum file. The recommended installation is therefore from the terminal, which
downloads without the quarantine flag, verifies the manifest's signature and installs the archive it
lists, checked by its SHA-256:

```sh
curl -fsSL https://ide.ifzenelse.space/install.sh | sh
```

The script lives in `deploy/releases/site/install.sh`, and a change to it reaches people with the
next deployment of the release server or the next live website. On Linux the same script installs the
`linux-x86_64` archive from the signed manifest for the user (`~/.local/share/zen-toolbox/app`, a
link in `~/.local/bin`, a desktop entry), checking its SHA-256 from the manifest. Once a Developer ID exists, the release
workflow signs with the hardened runtime and notarises the `.dmg`.

## IDEs (macOS)

```sh
cargo xtask release ides 26.10.1        # notes from `## 26.10.1` in each changelog/<ide>.md
```

This builds every IDE (all or none) in release mode with `ZEN_BUILD_VERSION`, then verifies and publishes them all. For each it packs `bin/<product>` into a `.tar.gz`
and publishes it, signed, to the release server; the manifest records the archive's size, which Zen
Toolbox shows as the download size ("Update to 0.38.7 · 41 MB"). Zen Toolbox then lists the product
under Available, installs it into its tools directory and launches `bin/<product>`. The bundle
carries zen-cli too, built in the same run and copied to `Contents/MacOS/zen`, which Zen Toolbox's
`zen` shell script starts (ADR-0086). `cargo xtask release toolbox <version>`
is the same as `release-toolbox`.

Every application, archive, package, plug-in module and driver program carries its licence notices
(`Contents/Resources/Licenses/` in a bundle, `Licenses/` next to `bin/` elsewhere, a `zen-licences`
section in a module, `--licences` of a driver program); the build refuses an artefact without them
([docs/licences.md](licences.md), ADR-0282).

To stop offering a product (or one version), withdraw it from the signed manifest; installed copies
are not touched:

```sh
set -a; . ~/.config/zen-ide/releases.env; set +a
target/release/zen-releases withdraw --key ~/.config/zen-ide/publisher.key --product rustronin [--version 0.1.1]
```

## Linux and Windows builds

Linux and Windows builds are made on request, not with every release (the owner, 2026-10-18), for a
version released for macOS, or with it. Since 2026-10-25 they are cross-built natively on a Mac,
the CI runner's or a maintainer's, without Docker
([ADR-0150](adr/0150-linux-and-windows-builds-on-the-mac-runner.md)):

```sh
cargo xtask release-build ides 26.10.1 --platform linux     # or windows, linux,windows, all
cargo xtask release-build toolbox 0.3.15 --platform windows
cargo xtask release-publish --from <out>                    # on a maintainer's machine, as for macOS
```

The release workflow builds every platform with every release (above); these commands are for a
maintainer's machine.

- **Linux x86_64:** `cargo zigbuild --target x86_64-unknown-linux-gnu.2.35`: glibc 2.35, so the
  binaries run on Ubuntu 22.04 and later; X11, xkbcommon and Wayland are loaded from the desktop.
  The build is `bin/<executable>` (with the bundled plug-ins in `Resources/plugins` for an IDE),
  published as the `linux-x86_64` platform of the version, and a Debian package,
  `<package>_<version>_amd64.deb` (`zen-toolbox`, `gogarden`...), built in xtask without dpkg: the
  files in `/opt/<package>`, the command in `/usr/bin`, a desktop entry and a 256 px icon, all
  owned by root.
- **Windows x86_64:** `cargo build --target x86_64-pc-windows-gnu` with MinGW-w64's linker: the
  release as Zen Toolbox installs it (`bin/<product>.exe`, the plug-ins in `Resources/plugins`),
  published as the `windows-x86_64` platform, and `<Name>-<version>-windows-x86_64.zip` for the
  website's Windows tab.
- Each platform's build is a folder of its own with `build.json` (its `platform`), beside the macOS
  one (`<id>-<version>-linux`). `release-publish` verifies every build first (files, commit, tree,
  gate), publishes macOS first, and keeps the IDEs all or nothing: each platform in the set has every
  IDE once, and a Linux or Windows build of a version not yet published for macOS is refused unless
  its macOS build is published with it. The `.deb` and `.zip` are uploaded for download.
- The tools are listed in [development.md](development.md) (the runner's machine). A clean Linux
  build of an IDE takes about 6 minutes on the release runner, a Windows one about 7.
- **Fallback:** `cargo xtask release-linux <product> <version>` and `release-windows` still build in
  Docker (`deploy/linux/Dockerfile`, `deploy/windows/Dockerfile`; emulated on Apple Silicon, so slow)
  and publish at once, for a Mac without the native tools.

The packages are built and checked on macOS and in a clean Ubuntu container (the checklist below),
not yet run on a real Ubuntu desktop or on Windows, and not signed.

## Releasing: the checklist

Every release is for macOS and is checked before it is announced; Linux and Windows builds are
made only when asked for (the owner, 2026-10-18). Every step runs from the repository root on the
Mac; Docker is needed only for Linux and Windows.

1. **Merge first.** Merge every open pull request that is ready (`gh pr list`); each passes the
   required checks. Then `git fetch origin`: the version and the notes are taken from the current
   `origin/main`, since other people release too.
2. **Release notes.** On a branch from the current `main`, run
   `cargo xtask changelog gather <YY.M.N>` with `cargo xtask release-version` as the version (and
   `--toolbox <version>` when Zen Toolbox is released too): it moves the notes of
   `changelog/unreleased/` into the section `## <YY.M.N> - <date>` of every IDE's file, Zen
   Toolbox's and `CHANGELOG.md`, and removes the files it emptied. A section of the version that
   is there already and not released (written by hand, or gathered for an attempt that failed)
   takes the gathered notes after its own; a released version (one the published manifest lists)
   is refused. Read the sections, edit them for users where needed, and merge that one pull request.
   Notes merged after it make the plan refuse the release: run the gather again on a new branch,
   which adds them to the same section, and merge it.
3. **Check, then release.** On a detached checkout of the merged `origin/main`,
   `cargo xtask release-ci plan` must print what it releases and no refusal. Then move `release`
   to that commit, `git push origin origin/main:refs/heads/release` (the first time this creates
   the branch), and approve the run twice ("Releasing from the branch `release`", above): every IDE
   on every platform, and Zen Toolbox and the catalog's plug-ins when they have a new version. The
   fallback on a maintainer's machine: `cargo xtask release ides <YY.M.N>` (the gate, every bundle,
   the publish), or the run's artefact with `cargo xtask release-publish --run <id>` (two steps,
   above); then the tag by hand.
4. **Zen Toolbox** (the release workflow releases it when its changelog has a new version; by
   hand): `cargo xtask release-toolbox <version>` (the `.dmg`, the update archive).
5. **Check the macOS releases, every time.** Download what was published, start it with a
   throwaway data folder, and look at the screenshot: the window is drawn, the version is the new
   one, and the change of the release is there.

   ```sh
   V=<version>; W=$(mktemp -d)
   curl -fsSL https://ide.ifzenelse.space/releases/artifacts/gogarden/$V/gogarden-$V-macos-aarch64.tar.gz | tar -xz -C $W
   mkdir -p $W/proj && printf 'package main\n\nfunc main() {}\n' > $W/proj/main.go
   ZEN_DATA_DIR=$W/data ZEN_PLUGINS_CATALOG=none ZEN_RELEASES=none ZEN_UI_SCREENSHOT=$W/gogarden.png \
     ZEN_UI_SCREENSHOT_DELAY=6 "$W/GoGarden.app/Contents/MacOS/gogarden" $W/proj/main.go
   ```

   For Zen Toolbox, the same with the `.dmg` (`hdiutil attach -nobrowse -readonly`) and
   `Zen Toolbox.app/Contents/MacOS/zen-toolbox`: it must list the new IDE version.
6. **Linux and Windows:** the release workflow builds them with every release; by hand,
   `cargo xtask release-build ... --platform linux` or `windows`, then
   `cargo xtask release-publish --from <out>` (above); the Docker commands `release-linux` and
   `release-windows` are the fallback. Then check the Linux packages on a clean
   Ubuntu (they cannot be run on the Mac): install the published `.deb` with `apt` and start it
   under a virtual display.

   ```sh
   mkdir -p /tmp/zen-linux-check && docker run --rm --platform linux/amd64 -v /tmp/zen-linux-check:/out ubuntu:24.04 sh -c "
     apt-get update -qq && apt-get install -y -qq curl ca-certificates xvfb xauth libgl1 libegl1 \
       libgl1-mesa-dri libxkbcommon-x11-0 libxcursor1 libxi6 libxrandr2 >/dev/null 2>&1
     curl -fsSLO https://ide.ifzenelse.space/releases/artifacts/gogarden/<version>/gogarden_<version>_amd64.deb
     apt-get install -y -qq ./gogarden_<version>_amd64.deb
     mkdir /proj && printf 'package main\n\nfunc main() {}\n' > /proj/main.go
     ZEN_DATA_DIR=/tmp/zd ZEN_PLUGINS_CATALOG=none ZEN_RELEASES=none ZEN_UI_SCREENSHOT=/out/gogarden.png \
       ZEN_UI_SCREENSHOT_DELAY=8 timeout 120 xvfb-run -s '-screen 0 1600x1000x24' gogarden /proj/main.go"
   ```

   The same for `zen-toolbox_<version>_amd64.deb` with `zen-toolbox` (without a D-Bus session it
   says "no tray icon" and opens as a normal window, which is expected in a container).
7. **Check the manifest:** the new versions and their platforms are listed:

   ```sh
   curl -s https://ide.ifzenelse.space/releases/manifest.json | python3 -c "
   import json,sys
   for p in json.load(sys.stdin)['products']:
       r=max(p['releases'], key=lambda r: [int(x) for x in r['version'].split('.')])
       print(p['id'], r['version'], sorted(a['platform'] for a in r['artifacts']))"
   ```

8. **The website:** a release from the workflow brings the website and the guide of its commit,
   live with the second approval; look at `https://ide.ifzenelse.space/`. Deploying the release server
   brings its own code, and the website for a release made by hand.

If publishing fails on the network before anything is sent (for example "cannot fetch the
manifest: failed to lookup address"), check `curl https://ide.ifzenelse.space/api/v1/health` and run
the same command again; the builds are cached, so the second run is quick. If it failed later, look
at the manifest (step 6) first to see which platforms are already published.

## Release server

Deployment, secrets and the domain are described in [deploy/releases/README.md](../deploy/releases/README.md).

**The server first when a format changes.** When the manifest or the plug-in catalog gains a field
(a plug-in's archive platforms, a notice…), the server must be deployed before anything that uses it
is published. `zen-releases` checks that for you: before uploading any artifact or module it asks the
server to read the new manifest or catalog (`PUT /api/v1/check/…`), and a server that cannot (older,
or without the check at all) refuses with "deploy the release server first
(deploy/releases/README.md)", with nothing published. Deploy the release server, then publish
again. (Publishing the Java and Kotlin plug-ins on 2026-10-26 met an older server only at the
catalog's upload, after the modules; this check moves that before them.) The first publish after
this check landed needs one deploy, as the server had no check yet.
The production server is `https://ide.ifzenelse.space` ([ADR-0023](adr/0023-ide-subdomain.md)); the
clients and `zen-releases` use HTTPS ([ADR-0040](adr/0040-https-pure-rust-tls.md)), so
`ZEN_RELEASES_SERVER` in `releases.env` is `https://ide.ifzenelse.space`. Toolbox 0.1.0 uses the IP
address, which stays served over plain HTTP.
