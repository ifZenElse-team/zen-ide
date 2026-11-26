# ADR-0153: Java debugging with java-debug, and Java tests run by Maven and Gradle

- **Status:** Proposed
- **Date:** 2026-10-26
- **Builds on:** [ADR-0052](0052-debugger-zen-dap-and-delve-over-reverse-dap.md), [ADR-0090](0090-one-based-places-and-lean-test-results.md),
  [ADR-0147](0147-jvmjizo-app-and-jdks.md), [ADR-0148](0148-java-plugin-and-eclipse-jdt-ls.md),
  [ADR-0149](0149-maven-and-gradle.md)

## Context

Go is debugged with Delve, a program the IDE starts and that connects back over the Debug Adapter
Protocol (ADR-0052). Java's debug adapter is **java-debug** (Microsoft, **EPL-1.0**), which is not a
program: it is a bundle that Eclipse JDT LS loads (`initializationOptions.bundles`), started by the
server command `vscode.java.startDebugSession`, which answers the port the adapter listens on. The
same bundle resolves what a launch needs from the server's model of the project
(`vscode.java.resolveClasspath`). It is published on Maven Central as one jar,
`com.microsoft.java:com.microsoft.java.debug.plugin` (0.53.1, 2024-11-04, checked to work with JDT
LS 1.63.0 on Java 25; 0.53.2 is not published there).

Java tests are written for JUnit 5, JUnit 4 or TestNG, and the project's build runs them: Maven's
Surefire or Gradle's `test` task, with the build's own class path, dependencies and settings.

## Decision

1. **java-debug is installed with the server**, as a file of its archive install
   (`install.files: [{ url, sha256, path }]`, downloaded and checked after the archive is unpacked)
   and loaded by it (`initializationOptions.bundles` with `${serverFolder}`). A server is installed
   only when all its files are there. The jar is EPL-1.0 like JDT LS; it runs as part of the
   server's process, never linked with our code (clean-room, ADR-0002).
2. **A debug adapter that a language server runs** is a kind of adapter of the platform
   (`zen_run::AdapterLine::server`): the IDE fills the `launch` arguments with the answers of the
   server's commands it names (`resolve`), asks the server to start the adapter (`command`) and
   connects to the port it answers, on the loopback interface. `"request": "attach"` attaches.
3. **A program debugged by attaching** (`zen_run::DebugPlan::debuggee`): a command started after
   the build that waits for the debugger, whose output and tests are the session's; the adapter
   connects once it prints its `ready` line (`Listening for transport dt_socket at address`), and
   its exit code is the run's. Maven tests are debugged in Maven's own JVM (`MAVEN_OPTS` with the
   JDWP agent and `-DforkCount=0`): Surefire swallows what a forked JVM prints before its tests, so
   the ready line of a forked test JVM is never seen. Gradle's test JVM gets the agent from our init
   script.
4. **Exception breakpoints** are the platform's: the filters the adapter offers (java-debug's
   `caught`, `uncaught`), chosen with `debug.exceptionBreakpoint.set` and sent at the start and when
   they change; `debug.exceptionBreakpoint.list` shows them.
5. **The `junit` and `testng` run kinds are the platform's** (`zen_workbench::java`), in every IDE,
   with the build environment of the Maven and Gradle kinds (ADR-0149: the wrapper first, the
   project's JDK). The tests of a file for the gutter are read by our own small lexer (ADR-0144):
   classes with methods annotated `@Test`, `@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`,
   `@TestTemplate`, or a class annotated `@Test` (TestNG). Scopes: all, a package, a class, a method,
   a list (Rerun Failed Tests).
6. **Results come from the build's own output.** Maven: Surefire's plain report on the console
   (`-Dsurefire.reportFormat=plain -Dsurefire.useFile=false`), a test's line with its time and
   status, its stack trace after a failure. Gradle: an init script of ours (`-I`, written once to the
   temporary folder) whose test listener prints one `##zen-test` JSON line per event, the test's
   output and failure lines included; the console hides these lines. Each class is a group of the
   tree, its methods the tests, a parameterized test's runs its subtests. A stack frame in the project
   links to its source; a library's frame keeps no link, so that a failure's place is the project's
   frame, and its message starts at the exception.
7. **A server that needs a newer Java than any found** says so (`javaNeeded` in `lsp.servers.list`);
   the editor banner offers **Download JDK 25** (`toolchain.jdk.download`, ADR-0147). The JDK is
   looked for in the JDK store, and a server that failed for want of it starts again once it is
   found.

## Consequences

- Debugging a test or a program needs the Java plug-in's server running for the project; it is
  started by opening a Java file. Without it, the start says so.
- Maven tests debugged in Maven's JVM run without a fork: tests that rely on the fork's own system
  properties or class loader may behave differently under the debugger.
- Hot code replace (java-debug's `redefineClasses` after a build) is not done yet.
- The Application and JAR Application kinds (ADR-0152) debug the `java` line they run, with the JDWP
  agent waiting, and java-debug attaches to it (`zen_workbench::java::debug::attach_plan`), so a
  debug run has the run's class path, JRE and options. `application_plan`, which has java-debug
  launch a `main` class with the class path the server resolves, stays for a kind without a build
  of its own.
- The real-tool tests are ignored by default (`tests/java_debug_and_tests.rs`): Maven and Gradle
  runs, Rerun Failed Tests, a test and a program debugged, an exception breakpoint.
