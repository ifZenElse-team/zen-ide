# Working side by side with JetBrains IDEs

A project can be open in a JetBrains IDE and in a Zen IDE at the same time. Zen IDEs read the
project's JetBrains settings and never change them ([ADR-0051](adr/0051-jetbrains-keymaps-and-idea-project-settings.md)),
except two changes the user asks for: the excluded folders of the module files when the user
marks a folder ([ADR-0123](adr/0123-mark-directory-as-excluded.md), [below](#marking-a-folder-excluded)),
and the words saved to the project dictionary
([below](#project-dictionaries), [ADR-0161](adr/0161-spelling-quick-fixes-and-dictionaries.md)).
The formats are implemented clean-room from public documentation and our own synthetic files
(ADR-0002); the test project is `crates/platform/jb-import/tests/fixtures/idea-project`.

## What is read

`zen_jb_import::IdeaProject`, registered for every IDE, reads:

| Files | Status |
|---|---|
| `.idea/**/*.xml`, but `workspace.xml`, `shelf/`, `tasks.xml`, `usage.statistics.xml` | listed in `sources` |
| `.idea/modules.xml` and the `*.iml` files it lists, `*.iml` at the root | listed in `sources` |
| `.run/*.run.xml` | listed in `sources` |
| `.idea/codeStyles/` (code style per language) | `codeStyle`: Enter, Tab, Indent, the editor's tab width, the status bar and Reformat Code |
| `.idea/encodings.xml` (file encodings) | `encodings`: opening, saving and the status bar |
| `excludeFolder` and `excludePattern` of `*.iml` (excluded folders) | `excluded`, `excludePatterns`: the tree, the index, Find in Files |
| `.idea/scopes/` (named scopes) | `scopes`, exposed to agents |
| `.idea/inspectionProfiles/` (inspection profiles) | `inspectionProfiles`, exposed to agents, not applied |
| `.idea/runConfigurations/*.xml`, `.run/*.run.xml` (shared run configurations) | `runConfigurations`; Go ones adopted, read-only, by the run service |
| keymap `.xml` files exported by the user | imported as named keymaps (below) |

Paths in these files use the macros `$PROJECT_DIR$`, `$MODULE_DIR$` and `$USER_HOME$` and `file://`
URLs; they are resolved against the project and module folders, with `.` and `..` resolved. A file
that cannot be read or parsed is reported in `problems`. To stay inside the project and cheap:

- symbolic links under the project are not followed, neither listed nor read;
- a file over 4 MB is not read, and XML nested deeper than 256 elements is refused before parsing;
- the per-user and cache entries of `.idea/` (`workspace.xml`, `shelf/`, `tasks.xml`,
  `usage.statistics.xml`, `dataSources/`, `dataSources.local.xml`, `httpRequests/`, `sonarlint/`)
  are neither read nor watched, and only `.xml` files of `.idea/`, `.run/*.run.xml` and `*.iml`
  files make the settings be read again when they change on disk.

## Code style

The reader (`crates/platform/jb-import/src/idea/code_style.rs`) and its option table below are
written from the public documentation of the format and tested on our own synthetic files.

`.idea/codeStyles/codeStyleConfig.xml` says whether the project keeps its own scheme: only when its
`<state>` has `<option name="USE_PER_PROJECT_SETTINGS" value="true"/>` is
`.idea/codeStyles/Project.xml` read. Without the file, or with `PREFERRED_PROJECT_CODE_STYLE` (a
scheme of the IDE, not of the project), nothing is read. From the `<code_scheme>` of `Project.xml`:

| Written there | `CodeStyleOverride` of |
|---|---|
| `<option name="RIGHT_MARGIN">` of `<code_scheme>` | `default`: `rightMargin` |
| `<option name="SOFT_MARGINS">` of `<code_scheme>` (columns separated by commas) | `default`: `visualGuides` |
| `OTHER_INDENT_OPTIONS` `<value>` of `<code_scheme>` | `default`: the fields below |
| `<codeStyleSettings language="X"><option name="RIGHT_MARGIN">` | X: `rightMargin` |
| `<codeStyleSettings language="X"><option name="SOFT_MARGINS">` | X: `visualGuides` |
| `indentOptions` `USE_TAB_CHARACTER` (`true`, `false`) | X: `useTabs` |
| `indentOptions` `TAB_SIZE` | X: `tabSize` |
| `indentOptions` `INDENT_SIZE` | X: `indent` |
| `indentOptions` `CONTINUATION_INDENT_SIZE` | X: `continuationIndent` |

An option that is not written sets nothing: the IDE's own value stays, which is also how the JetBrains IDE reads the file. A
value that is not a number (or is zero, for the sizes and the margin) is ignored and reported in
`problems`. Other options are not read. Language names become our language ids:

| JetBrains language | Language id |
|---|---|
| `go` | `go` |
| `Rust` | `rust` |
| `JSON`, `JSON5` | `json`, `json5` |
| `yaml` | `yaml` |
| `TOML` | `toml` |
| `Markdown` | `markdown` |
| `XML`, `HTML`, `CSS`, `SCSS` | `xml`, `html`, `css`, `scss` |
| `JavaScript`, `TypeScript` | `javascript`, `typescript` |
| `Shell Script` | `shellscript` |
| `Python`, `SQL`, `JAVA`, `kotlin` | `python`, `sql`, `java`, `kotlin` |
| `Dockerfile`, `Makefile`, `Properties` | `dockerfile`, `makefile`, `properties` |
| `protobuf` | `proto` |
| `TEXT` | `plaintext` |
| any other | the name in lower case |

`zen_workbench::project_settings::code_style_override(session, language)` picks the entry of a
file's language (`zen_workbench::code_style::language_of`: `.txt` and `.text` are `plaintext`, an
unknown extension is itself, a file without one is `default`). A programming language we name
(`code_style::has_own_style`: Go, Rust, YAML, Kotlin...) or one the project has an entry for keeps
to that entry, with the project's right margin and visual guides when it has none of its own; the other options of
`OTHER_INDENT_OPTIONS` leave it alone, which is also how the JetBrains IDE reads the file. Plain text and the other file types
take each field from the first that sets it:

1. the language's own entry (`TEXT` for plain text);
2. the `default` entry (`OTHER_INDENT_OPTIONS` and the scheme's right margin);
3. the user's code style.

The override is laid over the user's code style of
Editor → Code Style ([ADR-0053](adr/0053-fonts-ui-scale-and-code-style.md), `zen_workbench::code_style`):
what the project's files set wins, the rest stays the user's (`project_settings::code_style_of` for
an open document, `code_style_for_path` without its text). That style is what Enter, Tab, Indent
and Unindent insert, how wide the editor draws a tab character, what the status bar shows (`Tab` or
`4 spaces`) and what Reformat Code sends the language server (`tabSize`: the tab size with tabs, the
indent with spaces; `insertSpaces`). A project without these files gets exactly the user's style.
The Editor → Code Style page still shows and edits the user's style only. The right margin (120
when the project sets none) and the visual guides are the vertical lines the editor draws while
Settings → Editor → Appearance → Show hard wrap and visual guides is on
(`zen_workbench::visual_aids::margins_of`); the continuation indent is read but not used yet.

## Encodings

From the `Encoding` component of `.idea/encodings.xml`:

| Written there | `Encodings` |
|---|---|
| `<file url="PROJECT" charset="…"/>` | `default` |
| `<file url="file://…" charset="…"/>` (macros expanded) | `files` as `{ path, charset }` |
| `addBOMForNewFiles` `with BOM`, `with NO BOM` | `bomForNewFiles` `true`, `false` |
| `addBOMForNewFiles` `with BOM under Windows, with no BOM otherwise` | `true` on Windows only |

A file takes the charset of the closest listed file or folder, otherwise the project's default
(`Encodings::charset_for`). `defaultCharsetForPropertiesFiles` is not read, and URLs other than
`file://` are reported in `problems`.

Zen IDEs edit only UTF-8. When a file is opened:

- valid UTF-8 opens as before, whatever the charset is (UTF-8, US-ASCII or none);
- a charset other than UTF-8 or US-ASCII, or bytes that are not valid UTF-8 and have no NUL byte,
  open read-only: the text is shown as UTF-8 with replacement characters, edits are refused, the
  message `workbench-error-encoding` names the charset, and Save and Save All never write it;
- bytes that are not valid UTF-8 and have a NUL byte, in a file the settings give no other charset,
  are a binary file and are not opened, as before ("the file is not UTF-8 text").

The status bar shows the charset the settings give the file (`UTF-8` otherwise) and, for a
read-only file, `Read-only`, whose tooltip says why. Converting between encodings is not done.

## Excluded folders

Every module file (the `*.iml` files at the root and those `.idea/modules.xml` lists, in any
folder) is read for its `<component name="NewModuleRootManager">`. Each `<content url>` adds its
folder to `contentRoots`. In each `<content url>` of it:

- `<excludeFolder url="file://…"/>` adds the folder to `excluded`, as an absolute path;
  `$MODULE_DIR$` is the folder of the module file (the project's root for a module file in
  `.idea/`, whose `<content url="file://$MODULE_DIR$">` is the whole project), `$PROJECT_DIR$` the
  project's root. URLs other than `file://` are ignored.
- `<excludePattern pattern="…"/>` adds its patterns to `excludePatterns`; one attribute may hold
  several, separated by `;`. A pattern has the `*` and `?` wildcards and matches the name of a file
  or folder (`*.log`, `node_modules`), case-sensitively, anywhere in the project.
- `<sourceFolder url="file://…"/>` adds the folder to `sourceFolders` with its kind: `resources`
  for `type="java-resource"`, `testResources` for `type="java-test-resource"`, `testSources` for
  `isTestSource="true"`, `sources` otherwise (ADR-0159). Resource roots (of both kinds) are drawn in
  the tree and on Settings → Project Structure; source and test roots are read, not used.
- The other children are not used.

The excluded folders and patterns are combined with the folders a kind of project excludes: the
project tree shows them in the excluded style and **Expand All** does not open them; the file index
leaves them out, so Go to File, Search Everywhere and Find in Files do not find their files. The
index is built again when the settings change. The file watcher still reports changes in them; they
are dropped by the index. Project ⋮ → Excluded Files off hides them from the tree.

No folder is excluded by its name alone (`zen_vfs::excluded_folders`, ADR-0123):

| Folder | Excluded when | Declared by |
|---|---|---|
| `node_modules` | `package.json` is beside it | the platform (basic web is in every IDE) |
| `target` | `Cargo.toml` is beside it | the Rust module (RustRonin; off in GoGarden) |
| `vendor`, `build`, `dist` | never by name | — |

A Go module with vendoring (`vendor/modules.txt`) therefore finds its files in `vendor` in Go to
File, Search Everywhere and Find in Files. Version control internals (`.git`, `.hg`,
`.svn`) and the ignored names of Settings → Editor → File Types (ADR-0108) are a separate list, kept
per IDE, not per project.

What Git ignores is left out of the index too, as its first walk leaves it out: `FileIndex::refresh`
drops the paths the file watcher reports when `FileIndex::git_ignores` says a `.gitignore` or
`.ignore` of their folders, `.git/info/exclude` or Git's global excludes file ignores them, Local
History does not record them, and Git's status is not computed again for changes inside an ignored
folder. A build that writes a Git-ignored folder (a Cargo project's `target` in an IDE without the
Rust module, Go's `bin`, a web project's `dist`) therefore costs the IDE nearly nothing; the tree
still shows the folder.

## Marking a folder excluded

Project tool window → a folder's context menu → **Mark Directory As → Excluded**
(`project.directory.markExcluded { path }`) and, on a folder the module files exclude, **Mark
Directory as → Cancel Exclusion** (`project.directory.cancelExclusion { path }`); both answer
`{ changed, files, excluded, revision }`. A folder excluded by its kind of project or by an
`excludePattern` shows Excluded greyed. `zen_jb_import::IdeaProject::set_excluded` writes:

- the module file whose `<content url>` holds the folder most closely, among those the reader reads;
  one line `<excludeFolder url="file://$MODULE_DIR$/<path>" />` is inserted with its neighbours'
  indentation and line ending, among the `excludeFolder` lines by URL order (else before the first
  `excludePattern`, else after the last child); everything else keeps its bytes. An empty
  `<content … />` is opened for it, and closed again as `<content … />` when its last child goes;
- Cancel Exclusion removes the line of every `excludeFolder` that resolves to the folder;
- a project with no module file and no `.idea/modules.xml` gets `.idea/<project folder>.iml` (a
  `WEB_MODULE` with the content root `file://$MODULE_DIR$`) and `.idea/modules.xml` listing it, as a
  JetBrains IDE makes them; other files of `.idea/` are not touched. When `.idea/modules.xml` lists
  no module that holds the folder, nothing is written and the command fails.
- an opened `<content … />` takes the indentation step of the file (tabs stay tabs); a read-only
  module file or a read-only folder is not written, and the command fails.

The settings are read again and the index rebuilt in the same call. Tests:
`crates/platform/jb-import/tests/idea.rs` (exact bytes, round trips through the reader),
`zen-workbench` `project_settings` and the `zen-ui` tests of the tree.

## Resource roots and Exclude files (ADR-0159)

**Mark Directory As → Resource Root** (`project.directory.markResourceRoot { path }`) and **Unmark
as Resource Root** (`project.directory.unmarkResourceRoot { path }`) answer as the exclusion
commands do, with `resourceRoots` and `excludePatterns` besides `excluded`
(`IdeaProject::set_resource_root`):

- marking inserts `<sourceFolder url="file://$MODULE_DIR$/<path>" type="java-resource" />` into the
  module file that holds the folder, as above: among the `sourceFolder` lines by URL order, else
  before the first `excludeFolder` or `excludePattern`, the order a JetBrains IDE writes them in;
- a folder has one mark: marking it as a resource root removes its `excludeFolder` and any other
  `sourceFolder` of it (a source root becomes a resource root), and Mark Directory As → Excluded
  removes its resource `sourceFolder`;
- unmarking removes every resource `sourceFolder` (`java-resource` or `java-test-resource`) that
  resolves to the folder.

Settings → Project Structure → **Exclude files** is `project.structure.setExcludePatterns
{ patterns }` (`IdeaProject::set_exclude_patterns`): each entry may hold several patterns separated
by `;`; a pattern with `/`, `\` or `;` in it is refused, as the patterns are names. The module
files' `<excludePattern>` lines that hold only wanted patterns, each once, are kept byte for byte;
the others are removed, and the missing patterns are added one per line, after the last
`excludePattern`, in the module of the `<content>` whose URL is the project's root (else the first
one). A project without module files gets one, as for Mark Directory As. `project.structure.get {}`
answers what the page shows: `{ root, contentRoots, excluded, resourceRoots, excludePatterns,
editable, revision }`.

## Scopes

Each `.idea/scopes/*.xml` file holds `<component name="DependencyValidationManager">` with
`<scope name="…" pattern="…"/>` elements. Each becomes a `Scope` with its name and its pattern as
written (`file[orchard]:internal//*&&!file:*_test.go`). Agents read them with `project.settings.get`;
Find in Files and Search Everywhere offer them as `named:<name>` scopes and apply their `file:`
patterns with `&&`, `||`, `!`, parentheses and `$Name` (ADR-0083); a scope with another kind of
pattern (`src:`, `lib:`...) is listed with its error. Scopes edited in Zen IDEs are kept in
`.zen/scopes.json` and hide a `.idea` scope of the same name, which is never rewritten.

## Inspection profiles (read, not applied)

`.idea/inspectionProfiles/profiles_settings.xml` says which profile the project uses:
`<option name="PROJECT_PROFILE">` (by default `Project Default`) and `USE_PROJECT_PROFILE` (by
default `true`; `false` means the IDE's own profile is used). Every other `.xml` file of the folder
holds `<profile>` with `<option name="myName">` (the file's name, with `_` as spaces, when missing)
and `<inspection_tool class enabled level/>` elements. Each profile becomes an `InspectionProfile`
with its `name`, `file`, `isProjectProfile` (it is the profile named and the project uses its own)
and `tools` (`class`, `enabled`, `level` as written; `enabled_by_default` and the options inside a
tool are not read). Nothing in Zen IDEs acts on them yet: the inspections of the JetBrains IDEs are
not ours, so the profiles are only exposed to agents through `project.settings.get`.

A module, scope or profile file that is not well-formed XML adds a problem (`cannot parse …`) and is
otherwise skipped; the other files are still read.

## Run configurations

The shared run configurations are read into `runConfigurations`, as
`zen_project::settings::ForeignRunConfiguration`, and the run service adopts the ones a run provider
knows (below).

- **Files.** `.idea/runConfigurations/*.xml` and `.run/*.run.xml`, each with a
  `<component name="ProjectRunConfigurationManager">` (the root, or a child of `<project>`) holding
  one or more `<configuration name type factoryName …>`. Every one of them has `shared: true`. The
  per-user configurations of `.idea/workspace.xml` (`RunManager`, temporary ones included) are not
  read: that file is the user's state and changes on every run.
- **Skipped.** Templates (`default="true"`).
- **Fields.** `name`, `type` → `typeId`, `factoryName` → `factory`, the file → `sourceFile`. A type
  we do not know is kept as written.
- **Environment.** `<envs><env name value/></envs>` → `env`, in order (a missing value is empty); it
  is not repeated in `options`.
- **Options.** Every other attribute of `<configuration>` (e.g. `folderName`) is an option under its
  own name. Every child element is flattened into `options`:
  - its key is the `name` attribute of an `<option>`, the `key` attribute of an `<entry>`, and the
    tag name of any other element (`<kind value="PACKAGE"/>` → `kind`);
  - a nested element's key is its parent's key, a dot and its own
    (`<option name="TAGS"><list><option value="a"/>` → `TAGS.list.option`), down to 17 levels;
    deeper elements are not read;
  - siblings with the same key are numbered from 0 (`TAGS.list.option.0`, `TAGS.list.option.1`);
  - the `value` attribute is the key's value; any other attribute is `key.attribute`
    (`<module name="app"/>` → `module.name`, `<method v="2"/>` → `method.v`,
    `<method><option name="Make" enabled="true"/>` → `method.Make.enabled`).
- **Before launch.** The `<option>` children of `<method v="2">` are also read in order into
  `beforeLaunch`, each with its `name` as `id`, `enabled="true"` as `enabled` and its other
  attributes as `options`, macros expanded; adoption maps them to before-launch tasks
  ([run-configurations.md](run-configurations.md#before-launch)).
- **Macros.** `$PROJECT_DIR$` and `$USER_HOME$` are expanded in every value and environment value.
  The macros our run configurations know (`$FilePath$`, `$ProjectFileDir$`, `$Prompt$`…) stay as
  written and expand at every launch, as in our own configurations; `$MODULE_WORKING_DIR$` and other
  macros stay literal.
- **Allow multiple instances.** The attribute `allowRunningInParallel="true"` of `<configuration>`
  turns on `common.allowMultipleInstances` of every adopted configuration. Environment files are not
  adopted: we found no public description of how the shared files store them.
- **Problems.** A file that is not XML, has no `ProjectRunConfigurationManager` component, or has a
  configuration without a name or a type is reported in `problems`, and nothing of it is read.

### Adoption into our run configurations

A run provider adopts a foreign configuration with `zen_run::RunProvider::adopt(&ForeignRunConfiguration,
root) -> Option<Configuration>` (none by default). The `run.*` commands
([run-configurations.md](run-configurations.md#configurations-shared-by-jetbrains-ides)) list what is
adopted after the configurations of `.zen/runConfigurations.json`, read-only, with the id
`jb:<file relative to the project>#<name>` and `adoptedFrom`, the file. Running and selecting one
works; `run.configuration.update` and `run.configuration.delete` refuse it with a message; Save as
Zen Configuration (`run.configuration.saveAdopted`) copies it into our file under the same name, and
the copy then hides it. A configuration of our file with the same kind and name hides an adopted
one, and so does an earlier adopted one. The `.idea` and `.run` files are never written: the
writer of `.zen/runConfigurations.json` checks the guard, `adoptedFrom` is never written, and the
never-write test changes, saves and removes run configurations too. A type no provider adopts is
not listed. The Go provider (`zen_lang_go::GoRun`) maps:

| JetBrains (`typeId`, option) | Ours |
|---|---|
| `GoApplicationRunConfiguration` | kind `go.build`, `runAfterBuild: true` |
| `GoTestRunConfiguration` | kind `go.test` |
| `kind` `PACKAGE` / `FILE` / `DIRECTORY` | `runKind` / `testKind` `package` / `file` / `directory` (`package` otherwise) |
| `package` | `package` |
| `filePath` | `files`, relative to the project (quoted when it has a space) |
| `directory` | `directory`, relative to the project (`.` for the root) |
| `output_directory` | `outputDirectory`, relative to the project |
| `pattern` (Go Test) | `pattern` |
| `framework` `gobench` with `pattern` | `bench` = `pattern` (`.` when empty), `pattern` = `^$` |
| `framework` `gocheck` | not adopted |
| `working_directory` | `common.workingDirectory`, relative to the project (none for the root) |
| `parameters` | `common.programArguments` |
| `go_parameters` | `common.toolArguments` |
| `env` | `common.env` |
| `<method v="2">` tasks | `common.beforeLaunch` ([run-configurations.md](run-configurations.md#before-launch)) |
| `folderName` | `folder` (every adopted configuration) |
| `module.name` and the rest | ignored |

The name is kept. Paths outside the project stay absolute.

The run service itself adopts `CompoundRunConfigurationType` as the kind `compound`: its
`<toRun name type/>` children (flattened to `toRun.name`, or `toRun.0.name`, `toRun.1.name`…) name
the configurations it starts, which are looked up by name among the configurations of
`.zen/runConfigurations.json` and the adopted ones; `type` is not used and a name not found is left
out ([run-configurations.md](run-configurations.md#compound-configurations)).

## Commands

- `project.settings.get {}` answers `{ revision, settings }`: the settings in the shape of
  `zen_project::settings::ProjectSettings`, with `sources` (every file read) and `problems`; the
  project's `.editorconfig` files and their parse errors are among them (ADR-0100).
- `project.settings.refresh { paths? }` reads the files again when one of `paths` is watched, or
  always; the IDE runs it when files change on disk.

## Never written

- Settings providers read through `ReadFiles`, which cannot write; the writers are
  `SettingsProvider::set_excluded`, `set_resource_root` and `set_exclude_patterns`, run only by
  Mark Directory As and Settings → Project Structure (above).
- Automatic writers refuse files under `.idea/` or `.run/` and `*.iml` files
  (`zen_workbench::guard::refuses_foreign_settings`): a rename whose changes reach such a file is
  refused as a whole, and so is Replace in Files; the writer of `.zen/runConfigurations.json`
  checks the guard too, and adopted run configurations are refused by `update` and `delete`.
- `apps/go-garden/tests/idea_project.rs` opens the test project, edits, formats, saves, renames and
  changes run configurations (adopted and our own), and checks that these files keep their bytes
  and modification times, and that a project without them does not get them.
- People can still open such a file and save it themselves.

## Keymaps

A keymap exported from a JetBrains IDE (File → Manage IDE Settings → Export Settings, or the `.xml`
files of the IDE's `keymaps` folder) is imported with Settings → Keymap → ⚙ → Import…, the command
`ui.keymap.import { path, select? }` or the MCP tool `import_keymap`. The reader is
`zen_jb_import::JetBrainsKeymapImporter`, an implementation of `zen_core::keymap::KeymapImporter`;
it is written from the public documentation of the format and tested on our own synthetic files.

The file is `<keymap version name parent>` with `<action id>` elements, each holding
`<keyboard-shortcut first-keystroke [second-keystroke]/>` and `<mouse-shortcut keystroke/>`. An
action that is listed replaces the parent's shortcuts of that action only; an `<action>` with no
shortcuts removes them. Keystrokes are written in Java's `KeyStroke` text form: the modifiers
`meta` (⌘), `control` or `ctrl` (⌃), `alt` (⌥) and `shift` (⇧), then a key: `A`–`Z`, `0`–`9`,
`F1`–`F24`, `ENTER`, `BACK_SPACE`, `TAB`, `ESCAPE`, `SPACE`, `DELETE`, `INSERT`, `HOME`, `END`,
`PAGE_UP`, `PAGE_DOWN`, `LEFT`, `RIGHT`, `UP`, `DOWN`, `SLASH`, `BACK_SLASH`, `COMMA`, `PERIOD`,
`MINUS`, `EQUALS`, `PLUS`, `OPEN_BRACKET`, `CLOSE_BRACKET`, `SEMICOLON`, `QUOTE` and `BACK_QUOTE`, and
the numeric keypad's `MULTIPLY` (`NumPad*`), `ADD` (`+`) and `SUBTRACT` (`−`). A shortcut with a
`second-keystroke` becomes a shortcut of two keystrokes in a row (`⌘NumPad*, 2`).

- **Parent.** A keymap whose parent is `Mac OS X 10.5+` becomes changes of our macOS keymap. Any
  other parent (or none) is reported as a warning (`foreignParent`): the file's own entries are
  applied over our macOS keymap as written, and the parent's shortcuts are not known.
- **Not carried over**, each listed in the report's `unmapped` as `{ actionId, reason, shortcut? }`:
  actions we do not have (`unknownAction`), mouse shortcuts (`mouseShortcut`) and keys we cannot
  name, such as the numeric keypad's digits (`unknownKey`, with both keystrokes when there are
  two). An action whose shortcuts were all left out has none. (`secondKeystroke` is no longer
  reported: such shortcuts are carried over.)
- **Several JetBrains actions on one of ours** (`GotoDeclaration` and `GotoDeclarationOnly`) give
  it the shortcuts of both.

The result is a named keymap, `<data>/keymaps/<id>.json`:

```json
{
  "formatVersion": 1,
  "id": "fixture-keys",
  "name": "Fixture Keys",
  "parent": "macos",
  "source": "/Users/me/Downloads/Fixture Keys.xml",
  "shortcuts": { "editor.line.duplicate": ["⇧⌘D"], "editor.comment.line": [] }
}
```

The id is the name in lower case with other characters turned into `-`, plus `-2`, `-3`… when it
is taken, and is always the file's name. The keymap in use is the setting `keymap` (none: the
built-in macOS keymap); edits of the built-in keymap go to `keymap.json`, edits of a named keymap
to its file. The commands `ui.keymap.list`, `select`, `duplicate`, `rename`, `delete` and `reload`
(and the MCP tools `list_keymaps` and `select_keymap`) manage them; `get_keymap` and `set_keymap`
act on the keymap in use. The JetBrains actions we read, and the actions they become (checked against the code by
`crates/platform/jb-import/tests/keymap.rs`):

<!-- keymap-actions:begin -->

| JetBrains action | Zen IDE action |
|---|---|
| `SaveAll` | `file.document.saveAll` |
| `CloseContent` | `file.document.close` |
| `CloseAllEditorsButActive` | `file.document.closeOthers` |
| `CloseAllEditors` | `file.document.closeAll` |
| `CloseAllUnmodifiedEditors` | `file.document.closeUnmodified` |
| `CloseAllUnpinnedEditors` | `file.document.closeAllButPinned` |
| `CloseAllToTheLeft` | `file.document.closeLeft` |
| `CloseAllToTheRight` | `file.document.closeRight` |
| `CloseAllReadonly` | `file.document.closeReadOnly` |
| `ReopenClosedTab` | `file.document.reopenClosed` |
| `PinActiveTabToggle` | `editor.tab.pin` |
| `NextTab` | `editor.tab.next` |
| `PreviousTab` | `editor.tab.previous` |
| `MoveEditorToOppositeTabGroup` | `editor.split.moveToOpposite` |
| `SplitVertically` | `editor.split.right` |
| `SplitHorizontally` | `editor.split.down` |
| `MoveTabRight` | `editor.split.right/move` |
| `MoveTabDown` | `editor.split.down/move` |
| `Unsplit` | `editor.split.unsplit` |
| `UnsplitAll` | `editor.split.unsplitAll` |
| `NextSplitter` | `editor.split.next` |
| `PrevSplitter` | `editor.split.previous` |
| `Synchronize` | `file.document.syncWithDisk` |
| `NewScratchFile` | `view.scratch.new` |
| `NewScratchBuffer` | `view.scratch.newBuffer` |
| `Scratch.ShowFilesPopup` | `view.scratch.show` |
| `ToggleReadOnlyAttribute` | `file.document.toggleReadOnly` |
| `$Undo` | `editor.edit.undo` |
| `$Redo` | `editor.edit.redo` |
| `OpenFile` | `view.project.open` |
| `NewElement` | `view.newElement.open` |
| `ShowSettings` | `view.settings.open` |
| `ShowProjectStructureSettings` | `view.projectStructure.open` |
| `TogglePresentationMode` | `view.presentationMode.toggle` |
| `ToggleDistractionFreeMode` | `view.distractionFreeMode.toggle` |
| `ToggleZenMode` | `view.zenMode.toggle` |
| `ToggleFullScreen` | `view.fullScreen.toggle` |
| `ZoomInIdeAction` | `view.ideZoom.in` |
| `ZoomOutIdeAction` | `view.ideZoom.out` |
| `ResetIdeScaleAction` | `view.ideZoom.reset` |
| `ShowNavBar` | `view.navigationBar.jump` |
| `ActivateProjectToolWindow` | `view.toolWindow.project` |
| `ActivateStructureToolWindow` | `view.toolWindow.structure` |
| `ActivateTerminalToolWindow` | `view.toolWindow.terminal` |
| `Terminal.NewTab` | `terminal.session.new` |
| `Terminal.SplitVertically` | `terminal.session.split/right` |
| `Terminal.SplitHorizontally` | `terminal.session.split/down` |
| `Terminal.NextSplitter` | `terminal.session.focusNext` |
| `Terminal.PrevSplitter` | `terminal.session.focusPrevious` |
| `ActivateProblemsViewToolWindow` | `view.toolWindow.problems` |
| `ActivateFindToolWindow` | `view.toolWindow.find` |
| `ActivateCommitToolWindow` | `view.toolWindow.commit` |
| `ActivateVersionControlToolWindow` | `view.toolWindow.git` |
| `ActivateNotificationsToolWindow` | `view.toolWindow.notifications` |
| `ActivateTODOToolWindow` | `view.toolWindow.todo` |
| `ActivateMavenToolWindow` | `view.toolWindow.maven` |
| `ActivateGradleToolWindow` | `view.toolWindow.gradle` |
| `ActivateBuildToolWindow` | `view.toolWindow.build` |
| `PreviousOccurence` | `view.occurrence.previous` |
| `NextOccurence` | `view.occurrence.next` |
| `Find` | `view.find.open` |
| `Replace` | `view.replace.open` |
| `FindNext` | `editor.find.next` |
| `FindPrevious` | `editor.find.previous` |
| `FindInPath` | `view.findInFiles.open` |
| `ReplaceInPath` | `view.replaceInFiles.open` |
| `RecentFiles` | `view.recentFiles.open` |
| `Switcher` | `view.switcher.open` |
| `RecentLocations` | `view.recentLocations.open` |
| `FileStructurePopup` | `view.fileStructure.open` |
| `SelectIn` | `view.selectIn.open` |
| `PasteMultiple` | `view.pasteFromHistory.open` |
| `QuickChangeScheme` | `view.quickSwitchScheme.open` |
| `GotoClass` | `view.gotoClass.open` |
| `GotoFile` | `view.gotoFile.open` |
| `GotoSymbol` | `view.gotoSymbol.open` |
| `GotoLine` | `view.gotoLine.open` |
| `SearchEverywhere` | `view.searchEverywhere.open` |
| `GotoAction` | `view.findAction.open` |
| `GotoNextError` | `editor.problem.next` |
| `GotoPreviousError` | `editor.problem.previous` |
| `GotoDeclaration` | `navigate.declaration.goTo` |
| `GotoDeclarationOnly` | `navigate.declaration.goTo` |
| `GotoImplementation` | `navigate.implementation.goTo` |
| `GotoTypeDeclaration` | `navigate.typeDeclaration.goTo` |
| `GotoSuperMethod` | `navigate.superMethod.goTo` |
| `GotoTest` | `navigate.test.goTo` |
| `FindUsages` | `navigate.usages.find` |
| `FindUsagesInFile` | `navigate.usages.findInFile` |
| `ShowUsages` | `navigate.usages.show` |
| `ShowSettingsAndFindUsages` | `navigate.usages.settings` |
| `TypeHierarchy` | `navigate.hierarchy.type` |
| `CallHierarchy` | `navigate.hierarchy.call` |
| `ActivateHierarchyToolWindow` | `view.toolWindow.hierarchy` |
| `HighlightUsagesInFile` | `editor.highlightUsages.toggle` |
| `GotoNextElementUnderCaretUsage` | `editor.highlightUsages.next` |
| `GotoPrevElementUnderCaretUsage` | `editor.highlightUsages.previous` |
| `Back` | `navigate.history.back` |
| `Forward` | `navigate.history.forward` |
| `ToggleBookmark` | `navigate.bookmark.toggle` |
| `ToggleBookmarkWithMnemonic` | `view.bookmark.mnemonic` |
| `GotoNextBookmark` | `navigate.bookmark.next` |
| `GotoPreviousBookmark` | `navigate.bookmark.previous` |
| `ShowTypeBookmarks` | `view.bookmark.goToMnemonic` |
| `ShowBookmarks` | `view.bookmark.show` |
| `ActivateBookmarksToolWindow` | `view.toolWindow.bookmarks` |
| `DeleteMnemonicFromBookmark` | `navigate.bookmark.removeMnemonic` |
| `EditBookmark` | `view.bookmark.rename` |
| `AddAnotherBookmark` | `view.bookmark.moveToList` |
| `ToggleBookmark0` | `navigate.bookmark.toggleMnemonic/0` |
| `GotoBookmark0` | `navigate.bookmark.goTo/0` |
| `ToggleBookmark1` | `navigate.bookmark.toggleMnemonic/1` |
| `GotoBookmark1` | `navigate.bookmark.goTo/1` |
| `ToggleBookmark2` | `navigate.bookmark.toggleMnemonic/2` |
| `GotoBookmark2` | `navigate.bookmark.goTo/2` |
| `ToggleBookmark3` | `navigate.bookmark.toggleMnemonic/3` |
| `GotoBookmark3` | `navigate.bookmark.goTo/3` |
| `ToggleBookmark4` | `navigate.bookmark.toggleMnemonic/4` |
| `GotoBookmark4` | `navigate.bookmark.goTo/4` |
| `ToggleBookmark5` | `navigate.bookmark.toggleMnemonic/5` |
| `GotoBookmark5` | `navigate.bookmark.goTo/5` |
| `ToggleBookmark6` | `navigate.bookmark.toggleMnemonic/6` |
| `GotoBookmark6` | `navigate.bookmark.goTo/6` |
| `ToggleBookmark7` | `navigate.bookmark.toggleMnemonic/7` |
| `GotoBookmark7` | `navigate.bookmark.goTo/7` |
| `ToggleBookmark8` | `navigate.bookmark.toggleMnemonic/8` |
| `GotoBookmark8` | `navigate.bookmark.goTo/8` |
| `ToggleBookmark9` | `navigate.bookmark.toggleMnemonic/9` |
| `GotoBookmark9` | `navigate.bookmark.goTo/9` |
| `JumpToLastChange` | `navigate.edit.last` |
| `JumpToNextChange` | `navigate.edit.next` |
| `QuickJavaDoc` | `editor.documentation.show` |
| `ParameterInfo` | `editor.parameterInfo.show` |
| `CodeCompletion` | `editor.completion.start` |
| `SmartTypeCompletion` | `editor.completion.smart` |
| `EditorCompleteStatement` | `editor.statement.complete` |
| `HippieCompletion` | `editor.completion.expandWord` |
| `HippieBackwardCompletion` | `editor.completion.expandWord/forward` |
| `EditorMatchBrace` | `editor.brackets.goToMatching` |
| `CollapseRegion` | `editor.fold.collapse` |
| `ExpandRegion` | `editor.fold.expand` |
| `CollapseAllRegions` | `editor.fold.collapseAll` |
| `ExpandAllRegions` | `editor.fold.expandAll` |
| `ExpandRegionRecursively` | `editor.fold.expandRecursively` |
| `CollapseRegionRecursively` | `editor.fold.collapseRecursively` |
| `ExpandToLevel1` | `editor.fold.expandToLevel/1` |
| `ExpandToLevel2` | `editor.fold.expandToLevel/2` |
| `ExpandToLevel3` | `editor.fold.expandToLevel/3` |
| `ExpandToLevel4` | `editor.fold.expandToLevel/4` |
| `ExpandToLevel5` | `editor.fold.expandToLevel/5` |
| `ExpandAllToLevel1` | `editor.fold.expandAllToLevel/1` |
| `ExpandAllToLevel2` | `editor.fold.expandAllToLevel/2` |
| `ExpandAllToLevel3` | `editor.fold.expandAllToLevel/3` |
| `ExpandAllToLevel4` | `editor.fold.expandAllToLevel/4` |
| `ExpandAllToLevel5` | `editor.fold.expandAllToLevel/5` |
| `ExpandDocComments` | `editor.fold.expandDocComments` |
| `CollapseDocComments` | `editor.fold.collapseDocComments` |
| `CollapseSelection` | `editor.fold.selection` |
| `GotoCustomRegion` | `view.customFolding.open` |
| `ReformatCode` | `editor.code.reformat` |
| `OptimizeImports` | `editor.imports.optimize` |
| `RenameElement` | `view.rename.open` |
| `SafeDelete` | `view.safeDelete.open` |
| `Move` | `view.project.move` |
| `ChangeSignature` | `view.changeSignature.open` |
| `MembersPullUp` | `view.pullMembersUp.open` |
| `MemberPushDown` | `view.pushMembersDown.open` |
| `EncapsulateFields` | `view.encapsulateFields.open` |
| `IntroduceParameterObject` | `view.extract.parameterObject` |
| `ExtractInterface` | `view.extract.interface` |
| `ExtractSuperclass` | `view.extract.superclass` |
| `ShowIntentionActions` | `view.intentions.open` |
| `Generate` | `view.generate.open` |
| `IntroduceVariable` | `view.extract.variable` |
| `IntroduceConstant` | `view.extract.constant` |
| `IntroduceField` | `view.extract.field` |
| `IntroduceParameter` | `view.extract.parameter` |
| `ExtractMethod` | `view.extract.method` |
| `Inline` | `view.inline.open` |
| `OverrideMethods` | `view.overrideMethods.open` |
| `ImplementMethods` | `view.implementMethods.open` |
| `Unwrap` | `view.unwrap.open` |
| `Run` | `run.process.start` |
| `RunClass` | `run.process.start/context` |
| `Stop` | `run.process.stop` |
| `ActivateRunToolWindow` | `view.toolWindow.run` |
| `ActivateServicesToolWindow` | `view.toolWindow.services` |
| `editRunConfigurations` | `view.runConfigurations.open` |
| `ChooseRunConfiguration` | `view.runConfigurations.choose` |
| `ChooseDebugConfiguration` | `view.runConfigurations.choose/debug` |
| `RunAnything` | `view.runAnything.open` |
| `CheckinProject` | `view.vcs.commit` |
| `Vcs.UpdateProject` | `view.vcs.update` |
| `Vcs.Push` | `view.vcs.push` |
| `ChangesView.NewChangeList` | `view.vcs.newChangelist` |
| `ChangesView.Move` | `view.vcs.moveToChangelist` |
| `ChangesView.Shelve` | `view.vcs.shelve` |
| `ChangesView.CreatePatch` | `view.vcs.createPatch` |
| `ChangesView.CreatePatchToClipboard` | `view.vcs.copyPatch` |
| `ChangesView.ApplyPatch` | `view.vcs.applyPatch` |
| `ChangesView.ApplyPatchFromClipboard` | `view.vcs.applyPatchFromClipboard` |
| `Annotate` | `vcs.blame.annotate` |
| `Git.Fetch` | `vcs.remote.fetch` |
| `Git.Branches` | `view.vcs.branches` |
| `Git.CreateNewBranch` | `view.vcs.newBranch` |
| `EditorLeft` | `editor.caret.move/left` |
| `EditorRight` | `editor.caret.move/right` |
| `EditorUp` | `editor.caret.move/up` |
| `EditorDown` | `editor.caret.move/down` |
| `EditorPreviousWord` | `editor.caret.move/wordLeft` |
| `EditorNextWord` | `editor.caret.move/wordRight` |
| `EditorLineStart` | `editor.caret.move/lineStart` |
| `EditorLineEnd` | `editor.caret.move/lineEnd` |
| `EditorTextStart` | `editor.caret.move/documentStart` |
| `EditorTextEnd` | `editor.caret.move/documentEnd` |
| `EditorPageUp` | `editor.caret.move/pageUp` |
| `EditorPageDown` | `editor.caret.move/pageDown` |
| `EditorLeftWithSelection` | `editor.caret.move/left+extend` |
| `EditorRightWithSelection` | `editor.caret.move/right+extend` |
| `EditorUpWithSelection` | `editor.caret.move/up+extend` |
| `EditorDownWithSelection` | `editor.caret.move/down+extend` |
| `EditorPreviousWordWithSelection` | `editor.caret.move/wordLeft+extend` |
| `EditorNextWordWithSelection` | `editor.caret.move/wordRight+extend` |
| `EditorLineStartWithSelection` | `editor.caret.move/lineStart+extend` |
| `EditorLineEndWithSelection` | `editor.caret.move/lineEnd+extend` |
| `EditorTextStartWithSelection` | `editor.caret.move/documentStart+extend` |
| `EditorTextEndWithSelection` | `editor.caret.move/documentEnd+extend` |
| `EditorPageUpWithSelection` | `editor.caret.move/pageUp+extend` |
| `EditorPageDownWithSelection` | `editor.caret.move/pageDown+extend` |
| `EditorBackSpace` | `editor.edit.backspace` |
| `EditorDeleteToWordStart` | `editor.edit.deleteWordBackward` |
| `EditorDelete` | `editor.edit.deleteForward` |
| `EditorDeleteToWordEnd` | `editor.edit.deleteWordForward` |
| `EditorEnter` | `editor.edit.newline` |
| `EditorTab` | `editor.edit.tab` |
| `InsertLiveTemplate` | `editor.templates.insert` |
| `SurroundWith` | `view.surround.open` |
| `SurroundWithLiveTemplate` | `view.surroundWithTemplate.open` |
| `EditorIndentSelection` | `editor.line.indent` |
| `EditorUnindentSelection` | `editor.line.outdent` |
| `$SelectAll` | `editor.selection.selectAll` |
| `EditorSelectWord` | `editor.selection.extend` |
| `EditorUnSelectWord` | `editor.selection.shrink` |
| `EditorEscape` | `editor.caret.keepPrimary` |
| `EditorCloneCaretAbove` | `editor.caret.cloneAbove` |
| `EditorCloneCaretBelow` | `editor.caret.cloneBelow` |
| `EditorToggleColumnMode` | `editor.selection.columnMode` |
| `EditorToggleUseSoftWraps` | `editor.softWrap.toggle` |
| `EditorIncreaseFontSize` | `editor.fontSize.increase` |
| `EditorDecreaseFontSize` | `editor.fontSize.decrease` |
| `EditorResetFontSize` | `editor.fontSize.reset` |
| `EditorIncreaseFontSizeGlobal` | `view.editorFontSize.increase` |
| `EditorDecreaseFontSizeGlobal` | `view.editorFontSize.decrease` |
| `EditorToggleShowWhitespaces` | `editor.whitespaces.toggle` |
| `EditorToggleShowIndentLines` | `editor.indentGuides.toggle` |
| `EditorToggleShowLineNumbers` | `editor.lineNumbers.toggle` |
| `EditorDuplicate` | `editor.line.duplicate` |
| `EditorDeleteLine` | `editor.line.delete` |
| `MoveLineUp` | `editor.line.moveUp` |
| `MoveLineDown` | `editor.line.moveDown` |
| `CommentByLineComment` | `editor.comment.line` |
| `CommentByBlockComment` | `editor.comment.block` |
| `SelectNextOccurrence` | `editor.selection.addNextOccurrence` |
| `UnselectPreviousOccurrence` | `editor.selection.unselectOccurrence` |
| `SelectAllOccurrences` | `editor.selection.selectAllOccurrences` |

<!-- keymap-actions:end -->

[check] `ZoomInIdeAction`, `ZoomOutIdeAction` and `ResetIdeScaleAction` are not yet confirmed by the
public documentation of the format or by an exported keymap file; confirm them with an exported
keymap that gives Zoom IDE shortcuts.

## Project dictionaries

The spelling check ([ADR-0161](adr/0161-spelling-quick-fixes-and-dictionaries.md)) reads every
`.idea/dictionaries/*.xml`: `<component name="ProjectDictionaryState">`, its
`<dictionary name="…">` elements and their `<words>` of `<w>` elements, whatever the dictionary's
name (older JetBrains IDEs name it after the user, newer ones `project`). A word saved at project
level goes to `.idea/dictionaries/project.xml` (`<dictionary name="project">`, created when it is
missing), in its sorted place; a removed word leaves every file that has it. Only the `<words>`
element of a dictionary is rewritten, so the declaration, comments, other elements and the
indentation stay as written, and a file that cannot be parsed is not touched. The files are read
again when they change on disk. `.idea/dictionaries` and `project.xml` must not be symbolic
links, and files over 4 MB are not read. The spelling check writes nothing else in `.idea`.

## Settings archives

A ZIP made by a JetBrains IDE's File → Manage IDE Settings → Export Settings… is read by Zen IDE's
File → Manage IDE Settings → Import Settings… (`settings.ide.inspect` and `settings.ide.import`,
[ADR-0157](adr/0157-export-import-and-restore-ide-settings.md)). It is told from our own archive by
the absence of `zen-settings.json` and by JetBrains entries (`keymaps/`, `colors/`, `options/`, or the
`IntelliJ IDEA Global Settings` marker). From it:

- `keymaps/<name>.xml` becomes a named keymap, through the reader of [Keymaps](#keymaps); the
  keymap's `source` is `<zip>/keymaps/<name>.xml`;
- `colors/<name>.icls` (or `.xml`) becomes a theme, through the colour-scheme reader of
  [themes](themes.md);
- every other entry (`options/*.xml`, `codestyles/`, `templates/`, `installed.txt`, the marker...)
  is listed as skipped, with the reason, and nothing is made of it.

The archive is checked before anything is read: over 64 MB, more than 4096 entries, an entry over
8 MB, ZIP64, or an entry name that is absolute or has `..` refuse it as a whole. Our tests build
such ZIPs themselves from this layout; no JetBrains export is used.
