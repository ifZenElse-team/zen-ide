## repository

### Added

- The workflow `release.yml` can stage, verify and release a tag from GitHub (ADR-0300, steps 4
  to 6), once the repository variable `ZEN_RELEASE_FROM_GITHUB` is `true` and the environment
  `release` holds the secrets: `stage` and `live` in that environment, the secrets given only to
  the step that signs or sends; `verify`; a draft GitHub release with the notes, the files and the
  checks, published after the release is live. Without the variable a tag's run still ends with
  the built release.
- `cargo xtask release-ci credentials` and the hand-started workflow `release-credentials.yml`
  check the signing key and the upload token against the release server, sending nothing.
- `cargo xtask release-ci notes` writes the text of the GitHub release from a stage.
- `cargo xtask release-publish --run <run id>` publishes a release built by the workflow from a
  maintainer's machine, fetched with `gh run download`; `--pipeline` and `--job` keep fetching
  from GitLab (docs/releasing.md, "Releasing from a tag on GitHub").
