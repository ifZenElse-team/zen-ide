## repository

### Changed

- A release starts with a push to the branch `release`, which always points at a commit of `main`
  and only moves forward (ADR-0301); a pushed tag `v*` starts nothing. The version is the
  calendar's next unpublished one, fixed by the plan for the whole run. The tag `v<version>` is
  created by the last job, after the release is live, on the released commit, together with
  publishing the GitHub release, so a failed attempt leaves no tag.
- "Released" means listed in the published manifest: `cargo xtask changelog gather`,
  `release-version` and `release-ci plan` agree on it, and a failed attempt's version takes more
  notes without anyone deleting anything.
- The release secrets are used only by a run of the protected branch `release` (never by a pull
  request's run, another branch or a tag), and `stage` and `live` release only the run's own
  commit, and only while `release` still points at it. The credentials check runs on `release`.
- `cargo xtask release-ci` takes the version as `v<YY.M.N>`, or the calendar's next without one.

### Removed

- The repository variable `ZEN_RELEASE_FROM_GITHUB`: the approval before `stage` is where a release
  is stopped.
- The GitLab paths of `cargo xtask release-ci` and of the CI checks: a GitLab job is refused as a CI
  of another kind, and `release-ci live` no longer creates a GitLab release.
