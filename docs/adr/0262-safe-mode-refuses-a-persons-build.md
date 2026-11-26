# ADR-0262: Safe mode refuses a person's build too

- **Status:** Accepted (the owner, 2026-11-06)
- **Date:** 2026-11-06
- **Relates to:** [ADR-0248](0248-safe-mode.md), whose section 3 it supersedes in part;
  [ADR-0205](0205-trusted-projects.md)

## Context

ADR-0248 section 3 keeps "a build tool's explicit reload or task, an external tool, a Run Anything
command or a database migration that the person starts" as the person's action. Reloading a Maven,
Gradle or Ant build, or running one of its goals, tasks or targets, runs the project's own code
whoever asks: `mvnw`, `gradlew`, a settings script, a plug-in the POM names. Safe mode exists so that
a project not trusted yet runs nothing of its own, and the person's click does not change what runs.

## Decision

1. **In safe mode these are refused for the person as well as for agents**, with code `notTrusted`,
   a message that says why and the offer to trust the project: a build tool's reload (Maven, Gradle,
   a build load of Show Dependencies, Download Sources, Show Effective POM, Analyze Dependencies, the
   Gradle tasks), Execute Goal, Gradle tasks and Ant targets, linking a build outside the project
   (`maven.project.add`, `gradle.project.add`), a Run Anything command that a run provider answers,
   and a plug-in's project setup (`plugins.project.setup`: Nuxt's `npm exec nuxi prepare` runs
   the project's `nuxi` and `nuxt.config`). This supersedes those words of ADR-0248 section 3.
2. **`BuildEnv::spawn` refuses a folder not trusted** as a backstop, so that a new path that starts a
   build load fails instead of running.
3. **These stay the person's action, as ADR-0248 section 3 says, and take no guard:** an external
   tool, which the person configured, and a database migration. A Run Anything line that no run
   provider answers goes to the terminal, which works in safe mode; agents are refused that too.
4. **Show Bytecode needs no guard:** it runs the JDK's `javap`, and `javac` with `-proc:none`, so no
   code of the project runs.

## Consequences

- A Reload in safe mode still shows the reason in the Sync view, with its fix.
- Tests that load or run a build trust their project first.
- Reading what is already known (`maven.dependencies.tree` without `load`, `maven.project.get`) stays
  open in safe mode.

## Alternatives considered

- **Guarding `crate::run::start`:** it would also refuse external tools and migrations, which stay the
  person's action. The guard is on each command that runs the project's own build instead.
