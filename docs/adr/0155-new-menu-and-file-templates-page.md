# ADR-0155: The New list by file type and the File and Code Templates page

- **Status:** Proposed
- **Date:** 2026-10-22
- **Builds on:** [ADR-0115](0115-file-templates.md) (file templates)

## Context

ADR-0115 gave file templates a model, storage and the `file.templates.*` commands; New File from
Template… was reachable only from Find Action. The requirement: ⌘N (Alt+Insert) in the Project tool
window opens the **New** popup — File, Directory, the file types of the templates that fit the
folder, and Edit File Templates… — File → New lists the same,
and Settings → Editor → File and Code Templates edits the templates: Scheme Default / Project, the
tabs Files, Includes and Code, the toolbar Create Template, Create Child Template File, Remove
Template, Copy Template and Revert to Original Template, and the variables described under the
editor. Main already had a File → New submenu with Scratch File ⇧⌘N and EditorConfig File, and
EditorConfig File in the tree's New › and the Project tool window's +.

## Decision

1. **One New list** (`ui::ide::new_popup`), shown three ways: the New popup (the window action
   `view.newElement.open`, ⌘N, also the Project tool window header's +, window state
   `newElement`), the tree's context menu New ›, and File → New, a submenu the native menu bar fills
   with the list of the folder in use (the folder selected in the Project tool window, else the
   folder of the file in front, else the project) and rebuilds when the list changes. The list is
   File, Directory, the templates of the Files tab that fit the folder, and Edit File Templates…
   (Settings → Editor → File and Code Templates); agents read it with `file.new.list { directory? }`.
   ⌘N works where the Project tool window has the keyboard; in the editor it belongs to
   Generate, which does not exist yet, so it does nothing there. The JetBrains keymap import maps
   `NewElement` to it, which gives Alt+Insert in the Windows and Linux keymaps.
2. **Fitting the folder.** A built-in template (module or plug-in) may declare `"markers"`: file
   names one of which must be in the folder or a folder above it inside the project for New to offer
   it (Go File with `["go.mod"]`). Templates without markers, and every template the user made,
   fit every folder.
3. **Plugged items.** Other window actions join the list through `new_popup::PLUGGED`, a list of
   `(place, action, folder parameter)`: `place` is `file` (between File and Directory) or
   `edit` (after Edit File Templates…); the item is named by the keymap and runs with the
   folder in the parameter it names, if any. Scratch File (`view.scratch.new`) comes after File and
   EditorConfig File (`view.editorConfig.new`, folder in `folder`) after Edit File Templates…, in
   this order (the templates, the user's after the built-in ones, then a separator, Edit File
   Templates… and EditorConfig File; no separator after Directory), so the earlier File → New
   submenu and the tree's separate EditorConfig button became this one list. `file.new.list` lists
   what the workbench makes (File, Directory, templates, Edit File Templates…); the plugged items
   are window actions agents call directly. `ide-tree-new-file` and `ide-tree-new-directory` are
   gone: File and Directory come from `workbench-new-file` and `workbench-new-directory`.
4. **Choosing a template.** A template that asks for nothing creates its file at once
   (`file.templates.create`); one that asks for the name or variables opens the creation dialog of
   ADR-0115 for that template only (no template chooser). With Enable Live Templates the caret ends
   at the template's `$END$` (ADR-0115, decision 8).
5. **Child templates** (Create Child Template File): a template of the Files tab
   has `children`, each with an extension, a file name pattern (empty: the main file's name), a text,
   Reformat according to style and Enable Live Templates. Creating a file creates every child beside
   it, under the same rules as the main file (inside the chosen folder, never over an existing file;
   nothing is written when one of the files exists), opens them behind the main file and reports
   them as `children` (`render` and `create`). Their variables are asked for with the main
   template's. On disk a child is a file of its own beside its template,
   `<name>.<extension>.child.<n>.<extension>` without `.ft`, its file name pattern and flags in an
   entry of `file.template.settings.xml` (`<template name="<name>.<extension>.child.<n>.<extension>"
   file-name="…" …/>`), which is the format of a JetBrains IDE's `fileTemplates` folder (a child
   is named there after its template's name when the child was made, here after the current name).
   A template name
   cannot contain `.child.`.
6. **Schemes.** `file.templates.list` takes `scheme`: Default lists the IDE's templates over the
   built-in ones, Project the project's over those. `file.templates.useScheme { scheme }` switches
   and writes or deletes no template: the choice is the user's, kept per project in the IDE's data
   folder (`file-template-schemes.json`), not in `.idea/workspace.xml`, which we do not write
   (ADR-0051). A project never switched uses Project when it has
   `.idea/fileTemplates` (ADR-0115), Default otherwise. Switching to Project writes nothing until a
   template differs; switching back to Default keeps `.idea/fileTemplates`, so the project's
   templates come back with Project (no warning, no deletion).
7. **The settings page** (Editor → File and Code Templates, after Inspections and before Live
   Templates, page `fileTemplates`) keeps a draft per scheme until OK or Apply, which send
   `file.templates.set`, `remove`, `reset` and `useScheme`. The Default draft is always saved; the
   Project draft only when Project is the scheme chosen. Built-in templates cannot be removed or
   renamed; a changed one is drawn in the link colour, as on Live Templates, and Revert to Original
   Template takes the selected one back. Create Template and Copy Template are off on the Code tab
   (the IDE makes those). The variables (`file.templates.variables`) are listed under the editor
   with their descriptions.

## Consequences

- Plug-ins already declare built-in file templates (`fileTemplates` in `plugin.json`): Java (Java
  Class, Interface…), Kotlin (Kotlin File, Class…), XML File, and the components, pages and layouts
  of Vue, React, Svelte, SvelteKit, Next.js and Nuxt. None of them declares `markers` yet, so New
  offers each in every folder of a project whose IDE has the plug-in. HTML File, Stylesheet,
  JavaScript File and TypeScript File in the base plug-ins and Go File in the Go module are separate
  tasks.
- Our own templates keep the `.ft` suffix of ADR-0115; templates named `<name>.<ext>`, the
  naming of JetBrains template files, are read too. A "Revert changes" link by the page title and the toolbar
  shortcuts (Create Template ⌃N, Remove Template ⌦) do not exist yet.
- File → New is native on macOS only; elsewhere the popup and the tree's menu give the same list.
