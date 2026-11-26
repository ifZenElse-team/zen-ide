# ADR-0300: Releases from GitHub

- **Status:** Proposed
- **Date:** 2026-11-26
- **Would supersede, once decided:** the GitLab parts of
  [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md) (the pipeline, its variables, the
  GitLab release); its staging on the release server and its one button stay
- **Extends:** [ADR-0020](0020-release-server.md), [ADR-0142](0142-release-builds-on-the-runner-published-on-the-release-machine.md),
  [ADR-0146](0146-one-calendar-version-for-every-ide-and-release-channels.md),
  [ADR-0150](0150-linux-and-windows-builds-on-the-mac-runner.md),
  [ADR-0188](0188-release-notes-and-decision-records-in-files-of-their-own.md)

This record is a proposal. It describes how a release works today, maps every step to GitHub, and
lists the choices that are the owner's, each with its options, what each costs, and a
recommendation. Nothing in it is decided.

File and line references are to the tree of `main` at commit `798300e` (2026-11-26). "Not checked"
marks what was not verified while writing; the list is repeated at the end.

## Context

The repository is public on GitHub. `main` takes changes only through pull requests, and the
required check runs the gate without tests on a GitHub-hosted Linux runner
(`.github/workflows/ci.yml:21-40`). Releases are still described, and coded, for a GitLab pipeline
on the maintainers' own Mac (docs/releasing.md:7-10, 46-51). The pipeline's definition is not in
this tree (there is no `.gitlab-ci.yml`), so what follows is read from the tooling it calls and
from docs/releasing.md.

### How a release works today

| # | Step | Where it is in the tree |
|---|---|---|
| 1 | The notes are gathered into the changelogs by one pull request before the tag | docs/releasing.md:16-23, 212-229; `cargo xtask changelog gather` |
| 2 | A protected tag `v<YY.M.N>` on a commit of `main` starts the pipeline; the version is the tag's and must be the IDEs' next calendar version | ADR-0171:41-45; `xtask/src/release_ci.rs:72, 99-105`; `xtask/src/release.rs:536-556` |
| 3 | **Build**, no secrets, one job on the Mac: refuses notes not gathered; the source check (a commit of `main` on `origin/main`, nothing uncommitted) and the full gate; every IDE for macOS, Linux and Windows in one invocation; Zen Toolbox and the catalog's plug-ins when their version is not published; the website | `xtask/src/release_ci.rs:207-247`; `xtask/src/release.rs:70-110, 558-604, 616-669`; `xtask/src/native.rs:92-181` |
| 4 | Each build is a folder with `build.json`: commit, tree, compiler, platform, the gate's result, every file's size and SHA-256 | `xtask/src/release_flow.rs:22-62, 101-123` |
| 5 | **Stage**, with the secrets: every file is checked against `build.json`, uploaded to the release server (stored, listed nowhere); the manifest and the plug-in catalog are signed and staged with the changelogs and the website under `v<version>-p<pipeline>` | `xtask/src/release_ci.rs:249-312`; `xtask/src/release_flow.rs:125-150, 189-308`; ADR-0171:65-74 |
| 6 | **Verify**, no secrets: every staged file is downloaded from its public address and compared with the build; each macOS application is checked with `codesign --verify --deep --strict`; a Debian package is installed in a container when the machine has one; the staged manifest, catalog, changelogs and website are checked | `xtask/src/release_ci.rs:325-340, 450-529, 548-582` |
| 7 | **Release**, manual, with the secrets: one request makes the stage live; then the GitLab release of the tag is created with the notes, the checks and a link to every file on the release server | `xtask/src/release_ci.rs:606-633, 678-700` |
| 8 | Installed copies find the update: Zen Toolbox and the IDEs read the signed manifest and the signed plug-in catalog from the release server, with the address and the public key compiled in, and check each download's size and SHA-256 | `apps/zen-toolbox/src/main.rs:15-16`; `crates/platform/update/src/lib.rs:13-22`; `apps/go-garden/src/main.rs:49-56`; docs/release-manifest.md:9-12 |
| 9 | The fallback: a maintainer's machine builds and publishes, or publishes a build the runner made, fetched with `glab` | docs/releasing.md:92-112, 169-184; `xtask/src/release_flow.rs:613-645` |

The secrets are the manifest signing key and the release server's upload token, protected GitLab
variables scoped to the environment `release`, which only the stage and release jobs name
(docs/releasing.md:63-78). `zen-releases` reads the key from `ZEN_PUBLISHER_KEY` and writes it
nowhere (`apps/zen-releases/src/main.rs:20`). The jobs share one macOS user with every other job on
that machine, a risk recorded in ADR-0171:95-101.

### What in the tooling knows about GitLab

| What | Where | What it does outside GitLab |
|---|---|---|
| The tag comes from `CI_COMMIT_TAG` when no tag is given | `xtask/src/release_ci.rs:72` | The tag must be given as an argument |
| A job releases only its pipeline's own tag, when `GITLAB_CI` is `true` | `xtask/src/release_ci.rs:76-80, 107-114` | **No check**: any tag given is taken |
| The stage's name is `v<version>-p<CI_PIPELINE_ID>` | `xtask/src/release_ci.rs:116-125, 261` | Named `…-local-<seconds>` |
| The release secrets are refused when `GITLAB_CI` is `true` and `CI_COMMIT_REF_PROTECTED` is not | `xtask/src/release.rs:9-27, 35-39` | **No check**: with `ZEN_PUBLISHER_KEY` set, the variables are used as they are (`release.rs:40-51`) |
| The GitLab release: `CI_API_V4_URL`, `CI_PROJECT_ID`, `CI_JOB_TOKEN`, the Releases API and its asset links | `xtask/src/release_ci.rs:626-643, 678-715` | Skipped with "Not in GitLab CI" |
| `release-publish --pipeline` and `--job` download a job's artifacts with `glab`, from project `87707608` | `xtask/src/release_flow.rs:10, 613-645` | Fails unless `glab` is installed and signed in |
| The error messages and docs/releasing.md name GitLab's pages and variables | `xtask/src/release.rs:23-24, 42-44`; docs/releasing.md | Text only |

Nothing in `release-build` reads a GitLab variable. What stops a build on a GitHub-hosted runner is
the release's own rules, not the host:

| Refusal | Where | Why it stops a per-platform job |
|---|---|---|
| The IDEs are built together; one IDE alone is refused | `xtask/src/release.rs:489-491` | A trial of one product is not possible |
| The version must be the next calendar version, read from the live manifest with `curl` | `xtask/src/release.rs:516-528, 536-556, 589-594` | A trial version is refused; the build depends on the release server answering |
| The notes must be in the changelog under the version | `xtask/src/release.rs:358-365, 618`; `xtask/src/native.rs:96` | No section, no build |
| The source check: branch `main` or a detached commit, on `origin/main`, nothing uncommitted | `xtask/src/release.rs:76-110` | A run from another branch is refused |
| The full gate, tests included, on the building machine; a pass is remembered by tree on that machine only | `xtask/src/release.rs:70-74`; [ADR-0141](0141-the-gate-remembers-a-pass-by-its-tree.md) | Every job would run every test again; whether the tests pass on a hosted runner is not checked |
| The macOS bundle is built only on macOS | `xtask/src/release.rs:381, 621` | By design: the bundle needs `iconutil`, `codesign`, `hdiutil` (`release.rs:167-175, 194, 348`) |
| A Linux or Windows build needs the version published for macOS, or macOS built in the same invocation | `xtask/src/native.rs:194-212`; `xtask/src/release.rs:566-571` | A Linux job alone is refused for a new version |
| The Linux build needs `zig` and `cargo-zigbuild`; the Windows build needs MinGW-w64's `x86_64-w64-mingw32-gcc` | `xtask/src/native.rs:60-76` | A matter of installing them on the runner |

A build that skips any of these is not a release, and the tooling already has the place to say so:
`build.json` carries the gate's result, and nothing is published unless it is `passed`
(`xtask/src/release_flow.rs:146-148`). The trial build added beside this record
(`release-build <product> <version> --trial`, and the hand-started workflow
`.github/workflows/release-build.yml`) uses exactly that: it skips the first five refusals in the
open and records the gate as `skipped`.

## Every step, on GitHub

| # | Today | On GitHub | Change |
|---|---|---|---|
| 1 | Notes gathered by a pull request | The same pull request | None |
| 2 | Protected tag `v*` starts the pipeline | A workflow `on: push: tags: ['v*']`; a tag ruleset limits who may create `v*` tags | A new workflow file; repository settings (the owner's) |
| 3 | One build job on the Mac, every platform | One job per platform on GitHub-hosted runners, plus one job that runs the gate once (choice 1) | `release-ci build` learns to build one platform and to take the gate's result from the gate job; a step that joins the per-platform folders into the one layout the stage step reads |
| 4 | `build.json` beside every file | Unchanged; the folders travel between jobs as workflow artefacts | None. Today the build stays on the runner's disk because it is over GitLab's limit (docs/releasing.md:48); GitHub's limits for workflow artefacts are not checked |
| 5 | Stage with the secrets | A job in the environment `release` (choices 2 and 4) | The host's variables (below); otherwise unchanged |
| 6 | Verify | A job on a macOS runner (it runs `codesign`); the Debian package check needs a container runtime, which a hosted macOS runner is not expected to have (not checked), so it becomes a small job of its own on Linux or stays a manual step as today (docs/releasing.md:50) | A split of `verify`, or none |
| 7 | The manual job makes the stage live, then creates the GitLab release | The same request to the release server, then a GitHub release of the tag (choices 2 and 3) | The release call is rewritten for GitHub's API |
| 8 | Clients read the release server | Unchanged unless choice 3 says otherwise | None for options A and C of choice 3 |
| 9 | Fallback on a maintainer's machine | Unchanged, except that `release-publish --pipeline` must fetch a GitHub run's artefacts | `release_flow.rs:613-645` rewritten |

## The choices that are the owner's

### 1. Where builds run

- **A. GitHub-hosted runners, one job per platform.** macOS on an Apple-silicon runner for the
  application bundles; Linux and Windows on a Linux runner, cross-built with the same commands as
  today (`cargo zigbuild` for glibc 2.35 and MinGW-w64, `xtask/src/native.rs:82-90`). Each job
  starts from an empty machine that is thrown away afterwards, so no job can read another's
  environment: the residual risk of ADR-0171:95-101 goes away. Costs: every job compiles from
  nothing (today each IDE after the first reuses what the first built, docs/releasing.md:162-165),
  so a release takes longer; the time is not measured yet, and the trial workflow will show it for
  one product. Whether hosted minutes stay free of charge for a public repository at this volume is
  not checked. The tests of the gate are run today on macOS only; whether they pass on a hosted
  runner of either system is not checked.
- **B. The maintainers' Mac as a self-hosted runner.** Fast, warm builds, as today. But a
  self-hosted runner keeps its disk between jobs, and in a public repository a pull request from
  any fork can change a workflow file and what the job runs. If such a job ever lands on that
  machine, a stranger's code runs there as the runner's user: it can read what earlier jobs left,
  leave something for the next release job to pick up, and reach the machine's network. The
  settings that require approval before a fork's workflow runs reduce this and do not remove it:
  one approval of a harmless-looking change is enough. **A public repository must never run
  pull-request code on a self-hosted machine**; if the Mac is used at all, it must be registered
  for release workflows only, on tags only, and never be selectable by a workflow a pull request
  can change. Restricting a runner to chosen workflows is a GitHub setting whose availability on
  the project's plan is not checked.
- **C. Hosted for pull requests and Linux/Windows, the Mac for the macOS release build only.**
  The same exposure as B for the artefact people run most, in exchange for build time.

**Recommendation: A.** The builds need no secret, so nothing is lost by building on machines we do
not own, and the signed manifest lists each file's SHA-256, which the verify step compares with the
build. Revisit only if the measured time is unacceptable.

### 2. The "Release" button

- **A. A job in a GitHub environment with required reviewers.** The workflow stops before the job
  that makes the stage live; the owner opens the run, looks at the staged website and the verify
  job's checks, and approves. Only then does GitHub start the job and give it the environment's
  secrets. This is today's manual job with the same meaning (docs/releasing.md:29-34). Costs: the
  environment and its reviewers are repository settings the owner keeps; whether required reviewers
  are available for this repository's plan is not checked (they are documented for public
  repositories).
- **B. A draft GitHub release that the owner publishes by hand.** Publishing a draft is an act on
  GitHub only: by itself it does not switch the manifest on the release server. A second workflow
  would have to start on "release published" and make the stage live, and that workflow would then
  hold the secrets with no person approving that job, or would need the environment of option A
  anyway. It also puts the release's page in front of the live switch, so for a moment the page
  says "released" while clients see the old manifest, and if the switch is refused
  (docs/releasing.md:57-61) the page is wrong.
- **C. Both in one:** the stage job creates the GitHub release as a draft, with the notes and the
  files or links; the approved job of option A makes the stage live and then publishes the draft.
  The draft is something to read before approving, and the page appears only after the switch.

**Recommendation: C**, which is A with the draft as a preview. Another change in progress keeps a
draft release with the notes of what is merged; this record does not depend on it, and the two
should agree on which draft a tag's release turns into.

### 3. Where people get updates from

Installed copies have the release server's address and the public key compiled in
(`apps/zen-toolbox/src/main.rs:15-16`, `crates/platform/update/src/lib.rs:13-22`), and the install
script reads the same server (`deploy/releases/site/install.sh:23-33`). Whatever is chosen, the
signed manifest and catalog must stay at those addresses for every copy already installed.

- **A. The release server, as now.** GitHub builds and stages there; the GitHub release carries the
  notes and a link to every file, as the GitLab release does today (`release_ci.rs:664-700`).
  Changes in Zen Toolbox and the update client: none. Costs: the server keeps carrying every
  download (about 700 MB a release for every platform, docs/releasing.md:49); its upload token
  lives in GitHub (choice 4).
- **B. GitHub Releases as the source of the files.** The manifest stays signed and stays on the
  release server; its artefact addresses point at the release's assets. The update client already
  takes an absolute `https://` address from the manifest (`crates/platform/update/src/fetch.rs:251-259`)
  and checks size and SHA-256 whatever the address. What does not allow it today: the release
  server refuses a manifest whose artefact address does not start with `artifacts/` and whose file
  it does not store (`apps/zen-releases/src/server.rs:206-214`); the publisher writes only such
  addresses (`apps/zen-releases/src/publish.rs:54, 224`); the install script prefixes every address
  with the server's (`deploy/releases/site/install.sh:129, 275`); the stage and verify steps build
  public addresses from the server's (`xtask/src/release_ci.rs:414-441`). So: a server release, a
  publisher change, an install-script change and a verify change, in that order; no client change
  for copies that already accept absolute addresses (since which version they do is not checked,
  and whether a download follows the redirect a release asset answers with is tested for the
  manifest request only, `fetch.rs:369-381`, not for a download). Costs beyond the work: updates
  then depend on GitHub being reachable from where people are; a release's assets can be replaced
  by anyone with write access unless the repository makes releases immutable (not checked), though
  a replaced file fails the SHA-256 of the signed manifest; the plug-in catalog and the driver
  programs have the same rule and the same work (docs/releasing.md:149-152).
- **C. Both: the server as the source, GitHub Releases as a mirror for people.** As A, and the
  files are also attached to the GitHub release, so the release page is complete by itself and a
  person can download from either. Clients never read GitHub. Costs: every file is uploaded twice;
  GitHub's size limit for one asset is not checked against our largest file.

**Recommendation: A now, C when the release workflow is stable; B only as a later decision of its
own**, because it is the one option that changes what installed copies depend on.

### 4. Secrets

Two secrets and one address: `ZEN_PUBLISHER_KEY`, `ZEN_RELEASES_TOKEN`, `ZEN_RELEASES_SERVER`
(docs/releasing.md:63-69).

- **A. Environment secrets of `release`.** The environment is limited to tags `v*` and has required
  reviewers; only the stage job and the live job name it. Who can reach them: a job that names the
  environment, on a ref the environment allows, after a reviewer approves. Not reachable: any
  `pull_request` workflow, from a fork or not; a job that does not name the environment. Who can
  change that: a repository administrator (the environment's rules, the tag ruleset), and anyone
  who can get a change to the release workflow merged into `main`, which is why that file needs the
  owner's review like any other code that touches a release.
- **B. The token on GitHub, the key on a maintainer's machine.** The server stages only a signed
  manifest (ADR-0171:65-67), so without the key GitHub cannot stage: this option is "GitHub builds,
  a maintainer's machine publishes", which is today's fallback (docs/releasing.md:114-184) made the
  rule. The key never leaves the machine; there is no button, and a release needs that machine.
- **C. As A, without the key at the stage:** not possible with today's server for the same reason
  as B.

What a compromised workflow could do with option A. The stage and live jobs hold both secrets. A
changed action, or a dependency whose build script runs in those jobs, could send them out; with
the key and the token an attacker can sign and publish a manifest that every installed Zen Toolbox
and IDE accepts, which is the worst case this project has, and replacing the key "means re-keying
every Toolbox" (deploy/releases/README.md:44). What limits it:

- actions pinned to commit SHAs, as in `ci.yml:26-36`, and few of them in the two jobs;
- the secrets given to the single step that signs and sends, not to the job, so the steps that
  compile do not have them in their environment;
- the two jobs build only `zen-releases` (`xtask/src/release_flow.rs:309-316`), from the tag's
  commit with `--locked`; that program reads the key, so its dependencies are the ones that matter.
  Building it in a job without secrets and handing the binary over would break today's rule that
  the program reading the key is built where it is used (docs/releasing.md:181); keep the rule;
- `GITHUB_TOKEN` read-only everywhere except the one step that creates the release;
- the checks of `xtask` that today refuse the secrets on an unprotected ref
  (`xtask/src/release.rs:21-27`) and a tag other than the pipeline's own
  (`xtask/src/release_ci.rs:107-114`) do nothing outside GitLab and must be rewritten for GitHub
  before any secret is added.

**Recommendation: A**, with every limit above, and B kept working as the fallback it is today.

### 5. macOS signing and notarisation

What the tree does today: every application bundle is signed ad hoc
(`codesign --force --deep --sign -`, `xtask/src/release.rs:194`), every driver program too
(`release.rs:947-948`), and the verify step checks the signatures
(`xtask/src/release_ci.rs:465-469, 502-515`). There is no Developer ID and nothing is notarised:
the installer downloaded in a browser is blocked on first launch, which is why the recommended
installation is the terminal command (docs/releasing.md:243-257; deploy/releases/README.md:16-18).
The installer is not in the signed manifest either (docs/releasing.md:244-245).

On GitHub: ad hoc signing needs no secret and is expected to work on a hosted macOS runner; the
trial workflow checks it (`codesign --verify --deep --strict` on the built application). A
Developer ID signature and notarisation would add a certificate and notary credentials to the
`release` environment and a step after the bundle is built. Whether to get them, and under which
account, is the owner's decision and is not assumed here.

**Recommendation:** move the release as it is, ad hoc signed, and decide notarisation separately.

### 6. What is reused, what is rewritten, and the order

Reused unchanged: `release-build` and everything under it (the builds, the bundled plug-ins, the
licence notices, the Debian package, `build.json`); `release_flow::verified` and `publish_all`;
`zen-releases` and the release server with its stage API; the calendar version and
`changelog gather`; the checks of `release-ci verify`; the website's staging through the server;
the fallback on a maintainer's machine except `--pipeline`.

Rewritten:

| What | Where | For GitHub |
|---|---|---|
| The tag, the "own tag" rule, the stage's name | `xtask/src/release_ci.rs:72, 76-80, 107-125, 261` | `GITHUB_REF_TYPE`, `GITHUB_REF_NAME`, `GITHUB_RUN_ID` |
| The rule that only a protected ref reads the secrets | `xtask/src/release.rs:9-55` | `GITHUB_ACTIONS`, `GITHUB_REF_PROTECTED` or the tag rule, and the messages |
| The GitLab release | `xtask/src/release_ci.rs:626-715` | GitHub's Releases API with the job's token, the token on standard input as today (`release_ci.rs:702-709`) |
| Fetching a build made by CI | `xtask/src/release_flow.rs:10, 613-645` | A run's artefacts |
| One build job for every platform | `xtask/src/release_ci.rs:207-247`; `xtask/src/native.rs:194-212` | One platform per job, joined before the stage; the "macOS first" rule checked at the join |
| The gate inside every build | `xtask/src/release.rs:70-74` | The gate once, its result carried to the build jobs by the commit's tree |
| The credentials check | docs/releasing.md:80-85 | A hand-started workflow in the `release` environment |
| The pipeline itself | not in this tree | `.github/workflows/release.yml` |
| docs/releasing.md, deploy/releases/README.md:40-46 | | Rewritten when the switch happens, not before |

The order, each step leaving releases working as they are until the last:

1. **Trial builds** (this branch): one product, three platforms, hosted runners, no secrets,
   nothing published. Shows whether the builds work there and how long they take.
2. **Host-neutral variables:** one place in `xtask` that answers "which tag, which run, is the ref
   protected" for either host, with tests. The GitLab pipeline keeps working.
3. **Per-platform release builds:** `release-ci build` for one platform, the join, the gate once. A
   workflow on tags that only plans and builds, and keeps the folders as artefacts. Run beside a
   real release and compare the two builds' file lists and sizes; nothing is published from GitHub.
4. **The environment** `release`, its tag rule and reviewers, the tag ruleset, the secrets (the
   owner's settings), and the credentials check as a workflow: it signs an empty manifest and the
   server checks it, storing nothing (docs/releasing.md:80-85).
5. **Stage and verify from GitHub.** A stage lists nothing and changes nothing live
   (ADR-0171:65-72), so this can be rehearsed on a real tag while the release itself is still made
   the old way.
6. **The live job and the GitHub release,** behind the environment's reviewers. The first release
   from GitHub; the old pipeline is switched off after it; `release-publish --pipeline` and the
   documents are rewritten.
7. Later, each a decision of its own: the files on GitHub Releases (choice 3), notarisation
   (choice 5), build provenance attestations.

## Consequences

- If the recommendations are taken, a release stays "merge the notes, tag, look, approve", and
  installed copies notice nothing: the same server, the same manifest, the same key.
- The Mac stops being part of a release. The fallback on a maintainer's machine stays.
- A release takes longer to build, by an amount step 1 measures.
- The signing key and the upload token live in one more place, GitHub's environment secrets, for as
  long as releases are made there. That is the main cost of the whole move and the reason for
  choice 4's limits.

## Not checked

- GitHub's current limits and terms: hosted minutes for a public repository at this volume, the
  size and retention of workflow artefacts, the size of one release asset, required reviewers and
  runner restrictions on this project's plan, immutable releases.
- Whether the gate's tests pass on a GitHub-hosted runner, on macOS or on Linux.
- How long a clean build of every IDE for every platform takes on hosted runners.
- Whether a hosted macOS runner can install a Debian package in a container for the verify step.
- Since which version the update client accepts an absolute address in the manifest, and whether an
  artefact download follows a redirect.
- The GitLab pipeline's own definition, which is not in this tree.
- The trial workflow has not run on GitHub at the time of writing; its commands were run on Macs
  only (the macOS bundle on a Mac, the Linux and Windows builds cross-built on a Mac).
