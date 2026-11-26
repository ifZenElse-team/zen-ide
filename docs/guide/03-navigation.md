# Finding your way

Search Everywhere, Find Action, Go to Type (Go to Class), Go to File, Go to Symbol, Go to Text, the
Switcher, Recent Files, Recent Locations, Go to Line, Back and Forward, Last Edit Location, the
Structure tool window, File Structure, the navigation bar, and the main menu.

What you type in a popup that filters as you type (Search Everywhere, Find Action, Go to Class,
File and Symbol, Recent Files, Recent Locations, File Structure, Paste from History, the projects
popup, Show Usages, the navigation bar's popups) stays in the popup: it never reaches the file under it, also when you type at
once after closing one popup and opening another. The popup's list grows
when more results come in, as the language server's do, up to its full height, and **↑** and **↓**
keep the selected row in view.

Typed with the wrong keyboard layout on, a query still finds what it means in Search Everywhere and
its tabs (Go to File, Class, Symbol and Action): `ьфшт` typed on the Russian layout finds `main`,
and `pfvtnrb` typed on the US layout finds `заметки`, after what the text finds as typed. A query
you paste is taken as it is.

## Search Everywhere

Press **⇧ twice** (or the magnifier in the toolbar). One popup searches types, files, symbols,
actions and text; its tabs are **All**, **Types** (**Classes** in an IDE whose language calls them
so), **Files**, **Symbols**, **Actions** and **Text**, and **Tab** and **⇧Tab** switch between them.
Type part of a name, or its camel humps (`mgo` finds `main.go`); **Enter** opens the file, goes to
the declaration, selects the folder in the Project tool window or runs the action, and the key stays
in the popup: the file it opens is not changed. **All** shows the first few of each; a group with
more ends with **more…**, and **Enter** on it (or a click) lists the next 50 of that group in place,
the selection on the first of them (**Enter** pressed again before they show opens that one). A tab of its own ends with **more…** too when it has more than
50. **⌘↓** and **⌘↑** jump to the end and the start of the list. The letters that matched are
marked in every row, on every tab. With nothing typed, **All** lists the files you opened lately
(the current one left out) and the other tabs show only the field.

Every tab, and Recent Files, finds names the same way:

- Case does not matter, but a capital letter starts a new word: `NPE`, `nPE` and `nullpoex` all
  find `NullPointerException`.
- `*` stands for any characters: `i*re` finds `ItemStore`.
- A space starts a new word (`dup lin` finds **Duplicate Line**); a space at the end means the name
  ends there: `item ` finds `Item` but not `ItemStore`.
- The best matches come first: the exact name, then names that start with what you typed, then
  camel humps, then a part inside a word. Within each of these your project's come before those of
  the Go SDK and the libraries, then the closer matches, then the ones you used lately, then the
  shorter names: `Cart` lists your `CartStore` before the SDK's `CartPool`, and `i*re` your
  `ItemStore` before the SDK's `IP_RECVIF`, but an SDK type named exactly `Cart` before both.

While you type, the list keeps the last results until the new ones are ready, so that it does not
jump: until Classes, Files and Actions have enough results, or for two seconds at most (Settings →
Advanced Settings can make it wait for every search instead). **Include non-project items**, at the
right of the tabs (or **⇧ twice** again in the open popup), adds the types and symbols of the Go
SDK and the libraries; without it you see your project's only. On Classes, Files and Symbols a list
of search scopes takes its place, the same as in Find in Files ([Scopes](04-find-and-replace.md#scopes)):
**All Places** there is Include non-project items on All, and the choice is remembered with the
project.
**Enter** pressed before the results of what you typed are shown waits for them, five seconds at
most, and opens the first; it never opens a result of an earlier query, and does nothing if the
query finds nothing or its results take longer.

Drag an edge or a corner of the popup to make it wider or taller; it opens at that size again
until GoGarden quits, and never grows past the window.

Type **/** to see the commands, as the field says: `/` lists the groups of Settings (`/appearance`,
`/keymap`, `/editor`, `/plugins`, `/vcs`, `/build`, `/languages`, `/tools`, `/advanced`), typing on
narrows them (`/ke`), and **Enter** opens the group chosen in Settings. `/plugins` also lists the
plug-ins, built-in and installed, each with a switch: **Enter** or a click turns it on or off and
the popup stays open, and words after the command filter them (`/plugins docker`); an **Enter**
pressed before the list came turns nothing on or off. A built-in plug-in switched this way says
**Restart required** until GoGarden restarts.

Type an arithmetic expression on **All**, such as `2 + 3 * 4`, `(1.5 + 2) ^ 2` or `17 % 5`, and the
first row, **Calculator**, shows its value: `2 + 3 * 4 = 14`. **Enter** copies the value and closes
the popup. Numbers with a decimal point, `+`, `-`, `*`, `/`, `%` (remainder), `^` or `**` (power) and
brackets work; a number alone is not an expression. When the interface is in Russian, a decimal
comma works too (`1,5 * 2`).

Search Everywhere remembers the queries you chose a result with and the actions you ran from it,
also after a restart: **⌥↓** in the field brings back the last query, again for older ones, **⌥↑**
for newer ones, and the magnifier with the arrow at the left of the field lists them to choose one.
Built-in plug-ins can add tabs of their own.

At the right of the tabs is the toolbar of the tab shown:

- **All**: **Include non-project items**, **Preview**, **Filter** and **Open in Find Tool Window**.
  Filter lists what All searches (Calculator, Recent Files, Types, Files, Symbols, Actions, Text and
  the tabs of plug-ins) with **All**, **None** and **Invert**; untick one to leave it out of All.
- **Types**, **Files** and **Symbols**: the search scope (**Project Files ⌄**) instead of the check
  box, **Preview**, **Filter** and **Open in Find Tool Window**. Filter on Files lists the file
  types, **Directory** first; on Types and Symbols, the languages.
- **Text**: the search scope, the file mask, **Preview** and **Open in Find Tool Window**, and
  **Cc**, **W** and **.\*** at the right of the field ([Go to Text](#go-to-text)).
- **Actions**: **Include disabled actions** and **Open in Find Tool Window**, which is off there.

**Preview** shows the selected file, type or symbol under the list in a read-only editor with line
numbers and colours, its line marked; drag the divider above it to change its height. Whether it is
on, its height and what the Filters leave out are remembered, also after a restart.
**Open in Find Tool Window** puts the results on a new tab of the Find tool window (**⌘3**), where
**Enter** or a double-click opens one. The footer shows the selected item's path and the link
**Open In Right Split**; **⇧⏎** does the same: the file opens in a new editor group on the right.

## Find Action

**⇧⌘A** opens Search Everywhere on **Actions**: every action of GoGarden by its name, in English or
in the language of the interface, with its shortcut. When only the English name matches, the row
shows it next to the name, the letters that matched marked. Words can come in any order (`line dup`).
Actions you used lately come first. Those that cannot run now (an editor action without a file
open, for one) are left out unless **Include disabled actions** is ticked; then they are dimmed.

An action is also found by its abbreviations, which you give it in Settings → Keymap → **Add
Abbreviation** ([Keymap](09-settings.md#keymap)): an action whose abbreviation is what you typed, or
starts with it, comes first. It is found by its synonyms too, in English and in the language of the
interface: `format` or `beautify` find **Reformat Code**, `preferences` finds **Settings…**, `grep`
finds **Find in Files…**, `quick fix` finds **Show Context Actions**; the row then shows the synonym
that matched next to the name.

**⌥⏎** on an action, here or on All, assigns it a shortcut: the **Keyboard Shortcut** dialog of
Settings → Keymap opens over the popup, the footer says so (**Press ⌥⏎ to assign a shortcut**).
Press the keys and choose **OK**; if another action has them already, **Remove from Others** takes
them from it and **Leave** keeps both. **⎋** closes only the dialog. The shortcut is saved in the
keymap in use at once and shows in the row; any command Find Action lists can get one this way,
not only the actions of the keymap.

Besides the actions with shortcuts, it finds every other command, such as **Install gopls**,
**Reload Project Model** or **Restart Language Servers**, and runs it at once. A command that needs
something from you opens the place where you give it: **Select Theme** and **Import Theme** open
Settings → Appearance, **Choose Go SDK** Settings → Languages & Frameworks → Go, **Select Keymap**
Settings → Keymap, **Checkout Branch** the Branches popup. With an image open, **Zoom In**, **Zoom
Out**, **Actual Size** and **Fit Zoom to Window** zoom it, as the image toolbar does.

## Go to Type

**⌘O** (Navigate → Type… in GoGarden, Navigate → Class… where the language says classes) opens
Search Everywhere on **Types**: the types of your project (Go structs, interfaces and other named types) by name, found by gopls, with the package each is
declared in. With **Include non-project items** on, the types of the Go SDK and the libraries
are listed too; among equally good matches, your project's come first. **Enter**
opens the file at the type's name. gopls starts if no Go file is open yet; until it answers, the
popup says it is searching.

Type the name, a part of it, or its camel humps: `IS` and `itSt` find `ItemStore` (a capital letter
starts a word); `*`, spaces and a space at the end work as in Search Everywhere (`i*re` finds
`ItemStore`). Put the package first to narrow the search: `model.Item`.

## Go to Symbol

**⌥⌘O** (Navigate → Symbol…) opens Search Everywhere on **Symbols**: every declaration of your
project by name: functions, methods, fields, constants, variables and types. Each row shows the
symbol's icon (the same as in the Structure tool window), its name with its type for a method or a
field (`Store.Get`), its package, and on the right its file and line. Camel humps work as in Go to
Class, and a dotted query narrows by what the symbol belongs to: `Store.get` finds the methods named
like `get` of types named like `Store`, `model.Store.get` only in the package `model`. **Enter** goes
to the symbol's name.

The symbols come from the language servers (gopls for Go). Files that only a plug-in understands
(HTML, CSS, JSON and the like) have no symbols here yet; their Structure tool window shows them.

## Go to File

**⇧⌘O** opens Search Everywhere on **Files**. `/` in the query matches folders: `internal/ca`
finds `internal/cart/cart.go`. Add `:line` or `:line:column` to go there: `main.go:42` opens
`main.go` at line 42. Folders whose name matches are listed too, with a folder icon; **Enter**
selects one in the Project tool window.

## Go to Text

**⌥⇧⌘E** (Navigate → Text…) opens Search Everywhere on **Text** (**⌥⇧⌘E** again closes it): the
lines of your project's files that hold what you typed, found as Find in Files finds them. Each row
is the line with what matched marked, and on the right the file and the line (`main.go:42`);
**Enter** or a click opens the file with the caret at the match, and **Preview** shows the lines
around it.

- **Cc** matches case, **W** whole words only, and **.\*** takes the query as a regular expression;
  they sit at the right of the field and are remembered, also after a restart.
- The **search scope** (**Project Files ⌄**) chooses where to search, as on the other tabs.
- The **file mask** next to it keeps the search to some files: `*.go`, several separated by commas,
  `!*_test.go` to leave files out; empty, it searches every file. Its **⌄** lists the masks you used
  lately, the same list Find in Files shows for its file mask, and a mask joins that list when you
  open a line found with it.
- Every key you type, and closing the popup, stops the search of what you typed before; the list
  shows the first 50 lines, and **more…** at its end lists 50 more.

**All** lists text too, as a last resort: the **Text** group comes at the bottom, and only when
the other groups found fewer than six results in all. Turn it off in Settings → Advanced Settings →
Search Everywhere (**Add text matches to All when there are few other results**), or untick **Text**
in All's **Filter**.

## Switcher

Hold **⌃** and press **⇥** (Ctrl+Tab) to switch between files and tool windows. A quick **⌃⇥**
goes straight back to the previous file without showing anything; keep **⌃** held and the Switcher
appears: every tool window on the left with its shortcut (Commit ⌘0, Project ⌘1, Bookmarks,
Find ⌘3, Run ⌘4, Debug ⌘5, Problems ⌘6, Structure ⌘7, Services ⌘8, Git ⌘9, Terminal ⌥F12,
Notifications), the open and recent files on the right, the previous file selected.

- **⇥** again moves down, **⇧⇥** up; **⌃⇧⇥** opens the Switcher moving up, to the last file.
- **↑** / **↓** move the selection, **←** / **→** switch between the tool windows and the files.
- Release **⌃** to open the selected file or show the selected tool window. **Enter** does the same,
  **Esc** closes the Switcher.
- **⌦**, **⌘⌫**, or **⌫** with nothing typed, deletes the selected file from the list and closes
  its tab; unsaved changes in it are saved first, so nothing is lost.
- Bookmarks is listed, greyed and without a shortcut: it is still in development.

## Recent Files

**⌘E** (View → Recent Files) is the Switcher's list kept open: the same tool windows on the left,
with the **Recent Locations ⇧⌘E** link under them, and the files you opened lately on the right,
the current one first and the previous one selected, so that **⌘E** then **Enter** goes back to
it. Type to filter the list: the files are found as in Search Everywhere, stay in the order you
opened them, and the matched letters are marked.

- **Show edited only**, at the top right, or **⌘E** pressed again while the popup is open, keeps
  only the files you edited since the project opened.
- **↑** / **↓** move, **←** / **→** switch columns; **Enter** opens the file or shows the tool window.
- **⌦**, **⌘⌫**, or **⌫** with nothing typed, deletes the selected file from the list and closes
  its tab, saving its unsaved changes first.
- Hover over a file for the **Open in Right Split** icon; **⇧⏎** does the same for the selected
  file: it opens in a new editor group on the right.
- The footer shows the selected file's path.

## Recent Locations

**⇧⌘E** (View → Recent Locations) lists the places in the code you visited and edited lately,
newest first; the title counts them. Each place shows its file, the declarations around it (as the
breadcrumbs name them), how long ago you were there ("3 minutes ago") and five lines of its code
with their line numbers.

- Type to filter the places by their code, the file name or the breadcrumbs.
- **Enter** or a click goes to the place.
- **Show edited only**, or **⇧⌘E** pressed again, keeps the places where you changed the text.
- **⌫** (with nothing typed) or **⌦** removes the selected place; Back and Last Edit Location no longer go
  there either.

A place is remembered when you jump to or from it (as Back remembers it) and when you edit the text
there, one per line; the 25 latest of each kind are kept while the project is open.

## Go to Line:Column

**⌘L**, then a line, or `line:column`. It works in a text file; over a diff tab or an image it
does nothing.

## Back and Forward

**⌘[** (Navigate → Back) returns to the place you were before, and **⌘]** (Navigate → Forward) goes
again where Back left; so do the arrows at the left of the toolbar and the side buttons of a mouse.
They go across files: a file you closed opens again, at the caret it had. The arrows and the menu
items are greyed out when there is nowhere to go.

A place is remembered when:

- another file becomes the active one: opened, picked from a tab, Recent Files or Go to File, or the
  active tab closed (the place left is where the caret was);
- a jump to a declaration or a usage moves the caret to another line;
- **Go to Line** (⌘L) or **Next / Previous Highlighted Error** (F2, ⇧F2) moves the caret more than
  10 lines.

Typing and moving the caret with the keys or the mouse are not remembered. Editing a file moves its
places with the text. A new jump after Back clears Forward. The history keeps the last 100 places
each way, while the project is open; it does not survive a restart.

## Last Edit Location

**⇧⌘⌫** (Navigate → Last Edit Location) goes to the place you last changed the text, in any file;
press it again for the place changed before that, and so on. **Navigate → Next Edit Location** (no
shortcut; give it one in Settings → Keymap) goes back toward the latest change. If the caret is
already where you last typed, the first ⇧⌘⌫ goes to the change before it.

Typing on one line is one place, wherever on the line you stop; when you go on typing on another
line, that is the next place. The places move with later edits, as Back's do. A file you closed
opens again at the place; a file deleted since is skipped. A new edit starts again from the latest
place. Both menu items are greyed out when there is nowhere to go. Going to an edit place is a jump:
**⌘[** returns to where you were. The places follow a file you rename or move. The last 100 places
are kept while the project is open.

## Bookmarks

A bookmark marks a line you want to come back to, or a whole file or folder (bookmarked from the
Project tool window and the editor tabs, below). **Edit → Bookmarks** has every action for lines:

- **Toggle Bookmark** (**F3**) puts an anonymous bookmark on the caret's line, or removes the one
  there. The gutter shows a bookmark icon right of the line number.
- **Add Mnemonic Bookmark…** (**⌥F3**) opens a chooser of the digits 0–9 and the letters A–Z, with
  a description field under it. Type or click a character, type a description if you like, and
  press **Enter**. Characters other lines use are marked; hover one to see where it is. Choosing
  one of them asks whether to move it to this line; **Don't ask again** moves it without asking from
  then on. A mnemonic bookmark shows its character in the gutter.
- **⌃⇧** and a digit (for example **⌃⇧1**) toggles that digit's bookmark on the caret's line at
  once, taking it from the line that had it; **⌃** and the digit (**⌃1**) jumps to it.
- **Show Line Bookmarks…** (**⌘F3**) opens a popup with every line bookmark under its list. Type a
  bookmark's digit or letter to jump to it at once, or pick one with the arrows and press
  **Enter** (a double-click works too); **Esc** closes it.
- **Go to Mnemonic…** (**⌥⌘F3**) opens the chooser with the characters in use; type or click one
  to jump to its bookmark.
- **Previous Line Bookmark** and **Next Line Bookmark** go round every bookmark, by file and line.
  They have no shortcut; give them one in Settings → Keymap.

Every jump to a bookmark is remembered by **Back** (⌘[). Hovering a bookmark icon shows its
description. Right-click the gutter for its **Bookmarks** menu: **Add Bookmark to Another List**,
**Rename Bookmark…** (the description; **⌥-click** on the icon does it too), **Toggle Bookmark**,
**Remove Mnemonic** and **Toggle Bookmark Mnemonic…**.

### Files, folders and open tabs

- In the Project tool window, **F3**, **⌥F3** and the context menu's **Bookmarks →** bookmark the
  selected files and folders, one or several at once
  ([Bookmarks of files and folders](05-project.md#bookmarks-of-files-and-folders)); the tree shows
  a bookmark icon after their names.
- Right-click an editor tab for **Bookmarks → Toggle Bookmark**, **Toggle Bookmark Mnemonic…** and
  **Delete**: the same for the tab's file, into the default list.
- **Bookmarks → Bookmark Open Tabs…** in the tab menu (also in Find Action) asks for a name and
  creates a new list with a bookmark of every open tab, in tab order; a file bookmarked already
  moves into the new list.

A file's or folder's bookmark is a row of its list in the Bookmarks tool window, beside the line
bookmarks, with the file's or folder's icon. Opening it opens the file as it was left; a folder is
selected in the Project tool window. **⌃** and a digit does the same for a file or folder with
that mnemonic. Show Line Bookmarks lists them only when **List only line bookmarks in the popup** is
off.

### The Bookmarks tool window

**⌘2** (View → Tool Windows → Bookmarks, or the bookmark button on the left stripe, under the line)
shows and hides the **Bookmarks** tool window: a tree of the bookmark lists, each with its
bookmarks as `file:line` and their description, and the breakpoints as a list of their own.

- At first there is one list, named after the project. It is the **Default** list (the tag next
  to its name), where new bookmarks go.
- Right-click a list for **Create Bookmarks List**, **Rename…** (**F2**), **Mark List as
  Default** (it moves to the top with the Default tag, and new bookmarks go there) and **Delete
  Bookmark List**, which asks first and removes the list's bookmarks from the code; it cannot be
  undone, and the other lists keep their bookmarks and mnemonics.
- Right-click a bookmark for **Jump to Source**, **Rename Bookmark…** (**F2**), **Add Bookmark to
  Another List**, **Move Up**, **Move Down** and **Delete**. Drag a bookmark onto another list to
  move it there (last), or onto another bookmark to put it before or after it; a line shows where
  it goes.
- **Double-click** or **Enter** opens a bookmark or breakpoint in the editor; **⌃⌥↓** and **⌃⌥↑**
  (also **⌥⌘↓** and **⌥⌘↑**) open the next and previous one of the tree, going round, while the
  tool window keeps the keyboard. **Delete** (or **⌫**) removes the selected bookmark or
  breakpoint, or deletes the selected list. **←** and **→** collapse and expand, **Esc** goes back
  to the editor.
- **⋮** in its header has **Group Line Bookmarks by File** (the bookmarks of a list under their
  files) and **Ask Before Rewriting Mnemonic** (off: choosing a mnemonic another line has moves it
  without asking, as the question's Don't ask again does).

The lists, their order, the default one and the ⋮ options are kept with the project.
**Settings → Advanced Settings → Bookmarks → List only line bookmarks in the popup** (on by default)
keeps the bookmarks of files and folders and the breakpoints out of Show Line Bookmarks.

Bookmarks move with their lines as you edit, and when another program changes an open file (a
deleted line takes its bookmark along). They follow a file or folder you rename or move in the
Project tool window, go away with a deleted file or folder, and are kept when the IDE restarts. They belong to the project and are kept in the IDE's own data folder
(`workspace.json`), never in the project's files.

## Structure

**⌘7** (View → Tool Windows → Structure, or the Structure button on the left) shows the symbols of
the file in the editor as a tree, in the place of the Project tool window; **⌘1** brings the project
back. For a Go file: the types (structs, interfaces and the others) with their fields, and under each
type its methods; the functions, constants and variables. Each row has a badge with the kind's letter,
the name, and its type or signature in grey.

- A click goes to the symbol in the editor; a double click or **Enter** also puts the keyboard back
  in the editor. **↑** and **↓** choose, **←** and **→** fold and unfold, **Escape** returns to the
  editor.
- **Show Fields** and **Sort Alphabetically** are the buttons in the header; **⋮** has **Navigate
  with Single Click** (off: a click only selects) and **Follow Caret** (the symbol at the caret is
  selected as you move). These choices, and whether the window is shown, are remembered with the
  project.
- The symbols come from gopls. Until gopls answers, they are read from the text, so the tree is
  there as soon as a file opens; after an edit, the last tree stays until gopls has caught up.
  A file nothing can be read from says so.

Agents read the same tree with `editor.structure.get`.

## File Structure

**⌘F12** (Navigate → File Structure) shows the same symbols in a popup over the editor, the symbol
at the caret selected, without leaving the keyboard.

- Type to find a symbol: its letters in order, camel humps first (`las` finds `ListenAndServe`); the
  letters found are marked, and the best match is selected.
- **Narrow down on typing** (on by default) leaves only the symbols found and the types that hold
  them; off, every symbol stays and only the selection moves.
- **↑** and **↓** choose; **Enter** or a click goes to the symbol and closes the popup; **Escape**
  closes it.
- The popup reads the symbols as the Structure tool window does, with its Show Fields and Sort
  Alphabetically choices. Pressing **⌘F12** again leaves it open: inherited and anonymous members,
  which it shows in IDEs for other languages, do not apply to the structure of a Go file.

## Select In

**⌥F1** (Navigate → Select In…) shows the file in the editor somewhere else; while the Project
tool window has the keyboard, it shows the file or folder selected there. A popup lists the
places, numbered; press the number, or choose with **↑** and **↓** and press **Enter**, or click;
**Escape** closes it.

1. **Project View** unfolds the Project tool window's folders down to the file, selects it and gives
   the tree the keyboard. A file of a library, such as one a jump to a declaration opened, is found
   under External Libraries.
2. **Structure** shows the Structure tool window with the symbol at the caret selected, whether
   Follow Caret is on or not.
3. **Commit**, only when the file has changes to commit, shows the Commit tool window with the file
   selected, its changelist unfolded.
4. **Finder** (Explorer on Windows, the file manager on Linux) shows the file selected in its folder.
5. **Terminal** opens a terminal tab in the file's folder.

The numbers follow the places shown: without changes, Finder is 3 and Terminal 4. Agents open the
popup with `view.selectIn.open`, which answers with the places, and go to one at once with
`view.selectIn.target` (`target`: `projectView`, `structure`, `commit`, `finder` or `terminal`;
`path` for another file than the editor's).

## Navigation bar

The left part of the status bar is the **navigation bar**: the path of the file in the editor,
`project › folders › file`, and, with **Show Members** on, the declarations around the caret after
it (`cart.go › Cart › Add`). Every segment is a button:

- A click on the project or a folder opens a popup of what is inside it: folders first, then files.
  A click on the file lists its declarations, and a click on a declaration the declarations inside
  it. The entry the bar leads through is selected.
- In the popup, **↑** and **↓** choose, and typing filters the list as you type (the letters in
  order, camel humps first; the letters found are marked). **Enter** or a click on a file opens it;
  on a folder it goes into the folder, which becomes the bar's last segment with its own popup open;
  on a declaration it goes there. **←** and **→** move to the popup of the segment before or after.
- A file without declarations to show, or a declaration with nothing inside, has no popup: a click
  opens the file or goes to the declaration.
- A path wider than the bar (a file deep in the Go SDK or the module cache) starts with **…**: the
  first segments give way so that the file and its declarations stay in view; the segment chosen
  with ← stays shown.

**Navigate → Jump to Navigation Bar** (**⌘↑** or **⌥Home**) gives the bar the keyboard with its last
segment selected. **←** and **→** move between segments, **↓** or **Enter** opens the selected
segment's popup, and **Escape** returns to the editor. With the bar hidden (**Don't Show**, or in
Presentation, Distraction Free and Zen Mode), ⌘↑ shows it over the top of the editor until you
choose something or press Escape. Move Caret to Text Start, which ⌘↑ did before, is **⌘Home**
(fn⌘← on a laptop keyboard).

**View → Appearance → Navigation Bar** chooses where the bar is:

| Item | What it does |
|---|---|
| **Top** | A bar of its own under the main toolbar, with the same segments and popups; the status bar keeps only its widgets |
| **In Status Bar** | The left part of the status bar (the default) |
| **Don't Show** | No bar; ⌘↑ still shows it over the editor |
| **Show Members** | The declarations at the caret follow the file (on by default) |

The choice is saved with the IDE's settings (`navigationBar` and `navigationBarMembers` in
`settings.json`) and applies to every project window.

AI agents run **`view.navigationBar.jump`** with `run_action`: it answers with the segments, the
selected one and the popup open, each entry with its name, kind (`project`, `folder`, `file`,
`symbol`) and path. Its parameters `segment` (counted from 0), `open`, `filter` and `close` select a
segment, open its popup, type into it and leave the bar; `press_keys` with ←, →, ↓, ⏎ and ⎋ works as
the keyboard does. `get_window_state` lists the bar as `navigationBar` and its popup as
`navigationBarPopup`. Without a window, `navigate.navigationBar.get` reads the segments and
`navigate.navigationBar.children` the children of one. The placement is set with
`view.navigationBar.top`, `view.navigationBar.statusBar`, `view.navigationBar.hide` and
`view.navigationBar.members.toggle`, or `set_settings`.

## The main menu

The macOS menu bar has GoGarden's menus: File, Edit, View, Navigate, Code, Refactor, Run, Git,
Window and Help. Every item shows its shortcut, and is greyed out when it cannot run (an editor
action without an open file). Choosing an item does exactly what its shortcut does. **View → Tool
Windows** opens every tool window: Project, Commit, Structure, Find, Git, Run, Debug, Problems and
Terminal.
