# ADR-0171: Releases from a protected tag through GitLab, staged and made live by one button

- **Status:** Proposed (the owner, 2026-10-27)
- **Date:** 2026-10-27
- **Supersedes:** AGENTS.md rule 12 as written on 2026-10-18 ("releases only from the release
  machine"); decision 7 of [ADR-0140](0140-ci-on-a-team-mac-runner.md) ("no secrets on the
  runner"); the publishing step of [ADR-0142](0142-release-builds-on-the-runner-published-on-the-release-machine.md)
  ("signed and published on the release machine"), which stays as the fallback
- **Extends:** [ADR-0020](0020-release-server.md) (the release server), [ADR-0140](0140-ci-on-a-team-mac-runner.md),
  [ADR-0141](0141-the-gate-remembers-a-pass-by-its-tree.md), [ADR-0146](0146-one-calendar-version-for-every-ide-and-release-channels.md),
  [ADR-0150](0150-linux-and-windows-builds-on-the-mac-runner.md)

## Context

A release meant: build on the team's Mac runner (ADR-0140), then sign and publish
from the release machine, where the signing key (`~/.config/zen-ide/publisher.key`) and the release
server's token (`releases.env`) lived and nowhere else, then deploy the website with
the server's deploy script (kept outside this repository) over SSH. The owner, 2026-10-27: everything must build by itself; he
presses a release button, and by then the new version must already be on the server and the website
ready, so that the button only releases it. Releases go through GitLab's Releases.

Where the secrets can be:

- **On the release runner's disk:** every job on the runner, merge requests' from any branch too,
  runs as one macOS user (the build user) through one shell runner, so any merge request's job
  could read them.
- **On the runner, under a second macOS user with its own protected runner:** the right isolation,
  but
  creating a user needs the runner's administrator password, which the owner does not have.
- **On GitLab's own Linux runners (SaaS), from protected variables:** isolated from the runner, but
  the
  owner trusts his own device more than GitLab's runners, and their free minutes are limited.
- **In GitLab's protected, masked, hidden CI/CD variables, given to jobs on the runner:** GitLab
  gives
  a protected variable only to a pipeline of a protected branch or tag, never to a merge request's
  or an ordinary branch's; scoped to an environment, only to the jobs that name it. Nothing is
  stored on the runner's disk. The owner chose this, accepting the runner as trusted.

## Decision

1. **A release starts with a protected tag.** Creating the tag `v<YY.M.N>` (e.g. `v26.10.4`) on a
   commit of `main` starts the release pipeline; tags `v*` are protected and only maintainers may
   create them. The jobs run only when `CI_COMMIT_TAG` is such a tag and `CI_COMMIT_REF_PROTECTED`
   is `true`. The version comes from the tag and must be the IDEs' next calendar version with its
   changelog sections, as `release-build ides` checks (ADR-0146).
2. **Four jobs, all on the release runner:**
   - `release: build` (no secrets): the gate, every IDE for macOS, Linux and Windows, Zen Toolbox
     when its changelog's newest version is not published, each plug-in **already in the catalog**
     whose version is not in it (a plug-in's first catalog release stays `cargo xtask release
     plugin`), and the website (`deploy/releases/site` with the documentation built from the guide),
     kept as the job's artifacts (`cargo xtask release-ci build`).
   - `release: stage` (the release variables): uploads every artifact (stored on the server, listed
     nowhere) and **stages** the signed manifest, the signed plug-in catalog, the changelogs and the
     website under the stage's name `v<version>-p<pipeline>` (`release-ci stage`).
   - `release: verify` (no secrets): downloads every staged file from its public address and checks
     its size and SHA-256 against its build, each macOS application with `codesign --verify --deep
     --strict`, each Debian package by installing it in Ubuntu when Docker is there (the runner has
     none, so it says so), and the staged manifest, catalog, changelogs and website
     (`release-ci verify`).
   - `release` (the release variables, manual): **the button**. One request makes the stage live,
     then the job creates the GitLab release of the tag: the notes of every product from the
     changelogs, what was checked, and a link to every published file (`release-ci live`). It
     builds nothing but the release tools, which the stage job built on the same machine; the switch
     takes about a second.
3. **The release server stages** (`zen-releases`, `apps/zen-releases/src/server/stage.rs`):
   `PUT /api/v1/stage/<stage>/manifest|plugins` (signed and checked as a live upload is, every
   listed artifact stored with its size and hash), `…/changelog` and `…/site/<path>`, with the
   upload token; `GET /releases/stage/<stage>/…` and `/releases/staged-site/<stage>/…` to look at
   them; `POST /api/v1/stage/<stage>/release` to make them live. A stage records the SHA-256 of the
   live manifest and catalog it was made from (`X-Zen-Base`), and is released only while they are
   still the live ones, so that a release never drops what was published meanwhile (re-run the
   pipeline: a new stage). Releasing a released stage again changes nothing. The earlier checks
   (`PUT /api/v1/check/…`) still run before anything is uploaded; a server without the stage
   endpoints is reported as needing a deploy.
4. **The website goes through the server too.** The live website is `site/current` in the server's
   data folder, a relative link to `site/<stage>/`, replaced by one rename; Caddy serves it when it
   exists, else the website deployed with the server. A release therefore needs no SSH key; only a
   change of the server's own code needs the server's deploy script (kept outside this repository), from the release machine.
5. **Secrets.** `ZEN_PUBLISHER_KEY` (the signing key's 64 hex digits, as in `publisher.key`),
   `ZEN_RELEASES_TOKEN` and `ZEN_RELEASES_SERVER` are protected, masked and hidden CI/CD variables
   scoped to the environment `release`, which only the stage and release jobs name. `zen-releases`
   reads the key from `ZEN_PUBLISHER_KEY` when no `--key` file is given; nothing writes it to
   disk or prints it. `cargo xtask` refuses the release secrets in any GitLab job whose ref is not
   protected (`release::ci_secrets_problem`), and a job releases only its pipeline's own tag. The
   two jobs share `resource_group: release`, so one runs at a time.
6. **The release machine stays a fallback.** `cargo xtask release ides <YY.M.N>`, `release-publish`
   (now also with `--stage <name>`) and the hand-started `release-build` pipeline work as before
   with the files in `~/.config/zen-ide`.

## Consequences

- A release is: merge, create the tag (GitLab → Code → Tags → New tag, or `git tag v26.10.4 &&
  git push origin v26.10.4`), wait for the pipeline to stop at the manual job, look at the staged
  website and the verify job's checks, press **release**.
- **Residual risk, accepted by the owner:** the stage and release jobs run on the runner as the same
  macOS user as every merge request's job, so a job running at the same moment could read their
  environment and with it the key and the token. Mitigated by: no secret on disk; the variables only
  in the two jobs of the `release` environment of a protected tag (the build and verify jobs and
  every merge request never get them), for minutes; the release jobs one at a time; GitLab masking
  them in logs. Removing the risk needs a second macOS user with its own protected runner (an
  administrator's password) or GitLab's own runners.
- The release's artifacts go up to GitLab once (about 700 MB for every platform, under the 1 GB
  limit of a job's artifacts) and back to the runner for staging; Linux builds that grow past that
  would split the build job.
- The release server needs one deploy before the first tag release (the stage endpoints, and
  Caddy's live website); until then the stage job fails with "deploy it first", having made nothing
  live.
- The production server keeps every stage's documents and website under `stages/` and `site/`; old
  ones can be deleted by hand.

## Alternatives considered

- **Secrets on the runner's disk, in the CI user's home:** readable by every merge request's job.
- **A second macOS user with a protected runner:** needs the runner's administrator password.
- **GitLab's shared Linux runners for the signing jobs:** the owner prefers his own device.
- **Publishing at once, without a stage:** the button would take the whole upload, and a failure
  midway would leave a half-published release.
- **Deploying the website with the deploy script from the pipeline:** would need the server's root SSH
  key in CI.
