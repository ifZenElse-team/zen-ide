# ADR-0115: File templates with a Velocity subset

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

Until now New → File created an empty file; only live templates existed (`editor.templates.*`,
`zen_workbench::templates`). New files are to be created from **file templates**: Settings →
Editor → File and Code Templates, with the tabs Files, Includes and Code, two schemes (Default, kept
in the IDE's configuration folder, and Project, kept in the project's `.idea/fileTemplates` and
shared through version control), templates written in the Velocity Template Language with
predefined variables (`${NAME}`, `${DATE}`, …), and variables without a value asked for in the
dialog that creates the file, including those in the template's file name. File → Save File as
Template… makes a template of the current file. The formats are worked out clean-room, from public
documentation and observed behaviour (ADR-0002).

Two later pieces of work build on this one: the New menu by file type with the File and Code
Templates settings page, and Go's templates (New → Go File). The model must serve both.

## Decision

1. **A template** (`zen_workbench::file_templates::FileTemplate`) has a name, an extension, a tab
   (`kind`: `file`, `include`, `code`), an optional file name pattern (the new file's name without
   the extension, with variables), its text, **Reformat according to style** (on by default) and
   **Enable Live Templates** (off by default). A template is known by its tab and name; `#parse`
   and the files on disk use `<name>.<extension>`.
2. **Built-in templates come from modules and plug-ins.** A built-in module gives
   `zen_modules::FileTemplateSet` through `ModuleHost::file_templates`: templates as JSON, the
   variables it adds (name and Fluent description) and a function giving their values for the new
   file's folder (Go's `GO_PACKAGE_NAME` will be one). A plug-in declares `"fileTemplates":
   { "templates": [...], "variables": [{ "name", "description", "value" }] }` in its `plugin.json`;
   the catalog, `cargo xtask release plugin` and the installed `plugin.json` carry it like
   `languageServers`, and the IDE reads the declarations of the plug-ins that are on whenever it
   lists templates, so installing or switching off a plug-in counts at once. A built-in template
   cannot be removed; a changed one is kept in a scheme and can be reset; one set back to its
   declared text is no longer kept. The first declaration of a name wins.
3. **A Velocity subset**, our own parser (`file_templates::velocity`): `${VAR}`, `$VAR`, `$!VAR`,
   `#set($VAR = value)` with a string (references in `"…"` are replaced), a number, `true`, `false`
   or a variable; `#if`/`#elseif`/`#else`/`#end` with `==`, `!=`, `!`, `&&`, `||` (and `eq`, `ne`,
   `not`, `and`, `or`) and parentheses; `#parse("Include name")` of an include, by
   `<name>.<extension>` or the bare name, nested at most 8 deep and taken in at most 64 times per
   template; `##` and `#* *#` comments; `#[[raw]]#`; `\$VAR` and `\#directive` for the text itself;
   `${DS}` is a dollar sign. A directive or comment alone on its line takes the line with it
   (Velocity 2's "lines" whitespace gobbling). A value is false when it is missing, empty or
   `false`. Everything else Velocity has — `#foreach`, `#macro`, `#include`, `#define`, `#evaluate`,
   `#break` and `#stop` when called with parentheses, methods and properties
   (`${NAME.toUpperCase()}`), arithmetic, lists — is reported as a problem with its line and column
   (and the include it is in), never a failure: a template with problems lists them
   (`file.templates.list`, the dialogs) and creates no file. A `#` or `$` that starts none of these
   is text, so `#include <stdio.h>`, `# Title`, `#fff` and `$5` stay as they are. The parser is
   checked by property tests on arbitrary text. Templates come from repositories nobody vetted
   (`.idea/fileTemplates`), so rendering has limits: `#if` is nested at most 64 deep and a condition
   has at most 64 terms, the rendered text and every value are at most 1 MiB, at most 100 problems
   are reported, and each limit reached is a problem rather than a hang or a crash. `#parse` reads
   only the includes in use, by name, never a file. A variable's value is text: a `$` or `#` in it
   is written as it is, never rendered again.
4. **Predefined variables**: `NAME` (the name typed, without the
   extension), `FILE_NAME` (with it), `DIR_PATH` (the folder relative to the project), `PROJECT_NAME`
   (`.idea/.name` when the project has one, its folder's name otherwise), `PRODUCT_NAME`, `USER`
   (the login name), `DATE`, `TIME`, `YEAR`, `MONTH`, `MONTH_NAME_SHORT`, `MONTH_NAME_FULL`, `DAY`,
   `DAY_NAME_SHORT`, `DAY_NAME_FULL`, `HOUR`, `MINUTE`, `SECOND` and `DS`, in the computer's time
   zone. `DATE`, `TIME` and the names of months and days do not follow the system's
   locale: we write `DATE` as `2026-10-22`, `TIME` as `14:05`, numbers with two digits and the names
   in English, the same on every machine.
5. **Asked variables.** A variable a template (its file name pattern, its text and the includes it
   parses, in every branch of every `#if`) reads before `#set` gives it a value, and that is neither
   predefined nor given by a module or plug-in, is asked for when a file is created, in the order it
   first appears; `NAME` is asked first when the file name pattern is empty or uses it. The creation
   dialog has a field for each; `file.templates.create` takes them as `variables` and refuses,
   creating nothing, while one is missing.
6. **Storage in the `.idea` layout.** A scheme's templates are a `fileTemplates` folder with one
   file per template, `<name>.<extension>.ft` (files without `.ft` are read too), the Files tab at
   its top, Includes in `includes/`, Code in `code/`, and a `file.template.settings.xml` with the
   component `ExportableFileTemplateSettings` and, per tab (`default_templates`,
   `includes_templates`, `code_templates`), `<template name="<name>.<extension>" file-name="…"
   reformat="…" live-template-enabled="…" />`. The Default scheme is the IDE's, in its data folder
   (`fileTemplates/` and `options/file.template.settings.xml`); the Project scheme is the project's
   `.idea/fileTemplates/` and `.idea/file.template.settings.xml`. Only templates the user made or
   changed are kept. Writing the settings file replaces only the `ExportableFileTemplateSettings`
   component and keeps the rest of the file, and the entries of templates without a file, byte for
   byte; a template or an entry that did not change is not written again, so a template file without
   `.ft` keeps its name and its line endings. Symbolic links and files over 1 MiB in `fileTemplates`
   are not read, and templates are not written through a linked folder.
7. **Which scheme.** A project with a `.idea/fileTemplates` folder uses the Project scheme; its
   templates go over the IDE's, which go over the built-in ones, by tab and name. Writing a template
   to the Project scheme creates the folder. The scheme switch of the settings page will
   create or leave that folder; until then `file.templates.set`, `remove`, `reset` and `saveFileAs`
   take `scheme` and default to the scheme in use.
8. **Creating a file** (`file.templates.create`): the name may start with folders and may end with
   the template's extension; the file name pattern, when there is one, gives the name (it may name a
   folder under the chosen one, as `docs/${NAME}`, but never leave it: a pattern that gives `..`, an
   absolute path or nothing is refused, whatever the values of its variables). Reformat according to
   style gives the text the indentation of the file's code style (a leading tab becomes one indent)
   and, when the file's language server runs, reformats it as `editor.code.reformat` does (half a
   second at most). Enable Live Templates makes the rendered text a live template: `$VAR$` and
   `$END$` (written as `#[[$VAR$]]#` in the Velocity text) are filled in with Tab in the new file,
   each variable starting as its name. The file opens in front.
9. **Commands** (ADR-0007, ADR-0033): `file.templates.list`, `.variables`, `.render` (the file a
   template would make, its asked and missing variables and problems, without creating it),
   `.create`, `.set`, `.remove`, `.reset` and `.saveFileAs`; the window actions
   `view.fileTemplates.new` (New File from Template…, in Find Action; the New menu opens it with a
   template chosen) and `view.fileTemplates.saveAs` (File → Save File as Template…, after
   Local History among the File menu's items). Window state names the
   dialogs `newFromTemplate` and `saveFileAsTemplate`.

## Consequences

- Go's templates (New → Go File) and the New menu and settings page are declarations and a page on
  top of this model; no product carries a built-in file template yet.
- The XML component and section names of decision 6 are those of settings exported by a JetBrains
  IDE, as far as they were observed; whether it writes `.ft` in a project's `.idea/fileTemplates`
  was not observed (we read both). A project shared with a JetBrains IDE should be checked against
  it.
- Velocity text that is not in the subset is reported rather than half-rendered; templates
  imported from a JetBrains IDE that use `#foreach` or methods have to be simplified.
- **Merging with the parallel change that adds File → New → EditorConfig File** (checked with
  `git merge-tree --write-tree`): `file.fs.newFile` here opens the new file through `open_created`,
  which `file.templates.create` shares, while that change edits the same lines to open it
  with the EditorConfig `charset` (`open_text(path, None, editor_config::charset(..))`) and
  `line_separators::settle`. Resolve by moving those two calls into `open_created`, so that files
  made from templates get them too. `IdeView` (`crates/platform/ui/src/ide/mod.rs`) keeps both
  dialogs' fields, their `None`s and their calls; `ui.ftl` and `workbench.ftl` keep both sets of
  messages; `docs/README.md`, `CHANGELOG.md` and `changelog/` keep both entries.
