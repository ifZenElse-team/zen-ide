# ADR-0123: Mark Directory as Excluded writes the module file, and folders are excluded by the kind of project, not by name

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md) (`.idea` settings,
  read only; amended here for exclusions), [ADR-0108](0108-file-type-registry-and-ignored-files.md)
  (ignored files and folders),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents), [ADR-0050](0050-products-are-plugin-bundles.md)
  (capabilities in modules)

## Context

Excluded folders were only read: `excludeFolder` and `excludePattern` of the `*.iml` files
(ADR-0051) and a built-in list, `zen_vfs::EXCLUDED` = `target`, `node_modules`, `dist`, `build`,
`vendor`, excluded by name in every project. The requirement (the analysis of
2026-10-20): a folder is excluded with the Project tool window's Mark Directory
as → Excluded and included again with Cancel Exclusion; the mark is kept per project in the module's
`*.iml` (`<excludeFolder url>` inside `<content>`), listed by `.idea/modules.xml`; Project ⋮ →
Excluded Files hides excluded folders. No folder is excluded by its name alone: `vendor` is not
excluded (a Go module with vendoring resolves through it and finds its files), nor are `build` and
`dist`; folders are excluded by the model of a kind of project (Cargo's support excludes a
package's `target`, the JavaScript support an npm package's `node_modules`). Ignored files and
folders (ADR-0108) are another thing: names the IDE never shows, kept per IDE.

## Decision

1. **Writing the module file is the one exception to ADR-0051's "never written".** Only an explicit
   request, the menu item or the commands below, writes it. `SettingsProvider` gets
   `set_excluded(root, folder, excluded) -> Option<Result<ExclusionChange, String>>` (default:
   `None`, the provider keeps no exclusions); `zen_jb_import::IdeaProject` implements it. Automatic
   writers keep refusing `.idea`, `.run` and `*.iml` (`guard::refuses_foreign_settings`).
2. **The module.** The module files are those the reader reads (the `*.iml` at the root and those
   `.idea/modules.xml` lists). The folder goes into the module whose `<content url>` holds it most
   closely. Without any module file and without `.idea/modules.xml`, `.idea/<project folder>.iml`
   (a `WEB_MODULE` with one content root `file://$MODULE_DIR$`, `inheritedJdk` and `sourceFolder`
   order entries) and `.idea/modules.xml` listing it are created, in the `.idea` format; when
   `.idea/modules.xml` exists but no module holds the folder, the command fails and writes nothing.
   `$MODULE_DIR$` of a module file in `.idea/` is the project's root (how such files are written:
   `<content url="file://$MODULE_DIR$">` is the whole project); the reader follows the same rule.
3. **Byte for byte.** The file is edited as text, never re-serialised: one line
   `<excludeFolder url="file://$MODULE_DIR$/<path>" />` is inserted with the indentation and line
   ending of its neighbours, before the first `excludeFolder` whose URL sorts after it, else after the
   last one, else before the first `excludePattern`, else after the last child; an empty
   `<content … />` is opened for it and closed again when Cancel Exclusion removes its last child.
   Cancel Exclusion removes the line of every `excludeFolder` whose URL resolves to the folder. A
   folder outside the module's folder is written with `$PROJECT_DIR$`, or as an absolute URL.
   Writes are atomic (`zen_vfs::write_atomic`).
4. **At once.** The commands read the settings again and build the index again when they changed
   (`index_project`), so Go to File, Search Everywhere and Find in Files drop or find the folder in
   the same call; Search Everywhere starts its run again when the settings' revision changes. The tree
   follows the settings' revision.
5. **Commands** (ADR-0033): `project.directory.markExcluded { path }` and
   `project.directory.cancelExclusion { path }` answer `{ changed, files, excluded, revision }`.
   The view action `view.project.excludedFiles { show? }` turns Project ⋮ → Excluded Files on or off
   (toggles without `show`) and answers `{ shown }`; the choice is kept per project in
   `workspace.json` (`hideExcludedFiles`). With
   it off, the tree leaves out the rows it would draw as excluded (folders marked in the project,
   folders the kind of project excludes, names matching `excludePattern`).
6. **No folder is excluded by its name alone.** `zen_vfs::EXCLUDED` is removed. A kind of project
   declares a folder it excludes and the file beside it that makes a folder that kind
   (`zen_vfs::excluded_folders`): the platform declares `node_modules` beside `package.json` (basic
   web is in every IDE), the Rust module declares `target` beside `Cargo.toml`
   (`ModuleHost::excluded_folder`), so a Cargo project's `target` is excluded where the Rust module
   is on, and not in an IDE without it. The tree, Expand All, the
   index, the file watcher and Local History use `excluded_folders::is_excluded` /
   `excluded_folders::excludes`. `vendor`, `build` and `dist` are ordinary folders. Such folders are
   shown excluded but cannot be marked or cancelled in the menu (Excluded is greyed); only folders of
   the module files are.
7. **What Git ignores stays out while it changes.** The index's first walk already respects
   `.gitignore`; refreshing it from the file watcher now does too (`FileIndex::git_ignores`: the
   `.gitignore` and `.ignore` files of a path's folders up to its repository, `.git/info/exclude`
   and Git's global excludes file, read once per folder and again when one of them changes), so the
   index after a build is the index a new walk would give. Local History does not record external
   changes to ignored files, and the window does not compute Git's status again for changes inside
   an ignored folder. This keeps a build writing an ignored `target`, `bin` or `dist` cheap in every
   IDE without excluding those folders by name: in GoGarden a Cargo build writing 200,000 files into
   `target` adds nothing to the index or Local History (before: every file indexed and recorded).

## Consequences

- A project shared with a JetBrains IDE sees the same exclusions in both IDEs; a project that had no
  `.idea` gets one the first time a folder is marked, which version control shows as new files.
- In GoGarden, a Go module's `vendor` is indexed and searched; in an IDE without the
  Rust module, a Cargo project's `target` is not excluded, but Git's `.gitignore` keeps it out of
  the index and Local History, when the project opens and while a build writes it (decision 7).
- Settings → Project Structure (editing exclusions and Exclude files patterns in one page) and
  Resource Root are later work.
