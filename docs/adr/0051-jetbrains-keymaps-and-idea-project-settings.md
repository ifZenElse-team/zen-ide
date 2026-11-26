# ADR-0051: JetBrains keymaps and .idea project settings

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

People move to a Zen IDE from JetBrains IDEs, and many keep using both on the same project. The
requirement: import their colour schemes and keymaps, and the owner added on 2026-10-18: a project
opened in GoGarden reads its `.idea/` settings (code style, file encodings, excluded folders, run
configurations, inspection profiles, scopes) and never breaks or rewrites them, so the same project
keeps working in the JetBrains IDEs and in Zen IDEs side by side.

ADR-0011 already says that JetBrains data formats are read through `zen-jb-import`; colour schemes
and UI themes are done (ADR-0045). The project files are different from a theme: they live in the
user's repository, are often committed and shared with a team, and the JetBrains IDE rewrites them
on its own schedule. A Zen IDE that rewrote one of them, even with the same meaning, would produce
diffs, conflicts with the other IDE and lost settings. The formats are documented only in part, so
the readers are built clean-room (ADR-0002) from public documentation and our own synthetic files.

The language-independent shape of these settings is needed below the feature layer, as the code
style of ADR-0053 is, and the platform must not depend on the importer (ADR-0014).

## Decision

1. **`zen_project::settings` (L1) holds the shape and the extension point.** `ProjectSettings` has
   `codeStyle` (by language id, a `CodeStyleOverride` of `useTabs`, `tabSize`, `indent`,
   `continuationIndent` and `rightMargin`, each optional, applied over ADR-0053's `CodeStyle`),
   `encodings` (`default`, `files` of `{ path, charset }`, `bomForNewFiles`), `excluded` folders,
   `excludePatterns`, `inspectionProfiles` (`name`, `file`, `isProjectProfile`, `tools` of
   `{ class, enabled, level }`), `scopes` (`name`, `pattern`), `runConfigurations`
   (`ForeignRunConfiguration`: `sourceFile`, `name`, `typeId`, `factory`, `options`, `env`,
   `shared`), `problems` (`file`, `message`) and `sources`, every file read. A format implements

   ```rust
   pub trait SettingsProvider: Send + Sync {
       fn id(&self) -> &str;
       fn watches(&self, relative: &Path) -> bool;
       fn load(&self, root: &Path, files: &dyn ReadFiles) -> Option<ProjectSettings>;
   }
   ```

   and reads only through `ReadFiles` (`read`, `list`), which has no way to write.
2. **`zen_jb_import::IdeaProject` reads JetBrains projects** for every IDE: the `.xml` files of
   `.idea/` (but the per-user `workspace.xml`, `shelf/`, `tasks.xml` and `usage.statistics.xml`),
   `.run/*.run.xml`, the `*.iml` files at the root and those `modules.xml` lists. Paths are written
   with the macros `$PROJECT_DIR$`, `$MODULE_DIR$` and `$USER_HOME$` and as `file://` URLs. Only
   shared run configurations are read; the per-user ones of `workspace.xml` are not. A file that
   cannot be read or parsed is a problem in the result, never an error of the command.
3. **The commands** (`zen-workbench`, session state `ProjectSettingsState`):

   | Command | Parameters | Result |
   |---|---|---|
   | `project.settings.get` | `{}` | `{ revision, settings }` |
   | `project.settings.refresh` | `{ paths?: [path] }` | `{ revision, changed }` |

   The settings are read when `file.project.open` opens a project and again when a watched file
   changes on disk (the same path as `project.model.refresh`); `revision` rises when they change.
4. **Never writing is guaranteed three ways.** By construction: providers get only `ReadFiles`, and
   nothing in Zen IDEs writes these formats. By guards: `zen_workbench::guard::refuses_foreign_settings`
   is true for a path with a `.idea` or `.run` component or ending in `.iml`, and every automatic
   writer refuses such a file with a message, changing nothing: the edit engine of refactorings
   (open documents included, so a later Save All cannot carry the change) and Replace in Files. By a
   test: a synthetic project is opened, its settings read, files edited, formatted, saved and renamed
   through the commands, and the bytes and times of `.idea/`, `.run/` and `*.iml` must not change; a
   project without them must not get them. The user can still open and save such a file explicitly.
5. **What the settings change:** keymap `.xml` import as named keymaps with our action mapping
   table; the code style laid over the user's code style of ADR-0053 for Enter, Tab, Indent, the
   editor's tab width and Reformat Code (a language's own entry first; plain text takes `TEXT`,
   then `OTHER_INDENT_OPTIONS`, as other file types take `OTHER_INDENT_OPTIONS`, added 2026-10-18); encodings for opening (a file in another charset, or text
   that is not valid UTF-8, opens read-only with a clear message and is never saved; bytes that are
   not UTF-8 and contain NUL, with no charset of their own, are a binary file and are not opened);
   excluded folders in the tree, the index and Find in Files, combined with our built-in ones;
   scopes and inspection profiles exposed to agents. Each documents its mapping in
   [docs/jetbrains-compatibility.md](../jetbrains-compatibility.md).
6. **Keymaps** are the user's files, not the project's, so they are imported, not read in place.
   `zen_core::keymap::KeymapImporter` (`detects`, `import`) turns a file into a `KeymapImport`
   (`name`, `parent`, `shortcuts` by our action, `mapped`, `unmapped` of `{ actionId, reason,
   shortcut? }` and `warnings`); `zen_jb_import::JetBrainsKeymapImporter` reads `<keymap>` files
   with our own table of JetBrains action ids (`KEYMAP_ACTIONS`, several ids may share an action;
   a test checks every target against `Keymap::default_macos` and the table in the document).
   Keystrokes in Java's `KeyStroke` text form become ours only when they read back from their
   text; two-keystroke and mouse shortcuts and unknown keys are reported, not guessed. The parent
   `Mac OS X 10.5+` is our macOS keymap; any other parent is a warning, and the file's entries are
   applied over our macOS keymap as written. A named keymap is `<data>/keymaps/<id>.json`,
   `{ formatVersion: 1, id, name, parent: "macos", source?, shortcuts }`, with the id taken from the
   name (`-2`, `-3`… when taken) and always equal to the file name, so no id reaches outside the
   folder; `IdeSettings.keymap` names the keymap in use (none: the built-in macOS keymap). The
   active keymap is `Keymap::default_macos().with_user_changes(shortcuts)`; edits of the built-in
   keymap still go to `keymap.json` (ADR-0038), edits of a named one to its file. The store is
   `zen_workbench::keymaps::KeymapStore`, with the commands `ui.keymap.list`, `select`, `import`,
   `duplicate`, `rename`, `delete` and `reload` and the MCP tools `list_keymaps`, `select_keymap`
   and `import_keymap`; `get_keymap` and `set_keymap` act on the keymap in use.
7. **Run configurations are adopted, never written.** `zen_run::RunProvider::adopt(&ForeignRunConfiguration,
   root) -> Option<Configuration>` (none by default) maps a foreign configuration to ours;
   `zen_lang_go::GoRun` adopts Go Application and Go Test. `zen_project::run::Configuration` gets
   `adoptedFrom` (the foreign file), never written to `.zen/runConfigurations.json` and dropped when
   that file is read. The `run.*` commands list adopted configurations after the file's ones, with
   the id `jb:<relative file>#<name>`; they run and can be selected, `update` and `delete` refuse
   them, and `run.configuration.saveAdopted { id }` copies one into our file, where a configuration
   of the same kind and name hides the adopted one. The writer of our file checks the guard.

## Consequences

- A project shared with a JetBrains IDE keeps working there; nothing Zen IDEs do appears in its diffs.
- Agents see the same settings and the problems found in them through `project.settings.get`.
- `zen-jb-import` depends on `zen-project`; both still build for WebAssembly.
- ADR-0048's file format gains `adoptedFrom`, which our file never holds; its run commands gain
  `run.configuration.saveAdopted`.
- A future writer of project files must use the guard, and the test fails if one forgets.
- Settings we change in our own dialogs stay in our own files; they never flow back into `.idea/`.

## Alternatives considered

- **Writing `.idea/` when the user changes a setting in a Zen IDE:** the JetBrains IDE owns these
  files and rewrites them; two writers would fight, and partial knowledge of the formats would lose
  what we do not understand.
- **Converting `.idea/` into our own files once, on open:** the copies would go stale as soon as the
  JetBrains IDE changed the originals.
- **Reading `workspace.xml`:** it is per-user state, not project settings, and changes on every run
  of the other IDE.
- **The reader in `zen-workbench`:** the format belongs with the other JetBrains formats in
  `zen-jb-import` (ADR-0011), and other platform crates could not reuse the shape.
