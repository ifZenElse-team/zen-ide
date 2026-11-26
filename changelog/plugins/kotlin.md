# Kotlin plug-in changelog

Every release of the Kotlin plug-in (`plugins/kotlin`), newest first; the format is in
[../README.md](../README.md).

## 0.4.0 - 2026-11-13

### Added

- **Convert Java File to Kotlin File** (Code menu, ⌥⇧⌘K) and the offer to convert Java pasted into a
  Kotlin file: the plug-in's own converter, without a language server; what it cannot carry over is
  reported with its line and kept in a comment. It needs an IDE from this release.

## 0.3.0 - 2026-11-05

### Added

- **Settings → Editor → Code Style → Kotlin**: spaces, hard wrap, trailing commas, new lines,
  blank lines and the import layout, as the `.editorconfig` properties Kotlin LSP reads; with the
  Project scheme they are kept in the project's `.editorconfig`. It needs an IDE from this release
  on.
- Kotlin live templates in Settings → Editor → Live Templates: `main`, `maina`, `maino`, `sout`,
  `serr`, `soutv`, `fun0`, `fun1`, `fun2`, `void`, `ifn`, `inn`, `iter`, `fori`, `interface`,
  `singleton`, `closure`, `anonymous`, `exfun`, `exval` and `exvar`.
- Postfix templates: `.val`, `.var`, `.if`, `.else`, `.not`, `.nn`, `.notnull`, `.null`, `.for`,
  `.iter`, `.while`, `.when`, `.return`, `.throw`, `.sout`, `.try`, `.cast`, `.par`, `.lambda`,
  `.let`, `.arg`, `.spread` and `.assert`.
- Surround With: if, if / else, while, do / while, for, try / catch, try / finally,
  try / catch / finally, synchronized, Runnable, a function literal, (expr), !(expr) and "${expr}".
- `/**` and Enter above a declaration puts a KDoc stub with `@param`, `@return` and `@throws`
  (from `@Throws`).
- Complete Current Statement adds the block of a statement or declaration.
- It needs an IDE from this release on; an older one shows none of the templates.
- **Navigate → Test** (⇧⌘T) goes from a class or function to its tests and back: `Cart.kt` in
  `src/main/kotlin` (or `src/main/java`, `src/commonMain/kotlin`, `src/jvmMain/kotlin`) is tested by
  `CartTest`, `CartTests`, `CartIT` or `TestCart` in the matching test folder; extension functions
  and test names in backticks count.
- **Create Test** writes a JUnit 5, JUnit 4, TestNG or kotlin.test test class, the ones the build
  names first, with `setUp`, `tearDown` and a `@Test` function for each function chosen. Needs a
  Zen IDE released after 26.11.2; an older one ignores it.
- **New Project** has a **Kotlin** type: Build system Gradle (Kotlin or Groovy DSL) or Maven, the
  JDK, Add sample code, and GroupId and ArtifactId under Advanced Settings. It writes the build
  files itself, without the network. It needs an IDE from this release on.
- **Use compact project structure** keeps the sources directly in `src`. New offers the class
  templates only in source folders, as one row, **Kotlin Class/File**.
- **New Module** offers the same type; a Gradle module gets no `settings` file of its own.

## 0.2.0 - 2026-11-03

### Added

- The IDE finds Kotlin `main` functions and tests with the plug-in: a top-level `main`, a
  `@JvmStatic main` in an object or companion object, and test functions of JUnit 5, JUnit 4,
  kotlin.test and TestNG, each with a ▶ in the gutter and in Run Context Configuration. They run,
  are tested and are debugged with Maven and Gradle as Java code is, with breakpoints in `.kt`
  files.

## 0.1.0 - 2026-10-25

### Added

- The first release, offered when a Kotlin file (`.kt`, `.kts`) opens and in the Marketplace.
- Kotlin and Kotlin scripts, including Gradle's Kotlin DSL (`*.gradle.kts`), read by the IDE's own
  Kotlin reader:
  - highlighting, with string templates, raw strings, KDoc, annotations and labels;
  - folding of blocks, comments and imports;
  - the Structure view of classes, objects, functions and properties.
- Two Kotlin language servers to choose from in Settings → Language Servers: the Kotlin team's kotlin-lsp
  (263.7084.0), the default, and fwcd's open-source Kotlin Language Server (1.3.13). Either one is
  downloaded into the IDE's folder when you click Install and checked against its SHA-256. The IDE
  says where it offers kotlin-lsp whose software it is and under whose terms. The server
  brings problems, completion, navigation, rename, formatting and Optimize Imports.
- The file templates Kotlin File, Class, Data Class, Interface, Enum, Object and Script, with the
  folder's package.
