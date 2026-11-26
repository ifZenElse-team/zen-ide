# ADR-0147: The JvmJizo app, and JDKs in the platform

- **Status:** Proposed
- **Date:** 2026-10-25
- **Changes:** adds an IDE, released later under [ADR-0146](0146-one-calendar-version-for-every-ide-and-release-channels.md)'s
  shared version, and a platform toolchain beside [ADR-0071](0071-built-in-modules.md)'s module ones

## Context

JvmJizo, the family's IDE for Java and the JVM ([ADR-0145](0145-jvmjizo.md)), is a bundle of
plug-ins like every Zen IDE ([ADR-0050](0050-products-are-plugin-bundles.md)): Java (with Eclipse JDT
LS), Maven and Gradle are being written as plug-ins in parallel. All of them, and run configurations
of a JVM project, need a JDK: which one the project uses, at which language level. A language
module keeps its own toolchain (Go's SDKs, ADR-0071), but these are WebAssembly plug-ins with
declarations, which cannot look for JDKs themselves, and a JDK is useful to any IDE that opens a
Maven or Gradle project.

## Decision

1. **The app.** `apps/jvm-jizo` is JvmJizo as WebWave is WebWave: product id and launcher `jvmjizo`,
   bundle id `space.ifzenelse.jvmjizo`, MCP port 63533, its tile as the icon, its Kozō on the splash
   and welcome screens; Go and Rust, the built-in modules, off. Its bundle (`PRODUCT_PLUGINS`) is the
   base set and `java`, `maven`, `gradle`; Kotlin is offered from the catalog, not bundled. While
   those plug-ins are being written, `PENDING_PLUGINS` lets the build leave out one that has not
   landed in the repository; each leaves the list when it lands.
2. **Released with the others from 26.10.2, as a preview** (the owner, 2026-10-26): until then it
   was in `release::UNRELEASED_IDES`, built and tested with the workspace but left out of
   `release-build ides` and `release-publish`; its release moved it to `release::IDES` in one line,
   and `changelog/jvmjizo.md` starts at `## 26.10.2`. `cargo xtask changelog` compares the released
   IDEs' calendar releases from the newest one's first on, so that an IDE can join later. Zen
   Toolbox lists it as Available once the signed manifest has it. The checks of the Java and Kotlin
   trademarks and licences for an announced release are a separate task.
3. **JDKs are the platform's** (`zen_workbench::jdk`), in every IDE:
   - detection without the network: `JAVA_HOME`, `/usr/libexec/java_home -V`, the JVM folders of
     macOS and Linux, Homebrew, SDKMAN!, asdf, mise, Gradle's toolchains (`~/.gradle/jdks`,
     `org.gradle.java.installations.paths`) and Maven's (`~/.m2/toolchains.xml`); version and vendor
     from the JDK's `release` file; each JDK once, by its real path;
   - per project, a JDK (else the default: `JAVA_HOME`'s, else the newest with a compiler) and a
     language level (else the JDK's), kept in the IDE's data folder (`jdks.json`), not in the
     project: the project's own build files say what the build needs, and the Maven and Gradle
     plug-ins read those;
   - JDKs added by hand, and **Eclipse Temurin downloaded only on the user's word** (AGENTS.md rule
     10): the Adoptium API's newest build of a feature release for this computer, checked against
     the SHA-256 it gives, unpacked into `<data>/jdks/` in the background as a task;
   - Settings → Languages & Frameworks → JDKs (a form, as the module pages), and `toolchain.jdk.*`
     for agents (ADR-0033).
4. **The interface for plug-ins and features.** A program started for a project gets its JDK:
   language servers of a JVM language (`java`, `kotlin`, `groovy`, `scala`, `clojure`) or naming a
   JDK variable, and run configurations of a Maven or Gradle project, start with `JAVA_HOME` and the
   JDK's `bin` first on `PATH`. A server's arguments and `initializationOptions` may say
   `${jdkHome}`, `${javaVersion}` and `${javaLanguageLevel}`. Rust code calls
   `zen_workbench::jdk::{project_jdk, project_home, with_project_jdk}`; agents
   `toolchain.jdk.project`.

## Consequences

- Every IDE knows the JDKs; JvmJizo is only the one that comes with the JVM plug-ins.
- JvmJizo's release is one line in `release::IDES` and the rename of its changelog section.
- Other vendors (Zulu, Corretto, GraalVM) can join the download list through the same interface.
- Windows downloads (zip archives) are not supported yet; detection there covers `JAVA_HOME` and
  added JDKs.
