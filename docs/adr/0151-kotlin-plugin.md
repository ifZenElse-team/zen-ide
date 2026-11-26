# ADR-0151: The Kotlin plug-in: our own lexer, two Kotlin servers to choose from

- **Status:** Proposed
- **Date:** 2026-10-25
- **Extends:** [ADR-0055](0055-language-servers.md) (language servers), [ADR-0128](0128-a-plugin-installs-its-language-servers.md)
  (a plug-in installs its servers), [ADR-0148](0148-java-plugin-and-eclipse-jdt-ls.md) (servers
  installed from an archive)
- **Relates to:** [ADR-0144](0144-code-is-parsed-in-pure-rust.md), [ADR-0145](0145-jvmjizo.md), ADR-0147
  (the JDK toolchain), [ADR-0149](0149-maven-and-gradle.md)

## Context

JvmJizo is Java-first, and Kotlin must stay first-class in it. A plug-in, like every capability
(ADR-0050), must give Kotlin its syntax layer in pure Rust (ADR-0144) and its semantics from a
language server (ADR-0003), and must work in every IDE.

There are two Kotlin language servers:

| | The Kotlin team's **kotlin-lsp** (`Kotlin/kotlin-lsp`) | **fwcd/kotlin-language-server** |
|---|---|---|
| Licence | Apache-2.0 repository; the server is built on proprietary parts of its vendor's products, which makes it partially closed-source | MIT |
| Maturity | Alpha, actively released: 263.7084.0 on 2026-10-17, about every two weeks | Deprecated by its author in favour of kotlin-lsp; last release 1.3.13, January 2025 |
| Features | Its vendor's IDE analysis: completion, diagnostics (pull model) and quick fixes, navigation, rename, formatting, organise imports, call hierarchy, semantic highlighting | Completion, diagnostics, hover, definition, symbols; no rename to speak of, lags new Kotlin versions |
| Builds | Imports Gradle and Maven projects itself; Kotlin Multiplatform planned | Gradle and Maven through its own resolver |
| Install | A standalone archive per platform from the vendor's download server (zip for macOS, tar.gz for Linux, zip for Windows), with SHA-256 files; about 360 MB, 1.1 GB unpacked | `server.zip` from GitHub (87 MB) |
| JDK | Brings its own Java runtime (its vendor's build of JDK 25) to run on; analyses the project with the project's JDK | Needs a JDK 11+ on `PATH` or `JAVA_HOME` to run |
| Start | the launcher in its `bin` folder, with `--stdio` | `bin/kotlin-language-server` (stdio) |

## Decision

1. **Kotlin's syntax layer is our own lexer** (`plugins/kotlin/src/kotlin.rs`, from the Kotlin
   language specification's grammar of tokens):
   - **Keywords:** hard, soft and modifier; a soft keyword is a keyword only where it stands as one.
   - **Strings:** `$name` and `${…}` templates (code inside them is highlighted), raw strings,
     characters and escapes.
   - **Literals and names:** numbers with their prefixes and suffixes, nested block comments, KDoc,
     annotations with use-site targets, labels, backtick names, types (constructor calls too) and calls.
   - **Folding:** blocks, block comments and runs of imports.
   - **The Structure view:** classes, interfaces, objects (companions too), enum classes, type aliases,
     functions and methods, properties, fields and constants, nested as declared; the Gradle Kotlin
     DSL's top-level blocks (`plugins`, `dependencies`, `tasks.register("x")`) as modules.
   - **Kotlin scripts:** the plug-in takes `kt` and `kts`, so Gradle's Kotlin DSL (`*.gradle.kts`) is
     read by the same lexer. The Gradle plug-in (ADR-0149) leaves `kts` to it.
2. **Both Kotlin servers, one chosen** (the owner, 2026-10-26).
   - **The default is the Kotlin team's kotlin-lsp:** the maintained one, with its vendor's analysis, rename
     and formatting.
   - **The alternative is fwcd's Kotlin Language Server (MIT),** which the user chooses in Settings →
     Language Servers.
   - **The choice is general.** When several declared servers serve the same language (the first of
     their `languages`) in its place (not alongside), one of them starts: the one the user chose
     (`lsp.server.choose`, kept as `chosen` in `language-servers.json`), else the first declared.
     The others count as off.
   - **What lists and offers follow it:** `lsp.servers.list` gives each server its `alternatives`, the
     chosen one first. The editor's banner offers to install only the chosen one, and Settings →
     Language Servers shows "Server for Kotlin" with the choice.
3. **kotlin-lsp is its vendor's software, and the IDE says so.**
   - It is downloaded only on the user's click, straight from the vendor's official URL; we never host
     or bundle it.
   - Its declaration carries a `notice`, its text by interface language and a link. It is shown where
     the server is offered (the banner, with an "About this server" link to its repository), in
     Settings → Language Servers, and in the installation's log; the notice names the vendor and says that the
     server is downloaded from it under its terms and is partly closed-source.
   - It runs as a separate process, as every language server does, so nothing of it is in our code
     (ADR-0002).
   - **Open question: its licence.** The archive of 263.7084.0 ships no licence for kotlin-lsp itself,
     only the notices of third-party libraries; the repository is Apache-2.0. Its terms must be
     clarified before JvmJizo's public release. If the answer is negative, fwcd's
     server becomes the default by changing which is declared first, without changing the plug-in.
4. **Both are installed from archives, with ADR-0148's `archive` kind, extended:**
   - **Per-platform archives:** an optional `platforms` map gives each platform's archive
     (`zen_lsp::servers::platform()`, e.g. `macos-aarch64`) in place of the single `url` and
     `sha256`. kotlin-lsp is built per platform and brings its own Java runtime; a platform without
     an archive (Windows here, for now) gets the hint.
   - **Zip archives:** archives are told apart by their first bytes. A zip (the vendor's `.sit` archives
     are zips, fwcd's `server.zip`) is unpacked with the system's `tar` (bsdtar reads zips and keeps
     entries inside the folder) into the same staging folder, so no zip library is added.
   - **The version** is read from a folder of the URL when the archive's name has none
     (`…/download/1.3.13/server.zip`).
   - **Launch:** both are started through `${glob:bin/<program>}`, which also tells that they are
     installed. fwcd's server runs on the project's JDK (`JAVA_HOME`, ADR-0147); kotlin-lsp on its
     own runtime, with the project's JDK given to it the same way for the project.
5. **Templates:** the file templates Kotlin File, Class, Data Class, Interface, Enum, Object and
   Script, with `package ${PACKAGE_NAME}` (ADR-0148's variable) when the folder has a package.
6. **Offered, not bundled.** The plug-in is offered when a `.kt` or `.kts` file opens, by its
   extensions. JvmJizo is Java-first and offers Kotlin (ADR-0147).

## Consequences

- Without a server, the editor still highlights, folds and outlines Kotlin. kotlin-lsp is a 360 MB
  download (macOS), fwcd's 87 MB.
- kotlin-lsp is in alpha at its vendor, and a plug-in release pins one version of it. fwcd's server is
  deprecated by its author but open, the fallback should kotlin-lsp's terms not allow the default.
- Any language can now offer a choice of servers by declaring several; nothing about Kotlin is in
  the platform.
