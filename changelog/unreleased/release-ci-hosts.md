## repository

### Changed

- The release tooling reads what it needs from CI in one place, `xtask/src/ci_host.rs`, for GitLab
  CI and GitHub Actions: the tag, the run's number for the stage's name, whether the ref is
  protected. The two rules that held only in GitLab now hold on GitHub too: the release secrets are
  refused on a ref that is not protected and in every pull request's run, and `release-ci` releases
  only its run's own tag. A CI that is neither host is refused the secrets and the release.
  Nothing changes in GitLab CI or on a maintainer's machine (ADR-0300).
