# ADR-0235: The dependency analyzer: one model from Maven and Gradle, Exclude in the build file

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the build model),
  [ADR-0196](0196-load-changes-and-the-sync-view.md) (Load Changes and the Build tool window)

## Context

The Maven tool window shows `dependency:tree` without what the tool left out, and the Gradle tool
window shows no dependencies at all. People cannot see why a version is on the classpath, and
fixing a conflict means reading text output and editing the build file by hand. The requirement
is a dependency analyzer: a module's dependencies as a list and a tree, conflicts marked, filters
by scope and to conflicts, and Exclude.

## Decision

1. **One model, behind a provider.** `zen_workbench::dependency_analysis` owns the model (modules,
   nodes with requested and selected version, scope or configuration, children, whether the place
   lost a conflict, whether it adds nothing, the reason, the tool's own words), the list and tree
   filters and the commands `build.dependencies.analyze`, `build.dependencies.declaration` and
   `build.dependencies.exclude`. A build system implements `DependencyAnalyzer`: how to load, how to
   read the output, which files can declare a dependency, how to find a declaration and how to add
   an exclusion. Another build system adds a provider and nothing in the platform or the UI changes.
2. **The analysis is a run of its own, only when asked for.** It is a background load next to the
   dependency tree of a reload, because it needs other output and the person may never open it.
   - Maven: `dependency:tree -Dverbose` read from standard output. With `-Dverbose`,
     `maven-dependency-plugin` 3.7.0 keeps only the last module in `-DoutputFile` even with
     `-DappendOutput=true` (observed), so the file used for the plain tree cannot be used. A pinned
     plug-in version without verbose support still gives a tree, without what was omitted.
   - Gradle: an init script (`gradle/zen-dependencies.gradle`) adds the task `zenDependencies`,
     which prints one line of JSON per project: for each resolvable compile and runtime classpath,
     the components of the resolved graph with their selection reasons and each edge with the
     version requested. Failed dependencies come through as nodes, so one missing library does not
     lose the rest of the analysis. The tree is expanded from the graph with a limit of nodes per
     configuration, because a graph can repeat a subtree many times; a repeat past the limit is marked
     as a duplicate, and a cycle ends in a marked node.
3. **A conflict is a place that asked for another version than the one used.** Requests for a
   dynamic version or a range (`1.+`, `[1,2)`) are not conflicts. A version set by dependency
   management, a constraint, a rule or a force is one too, with its own reason. `omitted` is another
   thing: Maven drops the place (conflict, duplicate, cycle), while in Gradle the winner stands at
   the place that lost.
4. **A list and a tree in the Build tool window.** The analyzer is a tab next to Sync
   (`view.dependencyAnalyzer.open`, with buttons in the Maven and Gradle tool windows). It is
   not an editor tab: the editor area has no general kind of tab for a tool's view, and
   the Build tool window already belongs to the build systems and is wide enough for a list and its
   details. The tree of a Gradle module has a folder per configuration, Maven's one tree of scopes.
5. **Exclude goes to the direct dependency, in the file as written.** Maven: `<exclusions>` under
   the `<dependency>` that brings the library, in the module's POM or a parent POM on disk. Gradle:
   `exclude group: 'x', module: 'y'` or `exclude(group = "x", module = "y")` in that declaration's
   closure, creating one when there is none, on every line that declares it, for strings, the map
   notation, `project(...)` and version catalog aliases. Without a named direct dependency, every one
   that brings the library gets the exclusion. Comments, indentation and line endings stay. Then the
   project and the analysis load again (ADR-0196); `noReload` leaves it to Load Changes.
6. **The edits live in `dependency_analysis/edits.rs`** for now, small and independent of the
   dependency manager's edits, built at the same time. They are to be unified once
   both are merged.

## Consequences

- The analyzer needs Maven or Gradle and may download; offline it works from what the tool has.
- The analysis is not cached by build file hash as ADR-0195 asks of the model: it stays in memory
  until the next analysis, and Refresh or Exclude runs it again.
- Parsing the verbose text depends on the plug-in's wording; words it does not know stay in the note.
- Gradle statements inside `if` blocks and declarations made by convention plug-ins are not edited:
  Exclude says no build file declares them.
