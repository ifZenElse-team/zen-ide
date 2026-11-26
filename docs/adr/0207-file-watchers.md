# ADR-0207: File watchers

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

The requirement is File Watchers: a program set up once that runs by itself when matching
files are saved or changed on disk, to format, compile, transpile or lint them. They need a project
page Settings → Tools → File Watchers (columns Enabled, Name, Level; + − Edit Up Down Copy Import
Export; + offers a custom watcher and templates), a New Watcher dialog (name, file type, scope,
program, arguments, output paths to refresh, working directory, environment, and advanced options),
a File Watcher row in Tools → Actions on Save, and a Before launch task that runs the watchers.
Zen IDE had External Tools ([ADR-0160](0160-external-tools.md)) with their macros and runs, the
disk watcher of each window, Actions on Save and background tasks, but nothing that runs a program
when files change.

## Decision

1. **Two levels, two files.** Project watchers are kept in `.zen/fileWatchers.json`, shared through
   Git like the project's run configurations; global watchers in `fileWatchers.json` in the IDE's
   data folder, shared by every window. Both are `{ "formatVersion": 1, "watchers": [...] }`; a
   watcher's level is the file it is in, never a field. Ids are made from the first name and kept
   across renames, as for external tools. A damaged file is kept aside as `*.json.corrupt` with a
   notice. Import and Export read and write the same format.
2. **A watcher's run is an external tool's run.** It is a run of the run service with the kind
   `externalTool` and the launch expanded by the External Tools macro engine (`zen_run::macros`), so
   macros, the no-shell argument rule, Stop, Rerun and `run.output.get` are the same. The launch got
   the environment variables and `consoleOnError`; output filters gained `$MESSAGE$` and, for
   watchers, paths relative to the working directory, because linters print relative paths.
3. **Triggers come from what the window already sees.** Every frame the window steps the watchers
   (`file_watchers::step`, given the time, so tests never wait on a clock): a document whose save
   count grew is a save; a modified document whose text changed is an edit; the paths the window's
   disk watcher reports are external changes (`file_watchers::files_changed`). Each file waits
   300 ms after its last save or change, and 1 s after its last edit; then an edit is saved when a
   matching watcher has Auto-save on (otherwise only an explicit save runs it). The disk's echo of a
   save and the changes a watcher makes to its file and output paths are ignored for 2 s, so a
   formatter never triggers itself.
4. **Which watchers run.** An enabled watcher whose file type matches (empty matches every type),
   whose scope holds the file and, for an external change, that has Trigger on external changes.
   Project Files is the project's files without excluded folders, ignored names and `.zen`, `.idea`
   and `.git`; Open Files is the files open in tabs, in the project or not; Changed Files is the
   project's files Git lists as changed, unversioned ones included. Without Trigger regardless of
   syntax errors a file whose known problems include an error whose source or code names `syntax`
   is skipped. A watcher never runs on a file outside the project unless its scope is Open Files.
5. **One run at a time per watcher.** Files wait in a queue per watcher; runs of different watchers
   go on together. Each run is a background task.
6. **After a run.** The file and the output paths are read again (an open unmodified document
   follows the change); Create output file from stdout writes the standard output into the first
   output path. Output filter matches become problems of the files they name (severity warning,
   source the watcher's name) until the watcher runs on that file again. A run that cannot start
   (a missing program, a macro without a value) or ends with a non-zero code is a failure: the
   watcher is switched off and a notification says why, with Enable again
   (`run.fileWatchers.enable`). A non-zero exit whose output filters found problems is not a
   failure: those are the watcher's findings.
7. **Show console.** Always gives the run a Run tool window tab; On error (the default) gives it one
   only once it failed; Never gives none.
8. **Templates are contributions.** A language module offers its templates through
   `ModuleHost::file_watcher_templates` (Go: go fmt, goimports, golangci-lint). Basic web is in
   every IDE ([ADR-0050](0050-products-are-plugin-bundles.md)), so the web templates (Prettier,
   TypeScript, SCSS) are part of the platform until the plug-in manifest can declare them.
9. **Commands for everything** ([ADR-0033](0033-ai-access-to-every-api.md)): `run.fileWatchers.list`,
   `templates`, `save`, `replace`, `remove`, `move`, `copy`, `enable`, `import`, `export`, `run` and
   `status`. `run.fileWatchers.run` runs watchers on files now, whatever their trigger options;
   the Before launch task Run File Watchers is to call it.
10. **A project's watchers wait for trust** ([ADR-0205](0205-trusted-projects.md)). A cloned
    repository's `.zen/fileWatchers.json` would run its programs on the first save, so project
    watchers run only in a project the person trusts: until then they are skipped, and
    `run.fileWatchers.run` asking only for them is refused with code `notTrusted` (the person is
    asked to trust the project). Global watchers are the person's own and always run. Opening a
    project that has `.zen/fileWatchers.json` asks to trust it.
11. **Only the person sets a watcher up.** A watcher runs a program on every save, so a watcher an
    agent wrote would be code running each time the person saves. `save`, `replace`, `copy`,
    `import` and switching one on with `enable` (also through the notification's Enable again) are
    refused for agents with code `agentNotAllowed`; agents may list watchers, run them, switch them
    off, move, remove and export them, but never run one on, or export into, a file hidden from
    agents (code `redacted`).

## Consequences

- Agents can run the person's watchers and see what they found over MCP, but not set one up.
- A project's watchers travel with it; a program they run is one its author chose, run with the
  IDE's environment, like a run configuration.
- Not part of this decision: custom scopes, the Before launch task (it comes with the task list),
  and templates declared by plug-in manifests.
