# ADR-0149: Maven and Gradle: plug-ins for their projects, run kinds and tool windows in the platform

- **Status:** Proposed
- **Date:** 2026-10-25
- **Extends:** [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md) (project plug-ins, npm in every
  IDE), [ADR-0048](0048-run-configurations-and-the-run-service.md) (run configurations),
  [ADR-0050](0050-products-are-plugin-bundles.md) (products are bundles)
- **Relates to:** [ADR-0145](0145-jvmjizo.md) (JvmJizo), ADR-0147 (the JDK toolchain), ADR-0148 (the
  Java plug-in and its language server), [ADR-0144](0144-code-is-parsed-in-pure-rust.md)

## Context

JvmJizo needs Maven and Gradle: the build recognised, its goals and tasks
in a tool window, run configurations of their kinds, the build re-imported when it changes, the
dependencies shown. Every capability is a plug-in that works in every IDE (ADR-0050); but a plug-in
is a WebAssembly module that highlights and completes text (ADR-0031, ADR-0056) and declares what
the IDE does for its projects (ADR-0129). It cannot start processes, draw a tool window or read the
local Maven repository. npm met the same limit and became a run provider of the platform in every
IDE, with plug-ins on top (ADR-0129).

Both tools are external programs. Running them can download (Maven its plug-ins and dependencies,
the Gradle Wrapper Gradle itself), so nothing may run them before the user asks (AGENTS.md rule 10).
The semantics of the Java code come from the Java language server, which imports Maven and Gradle
builds itself (ADR-0148); the project's JDK comes from the JDK toolchain (ADR-0147).

## Decision

1. **Two plug-ins, `maven` and `gradle`**, offered for their projects through ADR-0129's markers and
   bundled by JvmJizo: `pom.xml` for Maven; `settings.gradle(.kts)` or `build.gradle(.kts)` for
   Gradle.
   - The Maven plug-in takes `pom.xml` by name, so it is more specific than the XML plug-in for that
     file. It includes the XML plug-in's code (`#[path]` includes, as the Docker plug-in includes the
     YAML highlighter), so `pom.xml` keeps highlighting, folding, the Structure view, end tags and
     problems. It adds completion of the POM from the text alone:
     - the elements the POM 4.0.0 model allows in the element around the caret;
     - the values of `scope`, `packaging`, `type` and the booleans;
     - the lifecycle phases in `<phase>`;
     - the published goals of the well-known plug-ins in `<goal>`;
     - the group and artifact ids the file already uses;
     - the properties after `${`.
   - The Gradle plug-in highlights, folds and outlines Groovy build scripts (`*.gradle`) with a
     Groovy lexer of our own (ADR-0144). It leaves `*.gradle.kts` unclaimed: plain text until a
     Kotlin plug-in reads Kotlin.
2. **The run kinds and the commands are the platform's, in every IDE**, as npm's are (ADR-0129). They
   live in `zen_workbench::maven`, `zen_workbench::gradle` and `zen_workbench::build_tools`.
   - **Run kinds:** `maven` (`pom`, `goals`, `profiles`, `offline`, `skipTests`) and `gradle`
     (`projectPath`, `tasks`; the common program arguments follow).
   - **Programs:** the project's wrapper first (`mvnw`, `gradlew`, in the build's folder or above it
     inside the project). Otherwise `mvn` or `gradle` on `PATH`, in the usual tool folders, in
     SDKMAN!'s `current`, or a Maven the IDE downloaded.
   - **Run configurations** in `.run/*.run.xml` and `.idea/runConfigurations` (types
     `MavenRunConfiguration` and `GradleRunConfiguration`) are adopted.
   - **Maven commands:** `maven.project.get`, `maven.goal.run`, `maven.options.set`,
     `maven.project.reload`, `maven.dependencies.tree`, `maven.dependency.open`,
     `maven.distribution.download`.
   - **Gradle commands:** `gradle.project.get`, `gradle.tasks.list`, `gradle.task.run`,
     `gradle.project.reload`.
   - Agents reach all of them over MCP like every command (ADR-0033).
3. **The tool windows are the platform's and show only where they apply.** Maven and Gradle sit on
   the right stripe. They are listed only while the open project has the
   marker files and the plug-in is installed and on. Without the plug-in, the run kinds and commands
   still work: a `.zen/runConfigurations.json` of kind `maven` runs in any IDE. The tool windows and
   the POM completion are what the plug-ins add.
4. **The build is read without running the tool; the tool runs only when asked.**
   - The Maven tool window reads the `pom.xml` files and their modules with our own reader
     (`roxmltree`):
     - coordinates (the parent's when missing) and `${}` from `<properties>` and `project.*`;
     - profiles, declared dependencies and modules;
     - plug-ins: those the packaging binds by default and those the build declares, with the
       documented goals and those of their executions.
   - The Gradle tool window reads the projects from the settings script (`rootProject.name`,
     `include`).
   - What needs the tool runs in a background task, only on the user's action:
     - Maven's resolved dependency tree: `mvn dependency:tree -DoutputFile=… -DappendOutput=true`,
       read by our parser (Reload, Show Dependencies).
     - Gradle's tasks: `gradle tasks --all --console=plain -q`, read by our parser (Reload).
     - Both may download, so they run only on this explicit action.
   - Downloading Maven itself is its own command: Maven 3.9.16 from Maven Central, checked against
     its SHA-512 pinned in the code, into the IDE's data folder (`tools/maven/<version>`).
5. **Two small interfaces belong to the other JVM parts:**
   - **The project's JDK:** the JDK toolchain's choice for the project (`zen_workbench::jdk::project_home`,
     ADR-0147); `BuildEnv::set_jdk` can answer first (tests). Without either, `JAVA_HOME` is used.
     A known JDK goes into `JAVA_HOME` and first on `PATH` of every Maven and Gradle process.
   - **Re-import:** `BuildEnv::set_reimport`, a function the Java language server sets (ADR-0148).
     Reload calls it; until it is set, Reload answers `reimported: false` and only reads the build
     again.
   - Both are set on `BuildEnv::global()`, the IDE's one environment that its windows and providers
     share.

## Consequences

- Maven and Gradle projects run, list and reload in every Zen IDE with the plug-ins; JvmJizo bundles
  them. A project needs no IDE-specific files: the wrapper, `pom.xml` and the settings script are
  read as they are.
- The plug-in list of the Maven tool window is not the effective POM: plug-ins inherited from a
  parent outside the project, or bound through a BOM, show only once declared or bound by the
  packaging. Their goals come from a table of the well-known plug-ins plus their executions. The
  resolved dependency tree is Maven's own and is exact.
- Gradle's task list depends on running Gradle once (Reload); until then the tool window says so.
  A tooling-model connection (the Gradle Tooling API is Java) is not used.
- `pom.xml` belongs to the Maven plug-in once it is installed; uninstalling it gives the file back
  to the XML plug-in.
- Navigation from `pom.xml` to a dependency's POM is the command `maven.dependency.open` (from the
  caret, or from a dependency row of the tool window). Go to Declaration in the editor is the
  language server's, as it is for every file.
