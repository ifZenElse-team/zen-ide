# ADR-0152: Java run configurations and the classpath from the build tool

- **Status:** Proposed
- **Date:** 2026-10-26
- **Changes:** adds run kinds beside [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md)'s npm kind; uses
  [ADR-0147](0147-jvmjizo-app-and-jdks.md)'s JDKs

## Context

JvmJizo and every Zen IDE that opens a Java project need to run a `main` and a JAR, with
Application and JAR Application run configurations. The hard part is the classpath: the
project's compiled classes and its dependencies, which Maven, Gradle or the Java language server
(Eclipse JDT LS, ADR-0148) each know. The run must also build first, and pass a classpath that may be
long and hold spaces.

## Decision

1. Two run kinds in the platform (`zen_workbench::java_run`), for every IDE: `java.application`
   (main class, module, JRE, build before run; the VM options are the common tool arguments) and
   `java.jar` (`java -jar`). The JUnit test kind belongs to the Java plug-in (ADR-0148).
2. **The classpath comes from the build tool that builds the module**, in the same run, before the
   program: Maven `compile dependency:build-classpath` (runtime scope) with the module's
   `target/classes` first; Gradle with an init script of the IDE's (written into the run's own
   folder, never the project) whose `zenClasspath` task depends on `classes` and writes the main
   source set's runtime classpath; `javac` into the run's folder at the project's language level
   for a folder without either. The project's wrapper is preferred. JDT LS's classpath commands were
   not chosen: the server may not be running or may still be importing, it compiles into its own
   view of the project, and it would make running depend on a language server; the build tool is
   what builds and what the user's terminal would run.
3. **The classpath reaches `java` in an argument file** (`-cp @file`, one quoted argument), so that
   it has no length limit and paths may hold spaces. Maven's output is turned into that file by a
   tiny Java program of ours run with the JDK's source launcher (`java ZenClasspath.java`), so that
   nothing but the JDK is needed on any system.
4. Programs run with the configuration's JRE, else the project's JDK (`JAVA_HOME`, ADR-0147).
5. `main` methods are found in Rust (ADR-0144): `static void main(String[])`, Java 21+ instance and
   argument-less mains, implicitly declared classes, nested classes as `Outer$Inner`. Stack frames
   link to `<package>/<File>.java` in the project's source folders.
6. The Application and JarApplication configurations of an `.idea` project folder are adopted
   read-only.

## Consequences

- Running needs Maven or Gradle as building does, and they may download dependencies as they do in
  a terminal (an explicit run is the user's action).
- Debugging Java (java-debug through JDT LS) comes later.
- A Kotlin `main` can use the same kinds once a Kotlin plug-in finds its mains.
