# ADR-0197: Ant: a plug-in for its build files, the run kind and the tool window in the platform

- **Status:** Proposed
- **Date:** 2026-11-03
- **Extends:** [ADR-0149](0149-maven-and-gradle.md) (Maven and Gradle)
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (products are bundles),
  [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md) (project plug-ins),
  [ADR-0147](0147-jvmjizo-app-and-jdks.md) (JvmJizo and the JDKs), ADR-0195 (Ant is a plug-in of its own)

## Context

JvmJizo should run Ant builds: the build files and their targets in a tool
window, a target run with the project's JDK, each build file's own JDK, classpath, memory and
properties, and `build.xml` edited with completion and navigation. A WebAssembly plug-in can
complete text but cannot start processes, draw a tool window or move the caret to another place
(ADR-0031), the same limit Maven and Gradle met (ADR-0149). Ant has no wrapper, and the list
of build files and their settings belongs with the project.

## Decision

1. **A plug-in, `ant`**, offered for projects with a `build.xml` at the root (ADR-0129's markers)
   and bundled by JvmJizo, available to every IDE from the catalog. It takes `build.xml` by name
   and reads it as XML (the XML plug-in's code, as Maven's does), with completion of tasks,
   attributes, target names, properties and references.
2. **The platform does what a module cannot**, as for Maven and Gradle: `zen_workbench::ant` reads
   build files with an XML parser, runs Ant as a run kind `ant` (`AntRun`), goes from a target's
   name to the target for Go to Declaration, and owns the commands `ant.*`; the UI draws the Ant
   tool window, offered only while the plug-in is on and the project has a build file.
3. **The build files and their settings are kept in the project's `.zen/ant.json`**:
   `{ "buildFiles": [{ "path", "jdk", "classpath", "maxMemory", "properties": [{ "name", "value" }] }],
   "filterTargets" }`, paths relative to the project. Until the file exists, the project's
   `build.xml` is listed; the IDE writes it only when the user adds or removes a build file, sets
   the filter or a build file's settings.
4. **Ant is found, never fetched silently**: `PATH`, the usual tool folders, `ANT_HOME/bin`,
   SDKMAN, then the IDE's own copy. **Download Ant** fetches Apache Ant 1.10.18 from
   `archive.apache.org` only when the user asks, checks it against the SHA-512 Apache publishes
   for it, and unpacks it into `<data>/tools/ant/1.10.18`.
5. **A run passes the settings as Ant takes them**: `-buildfile <file>`, `-lib <entry>` for the
   extra classpath, `-Dname=value` for the properties, `-Xmx<n>m` appended to `ANT_OPTS` for the
   memory, and `JAVA_HOME` with the JDK's `bin` first on `PATH`. The output goes through the Run
   tool window's own link detection, so the absolute paths Ant and `javac` print are links.

## Consequences

- Ant works in every IDE with the plug-in; JvmJizo ships it.
- `.zen/ant.json` is a new project file; it may be committed with the project like the run
  configurations.
- Running a target before or after a build or before a run configuration, a shortcut for a target,
  and a Properties dialog in the tool window need platform mechanisms that do not exist yet; they
  are later work.
