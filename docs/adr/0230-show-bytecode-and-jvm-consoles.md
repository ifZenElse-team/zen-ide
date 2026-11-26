# ADR-0230: Show Bytecode, the JShell console and Kotlin with the installed compiler

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

A Java developer needs to try things out without leaving the IDE: View → Show Bytecode
shows the compiled form of the class at the caret, Tools → JShell Console opens an interactive Java
console with the module's classpath, and Kotlin has a REPL and runnable scratch files. Zen IDE had
the pieces around them (the project JDK, the module classpath of an Application run from
[ADR-0152](0152-java-run-configurations-and-the-classpath.md), single-file scratch runs from
[ADR-0212](0212-scratch-files-run-through-language-providers.md), read-only virtual documents and
the Terminal tool window) but none of these features. The IDE ships no Kotlin compiler, and the
clean-room rule ([ADR-0002](0002-open-source-clean-room-pure-rust.md)) leaves the JDK's own tools as
the way to read class files the user compiled.

## Decision

1. **Show Bytecode** (`java.bytecode.show`) runs the project JDK's `javap -c -p -v` on the user's
   own compiled class, never on a third-party JAR. The class at the caret is the innermost type
   around it (its binary name, `Outer$Inner`); its `.class` is looked up in the output folders the
   build declares (the project model), then in the conventional ones (Maven `target/classes`,
   `target/test-classes`; Gradle `build/classes/<language>/main|test`). javap runs on a thread of
   its own; the result is a read-only virtual document (`VirtualKind::Bytecode`) opened in the
   editor's right split without taking the focus, and it follows the caret to another class while
   it is open. A Maven or Gradle class that is not compiled is not compiled behind the user's back:
   the command answers `compiled: false` with the build command, which a notification offers as
   **Build**. A file outside Maven and Gradle (a scratch, a plain project) is compiled alone with
   `javac -sourcepath <its source root>` into a private temporary folder, removed afterwards.
2. **The JShell console and the Kotlin REPL run in a Terminal tab**, not in a console of their own:
   the PTY gives JShell its own line editing, history (kept by JShell between sessions) and
   completion, and agents reach it with `terminal.session.send` and `terminal.session.read`. Before
   it starts, the module's classpath is made exactly as for an Application run (ADR-0152), as a
   background task; `jshell` has no `@argfile` support, so the IDE reads the classpath file and
   passes `--class-path` itself. `*.open`, `*.restart` (which builds the classpath again) and
   `*.stop` are commands for each console. Building and loading the classes runs project code, so
   a console with the classpath is refused in a project not trusted (ADR-0205), for a person too;
   `classpath: false` starts the JDK alone. Show Bytecode runs no project code (`javac -proc:none`,
   `javap`, and a build only when asked) and refuses an agent a hidden source or class file.
3. **Kotlin uses the `kotlinc` installed on the computer** (`KOTLIN_HOME`, PATH, Homebrew, SDKMAN),
   and the IDE never downloads it. The REPL is `kotlinc` in a Terminal tab with `-cp`. A Kotlin
   scratch with a `main` runs as the new kind `kotlin.file`: `kotlinc <file> -include-runtime -d
   <run folder>/kotlin-scratch.jar`, then the project JDK runs its class; with the module's
   classpath, `kotlinc -cp @classpath.args` and a classpath that joins the JAR and the module's
   (the classpath helper of ADR-0152 now also reads its own quoted argument files). Without
   `kotlinc` the ▶ and the REPL say what is missing. The Kotlin compiler from the project's build
   (Maven or Gradle caches) is not used yet.

## Consequences

- The features need nothing the user does not have: a JDK, and for Kotlin a `kotlinc`.
- The consoles look like terminals rather than editor-backed consoles: there is no editor-style
  input with syntax highlighting, and Restart and Stop are commands (Find Action) rather than
  toolbar buttons.
- Show Bytecode shows exactly what the build produced, and so is empty until the module is built.
