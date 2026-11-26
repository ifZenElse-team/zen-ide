# ADR-0236: The Gradle model loads after import, and tasks join the sync's Gradle run

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the build model),
  [ADR-0233](0233-build-triggers-and-keymap-actions-with-parameters.md) (build triggers),
  [ADR-0205](0205-trusted-projects.md) (trusted projects),
  [ADR-0152](0152-java-run-configurations-and-the-classpath.md) (the Gradle init script)

## Context

ADR-0195 says an init script writes the Gradle model as JSON, cached per build file hash, and that
a failure leaves the last good model shown. It does not say when the model loads, how composite
builds and `buildSrc` are reached, how a task runs around a sync, or how a Gradle task is debugged.
The tool window showed tasks only after Reload All, from `gradle tasks --all`, and ADR-0233's
triggers run only around Build Artifacts.

## Decision

1. **The model loads by itself once the project is trusted.** When the Gradle tool window becomes
   available in a window, whether shown or not, `gradle.model.get` with `load` runs: an unchanged
   hash reads the cache and Gradle does not run; otherwise Gradle runs in a background task and its
   result lands in the Sync view. An untrusted project runs nothing and shows the settings script's
   projects until it is trusted.
2. **One Gradle run answers for the whole composite.** Each project's `zenModel` task writes one
   JSON file into a folder named by the system property `zen.model.out`; every build's root task
   depends on its projects' and its included builds' tasks. System properties reach every build of
   the composite, project properties do not. From Gradle 8.0 the script adds `:buildSrc:zenModel` to
   the requested tasks; before 8.0 `buildSrc` shows from the settings reader alone.
3. **The model replaces `gradle tasks --all`,** which is gone. The settings reader stays for the
   first view and for the features that read projects without Gradle (Project Structure, Lombok,
   External Libraries' declared list).
4. **Paths are tree paths** (`:tools:core`), so a task of an included build runs from the composite
   as `:tools:core:test`.
5. **Execute Before Sync and After Sync are `when` values of the build triggers** (`beforeSync`,
   `afterSync`), held in `.zen/build-triggers.json` as the others, but only for `gradle.task`. They
   run in the same Gradle invocation as `:zenModel`, before and after it on the command line, not
   as separate programs: a failing task fails the sync, and the last good model stays. A cache hit
   runs no Gradle and so runs no such task; Reload All always does.
6. **Debug attaches to the JVMs a task forks.** An init script gives `JavaExec` and `Test` tasks a
   suspended debug agent on one port, and tests one fork; a task that forks nothing runs to its end
   with nothing to attach to. Gradle's own daemon is never debugged.
7. **Offline is one setting.** `gradle.options.set` and Toggle Offline change `offline` of the
   Gradle settings, kept per project, which every Gradle run reads.

## Consequences

- Opening a trusted Gradle project starts Gradle once when no valid cached model exists, and Gradle
  may download plug-ins and libraries unless offline is on.
- Before Sync tasks run after Gradle configured the build, so they cannot fix a build that fails to
  configure.
- A linked build (`gradle.project.add`) gets its own model, and its folder must be trusted.
- A build that resolves a configuration in a way the init script cannot read reports the error on
  that configuration's row; the other configurations stay.
