# Ant plug-in changelog

Every release of the Ant plug-in (`plugins/ant`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-03

### Added

- The first release, for Ant builds; JvmJizo comes with it, and every other IDE offers it in a
  notification when a project with a `build.xml` at its root opens. It is also in the Marketplace.
- The Ant tool window at the right, with:
  - the build files and their targets, the default target marked;
  - Add (**+**) and Remove build files, Filter Targets (only those with a description), Run and
    Collapse All.
  Double-click a target to run it: Ant runs with the project's JDK, its output in the Run tool
  window with links to the files it names.
- Each build file's own JDK, extra classpath, maximum memory and properties for its runs.
- Run configurations of kind Ant.
- `build.xml` read as XML, with completion of tasks and their attributes, target names (in
  `depends`, `default`, `antcall`), `${properties}` and references, and Go to Declaration from a
  target's name to the target.
- Ant is found on `PATH`, in `ANT_HOME`, SDKMAN and the usual folders; when it is missing, the tool
  window says so and offers Download Ant (Apache Ant 1.10.18, checked against its SHA-512).
