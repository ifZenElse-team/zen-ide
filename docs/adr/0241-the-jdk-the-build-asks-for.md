# ADR-0241: A project without a chosen JDK uses the JDK its build asks for

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0149](0149-maven-and-gradle.md) (Maven and Gradle),
  [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the build model),
  [ADR-0205](0205-trusted-projects.md) (trusted projects), AGENTS.md golden rule 10

## Context

A project with no chosen JDK used `JAVA_HOME`'s JDK, else the newest one on the computer. On a
Spring Boot Maven project at Java 17 that meant JDK 27, and `compile` failed inside `javac` with
Lombok (`TypeTag :: UNKNOWN`). A build states the Java it wants; the IDE read that and only showed
it in Project Structure.

## Decision

1. **The level.** The build's level is the highest one its files state: for Maven,
   `maven.compiler.release`, `maven.compiler.source`, the compiler plug-in's `release` or `source`,
   then `java.version` (the property a Spring Boot parent gives the compiler), across the modules;
   for Gradle, the toolchain, `JavaVersion`, `sourceCompatibility` or `release` of each project's
   build script. It comes from the own readers, so it needs no build to run. A level written in a
   parent POM outside the project, a convention plug-in or a catalog is not seen: the JDK then
   stays the default.
2. **The choice.** With no chosen JDK, the project uses the installed JDK of exactly that feature
   release, with a compiler. It is worked out on every read and never stored, so it follows the
   build. A newer JDK is not a match: `--release` makes it compile the language, not run every
   annotation processor, which is the failure above.
3. **A chosen JDK is never replaced.**
4. **No match.** The default stays, and a notification once a window offers **Download JDK n** and
   **Choose a JDK**. Nothing is downloaded until the person presses it (golden rule 10).
5. **One JDK.** Runs, tests, Maven, Gradle and the language server's project runtime
   (`java.configuration.runtimes`) all read the same project JDK; the server itself still runs on the
   Java it needs. When the project JDK changes, the server restarts with a notice.
6. **The language level** with none set follows the build when the JDK can compile it, so an IDE
   compile on a newer JDK passes the build's `--release`.
7. **A mismatch** is a JDK older than the level, or a newer one the project did not choose. It shows
   in the build tool windows and `*.project.get`. A newer JDK the person chose is reported in the
   state but shows no warning: it was their decision and usually compiles.
8. **Gradle toolchains.** Gradle finds or provisions the toolchain's JDK itself, so a toolchain
   level is never a mismatch. A toolchain below what the build's Gradle runs on (Gradle 9 needs
   Java 17) is not taken as the project's JDK.

## Consequences

- Opening a project may give it another JDK than before, without anyone choosing: its build's.
- Enforcer rules, Maven toolchains and profiles that change the level are not read; ADR-0195's
  effective POM can replace the own reader here later.
- The JDK file of the IDE stays as it was: only a person's choice is written.
