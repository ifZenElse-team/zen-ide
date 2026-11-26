# ADR-0195: A dependency manager for every build system, and the build model from Maven and Gradle themselves

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0059](0059-go-mod-dependency-checks.md) (network checks of dependencies),
  [ADR-0149](0149-maven-and-gradle.md) (Maven and Gradle), [ADR-0152](0152-java-run-configurations-and-the-classpath.md)
  (the Gradle init script), AGENTS.md golden rule 10

## Context

JvmJizo shows Maven and Gradle projects from models it builds itself: a reader of `pom.xml` that
does not compute inherited plug-ins, BOMs or the effective POM, and for Gradle only the settings
script and `gradle tasks`. People cannot search for a library, add it to a module, see newer or
vulnerable versions, or see why a version is on the classpath. The owner asked for the dependency
manager and the Maven, Gradle and Ant tool windows (2026-11-03).
Searching repositories and checking versions need the network, which the IDE uses only in the
open (golden rule 10). Go already checks its dependencies over the network (ADR-0059).

## Decision

1. **The dependency manager is a platform feature with providers from plug-ins.** The platform owns
   the Dependencies tool window, the search, the version and advisory checks, and the commands
   (ADR-0033). A build-system plug-in provides the modules, the declared dependencies with their
   scope or configuration, the repositories, and the edits: add, change version, remove, exclude.
   Maven and Gradle come first; npm, Cargo and Go can add providers later without platform changes.
2. **The build model comes from Maven and Gradle themselves.**
   - Maven: the effective POM (`help:effective-pom`) and the resolved tree (`dependency:tree`) per
     module, run in the background with the project's JDK, the Maven the person chose and the
     project's `settings.xml`.
   - Gradle: an init script, as `zenClasspath` already does (ADR-0152), writes the projects,
     included builds, tasks, source sets and the resolved configurations as JSON.
   - Results are cached per build file hash; the own readers stay for the first view before the
     tool answers and for editing build files, never as the truth about what is resolved.
   - A failure shows in the Sync view with the tool's output; the last good model stays shown.
3. **Network use is on by default, visible and can be turned off**, as in ADR-0059. Declared
   settings name every host contacted. Only public, read-only GET requests without an account, plus
   the project's own repositories with the credentials `settings.xml` or Gradle already give them
   (never sent to another host):
   - **Search** by name or coordinates: the Maven Central search index. It lags behind (it gave
     3.5.3 as the latest `spring-boot-starter-web` while 4.1.2 was out), so it is used to find
     artifacts, never to say which versions exist.
   - **Versions:** `maven-metadata.xml` of the artifact in each repository of the module, the
     authoritative list; pre-releases are offered only when the person asks or already uses one.
   - **Advisories:** OSV (`api.osv.dev`, ecosystem `Maven`), as for Go.
   - **Tool versions:** `services.gradle.org/versions` for Gradle and `maven-metadata.xml` of
     `org.apache.maven:apache-maven` for Maven downloads.
   Answers are cached with their HTTP validators; offline, the manager lists and edits declared
   dependencies and says that search and checks need the network.
4. **Editing build files keeps them as the person wrote them.**
   - `pom.xml`: edits on the XML tree that keep formatting, comments and property references; a
     version managed by a parent or a BOM is not written again.
   - Gradle: when the project has a version catalog (`gradle/libs.versions.toml`), a new library
     and its version go into the catalog and the build script gets `libs.<alias>`; otherwise a
     line is added to the `dependencies` block in the script's own DSL (Groovy or Kotlin) and
     style. Changing a version edits the place the version is declared (catalog, property, or the
     line).
   - After an edit the project reloads (the Load Changes flow).
5. **Ant** is a plug-in of its own with its own tool window; it is not part of the dependency
   manager.

## Consequences

- One dependency window and one set of commands for every build system; Maven and Gradle behave
  the same in it.
- The model matches the build exactly, at the cost of running Maven or Gradle in the background
  with a JDK; the cached model and the own readers keep the first view fast.
- Network calls on by default extend the exception ADR-0059 made for Go to the JVM; each is listed
  in the settings with its host and can be turned off.
- The Maven Central search index's lag is a known limit; versions never come from it.
- Edits to build files need tests per format: XML with comments, Groovy and Kotlin DSL, version
  catalogs, properties and BOM-managed versions.
