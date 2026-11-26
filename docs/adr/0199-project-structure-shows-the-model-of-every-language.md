# ADR-0199: Project Structure shows the project model of every language

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0159](0159-project-structure-and-resource-roots.md) (the page and its folder
  marks; extended here), [ADR-0050](0050-products-are-plugin-bundles.md) (plug-ins first),
  [ADR-0147](0147-jvmjizo-app-and-jdks.md) (JDKs), [ADR-0149](0149-maven-and-gradle.md) (Maven and
  Gradle), ADR-0195 (the build model from Maven and Gradle themselves, proposed),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents)

## Context

Settings → Project Structure showed one content root with its Excluded and Resource Root marks and
the Exclude files patterns. A Java or Kotlin project needs more from
Project Structure: the project's SDK, language level and compiler
output, the modules with their source, test and resource roots, excluded folders and dependencies,
the libraries, the SDKs, and the problems. In a Maven or Gradle project most of it comes from the
build, and the build stays the owner: a value changed only in the IDE would be lost on the next
import. The language modules already give a project model (`ModelProvider`: modules, dependencies,
libraries, problems) for Go, Rust and, since an earlier change, the JVM libraries; JDKs and the
project's JDK and language level are kept by `toolchain.jdk.*` (`jdks.json`).

## Decision

1. **The page is a platform feature that shows the project model**, whatever languages supply it.
   It has these sections: Project Settings (Project, Modules, Libraries), Platform
   Settings (SDKs) and Problems. It opens on Project. The content root editor of ADR-0159 is the
   Sources tab of Modules, unchanged.
2. **The model gains what the sections need**, all optional so existing providers keep working: a
   module's `folders` (`sources`, `testSources`, `resources`, `testResources`, `excluded`), its
   `output` and `testOutput`, `assumed` when these are the build system's conventions rather than
   read from the build, and a dependency's `scope`. Go gives the module folder as sources; Rust
   gives `src`, `examples`, `tests`, `benches`, the target folder and its scopes (`dev`, `build`).
3. **The JVM provider gives modules.** Maven: every `pom.xml` of the reactor with `<build>`'s
   folders or Maven's defaults, `target` excluded, the output folders, the language level
   (`maven.compiler.release`, `maven.compiler.source`, or the compiler plug-in's `release` or
   `source`) and the declared dependencies with their scopes. Gradle: every project of the settings
   script with the conventional source sets that exist, `build` excluded, the conventional output
   marked `assumed`, the toolchain or source compatibility as the language level, and the declared
   dependencies with their configurations. A parent POM, a BOM or a build script that moves folders
   is not read: what is not read is not claimed. ADR-0195 will replace these readers with the
   tools' own model; the shape stays.
4. **What the build gives is read-only and says where it comes from**: "From pom.xml" with an
   **Open build file** link, or the convention note. The project's SDK and language level are the
   IDE's (`toolchain.jdk.choose`, `toolchain.jdk.languageLevel`, no second store) and are changed
   here, applied with Apply or OK; when the build asks for a level, the page says the build compiles
   with it. A project without a build says its compiler output is not set (run configurations
   compile into a temporary folder) rather than inventing one.
5. **Problems are honest and local**: no JDK for a JVM project, a language level above the JDK, a
   build that asks for a level above the JDK, a build file that does not read, the model's problems,
   and libraries not downloaded. A library being unpacked in the background is not reported.
6. **Commands (ADR-0033):** `project.structure.project`, `project.structure.modules`,
   `project.structure.libraries`, `project.structure.sdks`, `project.structure.problems` and
   `project.structure.openBuildFile { module? }`; `project.structure.get` keeps answering the folder
   marks.
7. **Artifacts are left out** until the IDE builds jars; the layout does not need a
   placeholder. Facets and Global Libraries are not offered.

## Consequences

- GoGarden and RustRonin show their modules, dependencies and SDK on the same page; the Go page of
  ADR-0159 moves to Modules → Sources.
- JVM modules join the model, so search scopes offer them and the Project tool window can name
  module folders, as for Go and Rust.
- Gradle's folders and output stay marked as conventions until the build model of ADR-0195 reads
  them from Gradle.
