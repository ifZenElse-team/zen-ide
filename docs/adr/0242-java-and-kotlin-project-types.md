# ADR-0242: Java and Kotlin project types come from their plug-ins

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0147](0147-jvmjizo-app-and-jdks.md),
  [ADR-0237](0237-new-project-wizard-and-project-generators.md)

## Context

The New Project wizard lists a language module's types, Empty Project, then plug-ins' types under
Generators. Java and Kotlin have no language module: they are WebAssembly plug-ins, and a plug-in
cannot look for JDKs (ADR-0147). Yet a language's type belongs above Empty Project, its form needs
the computer's JDKs with Download, and GroupId and ArtifactId are folded away.

## Decision

1. **The types are the plug-ins' own `projectGenerators`,** not `ModuleHost::project_generator`:
   the type is there exactly when the plug-in is, in every IDE, and JvmJizo's built-in modules are
   off. Kotlin's plug-in compiles the same source file as Java's (`plugins/java/src/new_project.rs`).
2. **`language: true`** in a declaration puts a type above Empty Project, after the modules' types,
   instead of under Generators. It extends ADR-0237's order; other plug-ins stay under Generators.
3. **A field of type `jdk`** is filled in by the IDE from `toolchain.jdk.list`: JDKs with a compiler
   by their home, the IDE's default first chosen. The wizard adds the Download entries. The value is
   the home, and the generator also gets `<id>Release`, the feature release. A required `jdk` field
   with no JDK refuses Create with its own reason.
4. **The chosen JDK becomes the project's** (`jdks.json`) when the project has been created.
5. **`advanced: true`** puts a field under a folded Advanced Settings.
6. **The generator only writes text, with no network.** A project has no Gradle or Maven wrapper:
   Gradle's wrapper needs a binary jar, and a plug-in's answer holds text.
   The IDE finds or downloads the tool as for any project (docs/guide/26-maven-and-gradle.md).
7. **Kotlin has no IDE build system,** as the IDE has no way to compile Kotlin without a build tool
   (`workbench-java-kotlin-needs-build`): its sample could not run.
8. **The Kotlin version is pinned in the plug-in** and moves with its releases: the newest stable
   release at the time, checked with `gradle build` and `mvn compile`.

## Consequences

- Any generator, Spring Boot's among them, can ask for a JDK with one line of data.
- A Gradle project needs Gradle on the computer, or Download Gradle, until someone runs
  `gradle wrapper`.
- `Field` has one more member: Rust code that builds fields names it.

## Alternatives considered

- **Built-in generators in `zen_workbench`:** they would show in an IDE without Java support.
- **A language module for the JVM:** a second home for what the plug-ins already give (ADR-0147).
- **A `dynamicForm` plug-in that gets the JDK list as input:** every generator would rebuild the same
  field, and Download would need its own protocol.
