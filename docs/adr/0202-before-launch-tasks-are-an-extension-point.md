# ADR-0202: Before-launch tasks are an extension point of the run service

- **Status:** Proposed
- **Date:** 2026-11-03
- **Relates to:** [ADR-0048](0048-run-configurations-and-the-run-service.md) (run configurations and the run
  service), [ADR-0050](0050-products-are-plugin-bundles.md) (products are plug-in bundles),
  [ADR-0007](0007-everything-is-a-command.md) and [ADR-0033](0033-ai-access-to-every-api.md) (commands)

## Context

A run configuration often needs something done first: code generated, another configuration built,
a goal of the build tool run, a tool window opened. Until now the only step before a launch was
saving the modified files. The steps people need depend on the language and the build tool, and
new ones keep coming with plug-ins (Ant targets, building artifacts), so a list of types fixed in
the platform would have to change with every one of them, and a product would end up with steps
the others lack.

## Decision

1. **A configuration keeps its tasks** in `common.beforeLaunch`, an ordered list of
   `{ "type": "<id>", ...options }`, and `common.showPage` for Show this page. Both are left out of
   the file when empty.
2. **The platform owns everything around the tasks:** the list and its order, running them one
   after another before the configuration's own programs, the console (each task starts with a
   `#before-launch <task>` line and its programs write into the run's console), failure handling
   and the Before launch part of the Run/Debug Configurations dialog. A task that fails (a program
   exits with a code other than 0, cannot be started, or its type refuses it) ends the run in the
   state `beforeLaunchFailed` with the reason, and the configuration is not started. Stopping the
   run during the tasks stops the running task and starts nothing. A task of a type nothing
   installed knows is skipped with a line in the console, so a project opened in a smaller IDE
   still runs.
3. **A type is one of two things:**
   - **a run provider's task kind**, for language modules and plug-ins. `RunProvider::task_kinds`
     declares it as a `KindSpec` (id, name message, fields, defaults; the same shape as a
     configuration's kind, so the dialog draws its form), `RunProvider::task_lines` turns a task
     into the programs to run, and `RunProvider::adopt_task` maps a task of an adopted run
     configuration file to it. The platform runs the programs and judges them by their exit codes.
     Go adds `go.command`, Maven `maven.goal`, Gradle `gradle.task` this way;
   - **a `zen_workbench::before_launch::TaskType`**, for a feature of the platform that has to act
     on the session: it gets the session and answers `Done`, `Lines` (programs, as above), `Run`
     (the id of a run it started, which the platform waits for and judges by its end) or `Skipped`.
     The built-in ones are Run Another Configuration (`runConfiguration`), Run External Tool
     (`externalTool`), Activate Tool Window (`toolWindow`) and Save Files (`saveFiles`); a feature
     adds its own with `before_launch::register_type`.
4. **The steps are advanced on the session**, not on a thread of their own: right at the start,
   then whenever the window draws a frame (which a program's output wakes) and while a command waits
   for a run. A type may therefore use the session at its turn (start another configuration, save
   files, ask the window to open a tool window) and the order of the list holds for every task.
5. **Every action is a command** (`run.beforeLaunch.types`, `list`, `add`, `update`, `remove`,
   `move`, `showPage`, `page`); Show this page is a matter of the window only, so `run.process.start`
   from an agent never waits for a dialog.
6. **Tasks of adopted run configuration files** are mapped where their type is known: the IDE's
   own types for `RunConfigurationTask` and `ToolBeforeRunTask`, a provider's `adopt_task` for the
   others; a task switched off in the file is left out and an unknown one is kept as `skipped` with
   its id, so the list shows what was not taken over.

## Consequences

- A new kind of step is a few functions in the module or plug-in that knows it; the platform,
  the dialog and the commands do not change. The changes that add Ant targets and artifact builds add
  their types this way.
- Cycles of Run Another Configuration are found at launch (a configuration already being launched
  in the chain fails the task), not when the list is edited.
- A configuration started from its dialog's Run or Debug button does not open the dialog again.
