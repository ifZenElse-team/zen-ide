# Maven plug-in changelog

Every release of the Maven plug-in (`plugins/maven`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-11-05

### Added

- **New Project** has a **Maven Archetype** type under Generators: the JDK, Catalog with **Manage
  catalogs…**, Archetype with **Add…**, Version, Additional Properties, and GroupId and ArtifactId
  under Advanced Settings. Maven generates the project and its output is shown; it uses the network.
  It needs an IDE from this release on.
- **New Module** and **Module from Existing Sources** list the module in the parent's `<modules>`
  and make the parent's packaging `pom`.

## 0.1.0 - 2026-10-25

### Added

- The first release, for Maven projects; JvmJizo comes with it, and every other IDE offers it in a
  notification when a project with a `pom.xml` at its root opens. It is also in the Marketplace.
- The Maven tool window at the right, with:
  - the build's projects and modules, their lifecycle phases, plug-ins with their goals, and
    dependencies;
  - the profiles, with ticks;
  - Reload All Maven Projects, Run, Toggle Offline Mode, Toggle 'Skip Tests' Mode and Show
    Dependencies (the resolved tree).
  Double-click a phase or goal to run it, or a dependency to open its POM.
- `pom.xml` read as XML, with completion of the POM: elements, values, phases, goals, coordinates and
  `${properties}`.
