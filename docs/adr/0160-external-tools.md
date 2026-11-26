# ADR-0160: External tools

- **Status:** Proposed
- **Date:** 2026-10-25

## Context

The requirement: people set up a program once (Settings → Tools → External Tools) and run it on
the current file or the project from the context menus and a shortcut, with macros such as
`$FilePath$` in its arguments. The Create Tool dialog has no menu check boxes, so the Tools menu
has no External Tools entry, while the editor's and the Project view's context menus show a submenu
named after the tool's group. Zen IDE had nothing alike. The feature needs a file
for the tools, a way to start them, the macros, and a place in the keymap; and the run
configurations work ("Environment files, macros and multiple instances") needs the same macros.

## Decision

1. **The tools belong to the IDE.** They are kept in `externalTools.json` in the IDE's data folder
   (`{ "formatVersion": 1, "tools": [...] }`), shared by every window and project: they are
   kept at the application level, in one JSON file of our own format. Each tool has a stable `id` made from its first name
   (`format-file`, `format-file-2`), kept across renames, so that its shortcut survives a rename. A
   damaged file is kept aside as `externalTools.json.corrupt` with a notice, and the list starts
   empty.
2. **One macro expander, in `zen-run`.** `zen_run::macros` knows the macros asked for
   (`$FilePath$`, `$FileDir$`, `$FileName$`, `$FileNameWithoutExtension$`, `$ProjectFileDir$`,
   `$ProjectName$`, `$SelectedText$`, `$LineNumber$`, `$ColumnNumber$`, `$Prompt$`, `$FilePrompt$`,
   `$ClipboardContent$`, `$GOROOT$`, `$GOPATH$`) and further ones that need nothing more
   (`$GoExecPath$`, `$GoFileImportPath$` from the nearest `go.mod`, `$OSName$` as Rust names the
   system, `$TempDir$`), listed by name, and expands them from a `MacroContext` the
   workbench fills; an unknown `$Name$` stays as written. Run configurations are to use the same expander.
3. **No shell.** The arguments are split once as a shell splits them (`zen_run::args`), then each
   word is expanded, so a value with spaces or quotes stays one argument and is never interpreted.
   A macro without a value fails the run with the macro and the reason; nothing starts.
4. **A run like any other.** A tool's run is a run of the run service with the kind
   `externalTool`, whose configuration holds the expanded launch (program, arguments, folder,
   output filters, console options); no provider lists that kind, so it is never offered as a run
   configuration. Stop, Rerun (the same command line), `run.output.get` and the Run tool window work
   unchanged. Output filters are regular expressions with `$FILE_PATH$`, `$LINE$` and `$COLUMN$`
   whose matches become links before the default ones. Synchronize rereads the open files once the
   run has ended.
5. **The context menus, not the Tools menu.** A tool has no menu options:
   each group of enabled tools is a submenu named after the group in the editor's and the Project
   view's context menus, and the Tools menu has no External Tools entry.
6. **Every tool is a keymap action** `run.externalTools.run/<id>` in a group of its own, External
   Tools, added to every keymap while the tool exists (`KeymapStore::set_extra`); removing a tool
   removes its shortcuts. The action runs the window action `view.externalTools.run`, which asks
   for `$Prompt$` and `$FilePrompt$` in a dialog, then the command `run.externalTools.run`.
7. **Commands for everything** (ADR-0033): `run.externalTools.list`, `save`, `replace`, `remove`,
   `move`, `copy`, `macros`, `preview` and `run`; the Settings page changes the tools only through
   them.

## Consequences

- Agents can set up and run tools over MCP; a tool is a program the user chose, run with the IDE's
  environment, like a run configuration.
- The macro set grows in one place for both external tools and run configurations.
- Remote external tools, the "before launch" step that runs a tool, Run Anything's tools,
  `$Password$` (a masked prompt) and other macros (`$GoBinDirs$`, `$ContentRoot$`,
  `$FileDirRelativeToProjectRoot$`, `$SelectionStartLine$` and the like) are not part of this
  decision.
