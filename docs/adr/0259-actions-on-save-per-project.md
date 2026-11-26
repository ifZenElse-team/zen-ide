# ADR-0259: Actions on Save per project, and fix-only tools

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0136](0136-linters-and-formatters.md)

## Context

Tools → Actions on Save had two check boxes kept in the IDE's `settings.json`, so every project
shared them. The page must say per action which files it covers and whether Reformat code takes the
whole file or the changed lines, keep this per project, and have defaults for new projects.

## Decision

1. **A project's actions are `.zen/actionsOnSave.json`** (`zen_workbench::actions_on_save`), written
   by `project.actionsOnSave.set`. The file is read at each save, so an edit on disk counts at once,
   and only in a trusted project: in safe mode a save takes the person's defaults, so that a file a
   project brings changes nothing. An agent is refused the file when a pattern hides it from agents.
2. **The IDE's settings are the defaults for new projects** (`reformatOnSave`,
   `optimizeImportsOnSave`, `reformatOnSaveFiles`, `reformatOnSaveScope`,
   `optimizeImportsOnSaveFiles`). A project without the file takes them, which also keeps what people
   chose before this change. **File → New Projects Setup → Settings for New Projects…**
   (`view.settings.newProjects`) opens the page for these; otherwise, with a project open, the page
   edits the project, and the file is written only when something changed.
3. **Files are language ids**, the ids of `zen_workbench::code_style`; an empty list is every
   language. The page shows a language as its first glob, since a language name would need a
   translated table the platform does not have.
4. **Changed lines means the lines that differ from the last commit.** A language server's edits are
   kept when they touch such a line; an external formatter's result is compared by lines and kept in
   the hunks that touch one. A file out of version control, or new in it, is reformatted whole. SQL,
   formatted by the IDE, is always reformatted whole.
5. **A tool that only fixes is a formatter with `fixer: true`** in `plugin.json`: it runs on a save
   and never for Reformat Code, and the language's own Reformat code still runs on that save.
   Stylelint is the first, in the plug-in `plugins/stylelint`; ESLint keeps its server's fix-all.
6. **Rows of what the IDE cannot do yet are shown disabled** (Rearrange code, Run code cleanup) or
   as a hint (Update copyright notice), and they do nothing.

## Consequences

- The options on a language's page (Go, Rust) follow the same target as this page.
- Hints on the page are general, since the page belongs to every IDE; they do not name one product's
  tools.
