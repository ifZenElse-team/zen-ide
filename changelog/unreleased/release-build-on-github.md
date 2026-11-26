## repository

### Added

- `cargo xtask release-build <ide|toolbox> <version> --trial [--platform ...]` builds one product's
  release artefact without the release checks (the source check, the quality gate, the version
  check and the release notes); its `build.json` says the gate was skipped, so `release-publish`
  refuses it. The workflow `.github/workflows/release-build.yml`, started by hand, runs it on
  GitHub-hosted runners for macOS (Apple silicon), Linux and Windows with no secret and keeps the
  results as workflow artefacts for three days (docs/releasing.md).
