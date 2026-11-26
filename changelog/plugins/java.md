# Java plug-in changelog

Every release of the Java plug-in (`plugins/java`), newest first; the format is in
[../README.md](../README.md).

## 0.3.1 - 2026-11-06

### Fixed

- Completion with a caret inside a multi-byte character no longer panics, which switched the
  plug-in off for the session.

## 0.3.0 - 2026-11-05

### Added

- **Settings → Editor → Code Style → Java**: indents, spaces, hard wrap, brace placement,
  wrapping, new lines and blank lines reach the Eclipse JDT Language Server's formatter as an
  Eclipse formatter profile, kept by the IDE or, with the Project scheme, in the project's
  `.zen/java-formatter.xml`; the import order and the class counts that make an import use `*`
  reach Optimize Imports. It needs an IDE from this release on.
- Java live templates in Settings → Editor → Live Templates: `psvm`, `main`, `sout`, `souf`,
  `soutv`, `serr`, `fori`, `iter`, `itar`, `itli`, `ifn`, `inn`, `psf`, `psfs`, `psfi`, `prsf`,
  `thr`, `lambda` and `sw`, with variables Tab goes through; they replace the completion snippets
  of 0.1.0.
- Postfix templates: `.var`, `.val`, `.if`, `.else`, `.not`, `.nn`, `.notnull`, `.null`, `.for`,
  `.iter`, `.fori`, `.forr`, `.while`, `.return`, `.throw`, `.sout`, `.serr`, `.try`, `.cast`,
  `.new`, `.par`, `.lambda`, `.arg`, `.switch`, `.synchronized` and `.assert`.
- Surround With: if, if / else, while, do / while, for, try / catch, try / finally,
  try / catch / finally, synchronized, Runnable, a block, (expr), !(expr) and ((Type) expr).
- `/**` and Enter above a declaration puts a Javadoc stub with `@param`, `@return` and `@throws`.
- Complete Current Statement adds the `;`, or the block of a statement, class or method.
- It needs an IDE from this release on; an older one shows none of the templates.
- **Navigate → Test** (⇧⌘T) goes from a class or method to its tests and back: `Cart` in
  `src/main/java` is tested by `CartTest`, `CartTests`, `CartIT` or `TestCart` in `src/test/java` of
  the same module and package, `total` by `total…` or `testTotal…`.
- **Create Test** writes a JUnit 5, JUnit 4 or TestNG test class, the ones the build names first,
  with `setUp`, `tearDown` and a `@Test` method for each method chosen. Needs a Zen IDE released
  after 26.11.2; an older one ignores it.
- **Code → Generate…** (⌘N) in a Java class offers Constructor, Getter, Setter, Getter and Setter,
  `equals()` and `hashCode()`, `toString()`, Override Methods, Implement Methods and Delegate
  Methods from the Eclipse JDT Language Server, each with a member chooser of the class's fields or
  methods. **Override Methods…** (⌃O) and **Implement Methods…** (⌃I) open their chooser directly.
  It needs an IDE from this release on.

### Changed

- The Eclipse JDT Language Server compiles the project with the project's JDK, the one its builds
  and runs use. It needs an IDE from this release on.
- **New Project** has a **Java** type: Build system IDE, Maven or Gradle (Groovy or Kotlin DSL), the
  JDK, Add sample code, and GroupId and ArtifactId under Advanced Settings. It writes the build
  files itself, without the network. It needs an IDE from this release on.
- **Maven** is the default build system; the choices are a row of buttons. New offers the class
  templates only in source folders: **Java Class** is one row, **package-info.java** and
  **module-info.java** have those names.
- **New Module** offers the same type; a Gradle module gets no `settings` file of its own.

## 0.2.0 - 2026-11-03

### Added

- **Go to Declaration** into a JDK or library class opens it: its sources when the library has
  them, otherwise the language server's view of the class file, read-only.
- The Eclipse JDT Language Server starts with the project's Lombok as its agent, and its
  annotation processing of Gradle projects follows the project's annotation processing settings.
  It needs an IDE from this release on.

## 0.1.1 - 2026-10-26

### Fixed

- The Eclipse JDT Language Server no longer writes `.project`, `.classpath` and `.settings` into a
  Maven or Gradle project: what it keeps of an imported project stays in the IDE's cache.

## 0.1.0 - 2026-10-25

### Added

- The first release, for every IDE: JvmJizo comes with it, the others offer it when a `.java` file
  opens.
- Highlighting of Java 25: keywords, the contextual ones only where they are keywords (`record`,
  `sealed`, `non-sealed`, `permits`, `var`, `yield`, `when`; the `module-info.java` words), types,
  methods, constants, annotations, strings, characters, text blocks, numbers, comments and Javadoc.
- Folding of bodies, blocks, lambdas, text blocks, comments and the import list; the Structure view
  with classes, interfaces, enums, records and annotation types, nested ones, fields, methods,
  constructors, enum constants and record components; completion of keywords, the file's names and
  `psvm`, `main`, `sout`, `serr`, `fori`, `iter`, `psf`.
- **New → Java Class**, **Interface**, **Enum**, **Record**, **Annotation**, **package-info** and
  **module-info**, the package from the file's folder.
- Eclipse JDT Language Server 1.63.0 with java-debug 0.53.1, downloaded into the IDE's own folder
  when you ask, run on Java 25 or newer (a JDK 25 is offered for download when none is found) with
  its data in the IDE's cache: debugging with java-debug, problems, completion, navigation, Find Usages,
  Rename, Reformat Code, Optimize Imports, quick fixes, Quick Documentation, the call and type
  hierarchies, and Maven and Gradle projects imported by the server.
