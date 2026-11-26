# ADR-0196: Load Changes and the Sync view, fed by the build plug-ins

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md) (what a plug-in gives a project),
  [ADR-0149](0149-maven-and-gradle.md) (Maven and Gradle),
  [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the build model)

## Context

After editing `pom.xml` or a Gradle script, the IDE kept the project as it was loaded and said
nothing; the person had to remember Reload All in the Maven or Gradle tool window, and a failed
reload showed only one line there. The requirement: a floating Load Changes button in the
build script's editor (⇧⌘I), a reload by itself after changes made outside the IDE, and each
import listed with its steps, timing, errors and fixes in the Build tool window's Sync tab. Zen
IDE had no Build tool window. Maven and Gradle are plug-ins, and more build systems will come (ADR-0050).

## Decision

1. **A plug-in declares its build system** in its manifest's `project` support:
   `"build": { "files": [...], "reload": "<command>" }`, the names of its build scripts anywhere in
   the project and the command that loads the project again. The platform knows no build system by
   name: Maven declares `pom.xml` and `maven.project.reload`, Gradle its settings and build scripts
   and `gradle.project.reload`.
2. **The platform tells loaded from changed** (`zen_workbench::build_sync`, command
   `build.changes.get`): when a project is loaded it keeps each build script's size, time and hash;
   an open, unsaved script or one whose content differs on disk is changed. A change is the IDE's
   own when its editor holds unsaved edits or saved the file since the load; anything else is
   external.
3. **Which changes load by themselves** is the setting `build.reloadAfterChanges`: `external` (the
   default) loads changes made outside the IDE at once and shows Load Changes for edits; `any` also
   loads edits once they are saved. Load Changes (`view.buildChanges.load`, ⇧⌘I, and the button in
   the build script's top-right corner) saves the files and runs each changed system's reload
   command.
4. **A reload records a sync** (`SyncRun`): its steps with their time, warnings and errors, the
   file, line and column of each, and the obvious fix as a command (download the tool, choose a
   JDK, work offline) with whether to load again after it. A background step (the dependency tree,
   the task list) is read from its background load when the Sync view asks
   (`build.sync.get`); the build system's own parser turns the tool's output into messages. A sync
   counts as a load, so a reload from the Maven or Gradle tool window hides Load Changes too.
5. **The Build tool window** is a platform tool window at the bottom, offered in projects of a build
   system; its first tab is Sync. A failed sync opens it.

## Consequences

- A new build system gets Load Changes and the Sync view by declaring `build` in its manifest and
  recording its reload's steps; nothing in the platform changes.
- Changes made outside the IDE (a branch switch) reload the project without asking, which may run
  Maven or Gradle in the background; `any` makes every saved edit do so.
- The detection reads only the build scripts found at the last load and those open in an editor; a
  new module's script is seen once its parent changes.
- The Build tool window has only the Sync tab until a build runs there.
