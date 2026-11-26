# Find and replace

In the open file, and in the whole project.

## In the file

**⌘F** opens the search bar above the editor; a selection on one line becomes the query (with
Regex on, its special characters are escaped so that it finds itself). A caret alone does not take
the word under it. A selection of several lines turns on **In Selection** instead. **⌘R** adds the
replace row, and so does the arrow at the bar's left end.

- **Match Case**, **Words** and **Regex** refine the search; the bar shows how many matches there
  are and which one is current.
- **Enter** and **⇧Enter** (or **⌘G** and **⇧⌘G**) go to the next and previous match; **⌥Enter**
  selects them all, each with a caret.
- **Replace**, **Replace All** and **Exclude**: Exclude skips the current match, so Replace All
  leaves it alone (it is struck through); on a skipped match the button reads **Include** and takes
  it back.
- With Regex, `$1` in the replacement is the first group, `\U…\E` and `\L…\E` put what is between
  them in capitals or small letters, `\u` and `\l` the next letter: `get_(\w+)` with `Get\u$1`
  turns `get_name` into `GetName`. Each match shows what it becomes right after it, before you
  replace.
- **Aa** in the replace field is **Preserve Case**: replacing `fooBar` with `bazQux` turns `FooBar`
  into `BazQux` and `FOOBAR` into `BAZQUX`. It searches without regard to case.
- Matches are highlighted in the text and marked by the scroll bar. **Escape** closes the bar.
- **⌥↓** in the search or replace field, or the lens at its left, lists the recent queries or
  replacements; choose one to search with it again. The list is shared with Find in Files and kept
  across restarts.
- **New Line** (the bent arrow in the field, or **⇧⌘⏎**) adds a line break: the field grows to show
  several lines and the search finds text that spans them.
- **In Selection** (**⌃⌥E**, or the first item of the Filter) searches only the text selected when
  you turn it on; closing the bar ends it.
- The **⋮** menu: **Open Results in Find Window** (**⌥F7** in the search field) lists every match in
  a tab "Occurrences of '…'" of the Find tool window, grouped like the tabs of Find in Files; the **Multiple Cursors** actions; and **Scroll to
  Results During Typing**, which you can turn off to keep the caret where it is while you type.
- **⌃G** adds the next occurrence of the selection with a caret of its own, **⌃⇧G** takes back the
  one added last, and **⌃⌘G** selects them all. **Edit → Find → Next / Previous Occurrence of the
  Word at Caret** jump to the word under the caret elsewhere in the file.
- The **Filter** (the funnel) narrows the search by where a match starts: **Anywhere**, **In
  Comments**, **In String Literals**, **Except Comments**, **Except String Literals** or **Except
  Comments and String Literals**. Comments and strings are those the editor colours as such; a file
  without highlighting has neither. The current filter is ticked, and the funnel is lit while it is
  not Anywhere.

## In the project

**⇧⌘F** opens **Find in Files**, **⇧⌘R** **Replace in Files**. The selected text becomes the query
(with Regex on, its special characters are escaped so that it finds itself); without a selection
the last query comes back selected, so typing replaces it. Drag an edge or a
corner of the window to resize it; it opens at that size again until GoGarden quits.

- The window does not block the IDE. **Pin Window** (the pin at the top right) keeps it open when
  you click the editor: you can edit the file, then click back in the window and go on with the same
  results. Without the pin, a click outside closes it. While the editor has the keyboard, the arrows
  and Escape are the editor's.
- The header shows how many matches there are in how many files ("10 matches in 3 files"). The
  window lists at most 100 matches; past that it says "100+ matches in 29+ files". Before you type,
  it reads "Type search query to find in files".
- The search starts a moment after you stop typing, not on every key.
- The query field: the lens on the left (or **⌥↓**) lists the recent queries, shared with the search
  bar and kept across restarts; **×** clears the field; **New Line** (**⇧⌘⏎**) adds a line break, so
  the search finds text over several lines; **Match Case** (**⌃⌥C**), **Words** (**⌃⌥W**) and
  **Regex** (**⌃⌥X**) refine it.
- **File mask** in the header: tick it and type the masks, `*.go` searches Go files, `*.go,
  !*_test.go` leaves the tests out. **▾** lists the masks used before, here and on the Text tab of
  Search Everywhere ([Go to Text](03-navigation.md#go-to-text)), also after a restart.
- The same **Filter** as in the file, next to the mask; Replace All keeps to it as well.
- The scope row says where to search; the choice is remembered with the project. **⌃⌥P**, **⌃⌥M**,
  **⌃⌥D** and **⌃⌥S** choose In **P**roject, **M**odule, **D**irectory and **S**cope (the
  underlined letters).
  - **In Project** searches everything but [excluded folders](05-project.md#excluded-folders) (also
    those of a project shared with a JetBrains IDE); a Go module's `vendor` is searched.
  - **Module** searches one module of the project, chosen in its list.
  - **Directory** searches a folder: typed, chosen with **…**, or picked from the recent ones (▾);
    it starts as the folder selected in the project tree. **Search recursively in subdirectories**
    (**⌃⌥Y**) takes in the folders below it.
  - **Scope** searches a scope from the list: All Places (with the libraries and the scratches),
    Project Files, Project and Libraries, Project Files Excluding Git-ignored, Project Files Tracked
    by Git, Scratches and Consoles, Recently Viewed Files, Recently Changed Files, Module '…', Current
    File, All Changed Files, Project Production Files, Project Test Files (`_test.go` and the like),
    and the project's named scopes.
- Each matching line is one row, with every match in it highlighted and its file and line number on
  the right; a file name that several folders hold shows its folder (`kucoin/client.go`). Rows from
  test files are tinted as test files are in the Project tool window. **Enter** or a double click
  opens the match.
- Below the list, the **preview** shows the selected line in its file, coloured as in the editor,
  with "file name · folder" above it. You can edit there: the change goes into the file a moment
  after you stop typing (into the editor when the file is open, with Undo; otherwise on disk, with a
  Local History version from before). If the file changed in the meantime, or is read-only, the
  edit is not written and the preview shows the file as it is now.
- **⌘⏎ Open in Find Window** (the button at the bottom) lists every match in the Find tool window as
  "Occurrences of '…'" (see [The Find tool window](#the-find-tool-window)); tick **Open results in
  new tab** to keep the previous results on their tab. The search goes on in the background:
  the status bar reads "Searching for '…' in <file>…" with a progress bar and **×**, which stops it.
  Past 1000 matches it pauses and asks **Too Many Results**: **Continue** lists them all, **Abort**
  keeps those found.
- Both limits, 100 in the window and 1000 before Too Many Results, are in **Settings → Advanced
  Settings → Find in Files**.
- Open files are searched, and previewed, with their unsaved changes. **Replace All** changes open files in the
  editor (one undo step per file) and the others on disk.
- The search reads only the files that can match: when a project opens, the IDE indexes the text
  of its files in the background (**Indexing…** in the status bar, with how many files
  it has read). You can search at once; until the index is built the files it has not read yet are
  searched in full, with the same results. Quitting does not wait for it. Binary files, files over
  2.5 MB, excluded folders and ignored files and folders (Settings → Editor → File Types →
  [Ignored Files and Folders](09-settings.md#editor--file-types)) are neither indexed nor searched;
  changing the ignored names indexes the project again in the background.
- The replacement works as in the file: groups, `\U…\E`, `\L…\E`, `\u`, `\l`, and **Aa**
  (Preserve Case) next to the replace field.
- Replacing never half-changes the project: a read-only file, a file not in UTF-8, or a file that
  changed since the search stops the whole replacement before anything is written, with a message
  naming the file; a file that cannot be written midway (a full disk, a locked folder) puts back the
  files written before it. Files that are not open are written with a Local History version from
  before, and their CRLF line breaks stay CRLF.

## The Find tool window

**Open in Find Window** of Find in Files and Replace in Files opens a tab "Occurrences of '…'" in
the Find tool window (**⌘3**). New results take the place of the last tab that is not pinned,
unless **Open results in new tab** is on. A tab whose title another tab already has gets a number:
**Usages of orders (2)**. [Find Usages](06-go.md#find-usages) (**⌥F7**) and the
search bar's **Open Results in Find Window** open tabs of the same kind, with the same tree, Group
By and toolbar.

- The tree: **Targets** (what was searched for), then **Found occurrences in Project** (or the
  folder, module or scope searched) with how many results, growing while the search runs. Below it
  the usages are grouped by usage type (**Unclassified**, **Usage in comments**, **Usage in string
  constants**), module, folders (a chain of single folders is one node: `internal/bootstrap`), file
  and line, with the match in bold. Rows of test files are tinted.
- **Group By** (on the toolbar at the left) turns the groups on and off: **Directory Structure**
  ⌃D, **File Structure** ⌃S (the functions and types holding the usages), **Module** ⌃M,
  **Flatten Modules** ⌃O, **Group by Directory** ⌃P (one node per folder, with its whole path),
  **Production/Test**, **Usage Type** ⌃T, **Merge Usages from the Same Line** ⌃L. Directory
  Structure and Group by Directory exclude each other. The choice is kept for every tab.
- The toolbar also has **Rerun** ⌘R, **Previous** and **Next Occurrence** (⌥⌘↑, ⌥⌘↓, which go to
  the usage in the editor), **Settings…** ⇧⌘F (opens Find in Files again with the tab's query,
  options, mask and scope), **Show Usages In Comments**, **Expand All** ⌘+, **Collapse All** ⌘−,
  **Preview Source** (the source of the selected usage on the right), **Pin Tab** and **Open
  results in new tab**. When the tool window is too short for all of them, the rest are under **⋮**.
  The shortcuts work while the tree has the keyboard.
- **⌫** (or **Exclude** in a row's context menu) excludes a node and everything under it: it stays
  listed, struck through, and is left out of replacing. ⌫ on an excluded node, or **Include**,
  takes it back.
- A tab of Replace in Files has **Replace Selected** (⌥L, the selected node) and **Do Replace All**
  (⌥D) at the bottom; both skip excluded nodes. Each open file changes in its editor as one undo
  step; other files are written on disk. The replaced usages leave the tree, and the others of the
  same file stay where they are, so they can be replaced one by one. Both buttons wait for the
  search to end.
- Enter, a double click or a click on a usage opens it; ←/→ collapse and expand; Escape goes back
  to the editor.

## Scopes

The named scopes of a project shared with a JetBrains IDE (`.idea/scopes`) are offered as they are.
**…** next to the Scope list opens **Scopes**: add (**+**), change or delete (**−**) the project's
own named scopes, kept in `.zen/scopes.json`. A pattern says which files a scope holds, and the
window shows how many files it matches or why it cannot be used:

- `file:*.go` — files by name; `file:internal/*` — the files of a folder; `file:internal//*` — and
  of every folder below it; `file[example.com/tools]:*.go` — inside a module.
- `&&` (and), `||` (or), `!` (not) and parentheses combine them:
  `file:internal//*&&!file:*_test.go`.
- `$Tests` stands for another named scope.

A scope from `.idea/scopes` is read-only here: saving it keeps a copy of your own with the same
name, which then hides it.

Search Everywhere searches in the same scopes: the Classes, Files and Symbols tabs have the list, and
the All tab **Include non-project items** (All Places when checked, Project Files when not).
