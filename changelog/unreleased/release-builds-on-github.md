## repository

### Added

- The workflow `.github/workflows/release.yml` builds the release of a tag `v<YY.M.N>` on
  GitHub-hosted runners and publishes nothing yet (ADR-0300, step 3): a plan, the full gate once on
  macOS, the application bundles in three macOS jobs, one Linux job for every other product and
  platform, and a join that keeps the whole release as one workflow artefact. New steps of
  `cargo xtask release-ci`: `matrix`, `gate`, `part <ide|toolbox|plugins> <platform>` and `join`.
  A part records the gate as `deferred` in `build.json`; `join` takes it only when its files match,
  its commit, tree and compiler are those of the gate's pass and the parts are exactly the release,
  and only then records `passed` (docs/releasing.md, "Releasing from a tag on GitHub").
