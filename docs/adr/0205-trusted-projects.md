# ADR-0205: Trusted projects: a build runs by itself only in a project the person trusts

- **Status:** Proposed (the owner, 2026-11-04)
- **Date:** 2026-11-04
- **Relates to:** [ADR-0195](0195-dependency-manager-and-the-build-model.md),
  [ADR-0196](0196-load-changes-and-the-sync-view.md), [ADR-0193](0193-external-database-tools.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

A cloned project can run code on the person's computer before they asked for anything: Gradle
build and settings scripts are programs, Maven and Ant builds run plug-ins, annotation processors
run inside the compiler and the Java language server, and Lombok runs as the language server's
`-javaagent`. The JVM wave added automatic Load Changes (a build script changed outside the IDE
reloads the project), Lombok as the server's agent and the Annotation Processors settings. The
owner's decision (2026-11-04): Lombok, annotation processors and automatic Load Changes run only in
a project the person trusts; agents can never trust a project.

## Decision

1. **Trust is the person's, kept outside the project.** A project is trusted when the person
   trusts its folder or a folder above it. Trust is kept in the IDE's data folder (the build
   settings file, key `projectTrust`, by canonical path), never in the project, so a repository
   cannot trust itself. Without a data folder a project is trusted only for the running IDE.
2. **Until it is trusted**, the IDE:
   - starts the Java language server without `-javaagent` (no Lombok), with annotation processing
     off, and with Gradle import off (the server would run the build scripts); Maven import stays
     on (a `pom.xml` is read, not run);
   - does not load build-script changes by itself: Load Changes waits for the person;
   - runs none of the run configurations it comes with (`.zen/runConfigurations.json` and the ones
     adopted from `.idea` and `.run`): they are refused with code `notTrusted`, for the person too,
     who is asked to trust the project; a configuration made from the code at the caret still
     runs, and any configuration whose before-launch tasks run a program (an external tool, a Gradle,
     Maven or Go task, another configuration) waits for trust;
   - builds, rebuilds and cleans none of its artifacts (`.zen/artifacts.json` names where they are
     written and they build with the project's build), for the person too;
   - refuses agents that reload or run its builds (`gradle.project.reload`, `gradle.tasks.list`,
     `gradle.task.run`, `maven.project.reload`, `maven.goal.run`, `maven.dependencies.tree`,
     `ant.target.run`) with code `notTrusted`. The person's own runs pass: they are the person's
     action.
3. **Asking.** Opening a project that is not trusted and has a Gradle, Maven or Ant build or run
   configurations of its own posts a notification with **Trust Project**, and so does a run the
   person starts that waits for trust (once while one is unread).
4. **Commands.** `project.trust.state` answers whether a project is trusted; `project.trust.grant`
   trusts it and is refused for agents (`agentNotAllowed`), also when an agent presses a
   notification's action that runs it (`ui.notifications.action`); `project.trust.revoke` stops
   trusting it and is open to agents, as it only makes the project safer. Trusting or revoking makes
   the Java language server read its launch again (Lombok after a restart of the server).
5. **Downloads stay explicit.** Gradle, Maven and Ant are downloaded only by their Download
   commands, checked against the SHA-256 published beside them and unpacked with the archive limits
   and name checks of the settings import; library sources and documentation only on request; the
   Lombok jar is never downloaded by the IDE (it is the one the build downloaded).

## Consequences

- A project opened for the first time shows Java without Lombok's generated members until the
  person trusts it.
- Tests that need Lombok or automatic reloads trust their project first.

## Alternatives considered

- **A trust file in the project (`.zen/trusted`):** a repository could ship it.
- **Asking before every build run:** the person's explicit runs are already their decision; asking
  again adds noise without safety.
