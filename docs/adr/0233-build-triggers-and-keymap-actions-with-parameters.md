# ADR-0233: Build triggers around the IDE's builds, and keymap actions that carry parameters

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0202](0202-before-launch-tasks-are-an-extension-point.md) (before-launch tasks),
  [ADR-0201](0201-jar-artifacts-the-ide-writes.md) (jar artifacts the IDE writes),
  [ADR-0195](0195-dependency-manager-and-the-build-model.md) (the build model),
  [ADR-0007](0007-everything-is-a-command.md) and [ADR-0033](0033-ai-access-to-every-api.md) (commands)

## Context

The requirement: the Maven tool window lets a goal run before or after the IDE's own build and
rebuild, before a run configuration, and from a keyboard shortcut; the Gradle and Ant tool windows
offer the same for tasks and targets. Zen IDE had before-launch tasks of run configurations
(ADR-0202) but no task run around a build, and its only build of its own is Build Artifacts
(ADR-0201): there is no Build Project. A keymap binds an action id to a command without
parameters, so a shortcut could not run one goal of one `pom.xml`.

## Decision

1. **Build triggers are a platform feature** (`zen_workbench::build_triggers`): a list of
   `{ when, task }` kept in the project's `.zen/build-triggers.json`, `when` one of `beforeBuild`,
   `afterBuild`, `beforeRebuild`, `afterRebuild`, and `task` a before-launch task of ADR-0202. The
   file is written only while the list is not empty.
2. **Only task types that run programs are triggers.** A trigger's task is started like a
   before-launch task; the programs it answers are run, a type that answers a run of its own
   (Run Another Configuration) is refused, one that answers nothing adds nothing. A provider's
   task kind (`maven.goal`, `gradle.task`, `ant.target`) is therefore a trigger without changes.
3. **The IDE's builds run them in their own run:** Build Artifacts runs the `beforeBuild` programs
   first, then the compilation, then the `afterBuild` programs, then writes the jars; Rebuild
   Artifacts does the same with the rebuild triggers. A failing program stops the run as a failing
   compilation does. When the IDE gets a Build Project, it runs the same triggers.
4. **Commands:** `build.triggers.list` and `build.triggers.set` (`on` true, false, or absent to turn
   it the other way, as a ticked menu item does).
5. **A keymap action may carry parameters:** an action id written as a command id, one space and a
   JSON object (`maven.goal.run {"goals":"clean","pom":"pom.xml"}`) runs that command with those
   parameters when the command is registered. Assign Shortcut of a build tool window's goal stores
   such an action; the keymap file, `ui.keymap.setShortcut` and the keymap page need no new format.

## Consequences

- Gradle tasks and Ant targets get Execute Before/After Build by calling `build.triggers.set` with
  their task kind; nothing in the platform changes.
- The triggers run only around Build Artifacts and Rebuild Artifacts until the IDE has a build of
  its own; this is said in the guide.
- A shortcut stays valid while its command and parameters do; a goal renamed in the `pom.xml`
  leaves a shortcut that runs the old goal until it is removed.
