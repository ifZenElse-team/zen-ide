# ADR-0200: Kotlin runs, tests and debugs through the Java kinds

- **Status:** Proposed
- **Date:** 2026-11-03
- **Builds on:** [ADR-0151](0151-kotlin-plugin.md) (the Kotlin plug-in),
  [ADR-0152](0152-java-run-configurations-and-the-classpath.md) (Java run kinds),
  [ADR-0153](0153-java-debugging-and-tests.md) (java-debug, JUnit and TestNG)

## Context

Kotlin compiles to JVM classes that the project's Maven or Gradle build makes (the Kotlin Maven and
Gradle plug-ins). Once built, a Kotlin `main` runs and a Kotlin test runs exactly as Java ones do:
the `java.application`, `junit` and `testng` kinds already build with the project's tool, take the
class path from it, run Surefire or Gradle's `test` task and read their results. What they lack is
knowing where a Kotlin file's mains and tests are, under which JVM class names, and a debugger that
binds breakpoints in `.kt` files.

The platform cannot read Kotlin itself (ADR-0050: the language is the plug-in's), and the Kotlin
plug-in already has a lexer (ADR-0151). java-debug, the Java debugger inside Eclipse JDT LS
(ADR-0153), finds the class of a line breakpoint by parsing the file as Java, so a breakpoint in a
`.kt` file has no class and is never set; and the stack frame of a class that holds inlined code
names its source as the class's own path (`com/acme/GreeterKt`, from the Kotlin compiler's SMAP),
which java-debug does not find.

## Decision

1. **A new optional export of plug-in ABI v1, `zen_jvm`:** for a file's text it answers what runs
   on the JVM, as JSON with byte offsets: the package; the file's class (`fileClass`, named by
   `@file:JvmName`, else empty); the classes with their ranges, outer ones first; the `main`
   functions with their class (empty for the file's class) and the line of the object or class
   that holds them; the test classes with their test functions and whether they are TestNG ones.
   The host turns offsets into lines and names an unnamed file class after the file
   (`App.kt` → `AppKt`, as the compiler does). `Feature::Jvm` and `TextFeatures::jvm` are its view.
2. **The Kotlin plug-in answers it with its own lexer:** top-level `fun main()` and
   `fun main(args: Array<String>)` (`vararg`, `suspend` too) in the file's class; a `@JvmStatic
   main(args)` in a named object (that object's class) or a companion object (the outer class);
   functions annotated `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory` or
   `@TestTemplate` (JUnit 5, JUnit 4, and kotlin.test, whose `@Test` is the framework's) in
   classes that are neither abstract nor objects, nested classes as `Outer$Inner`; a class
   annotated `@Test` in a file that imports TestNG has all its functions as tests.
3. **The Java kinds take `.kt` files.** `JavaRun` and `JavaTestRun` ask the build environment's
   JVM reader (`BuildEnv::jvm_file`, set by the IDE to the plug-ins' `zen_jvm`) for a `.kt` file's
   mains and tests, and give them the same gutter icons and context configurations as Java ones.
   Without the Kotlin plug-in a `.kt` file has none.
4. **Kotlin is built by the build tool.** When the module has Kotlin sources and the IDE was chosen
   to build (Maven's Runner without "Delegate", Gradle's "Build and run using: The IDE"), the
   run falls back to the build tool and the console says why before its command line; a folder
   with Kotlin and neither Maven nor Gradle cannot run. Tests always run through the build tool.
5. **Breakpoints in `.kt` files reach java-debug through a Java file of their class.** The DAP
   client takes a `SourceMap` (`zen_dap::SourceMap`) that may send a file's breakpoints for other
   files: for java-debug the IDE writes, per JVM class with breakpoints in a Kotlin file (the
   outermost class around the line, else the file's class), a small Java file in the run's own
   folder with a statement on each breakpoint's line inside a class of that name, and sends those
   breakpoints for it. java-debug reads the class name from it and sets the breakpoint on that
   class and its nested ones at the same line; the answers come back for the `.kt` file.
6. **Kotlin frames find their files.** The attach request names the project's source folders
   (`sourcePaths`), and a folder of links named after each class of each Kotlin file
   (`com/acme/GreeterKt` → `Greeter.kt`) so that a frame of a class with inlined code finds its
   file too; the IDE shows a linked frame at the file the link points to.

7. **The debugger's server starts when it is needed.** A server command the IDE sends for a debug
   session (java-debug's start, its resolve commands) keeps that server running even when no file
   it serves is open, so a project with only Kotlin sources is debugged too.

## Consequences

- Kotlin variables show as the JVM has them; evaluating an expression in a Kotlin frame needs a
  Java project for java-debug to compile it in, and says so.
- Inline functions are what the JVM's line table makes them: a breakpoint in an inline function's
  body stops only where the function runs as a call, not where the compiler inlined it; Step Over
  goes over an inlined call to the caller's next line, and frames of inlined code show the caller's
  file and line. Anonymous objects of a class with inlined code have no file in their frames.
- The links of decision 6 are symbolic links, made on macOS and Linux only.
- Another JVM language plug-in (Scala, Groovy) can answer `zen_jvm` the same way.
