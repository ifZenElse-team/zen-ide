# Releasing

> **Last updated:** 2026-11-26 · **Decisions:** [ADR-0020](adr/0020-release-server.md),
> [ADR-0022](adr/0022-toolbox-updates-itself.md), [ADR-0146](adr/0146-one-calendar-version-for-every-ide-and-release-channels.md),
> [ADR-0171](adr/0171-releases-from-a-protected-tag-through-gitlab.md), [ADR-0188](adr/0188-release-notes-and-decision-records-in-files-of-their-own.md)

> **Releases are made by the maintainers.** This page documents their tooling: the release
> pipeline runs on the maintainers' CI and release runner, with the signing key and the release
> server's token held as protected CI variables. A contributor never needs any of it: a change
> ends with its pull request.

> **A release is a protected tag, and the owner presses the button** (the owner, 2026-10-27,
> ADR-0171). Every IDE is released together under one calendar version `YY.M.N` (ADR-0146):
>
> ```sh
> V=$(cargo xtask release-version)          # 26.10.4: this month, the next number
> # The notes first (ADR-0188): on a branch from main, then merge it before the tag.
> git switch -c release/notes-$V origin/main
> cargo xtask changelog gather $V            # add --toolbox <version> when Zen Toolbox is released too
> git commit -am "changelog: the notes of $V" && git push -u origin HEAD   # its pull request, merged
> cargo xtask release-ci plan v$V            # on the merged main: what the tag builds; refuses notes left
> git tag v$V origin/main && git push origin v$V   # or GitLab → Code → Tags → New tag
> ```
>
> The tag's pipeline refuses, building nothing, a commit whose `changelog/unreleased/` still holds
> notes for the release (Zen Toolbox's count only when the tag releases it) or whose IDE changelogs
> have no section of the version: gather, merge, and tag the merged commit.
>
> The tag's pipeline builds every IDE for macOS, Linux and Windows (and Zen Toolbox and the
> catalog's plug-ins when they have a new version), stages it all on the release server, checks
> every staged download, and stops at the manual job **release**. Look at the staged website and the
> checks, then press **release** (Build → Pipelines → the tag's pipeline): in a few seconds the new
> version is in the manifest, the website shows it, and the GitLab release lists its notes and files. A maintainer's machine stays the fallback
> (below).

## Releasing from a tag

Create the tag only when the owner asks, on a commit of `main` whose pipeline passed, after the
pull request of `cargo xtask changelog gather <YY.M.N>` is merged (step 2 of the checklist below).
`release: build` checks it first: a tag whose commit still has notes in `changelog/unreleased/` for
the release, or an IDE changelog without the version's section, is refused before anything is
built; delete the tag, gather and merge, and tag again. Tags `v*` are
protected: only maintainers can create one, and only a protected tag's pipeline is given the release
variables.

| Job | Runner | Secrets | What it does |
|---|---|---|---|
| `release: build` | the release runner | none | `cargo xtask release-ci build`: refuses notes not gathered (above), then the gate, every IDE for every platform, Zen Toolbox and the catalog's plug-ins with a version not yet published, the website (`deploy/releases/site` and the guide). The build (about 1.2 GB, over GitLab's 1 GB limit for artifacts) stays on the runner in `~/ci/release/<tag>` (the last three tags are kept); the job's artifact is its SHA-256 list, and `release: stage` and `release: verify` check every file against it before using it |
| `release: stage` | the release runner | the `release` environment | `cargo xtask release-ci stage`: uploads the artifacts (stored, listed nowhere), three at a time and only those the server does not have with the same SHA-256, so a retried stage sends nothing again. A file whose upload breaks (broken pipe, a reset connection, a timeout, or a 502–504 answer) is tried again up to three times, after 5, 10 and 15 s, and each try first asks the server whether it already has the file with the same SHA-256 (the connection may have dropped after the server kept it); every upload has time limits (30 s to connect; for the body, 10 minutes plus the file's size at 16 KiB/s, which a slow but steady line meets; 5 minutes for the answer, plus the time to pass on what the connection may still have buffered, up to 16 MiB at the least rate), so a stalled connection is tried again instead of holding the job for hours. Files over 20 MB go one at a time, smaller ones as many at a time as `--jobs` says. Two variables of the job's environment change this: `ZEN_RELEASES_UPLOAD_PARALLEL` (how many small files at a time) and `ZEN_RELEASES_MIN_RATE` (the least body rate, in KiB/s). Then the stage the signed manifest and catalog, the changelogs and the website under `v<version>-p<pipeline>`. The Air sends about 1.5 MB/s to the server (measured 2026-10-27), so the 700 MB take about ten minutes |
| `release: verify` | the release runner | none | `cargo xtask release-ci verify`: downloads every staged file from its public address and checks its size and SHA-256 against the build, each macOS application with `codesign --verify --deep --strict`, each Debian package in Ubuntu when Docker is there (the release runner has none: the job says so; check one by hand after the release, step 6), and the staged manifest, catalog, changelogs and website |
| `release` (manual) | the release runner | the `release` environment | `cargo xtask release-ci live`: makes the stage live in one request to the server, then creates the GitLab release of the tag: each product's notes, what was checked, a link to every file |

`cargo xtask release-ci plan v<version>` prints what a tag would build, building and sending
nothing. The staged website is at `https://ide.ifzenelse.space/releases/staged-site/<stage>/index.html`
(the stage job prints its address). Pressing **release** twice changes nothing the second time.

The live switch is refused, with nothing changed, when the live manifest or plug-in catalog changed
after the stage was made (a plug-in published from a maintainer's machine meanwhile): it would drop that
release. Run the tag's pipeline again (Build → Pipelines → Run pipeline on the tag, or retry from
`release: build`): it stages afresh under the new pipeline's name. If a job fails before the
button, nothing is live; fix the cause on `main`, delete the tag, and tag again.

**The release variables** (GitLab → Settings → CI/CD → Variables), all **protected**:

| Variable | Value | Masked and hidden |
|---|---|---|
| `ZEN_PUBLISHER_KEY` | the signing key: the 64 hex digits of `~/.config/zen-ide/publisher.key`, one line | yes |
| `ZEN_RELEASES_TOKEN` | the release server's upload token, as `ZEN_RELEASES_TOKEN=` in `releases.env` | yes |
| `ZEN_RELEASES_SERVER` | `https://ide.ifzenelse.space` | no |

Set their **environment scope** to `release`: GitLab then gives them only to the two jobs that name
that environment (stage and release, and the credentials check), never to the build and verify jobs,
and never to a pull request's or a branch's pipeline. `zen-releases` reads the key from
`ZEN_PUBLISHER_KEY` (no file is written) and never prints it; `cargo xtask` refuses the release
secrets in any GitLab job whose ref is not protected. The jobs run on the release runner as the same macOS user
as every pull request's job: a job running at the same moment could read the release jobs'
environment. The owner accepted that risk (ADR-0171); the release jobs share
`resource_group: release`.

**Check the variables without releasing:** Build → Pipelines → Run pipeline on `main` with
`CHECK_RELEASE_CREDENTIALS=1` (or `glab ci run -b main --variables CHECK_RELEASE_CREDENTIALS:1`).
The job `release: check credentials` signs an empty manifest with the key and has the server check
it with the token (`PUT /api/v1/check/manifest`, which stores nothing), and prints only `ok`, or
which of the two the server refused. On a maintainer's machine: `zen-releases check-credentials --key
~/.config/zen-ide/publisher.key` with `ZEN_RELEASES_TOKEN` and `ZEN_RELEASES_SERVER` set.

**Once, before the first tag release:** deploy the release server ([deploy/releases](../deploy/releases/README.md)): it brings the stage endpoints and Caddy's live website
(`site/current` in the server's data folder; until a release makes one live, Caddy serves the
website deployed with the server, as before). A stage job against an older server stops with
"deploy it first", having made nothing live.

## On a maintainer's machine (the fallback)

```sh
V=$(cargo xtask release-version)
cargo xtask release ides $V                  # build, sign and publish every IDE at once
# or: glab ci run -b main --variables VERSION:$V, then
cargo xtask release-publish --pipeline <id>  # verifies and publishes the runner's build
```

`release-publish --stage <name>` stages instead of publishing, and `zen-releases release-stage
--stage <name>` makes such a stage live, as the tag pipeline does. Zen Toolbox and plug-ins keep
their own versions (`cargo xtask release-toolbox`, `cargo xtask release plugin <id> <version>`; a
plug-in's first catalog release is always made this way).

Every release command first checks that it runs on `main` (or a detached checkout of a commit of
`main`, as in CI), with no uncommitted changes and a commit that is already on `origin/main`
(`ZEN_RELEASE_ANY_BRANCH=1` allows another branch in an emergency), then runs the full quality gate
(`cargo xtask ci`), and refuses to release if either fails. A tree that already passed the gate with
the same toolchain is not run again ([ADR-0141](adr/0141-the-gate-remembers-a-pass-by-its-tree.md)).
On a maintainer's machine the secrets are `~/.config/zen-ide/publisher.key` and `releases.env`; they are
never copied anywhere but into the protected GitLab variables above.

## Two steps: build on the runner, publish on a maintainer's machine

A release is a **build**, which needs no secret, and a **publish**, which needs the signing key
(`~/.config/zen-ide/publisher.key`) and the release server's token (`releases.env`), both only on
a maintainer's machine ([ADR-0142](adr/0142-release-builds-on-the-runner-published-on-the-release-machine.md)).

**Build** (anywhere: the runner, or a maintainer's machine):

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
tag pipeline builds `all`). A macOS program is signed ad hoc and checked with `codesign --verify`.
The build writes each program's address (`artifacts/plugin-<id>/<version>/<file>`, relative to the
catalog), SHA-256 and size into the catalog entry it publishes; publishing checks that the two agree,
uploads the programs, then signs the catalog, and the release server refuses a catalog whose
programs it does not store as listed. `--platform` changes nothing for a plug-in without them.

**On the runner:** GitLab → Build → Pipelines → **Run pipeline** on `main` with the variables
`VERSION` and, optionally, `PRODUCT` (`ides`, the default, `toolbox` or `plugin:<id>`) and
`ALLOW_VERSION=1` (for the IDEs: `--allow-version`, below), or from a terminal:
`glab ci run -b main --variables VERSION:26.10.1` (every IDE),
`glab ci run -b main --variables PRODUCT:toolbox,VERSION:0.3.16`, and a plug-in with its driver
programs for every platform with
`glab ci run -b main --variables PRODUCT:plugin:duckdb,VERSION:0.1.0,PLATFORM:all` (`PLATFORM` is
`macos`, the default, `linux`, `windows` or `all`). Only the `release-build` job runs
(on the release runner, ADR-0140): for the IDEs it builds every IDE (GoGarden, RustRonin, WebWave, JvmJizo,
SqlScroll and DevopsDaruma) one after another in the same job (each after the first reuses the
shared crates the first built), and if one
fails the job fails and keeps nothing, so the IDEs' versions never diverge. Its artifacts,
`release-build/`, are kept for 7 days (an IDE's archive is about 40 MB, the six about 240 MB;
GitLab's limit is 1 GB).

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
cargo xtask release-publish --pipeline <pipeline id> [--dry-run]   # or --job <job id>, or --from <dir>
```

It downloads the `release-build` job's artifacts with `glab` (into `<target>/release-fetch/`),
finds every build in them (the IDEs' release has one per IDE: all are verified before any is
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
`ZEN_RELEASES`). "Today" is in local time, on a maintainer's machine and on the release runner
(`TZ` sets another). `release ides` and `release-build ides` refuse, building nothing, any other
version: one not of this month (a typo such as `27.10.1`, or `26.12.1` in October), or one that
skips a number. `--allow-version` (on the runner, `ALLOW_VERSION=1`) builds another on purpose; it
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
pipeline signs with the hardened runtime and notarises the `.dmg`.

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

or on the runner: Run pipeline on `main` with `VERSION`, `PRODUCT` (`ides` by default, or
`toolbox`) and `PLATFORM` (`macos` by default, `linux`, `windows`, `all`), e.g.
`glab ci run -b main --variables VERSION:26.10.1,PLATFORM:linux`, then
`cargo xtask release-publish --pipeline <id>`.

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

1. **Merge first.** Merge every open pull request that is ready (`glab mr list`); each passes
   `cargo xtask ci` after `main` is merged into it. Then `git switch main && git pull`: the release
   commands refuse a dirty tree, another branch or an unpushed `main`.
2. **Release notes.** On a branch from the current `main`, run
   `cargo xtask changelog gather <YY.M.N>` with `cargo xtask release-version` as the version (and
   `--toolbox <version>` when Zen Toolbox is released too): it moves the notes of
   `changelog/unreleased/` into the section `## <YY.M.N> - <date>` of every IDE's file, Zen
   Toolbox's and `CHANGELOG.md`, and removes the files it emptied. A section of the version that
   is there already and not released (notes written into it by hand) takes the gathered notes after
   its own; a released version is refused. Read the sections, edit them for users where needed, and
   merge that one pull request before the tag. Notes merged after it make the tag's pipeline
   refuse the release (`cargo xtask release-ci plan v<YY.M.N>` says so beforehand): run the gather
   again on a new branch, which adds them to the same section, and merge it. Take versions from the
   current `main`: other people release too.
3. **The release:** tag `v<YY.M.N>` on `main` and press **release** when its pipeline stops there
   ("Releasing from a tag", above): every IDE on every platform, and Zen Toolbox and the catalog's
   plug-ins when they have a new version. The fallback on a maintainer's machine:
   `cargo xtask release ides <YY.M.N>` (the gate, every bundle, the publish), or build them on the
   runner (Run pipeline on `main` with `VERSION=<YY.M.N>`; `PRODUCT=ides` is the default) and
   publish them with `cargo xtask release-publish --pipeline <id>` (two steps, above).
4. **Zen Toolbox** (the tag releases it when its changelog has a new version; by hand):
   `cargo xtask release-toolbox <version>` (the `.dmg`, the update archive), or the
   same two steps with `PRODUCT=toolbox`.
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
6. **Linux and Windows, when asked for:** the pipeline with `PLATFORM=linux` or `windows` (or
   `all` with the release), then `cargo xtask release-publish --pipeline <id>` (above); the Docker
   commands `release-linux` and `release-windows` are the fallback. Then check the Linux packages on a clean
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

8. **The website:** a tag release brings the website and the guide of its commit, live with the
   button; look at `https://ide.ifzenelse.space/`. Deploying the release server
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
