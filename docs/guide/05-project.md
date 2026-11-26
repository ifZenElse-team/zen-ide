# The project tree

The Project tool window (**⌘1**) shows the project's files.

## The header's buttons

From left to right:

- **+** creates a file or a folder in the selected folder (or the selected file's folder), or at the
  project's root when nothing is selected.
- **Select Opened File** (the target) unfolds the folders down to the file open in the editor,
  selects it and scrolls to it. It is hidden while **Always Select Opened File** is on, since the
  tree then does it by itself.
- **Expand All** unfolds every folder, except hidden and excluded ones (such as a Cargo package's
  `target` and an npm package's `node_modules`).

The tree leaves out the files and folders GoGarden ignores: `.git`, `.DS_Store`, `__pycache__`,
`*.pyc` and the others of Settings → Editor → File Types → [Ignored Files and
Folders](09-settings.md#editor--file-types). A change there shows in the tree at once.
- **Collapse All** folds everything back to the project's root.
- **⋮** has **Behavior** → **Always Select Opened File**: with it on, the tree follows the editor's
  tabs. It is kept for each project and is on again when the project opens. **Excluded Files** (on by default): turn it off and excluded
  folders leave the tree, until you turn it on again; it is remembered for each project. **⋮** also has
  **Move To** and **Resize** ([Tool windows](22-tool-windows.md)).
- **—** hides the tool window.

## Selecting

As in the Finder: a click selects, **⌘-click** adds or removes, **⇧-click** selects a range, **⇧↑**
and **⇧↓** grow or shrink the selection, **⌘A** selects every row shown. The arrows move, **→** and
**←** unfold and fold folders, **Enter** opens a file.

## Changing files

The context menu (right click on a file or folder), with the shortcuts on the right:

- **New ›**: **File** and **Directory** (a name with `/` creates the folders on the way), the file
  templates that fit the folder and **EditorConfig File**, the same list as the **New** popup below.
- **Cut** (**⌘X**), **Copy** (**⌘C**), **Copy Path** (**⇧⌘C**) and **Paste** (**⌘V**), described
  below. The same keys work while the tree has the keyboard.
- **Rename…** (**⇧F6**) renames the file or folder in place; open tabs of it follow. A language
  server that asks to hear of renames updates what refers to the file first: the TypeScript server
  its imports, rust-analyzer the `mod` declaration, Java's server the class name. **⌘Z** in a file
  it changed undoes those changes and the rename. Moving files with **Cut** and **Paste**, creating
  and deleting them tell the servers in the same way. A server has three seconds to answer; when it
  does not, the change goes on without it and the IDE says so. When the server renames the class
  or module named after the file but leaves another file of the same kind that still names it, the
  IDE asks it again within those seconds, and if a file is still left, says which ones, so that you
  can check them.
- **Reformat Code** (**⌥⌘L**), for a file whose language server formats it, opens and formats it.
- **Delete…** (**⌫**) moves the selected files and folders to the Trash, after asking; files with
  unsaved changes are named in the question.
- **Open in Right Split** (**⇧↵**) opens a file in a split to the right of the editor.
- **Open In → Finder** shows the file selected in the Finder (Explorer on Windows, the file manager
  on Linux); **Open In → Terminal** opens a terminal tab in its folder.
- **Bookmarks →** **Toggle Bookmark** (**F3**), **Toggle Bookmark Mnemonic…** (**⌥F3**) and
  **Delete**, for the selected files and folders: see [Bookmarks of files and folders](#bookmarks-of-files-and-folders).
- **Local History →** Show History and Put Label ([Local history](18-local-history.md)).
- **Git →** (in a Git repository) **Show History** (the Log with only the commits that changed the
  folder or file), **Annotate with Git Blame**, **Show Diff**, **Rollback…**, **Add** and
  **Commit File…**, which opens the Commit tool window with only these files checked (see
  [Version control](14-version-control.md)).
- **Reload from Disk** reads the folder's listing and its open files from disk again.
- **Compare With…** (**⌘D**) compares a file with another one chosen in a dialog; with two or three
  files selected it is **Compare Files** and compares them at once (see [Comparing files](#comparing-files)).
  With two folders selected it is **Compare Directories** (see [Comparing folders](#comparing-folders)).
  **Compare File with Editor** compares a file with the file in the active editor tab, unsaved
  changes included.
- **Associate with File Type…** (one file), **Override File Type →** and **Revert File Type
  Override**: see [below](#associate-with-file-type-and-override-file-type).
- **Mark Directory As → Excluded** (on a folder) excludes it from the project; see
  [Excluded folders](#excluded-folders). On an excluded folder the same menu has **Cancel
  Exclusion**. **Mark Directory As → Resource Root** marks it as a resource root, and **Unmark as
  Resource Root** takes the mark off; see [Resource roots](#resource-roots).
- **Add to .gitignore** adds the files and folders to the `.gitignore` at the repository's root.

Items that do not apply are greyed out (Paste with nothing to paste, Reformat Code without a
formatter, Compare File with Editor with no file open).

### Copy, cut and paste

- **⌘C** puts the selected files and folders themselves on the clipboard, as the Finder does: the
  Finder (or another app) can paste them, and **⌘V** in the tree pastes copies into the selected
  folder (or the folder of the selected file). A copy whose name is taken is named as the Finder
  names it: `main copy.go`, then `main copy 2.go`. Nothing is ever overwritten. Symbolic links inside a
  copied folder stay links.
- **⌘X** then **⌘V** moves the files instead, once; open tabs of them follow.
- **⌘V** also pastes files copied in the Finder, and files dragged from the Finder onto the tree go
  into the folder under the pointer. With a screenshot or a copied picture on the clipboard, **⌘V**
  saves it as a PNG file, named in a dialog.
- **⇧⌘C** (Copy Path) puts the absolute paths on the clipboard as text, one per line.

AI agents do the same with `project.file.copy`, `project.file.cut`, `project.file.copyPath` and
`project.file.paste` (each with `clipboard: false` to leave the system clipboard alone, and paste
with `sources` to paste given files), `file.fs.rename`, `file.fs.reveal`, `file.compare.files`,
`file.compare.editor` and `vcs.ignore.add`; `view.project.select` selects rows and gives the tree the
keyboard, and `view.project.contextMenu` opens the menu on a row. `view.project.selectOpened` is
**Select Opened File**, and `view.project.alwaysSelectOpened` switches **Always Select Opened File**
(`{ "on": true }` or `false` sets it; `get_window_state` tells it in `projectView`).

Excluded folders are shown in orange with a muted name, resource roots with a purple folder, and test files (`*_test.go` and the tests
of other languages) on a green-tinted row.

## Excluded folders

An excluded folder stays in the tree but is left out of the project: Go to File, Search Everywhere
and Find in Files do not find its files, and **Expand All** leaves it folded. Exclude a folder of
generated code, logs or downloaded data with **Mark Directory As → Excluded** in its context menu;
the searches drop it at once, without opening the project again. **Mark Directory As → Cancel
Exclusion** brings it back just as quickly.

The mark is saved where JetBrains IDEs keep it, in the project's module file (`*.iml`), so
JetBrains IDEs see the same excluded folders, and the other way round. A project
without a module file gets `.idea/modules.xml` and `.idea/<project>.iml` the first time you exclude
a folder; the rest of an existing file is left exactly as it was.

No folder is excluded only because of its name. A Go module's `vendor` folder is
searched like any other (a module with vendoring finds its files there), and so are `build` and
`dist`. Some folders belong to a kind of project and are excluded wherever that project is: an npm
package's `node_modules` (beside its `package.json`) and, with the [Rust plug-in](06-rust.md), a
Cargo package's `target` (beside its `Cargo.toml`). Those show **Excluded** greyed in the menu.
What Git ignores (`.gitignore`) is not searched either: it stays in the tree, but Go to File,
Search Everywhere and Find in Files leave it out, also when a build writes thousands of files there.

A module file that is read-only, or a read-only `.idea` folder, is not changed: the command fails
and says why.

Agents use `project.directory.markExcluded` and `project.directory.cancelExclusion` with the
folder's `path`, and `view.project.excludedFiles` (with `show: true` or `false`, or none to switch)
for **Excluded Files**.

## Resource roots

A resource root is a folder of files the code refers to by paths relative to it: templates, static
web files, test data. **Mark Directory As → Resource Root** marks a folder so, and its folder icon
turns purple; **Unmark as Resource Root** takes the mark off. A folder has one mark at a time:
marking an excluded folder as a resource root cancels its exclusion, and the other way round. The
mark is saved in the module file, as JetBrains IDEs save it (a `<sourceFolder>` of type
`java-resource`), and the resource, source and test roots a JetBrains IDE marked are read and shown.

Agents use `project.directory.markResourceRoot` and `project.directory.unmarkResourceRoot` with
the folder's `path`.

## Project Structure

**Settings → Project Structure**, the first page of Settings (also **⌘;** and **File → Project
Structure…**), shows how the project is put together, in sections on its left:

- **Project**: the project's name, its **SDK** and **Language level**, and its **Compiler output**.
  In a Java or Kotlin project the SDK is the project's JDK, chosen from the JDKs found (**Default**
  is `JAVA_HOME`'s, else the newest), and the language level is a Java version or **SDK default**;
  both are saved with **Apply** or **OK**. When the build asks for a language level, the page says
  so: the build compiles with its own. In other projects the SDK is the one the language support
  found (Go, Rust), shown read-only.
- **Modules**: the modules the build files describe (`pom.xml`, Gradle projects, `go.mod`,
  `Cargo.toml` members), chosen in **Module:**, with the build file and its **Open build file**
  link and the module's language level. **Sources** is the folder tree below; **Paths** shows the
  output folders; **Dependencies** lists what the module declares, with its version and scope.
- **Libraries**: the libraries the project uses, where each is, and the modules that declare it.
- **Artifacts**: what **Build → Build Artifacts…** builds (see [Artifacts](#artifacts) below).
- **SDKs**: the JDKs on this computer and the SDKs of other languages the project uses;
  **Add, download or remove JDKs** opens Build, Execution, Deployment → JDKs.
- **Problems**: what is wrong with the setup (no JDK, a language level above the JDK, a build file
  that does not read, a library not downloaded), each with a link to its section and its file.

What comes from the build is read-only and marked **From pom.xml** (or the build file's name);
change it in the build file, which **Open build file** opens. Values that are the build tool's
defaults rather than read from the build, such as Gradle's output folders, say so.

**Modules → Sources** shows the project's content root and its folders as a tree, excluded folders
in orange, resource roots in purple, and the build's source roots in blue and test roots in green;
on the right, under the content root, the build's folders are listed under **From pom.xml**. Select
a folder and press **Excluded** or **Resource Root** after **Mark as:** to mark it; press the same button again to take the mark off. The right side
shows the content root in bold and under it **Excluded Folders** and **Resource Folders**; **×** on
a folder's row takes its mark off. A project has one content root: **Add Content Root** and the
**×** of the root are greyed out.

**Exclude files** holds name patterns separated by `;`: `*` stands for any number of characters and
`?` for one, for example `*.log;tmp?`. They are names, not paths: a file or folder whose name matches
is excluded wherever it is, a folder with everything in it. Nothing is written until **Apply** or
**OK**: then the marks and the patterns are saved in the module file (one `<excludePattern>` line
each) and Go to File, Search Everywhere and Find in Files follow at once.

Agents read the page with `project.structure.get` (the folder marks), `project.structure.project`,
`project.structure.modules`, `project.structure.libraries`, `project.structure.sdks` and
`project.structure.problems`, open a module's build file with `project.structure.openBuildFile`
(`module`, the root module's when absent), set **Exclude files** with
`project.structure.setExcludePatterns` (`patterns`, a list) and the JDK and language level with
`toolchain.jdk.choose` and `toolchain.jdk.languageLevel`; `view.projectStructure.open` opens the
page.

## Artifacts

An artifact is a jar the project builds. **Project Structure → Artifacts** lists the project's
artifacts; **Add** offers, for a Maven or Gradle project, the build's own packaging first
(`mvn package`, or `gradle jar`, `bootJar` when the build applies the Spring Boot plug-in), then a
**JAR from modules with dependencies**; a project without a build is offered the jar alone.
**Remove** takes the selected one off. Changes are saved with **Apply** or **OK** in
`.zen/artifacts.json`, so they can be shared through Git.

- A **JAR** takes the compiled classes and resources of the modules ticked under **Compiled output
  of the modules**, writes `META-INF/MANIFEST.MF` with the **Main class** (found in the sources
  when the jar is offered; leave it empty for a library jar), and puts the libraries the modules
  need at run time either **inside** the jar (extracted, so `java -jar` runs it alone; service files
  under `META-INF/services` are merged, signatures are left out) or **next to it**, copied into the
  same folder and named in the manifest's `Class-Path`. It is written to **Output directory**,
  `out/artifacts/<name>/<name>.jar` when empty.
- **The build's own packaging** runs the **Phase or task** (`package`, `jar`, `bootJar`, with
  arguments if you like, such as `package -DskipTests`) in the chosen module.

**Build → Build Artifacts…** lists the artifacts (the offered ones while the project keeps none) and
**All Artifacts** when there are several; each opens **Build**, **Rebuild**, **Clean** and
**Edit…**. Building runs in the Run tool window: a jar's modules compile through Maven
(`compile` with the runtime class path) or Gradle (`classes` with the runtime class path), or with
the project JDK's `javac` in a project without a build; then the IDE writes the jar itself, no `jar`
tool needed. **Rebuild** removes the old jar and compiles from clean; **Clean** removes the jar and
the libraries copied next to it at once (and runs the build's `clean` for the build's packaging).
**Edit…** opens this page on Artifacts.

To build artifacts each time a run configuration starts, add **Build Artifacts** to its **Before
launch** list and tick the artifacts in its settings
([Run and test](15-run-and-test.md#before-launch)): they build first, in the configuration's Run
tab, and a build that fails stops the launch. Agents use
`project.structure.artifacts`, `project.artifact.save` (`{ artifact, previous? }`),
`project.artifact.delete` (`{ name }`) and `build.artifacts.build`, `build.artifacts.rebuild` and
`build.artifacts.clean` (`{ names, waitMs }`, every listed artifact when `names` is empty);
`view.artifacts.build` opens the popup and `view.settings.open` with
`{ page: "projectStructure", section: "artifacts" }` the page.

## Bookmarks of files and folders

Files and folders can be bookmarked as lines are ([Bookmarks](03-navigation.md#bookmarks)), into the
same lists:

- **F3** while the tree has the keyboard, or **Bookmarks → Toggle Bookmark** in the context menu,
  bookmarks the selected files and folders, or removes their bookmarks when every one has one.
  With several rows selected it acts on all of them. The bookmarks of their lines stay.
- **⌥F3**, or **Bookmarks → Toggle Bookmark Mnemonic…**, gives the selected file or folder a digit or
  letter, with a description, in the same chooser as for a line. With several rows selected, ⌥F3
  acts on the last one selected and the menu on the row you right-clicked. **⌃** and a digit then opens a
  bookmarked file; a bookmarked folder is selected in the tree.
- **Bookmarks → Delete** removes the bookmarks of the selected files and folders.

A bookmarked file or folder has the bookmark icon after its name in the tree, with its digit or
letter when it has one. New bookmarks go to the default list. Agents use
`navigate.bookmark.toggleFiles` and `navigate.bookmark.removeFiles` with `{ paths }`, and the other
`navigate.bookmark.*` commands with `{ path, whole: true }`.

## Comparing files

Every comparison opens in the diff tab, an editor tab with the two sides, the changed lines and
words, **↑** and **↓** (or **⇧F7** and **F7**) for the previous and next difference, the
**Side-by-side** and **Unified** viewers, the whitespace modes and **Swap Sides** (see
[Diffs](14-version-control.md#diffs)).

- **Two files:** select them in the Project tool window and press **⌘D** (**Ctrl+D**), or choose
  **Compare Files** in their context menu.
- **Three files:** select three and press **⌘D**: the three-side viewer shows them side by side,
  the middle one compared with the left and the right, each change joined across the gutters.
- **One file:** **⌘D** (or **View → Compare With…**) asks for the other file in a dialog.
- **View → Compare with Clipboard** compares the text on the clipboard (left) with the file in the
  editor, or with its selection when there is one (right). The file's side is editable: type,
  paste, undo there, and **⌘S** saves the file as usual; changes to a selection stay in its place in
  the file, and the file keeps its line separators. Changes made to the file elsewhere while the
  diff is open (in the editor, or on disk) show on its side.
- **Open Blank Diff Window** (in Find Action, **⇧⌘A**) opens two empty panes: paste or type texts
  into them, or drop a file on a pane to show its text; they are compared again as soon as you
  pause typing.

AI agents compare with `file.compare.files` (two files), `file.compare.three` (`left`, `middle`,
`right`), `file.compare.clipboard` (a file or its selection; `text` instead of the clipboard),
`file.compare.texts` (two texts, as the blank window), each with `whitespace` (`none`, `trim`,
`ignore`), and `file.compare.writeSide` (typing into the file's side). In a window,
`view.project.compareWith` with `paths` compares two or three files, `view.compare.clipboard` and
`view.diff.blank` open the diff tab, and `view.diff.swapSides`, `view.diff.whitespace` (`mode`),
`view.diff.viewer` (`unified`), `view.diff.next`, `view.diff.previous` and `view.diff.setText`
(`side`: `left` or `right`, with `text`, or the `path` of a file dropped there) work on it; each
answers what the tab shows, and `get_window_state` has it as `diff`.

## Comparing folders

Select two folders in the Project tool window and press **⌘D** (**Ctrl+D**), or choose **Compare
Directories** in their context menu; with one folder selected, **⌘D** asks for the other one. The
folders open side by side in an editor tab, compared in the background, so the IDE stays responsive
on large trees.

- **Every file has a state:** only on the left, only on the right, different or equal. A folder that
  is on one side only is one row. Files with a different size are different; files with the same
  size and modification time are equal; any other pair is compared by contents. What the IDE
  ignores (`.git`, `.DS_Store` and the names of Settings → Editor → File Types → [Ignored Files and
  Folders](09-settings.md#editor--file-types)) is left out, and symbolic links are compared by where
  they point, never followed.
- **The toolbar** shows how many files are in each state; each state's button shows or hides its
  files (equal files are hidden at first). The filter field narrows the list by path: `*.go` matches
  names, `src/` any part of the path, `&` means and, `|` or, and `!` not, so
  `*.go & !*_test.go | README*` lists Go files that are not tests, and the READMEs. **Refresh**
  compares the folders again.
- **Each row has an action** between its sides: **→** copies the file to the right, **←** to the
  left, **×** deletes it from the side that has it; click it to change it, down to nothing. A file only
  on one side is copied to the other at first, a different file to the right.
- **Synchronize Selected** and **Synchronize All** (every row shown) apply the actions. When they
  would overwrite or delete files, a dialog lists them first; **Synchronize** (or **⏎**) goes on,
  **Cancel** (or **⎋**) does nothing. Copies keep the file's modification time; a folder copied over
  a folder is merged into it, keeping what the comparison leaves out there (a `.git` stays);
  deletions are permanent (not to the Trash), and nothing is written outside the two folders. The list is compared
  again afterwards.
- **Double-click** a file, or select it and press **⏎**, to open its diff in the diff tab (against
  nothing when it is on one side only); **↑** and **↓** move through the list. Coming back to the
  folders' tab compares them again, so edits made in the diff show.

AI agents compare with `file.compare.folders` (`left`, `right`; `states` and `filter` narrow the
entries) and synchronise with `file.compare.synchronize` (`items` of `{ relative, action }` with
`toRight`, `toLeft` or `delete`): without `confirm: true` nothing is done when an item would
overwrite or delete, and those items come back in `needsConfirmation`. In a window,
`view.folderDiff.compare` opens the tab, and `view.folderDiff.filter` (`states`, `text`),
`view.folderDiff.select` (`paths`), `view.folderDiff.setAction` (`path`, `action`),
`view.folderDiff.synchronize` (`all`, `confirm`), `view.folderDiff.open` (`path`) and
`view.folderDiff.refresh` work on it; each answers what the tab shows, and `get_window_state` has it
as `folderDiff`. Files hidden from agents (Settings → Tools → MCP Server) are left out of what agents
are told, counted in `hiddenFiles`, and an agent's synchronisation that would touch one is refused.

## Associate with File Type and Override File Type

What a file is (its type) decides how it is highlighted and indented, which language server opens
it and which icon it gets ([Editor → File Types](09-settings.md#editor--file-types)).

**A file GoGarden does not know.** A file no pattern names gets its type from its content when it
can: the interpreter its first line names (`#!/usr/bin/env bash`), or what it starts with (an XML
file's `<?xml`, so `timeweb-cloud.cyberduckprofile` is XML). Otherwise it opens as plain text when
its content is text (`LICENSE`, `notes.log`). When it is not
text (a picture GoGarden cannot show, a dump, an archive), opening it shows **Register New File Type
Association** instead:

- **File pattern**: `*.ext` (every file with that extension) or the file's own name;
- **Open matching files in GoGarden**: as **Text (with file type detection by content)**, which shows
  text files as text and other files read-only, or as the **File type** chosen in the list;
- or **Open matching files in associated application**: the application macOS opens the file with,
  as a double click in the Finder would.

**OK** gives the pattern to that choice and opens the file. The choice is kept for the whole IDE, in
Settings → Editor → File Types, where the patterns of the first and the last choice are listed under
the types **File type auto-detected by file content** and **Files opened in associated
applications**; remove the pattern there to undo it. The same dialog opens from **File → File
Properties → Associate with File Type…** (for the file in the editor, or the file selected in the
Project tool window while it has the keyboard) and from the tree's menu on one file.

**One file of another type.** **Override File Type →** in the tree's menu gives the selected files,
and only them, the type chosen in its list: overriding `notes.go` as **Text** leaves the other `.go`
files Go. **Text** is plain text: no highlighting, no language server and no problems from one, the
plain text icon and indentation. Open files change at once. **Revert File Type Override** gives the
selected files their type by name again. Overrides are kept for the whole IDE by the file's path
(in `overridefiletypes.json` in GoGarden's data folder), so they last across restarts and projects.
Renaming or moving a file in the Project tool window takes its override along, and deleting it there
drops the override; a file renamed outside GoGarden loses it.

A file opens in its application only when you open it yourself: reopening a project's tabs,
navigating back and Replace in Files never start an application, and an executable file (or a link to
one) is never started this way. A UTF-16 text file (with zero bytes
between its Latin letters) is text, not binary: it opens read-only, as GoGarden cannot write UTF-16
yet. Pipes and devices are not opened.

AI agents use `editor.fileTypes.associate`, `editor.fileTypes.override`,
`editor.fileTypes.revertOverride` and `editor.fileTypes.overrides`; `editor.fileTypes.typeOf` tells
whether a file's type is its own and whether it is highlighted and served by a language server
([Automation](../automation.md#associate-with-file-type-and-override-file-type)).

## New (⌘N)

**⌘N** in the Project tool window (or its **+** button) opens the **New** popup for the selected
folder (a file's folder when a file is selected): **File**, **Scratch File**
([Scratches](25-scratches.md)), **Directory**, then the file templates that fit the folder (your
own after the built-in ones) and **EditorConfig File**
([EditorConfig](09-settings.md#the-status-bar-new-files-and-export)). Type in **Search file type** to
narrow the list; **↑** and **↓** move, **Enter** or a click chooses, **Escape** closes.

In a source folder (`src/main/java`, `src/main/kotlin`, a folder marked as sources, and what is below
them) the list is for code: **Java Class** and **Kotlin Class/File**, whose name dialog chooses the
kind, **File**, **Package** (`com.acme` makes the nested folders), **package-info.java**,
**module-info.java** and your own templates.

**File → New** starts with **Project…**, then lists the items for the project folder: it does not
depend on what is selected, and it creates no classes.

A template that asks for something — the file name, or a variable without a value — opens a dialog
with a field for each; one that asks for nothing creates its file at once. The new file opens in
front; with **Enable Live Templates**, the caret goes to the template's `$END$`. A template made by a
language module or plug-in may be offered only in some folders: Go File, for example, only under a
folder with `go.mod`. Your own templates are offered everywhere.

## File and Code Templates settings

**Settings → Editor → File and Code Templates** edits the templates:

- **Scheme**: **Default**, the IDE's templates for every project, or **Project**, the project's own,
  kept in `.idea/fileTemplates` and shared through version control. Project shows the IDE's
  templates until you change one, and nothing is written to the project before that. Going back to
  Default deletes nothing: the project's templates stay in `.idea/fileTemplates` and come back when
  you choose Project again. The choice is your own, kept in the IDE's data folder; a project that has
  `.idea/fileTemplates` and was never switched uses Project.
- The tabs **Files** (templates of new files), **Includes** (parts other templates take in with
  `#parse`) and **Code** (pieces of code the IDE generates: they can be changed, not created).
- The toolbar: **Create Template**, **Create Child Template File** (one more file the template
  creates beside the main one, with its own extension and file name, such as `${NAME}_test`),
  **Remove Template**, **Copy Template** and **Revert to Original Template**.
- The selected template's **Name**, **Extension**, **File name**, its text, **Reformat according to
  style** and **Enable Live Templates**, and under them the **Description**: every variable with
  what it holds.

Built-in templates (from language modules and plug-ins) cannot be removed or renamed; a changed one
is shown in blue and **Revert to Original Template** takes it back. Nothing is saved until **OK**
or **Apply**.

## File templates

New files can start from a **file template**: a text with variables, written in a subset of the
Velocity Template Language.

- **New** (**⌘N**, above) and **New File from Template…** (Find Action, **⇧⌘A**) ask for the
  template, the file name and every
  variable the template uses without a value, then creates the file in the folder selected in the
  Project tool window (or the folder of the file in front) and opens it.
- **File → Save File as Template…** makes a template of the file in front: its name, extension and
  text, with the file name pattern, **Reformat according to style** and **Enable Live Templates**
  to choose.

In a template, `${NAME}` (or `$NAME`) is replaced by the variable's value; `#set($A = "…")` gives a
variable a value, `#if($A == "x") … #elseif(…) … #else … #end` chooses a part,
`#parse("File Header.go")` takes in an include, `##` starts a comment and `${DS}` is a dollar sign.
`#[[…]]#` keeps its text as it is. Whatever else Velocity has (`#foreach`, methods such as
`${NAME.toUpperCase()}`) is reported with its line and column, and no file is created until it is
fixed.

The predefined variables are `NAME` (the name typed, without the extension), `FILE_NAME`,
`DIR_PATH` (the folder, relative to the project), `PROJECT_NAME`, `PRODUCT_NAME`, `USER`, `DATE`
(`2026-10-22`), `TIME` (`14:05`), `YEAR`, `MONTH`, `MONTH_NAME_SHORT`, `MONTH_NAME_FULL`, `DAY`,
`DAY_NAME_SHORT`, `DAY_NAME_FULL`, `HOUR`, `MINUTE` and `SECOND`; language modules and plug-ins may
add their own. Any other variable is asked for in the dialog, also when it is in the template's file
name (for example `${NAME}_${KIND}`). A value is written as it is: a `$` or `#` in it is never
read as Velocity. The file name may lead to a folder under the chosen one (`docs/${NAME}`), never
out of it: a name that gives `..` or an absolute path is refused.

With **Reformat according to style** the new file gets the indentation of its code style and is
reformatted when its language server runs. With **Enable Live Templates**, `#[[$VALUE$]]#` and
`#[[$END$]]#` in the template become places to fill in with **Tab** in the new file, as in a live
template.

Your templates are kept for the IDE, or, when the project has a `.idea/fileTemplates` folder, for
the project (the Project scheme, shared through version control) in the layout JetBrains IDEs use:
`.idea/fileTemplates/<name>.<extension>.ft`, includes in `includes/`, and the options in
`.idea/file.template.settings.xml`. The project's templates go over the IDE's. Saving a template
changes only that template's file and its entry in the settings file; the rest of the file stays as
it was. A project's templates come with the project, so they are kept in check: symbolic links and
files over 1 MiB are not read, and a template that nests `#if` or includes too deeply, or whose text
grows past 1 MiB, is reported instead of hanging the IDE.

## New Project

**File → New → Project…** and **New Project** on the welcome screen open the New Project wizard. The
project types are on the left, with a search above them: **New Project** (the languages, then
**Empty Project**) and **Generators**. **↑** and **↓** move through them, and **More via plugins…**
opens the Marketplace over the wizard. The fields are on the right:

- **Name** and **Location**: Location is the folder the project goes in, and the line under it says
  **Project will be created in** that folder and Name. Location starts in **Default project directory**
  ([System Settings](09-settings.md#appearance--behavior)), `~/Projects` until you set one, and `~` stands
  for your home folder.
- **Create Git repository** initialises a repository in the new folder.
- **The type's own fields**, such as those of the **Go** type ([New Go modules](06-go.md#new-go-modules)).
  The Go type comes with the Go plug-in; an IDE with that plug-in off does not list it.
  The [**Java**](06-java.md#new-java-projects) and [**Kotlin**](06-kotlin.md#new-kotlin-projects)
  types come with their plug-ins, in every IDE.

**Create** stays disabled while the location cannot hold a project, a required field is empty or the
type cannot use what it was given (a folder that is not a Go SDK); the line above the buttons says why. If the folder already holds something, the wizard asks first:
existing files stay as they are, and the generator's files of the same name are not written.

If creating fails, the wizard shows the error and nothing is left behind: nothing is opened, and an
existing folder stays as it was. The new project opens according to **Open project in**, trusted, so
that its build is imported at once. A folder that already held something is not trusted by this: the
[Trust dialog](#trust-and-safe-mode) comes first.

Plug-ins add types under **Generators**, in plug-in order. A generator may load its fields from the
network: the wizard shows the address and asks only after **Connect**, shows a line while they load, and shows the error
with **Retry** if they do not arrive. A generator with more fields than one page has **Next** in place
of **Create**, and **Previous** goes back.

The **Spring Boot** generator comes with the [Spring Boot plug-in](31-spring-boot.md#new-spring-boot-project).

Agents create projects with the `project.new.*` commands, also with no project open
([AI agents](11-ai-agents.md)). A project an agent creates is not trusted.

## Project from Version Control

**File → New → Project from Version Control…** and **Clone Repository** on the welcome screen open
**Clone Repository**. Enter the **URL**, or choose one you cloned before; **Directory** follows the
repository's name under the default project directory until you change it. **Shallow clone with a
history truncated to N commits** keeps only the last commits. **Clone** stays disabled without a
URL, or when the directory holds something.

The IDE connects to that address only when you press **Clone**. The clone shows how far it is and
leaves submodules until you trust the project: then **Initialise Submodule** in the
[Git tool window](14-version-control.md#submodules) fetches them; **Cancel**, or a failure, leaves nothing behind. It then asks whether to
open the project. A cloned project is **not trusted** until you trust it, so the
[Trust dialog](#trust-and-safe-mode) comes before it opens, and an agent's clone stays in safe mode.
Hosting accounts are not supported, since the IDE has no accounts: use the repository's URL
with the credentials Git already has. Addresses are remembered without passwords.

## Project from Existing Sources

**File → New → Project from Existing Sources…** takes a folder, or a file in it such as `pom.xml` or
`build.gradle`. The IDE says whether it is a Maven, a Gradle or a plain project and opens the folder:
the build is read as it is whenever a project opens, so there are no further steps. A project opened
this way is not trusted until you trust it: the [Trust dialog](#trust-and-safe-mode) comes first.

## Trust and safe mode

A project can run code on your computer: its language servers, go commands, build scripts and run
configurations. So before a project you have not trusted opens, the IDE asks **Trust and Open
Project '<name>'?**:

- **Trust Project** opens it with everything on. **Trust all projects in '<parent>'** adds the parent
  folder to [Trusted Locations](09-settings.md#appearance--behavior) instead, and warns when that
  folder holds your home folder.
- **Preview in Safe Mode** opens it in safe mode, for this time: the next open asks again.
- **Don't Open** leaves it closed.

In safe mode files open and can be edited, and search and the terminal work. **Git is off**: the IDE
makes no Git call, because a repository can run its own filters and hooks, and the Git tool windows
say so. Language servers, go.mod checks, `cargo metadata`, toolchain and module downloads, and formatters from the project do
not start, and **Run** and **Debug** say why instead of starting, for the code at the caret too. So do
a Maven, Gradle or Ant **Reload**, goal, task or target, a Run Anything command that runs a build, and a
plug-in's project setup, since they run the project's own code. External tools and database migrations
are yours to start. A
[scratch file](02-editor.md) still runs, in its own folder: it is your code, not the project's.
An editor banner says **Project is opened in safe mode**; its **Trust project…** (also in Find
Action, ⇧⌘A) turns trust on and starts all of it, Git too, without reopening the project.

Trust is kept in the IDE's data folder by the project's real path, never in the project, so a
repository cannot trust itself and a moved or renamed project asks again. AI agents cannot trust a
project: they cannot press these buttons, an agent's `open_project` opens in safe mode, and an IDE
started with `--headless` never asks. To let agents use language servers in a project, trust it, or
a folder above it, once in a window. What else waits for trust in a Java project: [Trusted
projects](26-maven-and-gradle.md#trusted-projects).

## Projects opened through a link

A project folder opened through a symbolic link (such as `/tmp/app`, where `/tmp` links to
`/private/tmp`) is the same project as its real folder: opening it either way shows the one window,
and the tree, the navigation path in the status bar, **Select Opened File**, version control
colours, language servers and changes made by other programs all treat its files alike. The tree's
first row shows the path you opened it by.

## New Module and Module from Existing Sources

**File → New → Module…** opens the wizard for a module of the open project. It looks like New
Project, with the search, the groups **New Module** and **Generators**, and **More via plugins…**. The
types are those of New Project, without Empty Project, and there is no **Create Git repository**, as
the module belongs to the project's repository. **Location** starts at the project's root, and the
line under it says where the module goes; **JDK** starts on **Project JDK**, and the build system on
the project's.

- In a Maven project, the parent's `pom.xml` lists the module in `<modules>`, and its packaging
  becomes `pom`, as Maven asks of a parent.
- In a Gradle build, `settings.gradle(.kts)` gets `include`; a build without that file gets one.
- In a project without a build, or for a module of another build system than the parent's, the
  module is a folder of the project.

The project then loads again. If the module cannot be created or listed, nothing is left behind.
While the build file has unsaved changes, the IDE asks you to save it first.

**File → New → Module from Existing Sources…** takes a folder, or a build file in it, and lists it in
the project's build in the same way. A folder that is listed already changes nothing. A project
without a build has nothing to list it in.

## Modules

The folders of the project's modules other than the project itself are drawn in bold: the members
of a Cargo workspace, the modules a `go.work` uses. When a module's name differs from its folder's,
it follows in grey: `core [zen-core]`.

## External Libraries

Below the project, **External Libraries** shows what the project's code uses besides its own files:

- **Go SDK 1.x.y**, the Go SDK chosen in Settings → Go, whose folders and files can be browsed and
  opened like the project's.
- **Go Modules** followed by the module's name, with one row per dependency of `go.mod` and `go.work`,
  as `path@version`, browsing its folder in the module cache (`GOMODCACHE`, by default
  `~/go/pkg/mod`). A module required at several versions shows the highest one, as Go chooses; a
  module replaced by a folder (`replace ... => ../fork`) browses that folder. A dependency that is not downloaded yet is greyed out, says "Not downloaded" when
  the pointer is over it, and cannot be unfolded; it appears as soon as `go mod download` or a build
  fetches it.
- In a Cargo project (with the [Rust plug-in](06-rust.md)): **Rust SDK 1.x.y**, the standard
  library's sources of the project's toolchain, and **Crates** (with nothing after it), one row per
  crate the workspace uses, as `name@version`, browsing its sources in Cargo's registry (`~/.cargo/registry/src`) or its
  folder for a path dependency outside the project. Only the crates built for this computer are
  listed, as `cargo metadata` resolves them, without going to the network.
- In a Maven or Gradle project: the JDK, as `< 21.0.5 >`, with its sources, and one row per library
  of the build, as `Maven: group:artifact:version` or `Gradle: group:artifact:version`, with the
  contents of its jar. A class opens with its sources when the library has them, otherwise as a
  read-only decompiled view; see [Libraries and their sources](26-maven-and-gradle.md#libraries-and-their-sources).
- In a project with a `package.json`: **node_modules**, one row per dependency that file declares, as
  `name@version`, the version installed there, browsing the package's folder. A dependency that is not
  installed yet shows the range the file asks for, greyed out, until `npm install` or your package
  manager's install brings it. The packages they depend on in turn are not listed; they are in the
  `node_modules` folder itself, with **Show Excluded Files**.

Nothing can be created, pasted, dropped or deleted under External Libraries; **Copy Path** works.
**Select Opened File** also finds a file of a library, such as one a jump to a declaration opened.
The list follows changes to `go.mod` and `go.work` (`Cargo.toml`, `Cargo.lock` and
`rust-toolchain.toml` in a Cargo project; `pom.xml` and the Gradle build scripts) as they are saved, and which of its folders are
unfolded is remembered with the project. Agents read the same list with `project.libraries.list`,
the modules in a few words and the problems found in their files with `project.module.info`
(`module` gives one in full, `all` every one and the whole workspace file), and reread the files
with `project.model.refresh`.

## Scratches and Consoles

After External Libraries, **Scratches and Consoles** shows your scratch files and buffers, which
are kept in the IDE's data folder and shared by every project; see
[Scratch files and buffers](25-scratches.md). Advanced Settings → Project View → Show Scratches
and Consoles in the Project tool window shows or hides it.

## Projects shared with JetBrains IDEs

A project that is also opened in a JetBrains IDE keeps working there: GoGarden reads the project's
`.idea/` folder, the `.run/` folder and the `*.iml` module files, and never changes them on its own:
only **Mark Directory As** and **Settings → Project Structure** write a module file, when you ask them to.
A rename whose changes would reach one of these files, or a Replace in Files that matches in one,
stops with a message and changes nothing. You can still open such a file and save your own changes.
Agents read what was found, and the files that could not be read, with `project.settings.get`.

When the project keeps its own code style (Settings → Editor → Code Style → Scheme: Project in the
JetBrains IDE) and the IDE's Code Style Scheme is **Project**, as it is unless you choose **Default
(IDE)** in Settings → Editor → Code Style or Quick Switch Scheme (⌃`), its tabs and indents win over
your own Editor → Code Style (and the project's `.editorconfig` files win over both, see
[EditorConfig](09-settings.md#editorconfig)): **Enter**, **Tab**,
**⇧Tab**, the width of a tab, the status bar and Reformat Code all use them; what the project does not
set stays yours. A file whose own lines are indented another way is still edited that way while
**Follow the indentation a file already has** is on; Reformat Code keeps the project's style. Plain text files (`.txt`) take its Plain text style, then its Other File Types
style, as do files of a type that has no style of its own. A file the project's encodings put in another encoding than UTF-8 (for example
windows-1251) opens editable and is saved in that encoding ([File encodings](09-settings.md#file-encodings)).
A text file that is not valid text in its encoding opens read-only with a message naming the
encoding, and is never saved, so its bytes stay as they are; the status bar shows its encoding and
a closed lock ([Read-only files](02-editor.md#read-only-files)); choose the right encoding in the
status bar to edit it. A binary file is not opened at all.
Folders the JetBrains IDE excludes (Mark Directory As → Excluded, saved in the `*.iml` files) and its
excluded name patterns (such as `*.log`) are excluded in GoGarden too (see
[Excluded folders](#excluded-folders)). When the JetBrains IDE changes them, GoGarden follows.
The shared Go run configurations of `.idea/runConfigurations/` and `.run/` (Go Application and Go
Test) appear in the run widget and the Run/Debug Configurations dialog and run as they are, but stay
read-only; see [Run and test](15-run-and-test.md#run-configurations).
