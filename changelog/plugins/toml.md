# TOML plug-in changelog

Every release of the TOML plug-in (`plugins/toml`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-11-13

### Changed

- Taplo no longer reads the public schema catalog or downloads the schemas TOML files name: its
  schema support is off, since it would download them itself, without asking.

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for TOML, also `Cargo.lock`, `Pipfile`, `poetry.lock` and
  `uv.lock`: table headers, keys, strings with escapes, numbers, dates and times, booleans and
  comments.
