## repository

### Added

- The workflow **Release dry run** (`.github/workflows/release-dry-run.yml`) runs the builds of a
  release on `main` every night and by hand: the plan, every build of every product for every
  platform and the join, at the calendar's next version, with the gate without its tests on macOS
  (the workflow **Tests** runs every test each night). It names no environment, reads no secret and
  keeps only workflow artefacts; its builds record the gate as `skipped`, so nothing publishes
  them. The jobs it shares with a release are in `.github/workflows/release-parts.yml`, which
  `release.yml` calls. `cargo xtask release-ci plan`, `matrix`, `part` and `join` take
  `--dry-run`: notes still waiting and a missing changelog section are reported, not refused
  (docs/releasing.md, "The nightly dry run").

### Changed

- A release on GitHub stops in its first job, before anything is built, when its version is not the
  calendar's next or its commit is not on `origin/main`, and, when it will be staged from GitHub,
  when its ref is not protected or the environment `release` has no required reviewers or does not
  allow it. `stage` and `live` check first that the release secrets are set, printing only which
  are missing.
