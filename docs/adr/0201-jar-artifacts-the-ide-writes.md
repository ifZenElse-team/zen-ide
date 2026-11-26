# ADR-0201: Jar artifacts the IDE writes

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0199](0199-project-structure-shows-the-model-of-every-language.md) (Project
  Structure; Artifacts were left to this decision), [ADR-0149](0149-maven-and-gradle.md) (Maven and
  Gradle), [ADR-0152](0152-java-run-configurations-and-the-classpath.md) (the Java run configurations and their
  class path), [ADR-0007](0007-everything-is-a-command.md) and
  [ADR-0033](0033-ai-access-to-every-api.md) (commands)

## Context

A person wants a runnable jar without writing build configuration. That takes
**Project Structure → Artifacts** (a jar from modules' output and libraries, with a manifest) and
**Build → Build Artifacts** (build, rebuild or clean one or all). A Maven or Gradle project already
knows how to package itself, and that is usually what the person wants. The platform reads ZIP
archives and writes them (`ide_settings::zip`, pure Rust with `flate2`'s Rust back end), compiles
through Maven, Gradle or `javac` for run configurations, and has no before-launch tasks yet.

## Decision

1. **Two kinds of artifact, kept in `.zen/artifacts.json`** (`{ artifacts: [...] }`, shared through
   Git): a `jar`, with `modules`, `mainClass`, `libraries` (`inside` or `nextTo`) and `output`; and a
   `build` artifact, the build's own packaging, with one module and its `task`. The file is the
   IDE's own; `.idea` artifacts are not read.
2. **The build's packaging is offered first.** Until an artifact is kept, Build Artifacts offers the
   suggestions: for Maven `<module>:mvn package`, for Gradle `<module>:gradle jar` (`bootJar` when a
   build file names `org.springframework.boot`), then `<module>:jar`, a jar of the first module with
   sources whose main class is the first `main` found. Add in Project Structure lists them in the
   same order.
3. **A jar's modules compile through their build**: Maven `compile dependency:build-classpath` with
   the runtime scope, Gradle the `zenClasspath` task of the run configurations' init script (the
   runtime class path, which holds the output folders), `javac` with the project JDK for a project
   without a build. The compilation is a run of the Run tool window; each module compiles once
   however many artifacts take it.
4. **The IDE writes the jar itself**, after the run's programs succeed, on a thread of its own:
   `META-INF/MANIFEST.MF` first (`Manifest-Version`, `Created-By`, `Main-Class`, `Class-Path` with
   72-byte lines), then the modules' files. Libraries `inside` are extracted, the first copy of a
   class wins, `META-INF/services` files are merged, manifests, signatures (`*.SF`, `*.RSA`, `*.DSA`,
   `*.EC`), `INDEX.LIST` and `module-info.class` are left out; libraries `nextTo` are copied into the
   jar's folder (a clashing name gets a number) and named in `Class-Path`. Jars are written without
   ZIP64, so a jar holds at most 65,534 entries and 4 GB.
5. **Clean removes only what the artifact wrote**: the jar and the files its manifest's Class-Path
   names in its folder, at once; a build artifact runs the build's `clean`. Rebuild cleans, then
   compiles with `clean` first.
6. **Commands:** `project.structure.artifacts`, `project.artifact.save`, `project.artifact.delete`,
   `build.artifacts.build`, `build.artifacts.rebuild`, `build.artifacts.clean`; the window action
   `view.artifacts.build` opens **Build → Build Artifacts…**.
7. **Building an artifact before a run configuration** waits for the platform's before-launch tasks;
   it is not faked here.

## Consequences

- Java and Kotlin projects get a runnable jar from the IDE in every product that has the Java
  support, without a `jar` tool on the computer.
- A multi-module Maven build compiles each chosen module in its own folder, so a module that
  depends on a sibling not installed in the local repository fails as `mvn compile` does there.
- Fat jars of more than 65,534 files need the build's own packaging until the writer learns ZIP64.
