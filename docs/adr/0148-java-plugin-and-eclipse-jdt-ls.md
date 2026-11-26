# ADR-0148: The Java plug-in, and Eclipse JDT LS installed from its archive

- **Status:** Proposed
- **Date:** 2026-10-25
- **Builds on:** [ADR-0031](0031-plugins.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0055](0055-language-servers.md), [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md),
  [ADR-0128](0128-a-plugin-installs-its-language-servers.md), [ADR-0144](0144-code-is-parsed-in-pure-rust.md),
  [ADR-0147](0147-jvmjizo-app-and-jdks.md)

## Context

JvmJizo is Java-first (ADR-0145), and Java, like every language, is a plug-in for every Zen IDE
(ADR-0050). What a file needs as it is typed (highlighting, folding, the Structure, completion of
what is in the file) the IDE parses itself, in Rust (ADR-0144); what needs the whole project and its
build (types, problems, navigation, refactoring, formatting) a language server gives, as gopls does
for Go. For Java that server is the Eclipse JDT Language Server, open source (EPL 2.0), the one most
editors use, and the only one that imports Maven and Gradle projects itself.

It is not an npm package, a Go module or a Cargo crate: it is a `.tar.gz` of Eclipse plug-ins
(milestones on `download.eclipse.org`, each with a `.sha256`), started with `java -jar` on its
Equinox launcher, with a configuration folder per operating system and a workspace folder where it
keeps its index and imported projects. Version 1.63.0 needs Java 25 to run: its bundles require
`JavaSE-25` and do not resolve on Java 21.

## Decision

1. **`plugins/java`, ABI v1.** Our own Java lexer (line by line, carrying open block comments and
   text blocks into the next line) and a declaration parser over its tokens: highlighting with the
   contextual keywords only where they are keywords (`record`, `sealed`, `non-sealed`, `permits`,
   `var`, `yield`, `when`; the module words only in a module declaration), annotations, generics and
   constants; folding of brace pairs, multi-line comments, text blocks and import runs; the
   Structure (types nested to 32 levels, fields with several declarators, methods, constructors,
   compact constructors left out, enum constants, record components, a module); completion of
   keywords, the file's names and snippets. Comment toggling, pairing and indentation are the
   platform's for Java already.
2. **File templates** Java Class, Interface, Enum, Record, Annotation, package-info and module-info,
   in the plug-in's `plugin.json`, with a new predefined variable `${PACKAGE_NAME}`: the file's
   folders below the source root (the folder after `src` named `java`, `kotlin`, `scala` or
   `groovy`, else `src` itself), joined with dots, empty when any is not a Java identifier.
3. **A server installed from an archive.** `install.kind: "archive"` with `url` and `sha256`. On the
   user's word only (`lsp.server.install`, rule 10) the IDE downloads it, checks the SHA-256 and
   unpacks it with the pure-Rust `tar` and `flate2` crates into a staging folder, then replaces
   `<data>/language-servers/<id>` with it (one top folder of the archive is dropped; entries that
   would leave the folder are skipped by `tar`). The archive's URL is kept beside the files, which
   gives the version shown. Nothing else is needed: no npm, no Python script.
4. **Launch placeholders** in `command` and `args`: `${serverFolder}`, `${glob:<pattern>}`, `${os}`,
   `${archSuffix}`, `${workspaceData}` and `${javaHome}`. A server with placeholders is installed
   when its `${glob:…}` files are in its folder, never looked up on `PATH`; it runs from
   "installed locally" (ADR-0128). Placeholders the server registry does not know are left for the
   JDK's expansion (`${jdkHome}`, ADR-0147).
5. **Its data in the IDE's cache, never in the project:** `${workspaceData}` is
   `<data>/caches/language-servers/<id>/<project name>-<hash of its path>`. JDT LS's `-data` and
   its own `-configuration` go there; the unpacked configuration is shared read-only
   (`osgi.sharedConfiguration.area`), so two projects at once do not write into the server's folder.
6. **The JDK it runs on** is `${javaHome}`, at least the declaration's `java` version (25): a
   finder the product may set (`LanguageServers::set_java_finder`), else the project's JDK
   (ADR-0147) when new enough, else the newest found in `JAVA_HOME` and the usual folders. Without
   one the server does not start and says which Java it needs. The project itself may use any JDK:
   `JAVA_HOME` and `PATH` of the server are the project's (ADR-0147).
7. **Maven and Gradle projects are imported by the server** (m2e and Buildship inside JDT LS), and
   again when a build file it watches changes; the Maven and Gradle plug-ins' tool windows are
   separate (work of their own) and do not build a model of their own for the editor.
8. **A real-server test**, ignored by default (`crates/platform/workbench/tests/java_language_server.rs`):
   `ZEN_JDTLS`, `ZEN_JDTLS_JAVA_HOME`, `ZEN_GRADLE_HOME`, `ZEN_MAVEN_REPO`; a Maven and a Gradle
   project, every feature through the window's commands; and one that installs the archive.

## Consequences

- Every Zen IDE gets Java from the catalog, offered when a `.java` file opens; JvmJizo bundles it.
- A new server distributed as an archive (Kotlin's, a Scala one) is a `plugin.json` away.
- JDT LS needs Java 25 while many projects use 17 or 21: the IDE may need a second JDK. Downloading
  Temurin 25 is one click (ADR-0147); when it is missing, the message says so.
- The milestone is pinned by URL and checksum in `plugin.json`: a newer JDT LS is a new plug-in
  release.
- JDT LS names files by their real path; a project opened through a symbolic link (macOS's `/var`
  temporary folders) gets its problems under the real path. Projects opened from the usual places
  are not affected; matching both paths is left for later.
