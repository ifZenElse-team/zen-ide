# Gradle plug-in changelog

Every release of the Gradle plug-in (`plugins/gradle`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-11-05

### Added

- **New Module** and **Module from Existing Sources** add `include` to `settings.gradle(.kts)`, and
  create that file when the build has none. It needs an IDE from this release on.

## 0.1.0 - 2026-10-25

### Added

- The first release, for Gradle builds; JvmJizo comes with it, and every other IDE offers it in a
  notification when a project with a `settings.gradle(.kts)` or `build.gradle(.kts)` at its root
  opens. It is also in the Marketplace.
- The Gradle tool window at the right: the build's projects and, after Reload All Gradle Projects,
  their tasks by group; double-click a task to run it.
- Groovy build scripts (`*.gradle`) highlighted, folded, and outlined in the Structure view.
