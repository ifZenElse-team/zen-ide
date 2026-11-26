# ADR-0048: Run configurations and the run service

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

GoGarden must run and test Go code: run configurations of several
kinds (build and run a package, run tests), a run widget in the toolbar with Run (⌃R) and Stop
(⌘F2), run icons in the gutter next to `func main` and tests, Run Context Configuration (⌃⇧R) for
what is at the caret, the Run tool window (⌘4) with the console and the test tree, and the
Run/Debug Configurations dialog. RustRonin will need the same for Cargo.

Which kinds exist, which command lines they start and how their output turns into tests is language
knowledge, which lives in language modules (L5). The platform must not depend on a language module
(ADR-0014), so it needs an extension point, as it has for project models (ADR-0046). Everything must
be reachable by agents as a command (ADR-0007, ADR-0033), and the parts that do not start processes
must compile for WebAssembly (ADR-0014, rule 4). The configurations are part of the project and are
shared through version control, so their file is a format others read.

## Decision

1. **The configuration model is `zen_project::run` (L1).** A `Configuration` has `id`, `name`,
   `kind` (a provider's kind id such as `go.build` or `go.test`), `temporary`, `common` (`Common`:
   `workingDirectory`, `env` as `[[name, value]]`, `programArguments`, `toolArguments`, the last two
   as typed and split like a shell) and `options`, free JSON in the kind's own shape. The permanent
   ones are kept in `<project>/.zen/runConfigurations.json` (`RunConfigurations`: `version` 1 and
   `configurations`), written atomically through `zen-vfs`, with folders inside the project
   written relative to it. A missing file is no configurations; a file that cannot be read is an
   error that names it, and nothing overwrites it. The selection and the temporary configurations
   (at most five) are the user's, not the project's: they live in the session state `Runs`
   (`selected`, `set_selected`, `temporary`, `set_temporary`), which the window saves with its
   workspace file, and are never written to the project's file. A file from an earlier build with
   `selected` or temporary configurations is still read: they move to the session (a selection only
   when the session has none) and the file is written without them. The format is documented in
   [run-configurations.md](../run-configurations.md).
2. **The run service is a new crate, `zen-run` (L2).** It holds the extension point and everything
   about a run that is not a process: `RunProvider` (`id`, `kinds`, `targets`, `context`,
   `command_lines`, `parser`, and the optional `rerun_failed` and `replay`), `KindSpec` and
   `FieldSpec` (what the dialog edits), `RunTarget` (a gutter icon: line, `main`, `test`,
   `benchmark`, `example` or `fuzz`, the package its test results name, and its configuration, the
   one the context at its line gives; `targets` gets the `RunContext`), `CommandLine`, `RunContext` (root,
   `RunEnv` with the tool and its environment, the project's modules), `OutputParser` (a line in,
   an optional console line and `TestEvent`s out), the `Console` (lines tagged `stdout`, `stderr`
   or `system`, escape sequences stripped, file links detected), the `TestTree` (run, packages,
   tests, subtests split on `/`, with statuses, durations and console line ranges) and `RunOutput`
   shared with the threads that feed it. Processes are started by `zen-run` on desktop; in
   WebAssembly builds `Process::spawn` fails. The `fake` feature has a `FakeProvider` that replays
   scripted lines without a process, for headless tests. A `Process` runs one `CommandLine` in its
   own process group (Unix `process_group(0)` through std, Windows `CREATE_NEW_PROCESS_GROUP`) with
   stdin closed, the IDE's environment plus the line's, stdout and stderr read line by line on their
   own threads and the exit reported after the last line; `stop` interrupts the group (SIGINT
   through `rustix`'s safe `kill_process_group`, `taskkill /T` on Windows) and kills it 2 s later or
   on a second call, and `kill` kills it at once. Each run gets
   `RunContext::scratch`, a folder of its own under `<temp>/zen-run` for files such as a built
   program, removed when the run ends; closing the window (dropping `Runs`) kills the programs that
   still run.
3. **An IDE lists its run providers** in `zen_ui::ide::IdeProduct::run_providers`; the window gives
   them and the toolchain's `RunEnv` to the session state `zen_workbench::run::Runs`.
4. **The commands** (`zen-workbench`, every id three segments):

   | Command | Parameters | Result |
   |---|---|---|
   | `run.configuration.list` | `{}` | `{ configurations, selected, kinds }` |
   | `run.configuration.create` | `{ configuration, select? }` | the configuration with its id |
   | `run.configuration.update` | `{ configuration }` | the configuration |
   | `run.configuration.delete` | `{ id }` | nothing |
   | `run.configuration.select` | `{ id? }` | nothing |
   | `run.context.get` | `{ path?, offset? }` | `{ configuration?, existing? }` |
   | `run.targets.list` | `{ path }` | `{ targets }` |
   | `run.process.start` | `{ configurationId? \| context? \| configuration? }` | `{ runId }` |
   | `run.process.stop` | `{ runId? }` | `{ stopped? }` |
   | `run.process.list` | `{}` | `{ runs }` |
   | `run.process.rerun` | `{ runId }` | `{ runId }` |
   | `run.output.get` | `{ runId, from?, waitMs? }` | `{ lines, next, state, exitCode?, error? }` |
   | `run.tests.get` | `{ runId }` | `{ tree, counts, done }` |
   | `run.tests.rerunFailed` | `{ runId }` | `{ runId }` |

   `run.process.start` with no parameters runs the selected configuration; with `context` it runs
   what is at a place (the active editor's caret by default), reusing a saved configuration with the
   same settings or keeping a temporary one, and selects what it ran. Every start first saves the
   modified documents (`file.document.saveAll`), then runs the command lines in order, each only
   when the one before exited with 0, with the toolchain's `RunEnv` variables before the line's own;
   the console shows each command line and ends with the exit code. A provider whose `replay` gives
   output starts no process. A run's state is `running`, `finished`, `stopped` or `failed`. The
   session keeps every running run and the newest 20 that have ended.
5. **Shortcuts are commands, not window actions.** ⌃R is `run.process.start` with `{}` and ⌃⇧R is
   `run.process.start` with `{ "context": {} }` (action `run.process.start/context`): the selection
   lives in the session, not in the UI, so neither needs the window. ⌘F2 is
   `run.process.stop`, ⌘4 the window action `view.toolWindow.run`, and Edit Configurations… the
   window action `view.runConfigurations.open`, without a shortcut. The Run menu follows Refactor:
   Run, Run Context Configuration, Edit Configurations…, Stop.

## Consequences

- Go (`go build`, `go run`, `go test -json`) and later Cargo plug into the same commands and the same
  windows; the UI draws the console, the tree and the gutter only from command results.
- `zen-run` is the first service that starts arbitrary programs. It does so only when the user or an
  agent runs a configuration; nothing runs on its own (AGENTS.md rule 10).
- The console keeps every line of a run in memory; a limit comes when long runs need one.
- The project's file holds only what the user saved, so committing it never shares someone's
  selection or the configurations their context runs made; those follow the window's workspace.
- `zen-run` depends on `rustix` (feature `process`, MIT OR Apache-2.0, already in the tree through
  other crates) on Unix, to signal a process group without `unsafe`.

## Alternatives considered

- **The service in `zen-workbench`:** the parsers, the console and the tree would not be usable
  below L5, and `zen-workbench` would mix process handling with documents.
- **The `.idea/workspace.xml` and `.run/*.run.xml` format:** XML shaped by
  another product's internals; reading it belongs to `zen-jb-import`, not to our own file.
- **⌃R as a window action (`view.run.selected`):** the run would not be reachable by agents with the
  same parameters, and the selection would have to live in the UI.
