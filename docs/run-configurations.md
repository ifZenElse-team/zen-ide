# The run configurations file

> **Last updated:** 2026-11-04 · **Decisions:** [ADR-0213](adr/0213-compound-configurations-templates-and-services.md)

A project's permanent run and test configurations are kept in `.zen/runConfigurations.json` under
the project's root folder ([ADR-0048](adr/0048-run-configurations-and-the-run-service.md)). The IDE
reads it whenever a `run.*` command needs it and writes it after every change to a permanent
configuration, atomically (a temporary file next to it is renamed over it). Commit it to share the
configurations.

Which configuration is selected and the temporary configurations (made by running a context) are
the user's own: the window keeps them (`zen_workbench::run::Runs`, saved with the window's
workspace) and never writes them to this file.

## Format

```json
{
  "version": 1,
  "configurations": [
    {
      "id": "go.build-1",
      "name": "go build app",
      "kind": "go.build",
      "temporary": false,
      "common": {
        "workingDirectory": "cmd/app",
        "env": [["GOFLAGS", "-mod=mod"]],
        "programArguments": "--port 8080 --data $ProjectFileDir$/data",
        "toolArguments": "-race",
        "envFiles": [".env", "conf/local.env"],
        "allowMultipleInstances": true
      },
      "options": { "package": "./cmd/app" }
    }
  ]
}
```

| Key | Meaning |
|---|---|
| `version` | The format's version, `1`. A file of another version is not read |
| `selected` | Not written. A file from an earlier build may have it: the window takes it as its selection when it has none, and the key goes when the file is next written |
| `configurations` | The configurations, in the order the user arranged them |
| `id` | Unique in the file; made from the kind (`go.test-2`) when a configuration is added without one |
| `name` | The name shown in the run widget, the Run tool window and the dialog |
| `kind` | The kind, as the IDE's run provider names it (`go.build`, `go.test`, ...) |
| `temporary` | Always `false` in the file. A temporary configuration (made by running a context, ⌃⇧R or a gutter icon, and not saved by the user) is kept by the window, at most five, the oldest going first; a file from an earlier build may have some, which move to the window and leave the file |
| `common.workingDirectory` | The folder the program runs in, relative to the project (absolute when outside it); the project's root when absent |
| `common.env` | Environment variables added for the program, as `[name, value]` pairs in order |
| `common.programArguments` | The program's arguments as typed; split like a shell does (quotes and backslashes) |
| `common.toolArguments` | The tool's own arguments as typed, e.g. `go build` flags |
| `common.beforeLaunch` | The tasks run in order before the configuration's programs, each `{ "type": "<id>", ...its options }` ([Before launch](#before-launch)); left out when empty |
| `common.showPage` | `true` opens the Run/Debug Configurations dialog on the configuration before each start from the window (Show this page); left out when `false` |
| `temporary` | Always `false` in the file. A temporary configuration (made by running a context, ⌃⇧R or a gutter icon, and not saved by the user) is kept by the window, at most five (Advanced Settings → Run/Debug → Keep at most this many temporary configurations, the IDE setting `runTemporaryLimit`, 1 to 100), the oldest going first; a file from an earlier build may have some, which move to the window and leave the file |
| `common.workingDirectory` | The folder the program runs in, relative to the project (absolute when outside it); the project's root when absent. Macros expand at launch |
| `common.env` | Environment variables added for the program, as `[name, value]` pairs in order, on top of `envFiles`. `$NAME$` in a value is the IDE's own variable `NAME` (case-sensitive; one that is not set stays as written) |
| `common.envFiles` | `.env` files read at launch in order, each relative to the project or absolute; a later file and then `env` override earlier values. A file that cannot be read fails the start with a message naming it. Lines are `NAME=value`, optionally `export NAME=value`; `#` starts a comment line, `"…"` values take `\n` escapes, `'…'` values are literal. Left out when empty |
| `common.programArguments` | The program's arguments as typed; split like a shell does (quotes and backslashes). Macros expand at launch, each value staying one argument |
| `common.toolArguments` | The tool's own arguments as typed, e.g. `go build` flags; macros expand as in `programArguments` |
| `common.allowMultipleInstances` | `false` by default: starting the configuration while it runs stops the running instance and starts it again in its tab (the window asks first while Advanced Settings → Run/Debug → Ask before a rerun stops the running process is on; `run.process.start` never asks unless given `confirmRerun`). `true`: another instance starts in a tab of its own, named `name (2)`, `name (3)`… |
| `common.activateToolWindow` | `true` by default: the Run tool window opens when the configuration starts. Written only when `false` |
| `common.focusToolWindow` | `false` by default: when `true` the Run tool window also takes the keyboard |
| `options` | The kind's own settings, in the shape its provider documents; paths in it are relative to the project |
| `folder` | The folder of the Run/Debug Configurations dialog's tree it is shown in, under its kind; left out when it is in none. Folders exist only through the configurations in them |
| `templates` | At the top level, next to `configurations`: the project's templates, at most one per kind, each `{ "kind", "common", "options" }` ([Templates](#templates)); left out when there are none |

Every key may be left out and takes its default (empty, `false` or absent; `activateToolWindow`
`true`). Unknown keys are ignored and are not kept when the IDE writes the file.

### Macros

`programArguments`, `toolArguments` and `workingDirectory` expand the same macros as external tools
(`$ProjectFileDir$`, `$ProjectName$`, `$FilePath$`, `$FileDir$`, `$FileName$`, `$LineNumber$`,
`$SelectedText$`, `$GOROOT$`, `$GOPATH$`, `$ClipboardContent$`, `$Prompt$`, `$FilePrompt$`, `$OSName$`,
`$TempDir$` and the rest that `run.externalTools.macros` lists), from the editor's file, caret and
selection, the project, the clipboard and the SDKs, at every start, adopted configurations included.
A macro without a value fails the start, naming it and why. `$Prompt$` and `$FilePrompt$` are asked
in the IDE's window; `run.process.start` takes them as `prompt` and `filePrompt` and otherwise fails
with the code `promptNeeded`. An unknown `$Name$` stays as written. `run.configuration.resolve`
answers what a configuration would launch with now, without starting it.

## Configurations shared by JetBrains IDEs

A project opened in a JetBrains IDE too may share run configurations in
`.idea/runConfigurations/*.xml` and `.run/*.run.xml` ([jetbrains-compatibility.md](jetbrains-compatibility.md#run-configurations)).
A run provider that knows the type adopts each one (`zen_run::RunProvider::adopt`), and the
`run.*` commands list it after the file's configurations, before the temporary ones, with:

- the id `jb:<file relative to the project>#<name>` (`jb:.run/Serve.run.xml#Serve`);
- `adoptedFrom`, the JetBrains file it comes from. This key is never written to
  `.zen/runConfigurations.json`, and a file that has it is read without it.

An adopted configuration runs and can be selected, but `run.configuration.update` and
`run.configuration.delete` refuse it with a message: the JetBrains file is never written.
`run.configuration.saveAdopted { id }` (Save as Zen Configuration in the dialog) copies it into
this file with the same name and a new id, and the copy takes its place and its selection; any
configuration of this file with the same kind and name hides the adopted one. A type no provider
adopts, or a second one with the same kind and name, is not listed.

## Before launch

`common.beforeLaunch` lists tasks that run before the configuration's programs, in order, in the
same run: each starts with a `#before-launch <task>` line in the run's console, and its programs
write their output there. A task that fails ends the run in the state `beforeLaunchFailed`, with
the reason as its `error`, and the configuration does not start
([ADR-0202](adr/0202-before-launch-tasks-are-an-extension-point.md)). The IDE's own types:

| `type` | Options | What it does |
|---|---|---|
| `runConfiguration` | `name` | Starts the configuration of that name in a run of its own and waits for it; it fails unless that run ends by itself with exit code 0, and a configuration already being launched by the chain fails it |
| `externalTool` | `tool` (an id, or a name) | Runs the external tool as `run.externalTools.run` does, in its own run, and waits for it the same way |
| `toolWindow` | `toolWindow`: `project`, `structure`, `bookmarks`, `commit`, `git`, `terminal`, `problems`, `find`, `hierarchy`, `todo`, `run`, `debug`, `services` or `notifications` | Opens that tool window of the project's window |
| `saveFiles` | none | Saves the modified files (a start saves them first anyway; this saves them again at its place in the list) |
| `buildArtifacts` | `artifacts`: the names of one or more artifacts (`project.structure.artifacts`) | Builds them as `build.artifacts.build` does, in its own run, and waits for it the same way; it fails when none is named or one is unknown |
| `fileWatchers` | `watcher`: `<level>:<id>` (`project:prettier`) or empty for every enabled watcher; `files`: `changed` (default, the files Git sees as changed), `open` or `all`, each in the configuration's working directory | Runs the watchers whose file type and scope match on each of those files, as `run.fileWatchers.run` does, and waits for the runs; a run that does not end with exit code 0, or a watcher switched off on the way, fails it; nothing to run skips it |

The language modules and build tools add theirs: `go.command` (`command`, the words after `go`
such as `generate ./...`; `workingDirectory`, relative to the project), `maven.goal` (the options
of a `maven` configuration: `pom`, `goals`, `profiles`, `offline`, `skipTests`) and `gradle.task`
(`projectPath`, `tasks`). `run.beforeLaunch.types` lists every type the IDE has with its fields.
A task of a type no installed module or plug-in knows is skipped with a line in the console.

A configuration adopted from the run configuration files of another IDE also takes the file's
`folderName` as its `folder`. A `CompoundRunConfigurationType` there is adopted as a `compound` whose
`configurations` are the ids of the configurations its `<toRun name>` elements name, found by name
among the file's and the adopted ones; a name not found is left out. Templates (`default="true"`)
are not adopted: the shared files have none, and per-user templates live in a file the IDE does not
read.

A configuration adopted from the run configuration files of another IDE gets the tasks of its
`<method v="2">` element that are switched on: `RunConfigurationTask` becomes `runConfiguration`
(by name), `ToolBeforeRunTask` becomes `externalTool` (by the tool's name, the id of the external
tool of that name when there is one), `GoBeforeRunTask` becomes `go.command`,
`Maven.BeforeRunTask` becomes `maven.goal` and `Gradle.BeforeRunTask` becomes `gradle.task`. Any
other is kept as `{ "type": "skipped", "foreignId": "<its id>" }`, listed as not supported and
skipped when the configuration runs.
## Compound configurations

The IDE's own kind `compound` (provider `run`, listed last by `run.configuration.list`) starts other
configurations: `options.configurations` is the list of their ids, in order
([ADR-0213](adr/0213-compound-configurations-templates-and-services.md)).

```json
{ "id": "compound-1", "name": "All", "kind": "compound", "options": { "configurations": ["go.build-1", "npm-2"] } }
```

- `run.process.start` starts every listed configuration in a run of its own, as if each were started
  alone (its before-launch tasks, its tab, its instance rules). A listed compound adds its own
  configurations in its place, and a configuration reached twice starts once. The answer's `runId`
  is the first run's and `children` lists every run; a configuration that cannot start is named in
  `errors` while the others still start. Each run's `compound` (in `run.process.list`) is the
  compound's id.
- `debug.session.start` debugs every listed configuration whose provider can debug it and runs the
  others; when none can be debugged it fails.
- `run.process.stop { configurationId }` stops every run the compound started; the run widget's
  **Stop** does that while a compound is selected.
- A compound that contains itself, directly or through other compounds, is refused by
  `run.configuration.create` and `run.configuration.update` and shown as an error by the dialog. An
  id that names no configuration fails the start with a message naming it.
- The common settings and before-launch tasks of a compound are not used.

## Templates

Each kind but `compound` has a template: the settings a new configuration of the kind starts with.
`run.template.list` answers every kind's, `run.template.update` sets the project's one and
`run.template.reset` removes it, so that the kind's defaults (`KindSpec::defaults` and the default
`common`) apply again. A template applies to:

- `run.configuration.new { kind }`, which the dialog's **+** uses: the kind's defaults, then the
  template's options over them, and the template's `common`;
- `run.configuration.create`, unless `template: false`: a key of `options` the configuration does not
  give, and every `common` setting it leaves at its default, come from the template;
- the configuration of a context (`run.context.get`, ⌃⇧R, gutter icons), in the same way, before it
  is matched with the saved ones; the temporary configuration made from it has the template's
  settings.

Configurations that exist already never change with their template.

## Services

The Services tool window shows the configurations of the kinds added to it
(`run.services.add { kind }`) or added one by one (`{ configurationId }`), without the hidden ones
(`run.services.hide`, `run.services.restore`). What it shows is the window's own choice, kept with
the window's workspace (`zen_workbench::run_services::RunServices`), and never written to this file.
`run.services.list` answers it with each configuration's latest run.

## Plug-in targets

The kind `plugin` runs what a plug-in's `zen_run_targets` offers ([ADR-0286](adr/0286-plugin-run-targets.md),
provider `zen_workbench::plugin_runs::PluginRun`), with these options:

| Option | Meaning |
|---|---|
| `plugin` | The plug-in's id |
| `target` | The target's id, stable for the same target |
| `file` | The file it came from, relative to the project |
| `program` | The program, one the plug-in declares (`"runs"` in its `plugin.json`), looked up on `PATH` and in the usual folders |
| `args` | The arguments, passed as the program's `argv`; `${file}`, `${fileDir}`, `${projectDir}` filled in |
| `cwd`, `env` | The plug-in's working folder (the file's folder by default) and variables (never `PATH`, `ZEN_*`, `DYLD_*`, `LD_*`) |

`common.programArguments` follow `args`, split as arguments, and `common.workingDirectory` and
`common.env` come after the plug-in's. A program the plug-in no longer declares is refused. A
command the plug-in declares person-only (`"personOnly"`, [ADR-0291](adr/0291-commands-only-a-person-starts.md))
is judged on the resolved arguments, after these are added: agents never start it, and the window
asks the person every time, showing the whole command line and folder.

## Run Anything

Run Anything (⌃⌃, `run.anything.list`, `run.anything.run`) asks the run providers what can be typed
([ADR-0204](adr/0204-run-anything.md)): `RunProvider::anything_prefixes` names a provider's first
words and suggestions (Go `go`, Cargo `cargo`, Maven `mvn`, Gradle `gradle`), and
`RunProvider::anything` makes a temporary configuration of a typed line in a folder. Go and Cargo
commands run as plain command lines with the project's `go` or `cargo`; `mvn` and `gradle` become
Maven and Gradle configurations of the module or build of that folder. The configuration is run, not
saved. A line no provider takes runs in a new Terminal tab.

## Errors

A file that is not valid JSON or has another `version` is an error: every `run.*` command that reads
it fails with a message naming the file, and nothing writes over it until it is fixed. A missing
file is a project without configurations.

## npm

Every IDE runs the kind `npm` ([ADR-0129](adr/0129-project-plugins-npm-run-and-nuxt.md), provider
`zen_workbench::npm::NpmRun`), with these options:

| Option | Meaning |
|---|---|
| `packageJson` | The `package.json`, relative to the project (`package.json` by default); the command runs in its folder unless `common.workingDirectory` says otherwise |
| `command` | `run` a script (the default), `exec` a program of the project's packages, or `install` |
| `script` | The script to run, or the program to execute (`nuxi`); not needed for `install` |
| `packageManager` | `auto` (the default: the one `packageManager` in `package.json` names, else the one whose lock file is there, `pnpm-lock.yaml`, `yarn.lock`, `bun.lock` or `bun.lockb`, else npm), `npm`, `pnpm`, `yarn` or `bun` |

`common.programArguments` follow the script or program. The command lines: `npm run <script> --
<arguments>`; `pnpm`, `yarn` and `bun run <script> <arguments>`; for `exec`, `npx --no <program>
<arguments>` (a program the project does not have is not downloaded), `pnpm exec`, `yarn run` and
`bun run`; `<manager> install`. The package manager is looked for on `PATH` and in the folders
tools install into (Homebrew, nvm, Volta, `~/.bun/bin`…), and runs with them on its `PATH`.

A plug-in may add configurations to the projects it supports (`"project"` in its `plugin.json`,
[docs/plugins.md](plugins.md)): they are written into this file once per project, as if the user
had added them, and a declaration with `when` only to the projects that match its markers (React's
`vite`, `vite build` and `vite preview`, in Vite projects); the Nuxt plug-in adds `nuxi dev`,
`nuxi build`, `nuxi generate` and `nuxi preview`, the Next.js plug-in `next dev`, `next build`,
`next start` and `next lint`, the SvelteKit plug-in `vite dev`, `vite build` and `vite preview`:

```json
{ "name": "nuxi dev", "kind": "npm", "options": { "command": "exec", "script": "nuxi" },
  "common": { "programArguments": "dev" } }
```

## Node.js

With the Node.js plug-in, the provider `zen_workbench::nodejs::NodeRun` ([ADR-0268](adr/0268-npm-and-nodejs.md))
runs two kinds. Their common options are `nodeOptions` (arguments for Node.js itself, as typed) and
`interpreter` (a path or a program's name; `node` on the `PATH` and in the usual folders when empty).

| Kind | Option | Meaning |
|---|---|---|
| `node` | `file` | The file, relative to the project; it runs in its folder unless `common.workingDirectory` says otherwise |
| `node.test` | `runner` | `auto` (the default: the runner the nearest `package.json` declares, Vitest first), `jest` or `vitest`; always the project's own in `node_modules` |
| `node.test` | `files` | Files as typed, quoted like arguments; every test when empty. It runs in the folder of the first file's `package.json` |
| `node.test` | `testName` | A regular expression for the runner's `-t`, matched on the suites and title joined by spaces |

`common.programArguments` follow the runner's arguments, or the file's. Results come from reporters
the IDE writes into the run's temporary folder, and **Rerun Failed** makes a temporary configuration
with the failed files and an exact `testName`. Debugging starts Node.js with `--inspect-brk` under
the IDE's own adapter; `debug.*` commands work as for every kind.

## Spring Boot

With the Spring Boot plug-in, every IDE runs the kind `springBoot`
([ADR-0217](adr/0217-spring-boot-plugin-metadata-from-the-projects-jars.md), provider
`zen_workbench::spring_boot::run::SpringBootRun`, id `springBoot`) as a Java application
(`java.application`): the same `mainClass`, `module`, `jre` and `build`, and

| Option | Meaning |
|---|---|
| `activeProfiles` | The profiles to activate, separated by commas or spaces: `--spring.profiles.active=dev,local` |
| `overrides` | Configuration properties to set, as `[[name, value]]`: each `--name=value` |

They follow `common.programArguments`. A Java or Kotlin class with `@SpringBootApplication` and a
`main` gives this kind as its gutter target and context in place of `java.application`, while the
plug-in is on.

```json
{ "name": "ShopApplication", "kind": "springBoot",
  "options": { "mainClass": "shop.ShopApplication", "module": "", "build": true,
    "activeProfiles": "dev", "overrides": [["server.port", "9090"]] } }
```

## Go

GoGarden's run provider (`zen_lang_go::GoRun`, id `go`) has two kinds. A package is an import path
(`example.com/app/calc`, run from the root of the module that holds it) or a folder that starts with
`.` or `/` (`./calc`, run from that folder as `.`). Gutter icons and Run Context Configuration make
folder packages relative to the project (`./calc`); a gutter icon runs the same configuration as
Run Context Configuration on its line, so both reuse one saved or temporary configuration.

| Kind | Option | Meaning |
|---|---|---|
| `go.build` | `runKind` | `package` (default), `file` or `directory` |
| | `package`, `files`, `directory` | What is built, by `runKind`; files are split like arguments and must share a folder |
| | `outputDirectory` | Where `go build -o` writes the program, which is kept; a temporary folder when empty |
| | `runAfterBuild` | `true` (default): build into a folder of the run's own (`<temp>/zen-run/<process>-<n>/<name>`), then run it with `programArguments` in `workingDirectory`; the folder is removed when the run ends |
| `go.test` | `testKind` | `package` (default), `directory` (`./...` in the folder) or `file` |
| | `package`, `directory`, `files` | What is tested, by `testKind` |
| | `pattern` | The `-run` regular expression; `^TestAdd$` for one test, `^TestA$/^case$` for a subtest |
| | `bench` | The `-bench` regular expression; empty runs no benchmarks |
| `go.generate` | `file` | The Go file `go generate` runs on, in its folder, relative to the project |
| | `directive` | The whole line (`//go:generate ...`), matched as text by `-run`; empty runs all the file's lines |

`go test` always runs with `-json`, and benchmarks with `-benchmem`; `toolArguments` follow the pattern and `programArguments` go to
the test binary after `-args`. Rerun Failed Tests keeps the configuration and narrows `pattern` to
the failed top-level tests (or to the one failed subtest) and `bench` to the failed benchmarks.

### Debugging Go

`GoRun::debug` gives the debug plan of ADR-0052 for both kinds; the workbench runs its build line,
starts Delve and launches the program it built.

- `go.build` needs `runAfterBuild` (otherwise the plan is an error). It builds with `go build
  -gcflags=all=-N -l -o <program> <toolArguments> <target>` (the run's scratch folder, or
  `outputDirectory`), and Delve runs the program (`mode: exec`) with `programArguments` in
  `workingDirectory` and the configuration's environment.
- `go.test` debugs one package or a set of files; a `directory` configuration is an error. It builds
  with `go test -c -gcflags=all=-N -l -o <scratch>/<name>.test` and runs the test binary in the
  package's folder with `-test.v -test.paniconexit0`, `-test.run <pattern>`, `-test.bench <bench>`
  and then `programArguments`.
- The build line gets `PWD` set to its folder, so that the paths in the program's debug information
  are the project's paths even when the project is reached through a symbolic link (macOS
  `/var` → `/private/var`), and breakpoints set on the project's files resolve.
- `-trimpath` in `toolArguments` is left out of a debug build, because breakpoints need the full
  paths; the console line of the build says so after `#gosetup`.
- The adapter is `<dlv> dap --client-addr=<host:port>` with the SDK's environment (`GOROOT`,
  `PATH`); when Delve is not found its program is empty and the start fails with `debuggerMissing`.
- The output of a debugged test binary is plain `-test.v` text, not `-json`. The provider's
  `parser` for `go.test` is the same for runs and debug sessions: `Test2JsonParser` reads JSON lines
  and gives every other line to a `VerboseParser` (`=== RUN`, `=== PAUSE`, `=== CONT`, `=== NAME`,
  `--- PASS/FAIL/SKIP/BENCH`, `PASS`, `FAIL`, `ok` and `FAIL <package>`), so the test tree fills in
  either way and `RunProvider` stays as it is.
- `zen_lang_go::explain_adapter_error` turns Delve's known errors into advice: debugserver missing
  on macOS (install the Xcode Command Line Tools), Delve too old for the Go SDK (update Delve), the
  SDK too old for Delve, ptrace not allowed, and a program without debug information.
