# ADR-0301: Releases start from a `release` branch; the tag records a finished release

- **Status:** Accepted (the owner, 2026-11-28)
- **Date:** 2026-11-28
- **Supersedes:** in [ADR-0300](0300-releases-from-github.md), that a pushed tag `v*` starts the
  release workflow (step 2 of "Every step, on GitHub"), that the environment `release` is limited to
  tags `v*` (choice 4 A), and that the GitHub release is made for a tag that exists before the
  release; in [ADR-0171](0171-releases-from-a-protected-tag-through-gitlab.md), that a release is a
  protected tag and that a protected tag's pipeline is given the release variables, and the GitLab
  pipeline and GitLab release (the GitLab project is archived)
- **Keeps:** everything else of ADR-0300 and ADR-0171: builds on GitHub-hosted runners, the gate once
  and the parts joined, staging on the release server, the environment `release` with its required
  reviewers approving before `stage` and before `live`, the secrets as its environment secrets, the
  draft GitHub release read before the live switch, and the maintainer's machine as the fallback
- **Extends:** [ADR-0146](0146-one-calendar-version-for-every-ide-and-release-channels.md) (the
  calendar version), [ADR-0188](0188-release-notes-and-decision-records-in-files-of-their-own.md)
  (the notes gathered before a release)

## Context

The first release from GitHub, 26.11.10, went live on 2026-11-27 (run 45956765766), but only after
a protected tag was deleted by hand. The tag `v26.11.10` started the workflow; the gate then failed
on the hosted Mac in tests that depended on the machine. The fixes were merged into `main`, and
their notes had to go into 26.11.10:

- `cargo xtask changelog gather 26.11.10` refused: a version with a tag counted as released
  ("released already (the tag v26.11.10)"), although nothing of it was published;
- the calendar allows no other version than the next unpublished one, so the release could not move
  to 26.11.11 either;
- so the owner allowed deleting a protected tag, and tagged again.

A tag was doing two jobs at once: asking for a release and recording one. A failed attempt left the
record of a release that did not happen. Before that release nothing had ever run the release
workflow either; the nightly dry run (`.github/workflows/release-dry-run.yml`) addresses that part.

## Decision

1. **A release starts with a push to the branch `release`.** `release` always points at a commit of
   `main` and only moves forward. The workflow **Release** runs on a push to it, and its first job
   refuses, before anything is built, a commit that is not on `origin/main`. No commit is ever made
   on `release` that is not on `main`. Hotfix releases that bypass `main` are out of scope.
2. **The version is not named by hand.** It is the calendar's next unpublished version
   (`cargo xtask release-version`), and Zen Toolbox's own version comes from its changelog when
   Toolbox is in the release, as before. The plan computes it once and every later job of the run
   takes it from the plan. The version's changelog section must exist and `changelog/unreleased/`
   must be empty (Zen Toolbox's notes count only when Toolbox is released), as before.
3. **"Released" means published.** A version is released when the published manifest on the
   release server lists it, for the IDEs and for Zen Toolbox. `changelog gather`,
   `release-version` and `release-ci plan` read that one source; none of them reads tags. A failed
   attempt publishes nothing, so the same version stays the next one, its notes stay open to
   `gather`, and nobody deletes anything.
4. **The tag `v<version>` is the fact of a release.** The last job, after the live switch, creates
   the tag on the released commit and publishes the GitHub release; the draft it publishes was
   written for that commit before the live switch. A failed attempt leaves no tag. Retrying is
   re-running the failed jobs, or moving `release` forward to a newer commit of `main` (with the
   fixes and their notes gathered), which starts a new run.
5. **A pushed tag `v*` starts nothing.** The workflow has no tag trigger any more.
6. **Approvals stay exactly as they were:** the environment `release` with a required reviewer,
   asked before `stage` and before `live`.
7. **The safety checks follow the branch.** `cargo xtask` (`xtask/src/ci_host.rs`) gives the
   release secrets only to a run of the protected branch `release` (`GITHUB_REF` is
   `refs/heads/release`, `GITHUB_REF_TYPE` is `branch`, `GITHUB_REF_NAME` is `release`, and
   `GITHUB_REF_PROTECTED` is `true`): never to a pull request's run, never to another branch or a
   tag. `stage` and `live` release only the commit the run was started for (`GITHUB_SHA`, which
   every build record must carry) and only while `release` still points at it; a run left behind
   when `release` moved on refuses to stage or go live. A CI that is neither a maintainer's machine
   nor GitHub Actions is refused the secrets and the release, as before. The environment `release`
   allows the branch `release` only; the credentials check runs on that branch too.
8. **The switch `ZEN_RELEASE_FROM_GITHUB` is removed.** It kept a tag's run from staging while the
   GitLab pipeline was still the way to release. Now a push to the protected branch `release` is the
   request for a release, and the approval before `stage` is the moment to stop one: a reviewer who
   rejects it keeps the run's artefact `release`, which `cargo xtask release-publish --run <id>`
   publishes from a maintainer's machine (the fallback).
9. **The GitLab paths of `release-ci` and of the CI checks are removed.** A GitLab job is now a CI of
   another kind and is refused. `release-publish --pipeline` and `--job`, which only read a GitLab
   pipeline's artefacts from a maintainer's machine, stay until a change of their own removes them.
10. **The tag is created by the workflow's token.** The tag ruleset "release tags" restricts who
    creates, moves and deletes `v*` tags. Rulesets apply to a tag the workflow creates with
    `GITHUB_TOKEN`, and the only actors a ruleset can let through are those of its bypass list. The
    owner chooses one of: remove the rule "Restrict creations" from the ruleset and keep "Restrict
    updates", "Restrict deletions" and "Block force pushes" (recommended: a tag starts nothing any
    more, and the last job checks that the tag it creates or finds points at the released commit);
    or add the GitHub Actions app to the bypass list, if GitHub accepts it for this repository. The
    last job says which refusal it met, and an organisation admin can always create the tag by
    hand and re-run that job.

## Consequences

- A release is "gather and merge the notes, move `release` to that commit of `main`, look,
  approve twice". Nothing is ever deleted after a failed attempt.
- Between a failed attempt and the retry, `main` may take other changes; the retry releases the
  newer commit, with all notes gathered, at the same version.
- A version is fixed when the plan runs. Notes gathered into it while its release runs reach the
  changelogs on `main` but not the release being built: gather before moving `release`, never while
  a release runs.
- Tags remain the history of releases from 26.11.11 on, and every one of them is a published
  release; older tags stay as they are.
- The owner keeps four repository settings in step with this record: a ruleset on the branch
  `release`, the environment's allowed branch, the tag ruleset's creation rule, and the removal of
  the variable `ZEN_RELEASE_FROM_GITHUB` (docs/releasing.md).

## Not checked

- No run of the new workflow has happened when this is written; nothing here has run on GitHub.
- Whether GitHub lets the GitHub Actions app be a bypass actor of a tag ruleset in this repository
  (other repositories report a refusal for personal accounts; this one belongs to an organisation).
- That a draft release for a tag that does not exist creates no tag until it is published (the web
  interface says so; the REST documentation says only that `target_commitish` is where the tag is
  created from).
- That creating a ref with `GITHUB_TOKEN` for a commit already on `main` is not refused for
  touching workflow files: the REST documentation refuses a release whose target "adds or modifies
  any file under `.github/workflows/` relative to the repository's default branch", which a commit
  of `main` does not do as we read it.
