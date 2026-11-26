# Version control

GoGarden works with Git repositories: it colours changed files, shows the current
branch, switches and creates branches, and updates and pushes your project. Git must be installed
for the operations that change the repository; reading works without it.

## File status colours

In the Project tool window and on the editor tabs, file names are coloured by their Git status:

| Colour | Status |
|---|---|
| Blue | Modified since the last commit (folders holding modified files are blue too) |
| Green | Added to Git, not committed yet |
| Red | Unversioned: new and not added to Git |
| Olive | Ignored by `.gitignore` |

The colours follow your changes as you type, before they are saved: undo every change of a file and
it is no longer blue. Commits or checkouts made in a terminal show within a few seconds.

## Branches

The branch widget in the main toolbar, next to the project name, shows the current branch (or the
commit, when `HEAD` is detached) and, after it, a green ↗ when there are commits to push and a blue ↙
when there are commits to pull. Click it to open the branches popup:

- **Search for branches and actions** at the top: just type to filter the actions and the branches
  (every group opens to show what matches). The button next to it collapses or expands all groups;
  the gear holds **Fetch**.
- **Update Project…** (⌘T), **Commit…** (⌘K) and **Push…** (⇧⌘K).
- **New Branch…** (⌥⌘N) asks for the name; **Checkout branch** (on by default) switches to it at once.
  **Checkout Tag or Revision…** asks for a tag, a commit id or any revision (`HEAD~2`) and checks it
  out with a detached `HEAD`.
- The groups **Recent** (the branches you checked out last, the current one first, whether you
  switched in GoGarden or in a terminal), **Local**, **Remote** and **Tags**. Branches whose names
  contain "/" are grouped in folders (`feature/login` is `login` in the `feature` folder). The current
  branch has a yellow label. A local branch shows, right after its name, the commits to pull (blue ↙)
  and to push (green ↗), and at the right the remote branch it tracks.
- Point at a branch (or click it) for its actions: **Checkout**, **New Branch from '…'…**,
  **Merge '…' into '…'** and **Delete**. Checking out a remote branch creates the local branch that
  tracks it; a tag is checked out with a detached `HEAD`.

The keyboard works throughout: ↑ and ↓ move, → and ← open and close a group, a folder or a branch's
actions, Enter runs the selected action, and Escape closes the popup.

## Commit

**⌘0** opens the Commit tool window on the left, in place of the Project tool window; **⌘K**
(Git → Commit…) opens it with the commit message ready to type.

- **Changes** lists the files changed since the last commit, **Unversioned Files** the new ones. Every
  changed file is checked, new files are not: check what goes into the commit. A group's checkbox
  checks or unchecks all its files.
- A double click on a file shows its diff; **⌘D** shows the selected file's, and **Jump to Source**
  (**F4** or **⌘↓**, also in its context menu) opens it in an editor tab and gives the editor the
  keyboard. A deleted file has nothing to open.
- The toolbar: **Refresh**, **Rollback**, **Show Diff**, **Shelve Changes**, **Expand All** and
  **Collapse All**.
- Drag the line between the changes and the message to make the message field bigger or smaller;
  the IDE remembers its size for the project. A long message scrolls inside the field, and in a low
  window the list and the field get smaller first, so the buttons stay in view.
- Write the message and press **Commit** (or **⌘⏎** in the message). **Commit and Push…** pushes
  right after. **Amend** replaces the last commit instead, starting from its message.
- Only the checked files are committed, with their unsaved edits; whatever else is staged stays
  uncommitted.

### Changelists

Changed files are grouped in **changelists**. At first there is one, **Changes**;
once there are several, each is a group of its own in the Commit tool window, the active one in
bold. A file you change goes into the active changelist and stays in its list until it is
committed, rolled back or moved. New (unversioned) files are in no list.

- **New Changelist…** (Git → Uncommitted Changes, or a changelist's context menu) asks for a name, a
  comment and whether to make it active.
- A changelist's context menu: **Set Active Changelist**, **Edit Changelist…** (rename it, change its
  comment), **Delete Changelist** (its files move to the active list; the active list cannot be
  deleted) and **Shelve Changes…**.
- **Move to Another Changelist** (a file's context menu, or **⌥⇧M** for the selected file) moves
  files to another list or to a new one.
- To commit one changelist, check only its group's checkbox. GoGarden remembers the changelists of
  each project.

### Shelf

**Shelve Changes…** puts changes aside without committing them: the toolbar's tray button, a
file's or a changelist's context menu, or Git → Uncommitted Changes. The dialog lists the changed
files with checkboxes (the selected one's diff is shown below them) and a name, the changelist's
by default. Shelving keeps the changes in GoGarden's data folder as a patch and rolls the files
back; unversioned files are shelved and deleted.

**Check TODO** above the message, off by default, looks for TODO items in the lines the commit
changes; **Configure** picks a filter from [Settings → Editor → TODO](22-tool-windows.md#todo). When it
finds any, the commit stops and asks: **Review** opens the TODO tool window, **Commit Anyway** goes
on, **Cancel** goes back. Agents ask the same with `todo.commit.check`.

The **Shelf** tab of the Commit tool window (it appears once something is shelved, or with Git →
Uncommitted Changes → Show Shelf) lists the shelved changes, newest first, with their files. A
double click on a file shows its diff. The toolbar and the context menus: **Unshelve…**, **Show
Diff**, **Rename…** and **Delete** (asks first).

**Unshelve…** shows the files with checkboxes and the diff of the selected one, and the changelist
to put them in (the one they were shelved from, if it is still there). If the files changed since,
the changes are merged; when they conflict, GoGarden says so, the conflicting files are marked
and the shelf keeps the changes. Unshelved changes move to **Already Unshelved** unless you clear
**Remove successfully applied files from the shelf**; they can be unshelved again or deleted.

Shelving and unshelving can be undone in the Git workspace (**Undo**, ⌘Z there): undoing a shelve
brings the changes back into the files (the shelf keeps them too).

### Patches

A patch is a file of changes you can send to someone or keep: GoGarden writes them in Git's format,
so `git apply` and `git am` take them, and it applies patches made by Git, by `diff -u` and by other
IDEs.

**Create Patch…** is in Git → Patch, in a changelist's or a file's context menu in the Commit tool
window (**Create Patch from Local Changes…**), in a commit's context menu in the Git workspace, and
in a shelf's context menu on the Shelf tab. The dialog shows the changed files with checkboxes (for
local changes) and asks:

- **Save to file** (a `.patch` next to the project, named after the changelist, the commit or the
  shelf) or **Copy to clipboard**;
- **Base path**: the folder the paths in the patch start from — the project's root by default; a
  folder inside the project leaves out the files outside it, a folder above it puts the project's
  folder before every path;
- **Reverse patch**: a patch that undoes the changes;
- **Encoding** of the file (UTF-8 by default; windows-1251, KOI8-R and others).

A patch starts with a mail header: a commit's author, date and message, or your name, the time and
the changelist's or shelf's name. **Copy as Patch to Clipboard** (the same context menus) copies the
patch of the selected changes at once.

**Apply Patch…** (Git → Patch) asks for a patch file, **Apply Patch from Clipboard…** takes the
patch you copied, and dropping a `.patch` or `.diff` file onto the editor opens the same dialog. It
lists the patch's files: those that apply as they are, those merged three-way (the patch names the
version it was made from and the repository has it), and in red those with **conflicts**, **file
missing**, **already exists** or **cannot apply**. **Show Diff** (⌘D) shows the selected file as it
will be, the conflicting lines in red, with the hunks that do not apply named below the list.
Below the list the dialog also says why a file cannot apply, which hunks apply only approximately
(somewhere else than `git apply` would put them, with context lines left out or with whitespace
and line breaks ignored; `git apply` would refuse them, so check their diff) and whether the patch
makes the file executable or no longer executable.

- **Map base directory**: the project folder the patch's paths start from (for a patch made in a
  subfolder or another checkout).
- **Strip leading directories**: − and + take folders off the start of the patch's paths or put them
  back (Git's `a/` and `b/` are taken off by default).
- **Reverse patch** undoes the patch instead; **Encoding** is the patch file's.
- **Changelist**: where the changed files go — an existing changelist, or **New changelist…** with a
  name.
- **Import to shelf** puts the patch on the shelf instead of applying it; unshelve it later.

**Apply** changes the checked files. A hunk that does not apply leaves its file conflicted, the way a
merge does: GoGarden says which files, and **Merge…** in the Git workspace (or the Commit tool
window's Merge Conflicts group) opens the conflict editor to merge them. A patch made from a commit
fills the commit message in and commits as its author (shown above the message, with × to commit
as yourself). Applying a patch can be undone in the Git workspace (**Undo**), each open file takes
the patch as one undo step, and Local History keeps every file as it was before the patch ("Before
Apply Patch"). Paths in a patch never reach outside the project or into `.git`, in any spelling:
such a patch is refused. A file reached through a symbolic link, and a symbolic link or a submodule
the patch would create or change, cannot apply: apply those with `git apply` in the terminal.

## Diffs

A file's diff opens in an editor tab next to the others: the version of the last commit (HEAD) and
yours, with the changed lines coloured (green added, blue modified, grey deleted) and the changed
words marked. **↑** and **↓** (or **⇧F7** and **F7**) go to the previous and next difference. Choose
**Side-by-side viewer** or **Unified viewer** in the toolbar; GoGarden remembers the choice. Long lines
scroll sideways with a sideways swipe, **⇧** and the wheel, or the bar under the text; both sides
scroll together and the line numbers stay. Marks on the right edge, beside the scroll bar, show where the differences
are in the whole file; point at one to see its lines, click it to go there. Git → Show Diff shows the active file's diff.
Click a line to put the caret there; **F4** (or **⌘↓**, Jump to Source) opens the file at that
line, and from the HEAD side at the matching line of your version. Before you click, it opens the
current difference.
The toolbar also chooses how whitespace counts (**Do not ignore**, **Trim whitespaces**, **Ignore
whitespaces**: lines that differ only there are equal, and ↑ ↓ skip them) and has **Swap Sides**
(⇄), which puts the left side on the right. Staging hunks and lines from a diff is offered while
the sides are as read and every character counts. The same diff tab compares any files and texts;
see [Comparing files](05-project.md#comparing-files).

## Changes in the editor

The gutter marks the lines you changed since the last commit, saved or not: a green bar for added
lines, a blue bar for modified ones, and a small grey triangle where lines were deleted. Click a mark
to see the committed lines, to **Rollback Lines** (⌘Z takes it back) or to **Show Diff**.

**⌥⌘Z** (Git → Rollback…) discards changes: it lists the changed files (the active one, or the one
selected in the Commit tool window, checked) and puts back their committed content. Added files are
no longer added; **Delete local copies of the added files** removes them from the disk too.

## History

**⌘9** opens the Git tool window at the bottom, on its **Log** tab:

- **Tabs.** **+** next to **Log** opens another Log tab with its own filters; **▾** lists the tabs.
  A tab other than the first closes with its **×**. **Compare with Current** opens a tab with what a
  branch has that the current one does not.
- **Branches**, on the left. **Branch or tag** searches them. **HEAD (Current Branch)**, then the
  **Local**, **Remote** and **Tags** groups, which fold; names with "/" are grouped in folders
  (`feature/x` under **feature**). The current branch has a label icon, favourites a star (**main**
  and **master** are favourites until you unmark them), and a branch with commits to push or to pull
  shows how many (↗1, ↙2). A click shows only that branch's history. The tool bar on the left:
  **Hide Branches**, **New Branch**, **Update Selected** (brings a branch up to date with its
  upstream without checking it out), **Delete Branch**, **Compare with Current**, **Navigate Log to
  Branch Head**, **Fetch**, **Mark as Favorite**, **Group by Directory**, **Expand All** and
  **Collapse All**; the same and **Checkout**, **New Branch from…** and **Merge** are in a branch's
  context menu.
- **Filters**, above the commits: **Text or hash** (part of the message, or the start of a commit's
  id), with **.\*** for a regular expression and **Cc** to match case; **Branch**, **User** (**me**
  first), **Date** (the last 24 hours, 7 or 30 days, or **Select…** a range from one day to
  another, either end open) and **Paths**: **All**, **Select Folders…** (the repository's folders
  and files as a tree with checkboxes; a checked folder takes in everything inside it), the sets of
  paths you used last, or a path you type. **Git → Show History** in the project tree's context menu
  (or Git → Show History for the file in the editor) opens a tab with the history of that folder or
  file. The filters work together: a commit is listed when it matches every one. A set filter shows
  its value (a long one is shortened; rest the pointer on it to read it); its **×** clears it. Each
  tab keeps its own filters, and the tabs, their filters and the recent paths are still there the
  next time you open the project.
  The sort button lists the commits by date or topologically (each branch's commits together). On
  the right: **Refresh**, **Cherry-Pick**, **View Options** and **Go to Hash/Branch/Tag**.
- **Commits**, newest first, with the graph of branches and merges. After the subject, a label for
  every branch and tag at the commit: the current branch first, a branch and its upstream at the same
  commit as one label (`origin & main`), long names shortened (`../published-course…`). In a narrow
  row the subject is shortened first, then the last labels collapse into "+N": rest the pointer on
  it to see their names. The commit
  `HEAD` is at has a ring. Merge commits are dimmed, your own commits have the author in bold, and
  recent dates read **Today 19:47**, **Yesterday 18:42** or **5 minutes ago**. More commits are read
  as you scroll down. A commit's context menu: **Cherry-Pick**, **Checkout Tag or Revision…**,
  **New Branch…**.
- **Details**, on the right: the selected commit's changed files as a tree by directory, with the
  number of files of each folder (**Group by Directory** lists them flat, **Expand All** and
  **Collapse All** fold them; a double click or **Show Diff** shows how the commit changed a file),
  and below, its full message in the editor's font with its id, author, date and the branches that
  contain it.

## Git workspace

The Git workspace button at the top of the right tool window bar, **Git → Git Workspace** (or Find
Action, ⇧⌘A, "Git Workspace") opens the whole repository over the editor area, as the
[Database workspace](28-database.md) opens: the editor's tab strip stays on top with a **Git** tab
(click it to come back, × to close the workspace), and the workspace's own tabs are under it. The
first one, **Graph**, is always there; a diff opened from the workspace, the interactive rebase
editor and the conflict editor open as more tabs beside it. Press ⇧⎋, click the button again or
open a code file to collapse it: the **Git** tab stays in the strip until you close it, and the
workspace comes back as you left it, also after a restart. The **Graph** tab has:

- **The toolbar**: **Undo** and **Redo** (see [Undo and Redo](#undo-and-redo)), **Pull** (see
  [Pull](#pull); its **▾** is in its lower right corner), **Push**, **Branch** (New Branch),
  **Stash** and **Pop**, each a caption over its icon and all of the same width, from the left, and
  **Search** (⌘F) on the right. The project and the branch are in the IDE's title bar, as
  everywhere else. A button that cannot act now (Stash without changes, Pop without stashes) is dimmed; in a
  narrow tab the buttons keep only their icons. Resting the pointer on a button names it with its
  shortcut: **⌘Z** Undo, **⇧⌘Z** Redo, **⌘T** Pull, **⇧⌘K** Push, **⌥⌘N** Branch, **⌥⇧⌘S** Stash,
  **⌥⇧⌘P** Pop and **⌘F** Search (⌘Z, ⇧⌘Z and ⌘T act on the Git tab while it is in front; Stash
  and Pop work anywhere).
- **The commit graph** in the middle: every commit of every branch, newest on top. Each row is
  tinted in its branch's colour from its commit to the right edge (more under the pointer, most
  when selected), and the lines of the branches turn in rounded corners. The first branch or tag at
  a commit is a label filled in the branch's colour: a check before the current branch, a laptop
  for a local branch, a cloud for a remote one (both when a local branch and its remote branch are
  at the same commit), a tag's mark for a tag, and "+2" when there are more (rest the pointer on
  them to see all). The author's initials are in the commit's circle, or their picture when
  **Settings → Version Control → Git Workspace → Show the authors' pictures** is on (see
  [Authors' pictures](#authors-pictures)); then the message, the author,
  the date and the short id (author, date and id give way when the tab is narrow; a message too
  long for its column ends in "…", and resting the pointer on it shows it whole). The columns can
  be arranged as you like (see [Graph columns](#graph-columns)). Rest the pointer on a branch's
  label (or its row on the left) to highlight that branch's commits; the others fade. Rest it on a
  commit without a label to see, in its **BRANCH / TAG** column, the branch nearest to it: of the
  branches that have the commit, the one the fewest commits above it.
- **Very large histories** open at once: the newest commits show as soon as they are read, and the
  rest comes in below them while you work, with "Reading the history… 25600 commits so far" at the
  bottom of the graph until it is read to the end. The rows already shown never move as more come,
  and the window never waits for the reading. A repository of 200 000 commits is read in about a
  second, and each commit takes about 300 bytes of memory, so histories of hundreds of thousands of
  commits scroll, search and filter at once. The history is read again only when a branch, a tag or
  a stash moves; until the new reading is done, the graph shows the previous one.
- **WIP** on top when files are changed, with how many, in a dashed circle: click it to stage and
  commit on the right (see [Staging and committing](#staging-and-committing)).
- **On the left**, how many branches, tags, stashes, worktrees and submodules it is **Viewing**,
  the **Filter** field (**⌥⌘F**), then the **LOCAL** branches (the current one marked, with commits
  to push ↑ and to pull ↓), the **REMOTE** branches by remote, the **TAGS**, the **STASHES**, the
  **WORKTREES** and the **SUBMODULES** (see [Worktrees](#worktrees) and [Submodules](#submodules)),
  each with its icon and count; every section starts folded: click a title to unfold or fold it. Branches named with `/` sit in folders (`feat/login` in
  **feat**), which fold too and show how many branches they hold while folded. Click a branch, a
  tag or a stash to jump to its commit; double-click a branch to check it out (a remote one checks
  out a local branch that tracks it). Double-clicking a commit with a local branch checks that
  branch out too.
- **The details panel** slides in over the graph from the right when you select a commit or the
  WIP, and the graph keeps the whole width under it. Close it with its **×**, **⎋**, or a click on
  the graph's empty space under the last commit; the commit stays selected, and selecting one
  again opens it. Drag its left edge to make it wider or narrower (from 280 points to 70 % of the
  tab); the width is kept for each repository. In a narrow window it is never wider than the graph,
  so it never covers the left panel or its counts. While files are changed, a banner on top says how
  many, with **View Changes** (the WIP). Under it the selected commit: **commit:** and its short id (rest the pointer on it for the
  whole SHA, click to copy it), the subject in large letters with the rest of the message under it,
  the author (initials in a circle) and **authored** with the date, the **parents** (click one to
  go to it), how many files were **modified**, **added** and **deleted**, and the files: by
  **Path** (the folder dimmed, the name bright) or as a **Tree** of folders; **View all files**
  lists every file of the commit, the ones it did not change dimmed. Click a changed file to see
  how the commit changed it in the diff tab. Closing that diff brings the Git tab back.
- **Stashes in the graph**: each stash is a row of its own, a square with a tray, right above the
  commit it was made on and joined to it by a line in that branch's colour.
- **Other worktrees' work in progress in the graph**: each other worktree with changed files is a
  **WIP in** *branch* row with a dashed circle and how many files changed, right above the commit
  that worktree is at (see [Worktrees](#worktrees)).
- **↑ and ↓** move through the commits.

### Graph columns

The graph has six columns: **BRANCH / TAG**, **GRAPH**, **COMMIT MESSAGE**, **AUTHOR**, **DATE**
and **SHA**.

- **Move** a column by dragging its title sideways: an accent line shows where it goes.
- **Resize** a column by dragging its border: the right one of a column left of the message, the
  left one of a column right of it (the message takes the room the others leave). Double-click a
  border to give every column its own width back.
- **Right-click a title** for the columns menu: **Author**, **Date** and **SHA** switch those
  columns on and off; **Highlight Branch on Hover** turns the highlighting of a hovered branch's
  commits on and off; **Reset Columns** brings back every column in its usual place and width.
- In a narrow tab, the SHA gives way first, then the author, then the date, so that the message
  keeps its room.
- A title too long for its column ends in "…" (**BRANCH…**, **СООБЩЕНИЕ КОММ…**); rest the pointer
  on it to read it whole. The titles of the interactive rebase's columns and of the conflict
  editor's panes do the same.
- **The graph column** follows its branches' lines until you resize it, by its title's border or
  by its border anywhere along the rows. Narrower than its lines, the lines that fit stay and the
  commits of the others gather into one line of dots at the column's right, each dot in its
  branch's colour; a branch's line and its dots go into the line of dots together, so a dot
  never sits there while its line still runs on the left, and a line joining or leaving a
  gathered branch runs on into the line of dots, with every row still tinted in its branch's colour; a scroll bar under the
  column (or a sideways swipe, or ⇧ and the wheel over it) scrolls the lines sideways, and the
  lines scrolled out gather into the dots too. A swipe on the trackpad moves the graph in both
  directions at once: a diagonal swipe goes down the history and along the lines together. At the narrowest only the line of dots is left and
  the column's title becomes a graph icon. Widen it again, or double-click its border, and the
  lines come back.
- The graph always shows its scroll bars: up and down on the right, and sideways under the graph
  column while its lines do not fit.

### Authors' pictures

**Settings → Version Control → Git Workspace → Show the authors' pictures from Gravatar, GitLab
and GitHub** puts each author's picture in the commits' circles and in the details panel instead
of their initials. It is on at first; turned off, the IDE contacts nothing and shows the initials.
On, for the authors of the commits in view it asks the repository's GitLab (when a remote is on a
GitLab) for the author's public avatar by e-mail address (GitLab finds a person only by the e-mail
address they made public on their profile, and otherwise answers with a generated pattern), GitHub for the picture of a GitHub no-reply address
(`123+name@users.noreply.github.com`), and Gravatar by a hash of the address, in that order, and
keeps what it finds in the IDE's data folder (`caches/avatars`), so that the pictures show again
without the network. Authors without a picture, and everyone while you are offline (except the
pictures already kept), keep their initials; an address without a picture is asked about again
after a week. What is contacted, and with what, is in
[ADR-0074](../adr/0074-git-workspace-avatars.md); why it is on by default, in
[ADR-0075](../adr/0075-avatars-on-by-default.md).

GoGarden remembers the columns for each repository, whichever project window it is opened from.

### Pull

**Pull** on the toolbar (or **⌘T** while the Git tab is in front) brings in the commits of the
current branch's upstream. Its **▾** offers every way to do it:

- **Fetch All** fetches every remote; your branches stay where they are.
- **Pull (fast-forward if possible)** moves the branch forward when it has no commits of its own,
  and merges otherwise (the default).
- **Pull (fast-forward only)** only moves the branch forward; when it has commits of its own, it
  does nothing and says so.
- **Pull (rebase)** puts the branch's own commits on top of the upstream's.

Click an item to run it once. Click the circle at its right to make it what **Pull** does: the
filled circle marks it, and Pull's hint names it. GoGarden remembers it for each repository. The
way you choose is always the one used, whatever `pull.rebase` or `pull.ff` say in your Git
settings.

### Acting on a commit

Right-click a commit in the graph for its menu:

- **Create Branch Here…** opens New Branch, starting at the commit.
- **Create Tag Here…** asks for the tag's name; with a message the tag is an annotated one, without
  one a lightweight one.
- **Checkout This Commit…** asks first, then checks the commit out with a detached HEAD: new commits
  belong to no branch until you create one.
- **Cherry-Pick Commit** applies the commit's changes to the current branch as a new commit (not
  offered for a commit the branch already has). **Revert Commit** makes a new commit that undoes it
  (offered for commits of the current branch). A merge commit is taken against its first parent.
- **⌘-click** commits to pick several (they are highlighted; ⌘-click again takes one out); the menu
  of a picked commit then offers **Cherry-Pick *N* Commits…**, which opens them in the
  [interactive rebase editor](#interactive-rebase) to be picked onto the current branch. Two picked
  commits (or a commit and the **WIP** row) are compared on the right, and their menu offers
  **Compare** (see [Comparing commits](#comparing-commits)).
- **Interactive Rebase from Here…** (on an earlier commit of the current branch) opens the
  commits after it in the [interactive rebase editor](#interactive-rebase).
- **Reset *branch* to This Commit** moves the current branch there: **Soft** keeps the changes
  staged, **Mixed** keeps them unstaged, **Hard** discards every uncommitted change and asks first.
- **Copy Commit SHA** and **Copy Commit Message** put the full id or the whole message on the
  clipboard.

After a cherry-pick, a revert or a reset, the new HEAD is selected. If a cherry-pick or a revert
stops on conflicts, the repository is left as Git leaves it (the files with conflict markers, the
operation in progress): a dialog names the files, and **Resolve…** opens the first one in the
[conflict editor](#merge-conflicts). The operation can also be continued or aborted from the banner
under the toolbar.

### Comparing commits

See what changed between any two points of the history:

- Click a commit, then **⌘-click** another: the right panel shows **Comparing:** with the two short
  ids, the older one **From** and the newer one **To** (click an id to go to that commit), how many
  files were modified, added, deleted and renamed between them, and the files, by **Path** or as a
  **Tree**. Click a file to see how it differs between the two in the diff tab; a file that was
  renamed in between is compared with where it was.
- **⌘-click the WIP row** (with a commit picked or selected) to compare that commit with your
  work in progress: what is changed in the working tree now, staged or not, new files included.
- Right-clicking one of the two picked commits offers **Compare**, which shows them compared again.
- **×** in the panel's header, a click on a commit or a third ⌘-click ends the comparison.

### A file's history and blame

Right-click a file in the right panel (a commit's files, a comparison's or the work in progress's)
for its menu:

- **File History** lists in the right panel the commits that changed the file, newest first, from
  the current commit back, following it through renames: each with how it changed the file (an
  arrow for a rename; rest the pointer on it to see where it was), the subject, the short id, the
  author and the date. Click one to see the file's diff at that commit; click its short id to select
  the commit in the graph. **Show in Log** opens the same history in the Git tool window's Log.
- **Blame** opens the file in the editor with its [blame annotations](#blame); clicking an
  annotation brings the Git tab back with that commit selected.
- **Open File** opens it in the editor.

Blame and Open File are greyed out for a file that is not in the working tree (deleted, or renamed
since).

### Acting on a branch or a tag

Right-click a branch or a tag in the left panel, or the labels at a commit in the graph (with
several branches and tags there, each has its own submenu). What does not apply is greyed out:

- **Checkout *branch*** switches to a local branch; for a remote branch, to the local branch that
  tracks it, created when needed. **Checkout *tag*…** asks first, then detaches HEAD at the tag.
- **Merge *branch* into *current*** and **Rebase *current* onto *branch*** work with local and
  remote branches and with tags. After either, the new HEAD is selected. **Interactive Rebase
  *current* onto *branch*…** opens the current branch's commits in the
  [interactive rebase editor](#interactive-rebase) instead.
- **Fast-forward *branch* to *upstream*** moves a local branch that is behind its upstream (and has
  no commits of its own) up to it, without going online: fetch first to bring in the remote's
  latest commits. On a remote branch, it moves the local branch that tracks it.
- **Push *branch*** pushes a local branch, the current one or not (a branch the remote does not
  have yet is created and tracked). **Push Tag *tag*** sends a tag to `origin`.
- **Set Upstream…** chooses the remote branch a local branch tracks (or none): Pull and Push go
  there, and the ↑ ↓ counts compare with it.
- **Rename *branch*…** asks for the new name; the upstream stays.
- **Delete *branch*** deletes a local branch that is merged (Git refuses one that is not, and says
  so); the current branch cannot be deleted. **Delete *remote branch*…** asks first, then deletes
  the branch on the remote. **Delete Tag *tag*** deletes a tag here; a remote that has it keeps it.

The left panel and the graph show the change at once. Push and deleting a remote branch run in the
background, with their progress in the status bar. If a merge or a rebase stops on conflicts, the
repository is left as Git leaves it and a dialog says what to do, as for a cherry-pick: resolve the
conflicts in the [conflict editor](#merge-conflicts), then **Continue** or **Abort**.

### Drag and drop: merge and rebase

Drag a branch (or a tag) onto another branch to merge or rebase without the menus: pick up its
label in the graph's **Branch / Tag** column, or its row in the left panel, and drop it on another
branch's label or row. The branch it would land on is highlighted while you drag; **⎋** cancels the
drag. On release a menu offers what applies:

- **Fast-forward *target* to *dragged***, only when the branch you dropped on is behind and has no
  commits of its own; it moves without a merge commit (and without a checkout when it is not the
  current branch).
- **Merge *dragged* into *target***, when you drop on a local branch.
- **Rebase *dragged* onto *target***, when you drag a local branch: its own commits are replayed on
  top of the branch you dropped on.
- **Interactive Rebase *dragged* onto *target*…** opens the dragged branch's commits in the
  [interactive rebase editor](#interactive-rebase); the branch is checked out when you start it.

What would do nothing (merging what the branch already has, rebasing a branch that is already on
top) is greyed out, with the reason in its tooltip. A merge into a branch that is not the current
one, or a rebase of a branch that is not the current one, checks that branch out first, so its item
ends in "…" and asks first (**Checkout and Merge**, **Checkout and Rebase**); with uncommitted
changes in tracked files it is refused until you commit or stash them. Conflicts stop the operation
as described above.

### Staging and committing

Select the **WIP** row to commit without leaving the workspace. The panel on the right lists the
**Unstaged Files** (changes Git will not commit yet, new files too) and the **Staged Files** (what
the next commit takes). A file you changed again after staging it is in both lists: the staged part
and the newer, unstaged part.

- **Stage** a file with the **+** that shows when the pointer is on it, or double-click it;
  **Stage All** above the list stages every change. In the staged list, **−** or a double click
  unstages a file, and **Unstage All** unstages everything. When the panel is narrow, **Stage All**
  and **Unstage All** become **+** and **−** icons.
- **Discard** a file's unstaged changes with its bin icon (the bin above the list discards them
  all). It asks first: the file gets back what is staged for it (or its last committed content), and
  an unversioned file is deleted. Staged changes stay. To roll back staged changes too, use
  **Rollback** (⌥⌘Z) in the Commit tool window.
- **Click a file** to see that part of it in the diff tab: what is not staged yet (your version
  against the staged one), or what is staged (the staged version against the last commit). There,
  **Stage Hunk** (or **Unstage Hunk**) and **Discard Hunk…** act on the current difference (move
  with ↑ ↓ or F7, or click a difference). To take single lines, click their line numbers (⇧-click
  takes every changed line in between): the buttons become **Stage *N* Lines** or **Discard *N*
  Lines…**; ⎋ clears the choice. The diff follows at once, also when you stage from the panel.
- Under **Commit**, type a **Summary** (the counter shows its length, in the warning colour past 72
  characters, which many tools cut off) and, if you like, a **Description**. **Commit *N* Files**
  commits exactly what is staged (⌘⏎ in either field does the same); **Commit and Push** pushes
  afterwards. **Amend previous commit** puts the staged changes into the last commit instead, and
  starts from its message; with nothing staged it only changes the message.

The Commit tool window (⌘K) still works: it commits the files you check, whatever is
staged. Both show the same changes and use the same commands.

### Stashes

A stash puts your uncommitted changes aside and leaves the working tree as the last commit has it,
so that you can switch branches or pull; bring the changes back when you need them.

- **Stash** on the toolbar puts every change aside at once, new files too (Git names the stash
  "WIP on *branch*: …"); it is greyed out when nothing is changed. **Pop** brings the newest stash
  back and drops it; it is greyed out when there is no stash.
- In the **WIP** panel, under **Stash Changes**, type a message (optional) and choose **Stash All**
  (⏎ in the message field does the same) or **Stash Staged**, which puts aside only the staged
  changes and leaves the others where they are.
- Right-click a stash in the left panel or its row in the graph:
  - **Apply Stash** brings its changes back and keeps the stash; **Pop Stash** brings them back and
    drops it. The WIP row is selected afterwards. Changes in the way (a file the stash changes that
    you changed too) are refused before anything is touched.
  - **Rename Stash…** gives it a new message; it keeps its place in the list and its changes.
  - **Drop Stash…** asks first, then deletes the stash and its changes.
- Click a stash to see its message, the commit it was made on (click it to go there) and the files
  it changed; click a file for its diff.

If a stash does not apply cleanly, the files are left with conflict markers, Git keeps the stash
(also after Pop), and a dialog names the files: resolve them in the
[conflict editor](#merge-conflicts), then **Continue** (after a Pop, this drops the stash), or
**Abort**, which puts the files back as they were before and keeps the stash.

### Search, filter, Hide and Solo

- **⌘F** in the Git tab opens a search bar over the graph. Type to find commits by their message
  (the first line and the rest), the start of their SHA, or their author's name or e-mail; the
  matching rows are tinted, the matching part of the message, author or SHA is highlighted, and
  the bar shows which match you are on and how many there are ("2/5"). The first match from the
  selected commit down is selected as you type. **↓** or **⏎** goes to the next match, **↑** or
  **⇧⏎** to the previous one (after the last comes the first again); the arrows on the bar do the
  same. **Cc** matches only in the same case and **.\*** takes the text as a regular expression,
  as in the Log's filter. **Escape** or the cross closes the bar; the text is kept for the next ⌘F.
  The search follows the graph: after a commit, a fetch or a Hide it searches again.
- The **Filter** field at the top of the left panel narrows the branches, remotes, tags and stashes
  to those whose names contain the text, in any case (a remote's name lists all its branches, a
  stash matches by its message too). Sections with nothing left are folded away; those with
  matches, and the folders in them, are shown unfolded while you filter, whatever you left them as,
  and fold back when the filter is emptied. **Escape** or the
  cross empties it. The graph is not changed by the filter.
- Point at a branch, a remote branch or a remote in the left panel: an **eye** and a **spotlight**
  appear on its right (also in the branch's right-click menu, as **Hide in Graph** and **Solo**).
  - **Hide** takes the branch out of the graph: its label goes, and so do the commits no other
    branch, tag or HEAD reaches. Hiding a remote hides all of its branches. Click the struck-through
    eye (**Show in Graph**) to bring it back.
  - **Solo** shows only that branch (or remote) and the history it reaches; solo several to see
    them together. Tags at the commits shown keep their labels. Click the spotlight again
    (**Unsolo**) to stop. A hidden branch stays hidden even when soloed.
  - Branches the graph leaves out are greyed in the left panel. Under the filter field the panel
    says how many are hidden and soloed; **Show All** brings everything back.
  - Hide and Solo are kept for the repository, and come back the next time you open it.
- Searching stays quick on long histories: 10 000 commits are searched in a few milliseconds.

### Interactive rebase

The interactive rebase editor rewrites a branch's recent history in a list, in a tab of its own
next to the Git tab. Open it with **Interactive Rebase from Here…** in a commit's menu (the commits
after it), **Interactive Rebase *current* onto *branch*…** in a branch's or tag's menu, or the drop
menu's **Interactive Rebase** (a branch dragged onto another). Nothing changes until you start it.

- The commits are listed the newest on top, over the commit they go onto, each with its message,
  author, date and id. The panel on the right shows the selected commit.
- Choose what happens to each commit with its action button, the panel's list, or a key: **P**
  Pick (keep it), **R** Reword (keep it with a new message), **S** Squash (meld it into the commit
  under it, joining their messages), **F** Fixup (meld it in, keeping that commit's message), **D**
  Drop (leave it out). A melded commit says which one it goes into.
- A reworded commit's message is typed in its row (the summary) or in the panel (the summary and
  the description); **⏎** puts the cursor in its row.
- Reorder commits by dragging a row by its grip, or with **⌥↑** and **⌥↓**; **↑** and **↓** select.
- **Start Rebase** runs it; it is greyed out with the reason when the list cannot run as it is (a
  squash with no commit kept under it). Uncommitted changes in tracked files have to be committed
  or stashed first. **Reset** starts the list over; **Cancel** (or closing the tab) leaves
  everything as it was.
- The same editor cherry-picks several commits: **Cherry-Pick *N* Commits…** lists the commits
  picked with ⌘-click, the oldest at the bottom, to be applied onto the current branch (reword,
  squash, fixup, drop and reorder work the same; **Start Cherry-Pick** runs it).

GoGarden runs Git without ever opening a text editor. If Git stops on conflicts, the Git tab shows
the work in progress with the rebase in progress, the commit it stopped at and the conflicted files:
resolve them in the [conflict editor](#merge-conflicts), then press **Continue** (a reworded commit
keeps its new message); **Abort** puts the branch and the files back as they were before the
rebase. A finished rebase is one operation for [Undo](#undo-and-redo).

### Merge conflicts

When a merge, a rebase, a cherry-pick, a revert, a stash or Update Project stops on conflicts, the
Git tab says so in a banner under its toolbar ("Merging 'feature' into 'main' · 2 conflicted
files"; after Update Project, "Merging 'origin/main' into 'main'": a pull names the remote branch
it merges) with **Resolve…**, **Continue** and **Abort**, and the work in progress lists the conflicted
files first, above the unstaged and staged ones. The Commit tool window (⌘0) lists them in a
**Merge Conflicts** group on top, with **Resolve…**.

- **Resolve…**, a click on a conflicted file in the work in progress, or a double click in the
  Commit tool window opens it in the **conflict editor**, a tab next to the Git tab: **Ours** (the
  branch you are on; during a rebase, where the commits go) on the left, the **Result** in the
  middle, **Theirs** (what is being merged, picked or applied) on the right. Every change of either
  side is lined up across the three panes. Changes only one side made are already in the result;
  conflicts are red and the result has the common ancestor's lines there until you choose.
- **»** next to a change takes ours for it, **«** takes theirs; pressing the other one after that
  takes both (ours, then theirs). **↑**, **↓** (or **F7**, **⇧F7**) go to the next and previous
  conflict; the toolbar says how many are left. The marks by the scroll bar show every change, the
  conflicts in red; click one to go there.
- Click the result's lines to edit them right there; click elsewhere to keep the edit, or press
  **Escape** to drop it. An edited change counts as resolved.
- **Accept Ours** and **Accept Theirs** take one side's whole file and resolve it at once (the
  same is on a file's row in the work in progress, and in its menu in the Commit tool window). A
  file one side deleted, or one that is not text, offers only these.
- **Mark Resolved** writes the result into the file and stages it; with conflicts left it asks
  first. The tab closes and the next file waits in the list. Closing the tab instead writes
  nothing.
- When no file is conflicted, **Continue** finishes the operation: it commits the merge, the
  cherry-pick or the revert with Git's message (one resolved to no change at all is skipped),
  continues the rebase, or, after a stash, drops a popped stash. **Abort** puts everything back as
  it was before the operation (after a stash, the files it touched; the stash is kept). A merge,
  cherry-pick, revert or rebase taken to its end, by Continue or by a commit, is one operation for
  [Undo](#undo-and-redo).

### Undo and Redo

**Undo** on the toolbar takes back the last Git operation; **Redo** does it again. Rest the pointer
on either to see which operation it is ("Undo Commit 'Fix login'"); they are greyed out when there
is nothing to take back or do again. While the Git tab is in front, **⌘Z** and **⇧⌘Z** do the same
(in the tab's text fields they still edit the text).

- What can be undone: checking out a branch, a remote branch (the local branch it made goes away
  again) or a commit; a commit and an amend (their changes come back staged, as they were before
  you committed); a reset (soft, mixed or hard: the changes a hard reset threw away come back);
  creating and deleting a branch (it comes back at its commit, with its upstream); a merge without
  conflicts, a rebase and an interactive rebase (also one continued after conflicts), a
  cherry-pick of one or several commits, a fast-forward and a revert; discarding changes (whole files,
  hunks or lines; unversioned files deleted with them come back too), Rollback, and shelving and
  unshelving (see [Shelf](#shelf)).
- It does not matter where you did it: the Git tab, the Git tool window, the Commit tool window, the
  branches popup or an AI agent. Operations done in a terminal are not recorded; nor are fetch,
  pull, push, stashes and tags.
- Undo never loses your work. If the repository changed since the operation (a commit made in the
  terminal, HEAD on another branch, a file you changed again after discarding), Undo says what
  changed and does nothing. If going back would overwrite uncommitted changes, Git refuses, and the
  operation stays there to undo later. While a merge, rebase, cherry-pick or revert is in progress
  (stopped on conflicts), finish or abort it first.
- The last 100 operations of each repository are remembered, also after you close the project. The
  changes that discard, Rollback and a hard reset threw away are kept as copies in GoGarden's data
  folder until then. A new operation forgets what Redo could have done again.

### Worktrees

A worktree is another working folder of the same repository, on a branch of its own: you can work
on a fix while a feature is half done, without stashing or switching branches. The **WORKTREES**
section lists them: the main one (where the repository's `.git` folder is) first, this project's in
bold with its icon in the current branch's colour, each named by its branch (or its folder when it
is not on a branch), with a lock when it is locked and how many files are changed in it. Rest the
pointer on one to see its folder, its branch, its lock's reason and its changes; click it to jump
to its commit; double-click it to open it.

- **Create Worktree…** in the menu of a local or a remote branch (right-click it in the left panel
  or its label in the graph) asks for a **New branch** (leave it empty to check out the branch
  itself; a remote branch gets a local branch of the same name) and the **Folder** (next to the
  repository's folder, *repository*-*branch*, until you type another), and opens the new worktree
  in a new window unless you clear **Open it in a new window**. A branch can be checked out in one
  worktree only: Git says so if it already is.
- Right-click a worktree for its menu: **Open in New Window** opens its folder as a project in a
  window of its own (also when projects open as tabs); **Lock Worktree** keeps Git from removing or
  pruning it (a worktree on a removable disk), **Unlock Worktree** lets it go again; **Remove
  Worktree…** asks first, says how many changed files are lost with it, and offers to **Delete the
  branch** too. The main worktree and this project's own cannot be removed, nor a locked one.
- Each other worktree with changes is a **WIP** row in the graph right above its commit: click it
  to jump to that commit, double-click it to open the worktree, right-click it for its menu. The
  changes are counted again whenever the repository changes and every few seconds while the Git
  tab is shown.
- Removing a worktree together with its branch can be undone: [Undo](#undo-and-redo) brings the
  branch back (the folder and its changes are gone).

### Submodules

The **SUBMODULES** section lists the repository's submodules by path, each with whether it is at
the commit the repository records for it: a check when it is, a dot in the modified colour when it
is checked out at another commit, a dot in the conflict colour while that commit is in conflict,
and a ring when it is not initialised (not cloned yet). Rest the pointer on one to see that in
words, where it comes from, the recorded commit and the one checked out.

- Right-click a submodule: **Update Submodule** checks the recorded commit out (fetching it when
  needed, and its own submodules too); **Initialise Submodule** clones one that is not initialised
  yet and checks the recorded commit out; **Open as Project** opens its folder in a window of its
  own (double-click does the same). Updating runs in the background with its progress in the status
  bar.

The Git tab and the Git tool window (⌘9) show the same repository; use either.

## Blame

Right-click the editor's gutter and choose **Annotate with Git Blame** (or Git → Annotate with Git
Blame): each line shows the date and the author of the commit that last changed it, shaded by the
commit's age among the file's commits: the newest lines are bright and warm, older ones fade (the
theme's `editor.annotation.recent` and `editor.annotation.old` colours). Rest the pointer
on one to see the commit's id and message; click it to select the commit in the Git tool window
(or in the Git tab, when the annotations were opened with **Blame** from a file's menu there).
Lines you changed since the last commit have no annotation. **Close Annotations** in the same menu
hides them.

## Update Project and Push

- **⌘T** (Git → Update Project…) fetches the current branch's remote branch and merges its commits
  into yours, or rebases yours on top of them: choose in the dialog.
- **⇧⌘K** (Git → Push…) lists the commits to push and where they go (`main → origin : main`); a branch
  the remote does not have yet is marked **New** and is tracked after the push. **Force Push** replaces
  the remote branch, unless it has commits you have not seen: someone pushed to it since you last
  fetched, you fetched their commits but have not brought them into your branch, or a fetch moved it
  while the dialog was open.
- **Fetch** (Git → Fetch) brings in every remote's branches without changing yours.

These run in the background: the status bar shows their progress, and a notification says how they
ended (for example "Pushed 2 commits to origin/main"). GoGarden goes online only when you ask for one
of them.

Checkout, merge, rebase, cherry-pick, revert, reset and commit run in the background too, so a slow
checkout or a commit hook never freezes the window. The status bar shows them, one Git operation runs
at a time, and a failure or a commit's result arrives as a notification.

## For AI agents

Every action is a command (see [AI agents](ai-agents.html)): `vcs.repository.info`, `vcs.status.list`,
`vcs.status.refresh`, `vcs.branch.list`, `vcs.branch.create`, `vcs.branch.checkout`,
`vcs.branch.merge`, `vcs.branch.delete`, `vcs.revision.checkout`, `vcs.project.update`, `vcs.remote.fetch`,
`vcs.push.outgoing`, `vcs.remote.push`, `vcs.operation.status`, `vcs.commit.create`,
`vcs.commit.lastMessage`, `vcs.changes.rollback`, `vcs.diff.file`, `vcs.lineStatus.list` and
`vcs.lineStatus.rollback`; changelists: `vcs.changelist.list`, `vcs.changelist.create` (`name`,
`comment`, `active`, `paths` moved into it), `vcs.changelist.edit` (`name`, `newName`, `comment`,
`active`), `vcs.changelist.delete`, `vcs.changelist.setActive` and `vcs.changelist.move` (`paths`,
`to`), and `vcs.commit.create` with `changelist` commits one list (with `push` and `wait` it
also waits for the push and answers its outcome in `push`); the shelf: `vcs.shelf.list`,
`vcs.shelf.create` (`paths`, or a `changelist`; `name`), `vcs.shelf.diff` (`id`, `path`),
`vcs.shelf.unshelve` (`id`, `paths`, `changelist`, `remove`; fails with the code `conflicts` when
the changes conflict), `vcs.shelf.rename` and `vcs.shelf.delete`; their dialogs open with the
`view.vcs.newChangelist`, `view.vcs.moveToChangelist`, `view.vcs.shelve`, `view.vcs.unshelve` and
`view.vcs.showShelf` actions; patches: `vcs.patch.create` (`source` `changes`, `commit` with
`revision` or `shelf` with `shelf`; `paths`, `changelist`, `output` or `clipboard`, `basePath`,
`reverse`, `encoding`), `vcs.patch.read` (`path`, `text` or `clipboard`; `baseDir`, `strip`,
`reverse`, `encoding`: how each file applies, with `failed` and `approximate` hunks, `executable` and `reason`), `vcs.patch.diff` (the same and `file`),
`vcs.patch.apply` (the same, `paths` and `changelist`), `vcs.shelf.import` (Import to shelf),
`vcs.commit.draft` (the message and author a patch filled in) and `vcs.commit.create` with `author`;
the dialogs open with the `view.vcs.createPatch`, `view.vcs.copyPatch`, `view.vcs.applyPatch`
(`path`) and `view.vcs.applyPatchFromClipboard` actions; `vcs.log.list`, `vcs.commit.details`, `vcs.diff.commit`; the Log's tabs and
filters `vcs.log.view`, `vcs.log.filter`, `vcs.log.select`, `vcs.logTab.new`, `vcs.logTab.close` and
`vcs.logTab.select`, with `vcs.branch.favorite`, `vcs.branch.favorites`, `vcs.branch.update` and
`vcs.commit.cherryPick` (the panels' own toggles are the `view.vcs.log*` actions, Select Folders… is
`view.vcs.logSelectPaths` and Show History of a folder or file `view.vcs.showHistory` with `path`);
Jump to Source is `view.vcs.jumpToSource`: with `paths` (and `line`, from 0) it opens those files,
else the diff tab's file at its caret's line (`side`, `before` or `after`, and `line` put the caret
there first), else the Commit window's selected file; it answers the files `opened`;
the Paths filter's recent entries `vcs.log.recentPaths`; `vcs.blame.annotate` and `vcs.blame.get`
(`path` absolute or relative to the project; each line's `age`, from 0 for the file's newest commit to 100 for its oldest); the Git workspace's `vcs.workspace.open`, `vcs.workspace.close`,
`vcs.workspace.state`, `vcs.workspace.select` (a revision, or `wip: true`), `vcs.workspace.openDiff`,
`vcs.workspace.details` (opens or closes the floating details panel, `details` in the state),
`vcs.workspace.view` (`graphWidth`, `graphStage` `lanes`, `gathered` or `dots`, `graphScroll`,
`detailsWidth`; `graph` in its answer and in the state says the stage, the width and the lines
shown), `vcs.avatar.get` (the authors' pictures by e-mail address: `off`, `pending`, `found` with
the file, or `none`; the setting is `gitAvatars` of `set_settings`),
`vcs.graph.page` (rows of the graph with their lanes, `skip` and `limit`; while a long history is
read, `loading` is true and `total` grows, the rows given stay right while `rowsRevision` stays,
`wait` waits for the end, and `reading` says how soon the first rows came, how long the reading
took and how much memory the graph takes), `vcs.graph.find` and
`vcs.stash.list`; the commit menu's `vcs.workspace.menu` (opens it for a commit) and
`vcs.workspace.commitAction` (runs an item, opening its dialog where it asks first), and the
operations themselves: `vcs.commit.cherryPick` and `vcs.commit.revert` (with `mainline` for a
merge), `vcs.branch.reset` (`mode`: `soft`, `mixed` or `hard`), `vcs.tag.create` (`name`,
`revision`, `message`) and `vcs.commit.copy` (`what`: `sha` or `message`; `clipboard: false` only
answers with the text); the branch and tag menu's `vcs.workspace.refMenu` (opens it for a branch
or a tag, `name` and `kind`: `local`, `remote` or `tag`, or for the labels of a commit,
`revision`) and `vcs.workspace.refAction` (runs an item: `checkout`, `merge`, `rebase`,
`fastForward`, `rename`, `delete`, `setUpstream`, `push`, `deleteTag`), and the operations
themselves: `vcs.branch.merge` (`name` or `source`, and `target` to merge into a local branch that
is not the current one: it is checked out first), `vcs.branch.rebase` (`onto` or `target`, and
`branch` or `source` to rebase a local branch that is not the current one), `vcs.branch.fastForward` (`name`, `onto`),
`vcs.branch.rename` (`name`, `newName`), `vcs.branch.setUpstream` (`name`, `upstream`),
`vcs.branch.deleteRemote`, `vcs.tag.delete`, `vcs.tag.push` (`name`, `remote`), and
`vcs.remote.push` and `vcs.push.outgoing` with `branch` for a branch that is not the current one;
the stashes: `vcs.stash.list`, `vcs.stash.create` (`message`, `stagedOnly`, `includeUntracked`),
`vcs.stash.apply`, `vcs.stash.pop`, `vcs.stash.drop` and `vcs.stash.rename` (`message`), each
naming the stash by `index` (0 for the newest) or `commit`, and the stash menu's
`vcs.workspace.stashMenu` and `vcs.workspace.stashAction` (`apply`, `pop`, `drop`, `rename`; the
last two open their dialog). Stashes are rows of `vcs.graph.page` with `stash` set. Staging:
`vcs.index.stage` and `vcs.index.unstage` take `paths`, `all: true`, or for one file `hunks`
(their places in the diff of `vcs.diff.file` with `part: unstaged` or `part: staged`), `lines`
(new lines) and `removedLines` (old lines); `vcs.changes.discard` takes the same and discards
unstaged changes at once, while `vcs.workspace.discard` asks first in the `discardChanges` dialog;
`vcs.status.list` gives each file's `stagedStatus` and `unstagedStatus`;
`vcs.workspace.openDiff` takes `part` too; and `vcs.commit.create` with `staged: true` commits
what is staged (with `amend` and `push` as before); with `signOff: true` it adds your
`Signed-off-by:` line (Git's `user.name` and `user.email`) at the end of the message. Search: `vcs.graph.search` (`text`, `regex`,
`matchCase`, `step: next|previous`, `close`; answers with `count`, `current`, its row and the
first matches, and selects the match it moves to); Hide and Solo: `vcs.graph.hide` and
`vcs.graph.solo` (`name` and `kind`: `local`, `remote`, or `wholeRemote` for every branch of a
remote; `hidden`/`solo` true or false, or absent to switch; `showAll` and `clear` undo them all),
and `vcs.graph.focus` reads both lists or sets them at once; `vcs.graph.page` leaves out what they
hide and counts it in `hidden`; the left panel's filter: `vcs.workspace.filter` (`text`, answers
with what is listed). Drag and drop: `vcs.workspace.dropMenu` (`source` and `target`, each
`{ "name", "kind" }` with `kind` `local` unless given) opens the menu a drop opens and answers with
its items (`action`, `enabled`, the branch it checks out first in `checkout`);
`vcs.workspace.dropAction` (`source`, `target`, `action`: `fastForward`, `merge`, `rebase`,
`interactiveRebase`) runs one, opening the `checkoutAndMerge` or `checkoutAndRebase` dialog unless
`confirmed: true` (`interactiveRebase` opens the editor and answers `dialog: interactiveRebase`).
Interactive rebase: `vcs.rebase.plan` opens the editor with a plan (`onto` and an optional
`branch`, or `from` a commit of the current branch, or `cherryPick` with commits) and answers with
it: `kind`, `branch`, `checkout`, `onto`, `ontoLabel` and `steps` in the order Git applies them,
the oldest first (each `id`, `subject`, `body`, `author`, `action`, `message`);
`vcs.rebase.edit` (`index` or `id`, with `action`: `pick`, `reword`, `squash`, `fixup`, `drop`,
`message`, `moveTo`) changes a step; `vcs.rebase.reset` and `vcs.rebase.cancel`;
`vcs.rebase.start` runs it (or `steps` given at once, the commits left out dropped);
`vcs.rebase.state` answers with the plan, its `problem` and the rebase `inProgress` (`branch`,
`onto`, `step`, `total`, `stoppedAt`, `stoppedSubject`, `conflicts`); `vcs.rebase.continue` and
`vcs.rebase.abort` take on a rebase that stopped. `vcs.workspace.select` with `toggle: true` picks
commits as ⌘-click does (`picked` in `vcs.workspace.state`); the commit menu's items
`interactiveRebase` and `cherryPickPicked` and the branch menu's `interactiveRebase` open the
editor. `get_window_state` names it `rebaseEditor`. The look: `vcs.workspace.view` (`refsWidth` and `graphWidth` in points,
`resetColumns`, `files`: `path` or `tree`, `allFiles`) answers with the view; `vcs.commit.files`
(`id`) lists every file of a commit; `vcs.workspace.pullMenu` (`open`, true by default) opens or
closes the menu of Pull's ▾. Graph columns: `vcs.workspaceColumns.get` answers with `order` (of
`refs`, `graph`, `message`, `author`, `date`, `sha`), `hidden`, `widths` and `highlightOnHover`;
`vcs.workspaceColumns.set` takes `order`, `show`, `hide` (`author`, `date`, `sha`), `widths`,
`resetWidths`, `highlightOnHover`, `reset` and `set` (everything at once);
`vcs.workspace.columnsMenu` (`open`) opens the titles' menu; `vcs.workspace.hover` (`branch` as
`{ "name", "kind" }`, or `revision`; neither when the pointer leaves) answers with how many rows
are `highlighted`, whether the others `dims`, and the `nearest` branch of a commit. Pull:
`vcs.workspace.pull` (`mode`: `fetchAll`, `fastForward`, `fastForwardOnly` or `rebase`, the
default when absent; `wait`) and `vcs.workspace.pullDefault` (`mode` to choose the default, none
to read it); `vcs.project.update` takes `fastForwardOnly` besides `rebase`.
`vcs.workspace.state` reports `search`, `filter`, `focus`, `dropMenu`, `view` (with `columns`),
`pullMenu`, `pullMode`, `columnsMenu` and `hover`. Undo and Redo: `vcs.history.list` answers with `undo` and
`redo` (the operations they would take back and do again: `id`, `operation`, `title`, `before` and
`after` with `branch` and `head`, the `branches` moved, the `files` kept and git's `reflog` lines)
and the lists `done` and `undone`; `vcs.history.undo` and `vcs.history.redo` (an optional `id`,
refused when another operation is next) answer with the operation and the new `head`, and fail
with the code `moved` when the repository changed since. Worktrees: `vcs.worktree.list` (`refresh`
counts the changes again, `wait` waits for the counts; each worktree's `path`, `name`, `branch`,
`head`, `main`, `current`, `locked`, `lockReason`, `prunable` and `changes`),
`vcs.worktree.create` (`branch`, `path`, `newBranch`, `open`), `vcs.worktree.remove` (`path`,
`deleteBranch`, `force` to lose its changes; asks nothing), `vcs.worktree.lock` (`path`, `locked`,
`reason`) and `vcs.worktree.open` (`path`: opens it in a window of its own, which `list_windows`
then lists); the worktree menu's `vcs.workspace.worktreeMenu` (`path`) and
`vcs.workspace.worktreeAction` (`open`, `lock`, `unlock`, `remove`: the last opens the
`removeWorktree` dialog), and the branch menu's `createWorktree` opens the `createWorktree`
dialog; other worktrees' work in progress are rows of `vcs.graph.page` with `worktree` set.
Submodules: `vcs.submodule.list` (each one's `name`, `path`, `folder`, `url`, `recorded`,
`checkedOut`, `status`: `current`, `moved`, `uninitialized` or `conflicted`, and `atRecorded`),
`vcs.submodule.update` (`paths`, every one when empty; `init` to clone those not initialised;
`recursive`, true by default; `wait`) and `vcs.submodule.open` (`path`); the submodule menu's
`vcs.workspace.submoduleMenu` and `vcs.workspace.submoduleAction` (`update`, `initialize`,
`open`). `vcs.workspace.section` (`section`: `local`, `remote`, `tags`, `stashes`, `worktrees` or
`submodules`; `open`) folds and unfolds the left panel's sections (`openSections` in
`vcs.workspace.state`), and `vcs.workspace.filter` lists the `worktrees` and `submodules` that show
too. A
cherry-pick, a revert, a merge, a rebase or a stash that stops on conflicts fails with the code
`conflicts`. The background operations take `wait: true` to answer with their outcome.
Conflicts: `vcs.conflict.list` answers with the conflicted `files` (`relative`, `kind`:
`bothModified`, `bothAdded`, `deletedByUs`, `deletedByThem`, `addedByUs`, `addedByThem`,
`bothDeleted`) and the `operation` in progress (`kind`: `merge`, `rebase`, `cherryPick`, `revert`,
`stashApply`, `stashPop`; the banner's `title`, `ours`, `theirs`, `step`, `total`, `canContinue`);
`vcs.conflict.open` (`path`, the first file without it) opens the conflict editor, which
`get_window_state` names `conflictEditor`; `vcs.conflict.get` (`path`) answers with the `base`,
`ours` and `theirs` texts, the `result`, and every change with its lines in each (`kind`: `ours`,
`theirs`, `both`, `conflict`; `state`: `auto`, `unresolved`, `ours`, `theirs`, `both`, `edited`);
`vcs.conflict.resolve` takes a side (`take`: `ours`, `theirs`, `both`) for a `change`, the
`changes` or every unresolved one (`conflicts: true`), or alone for the whole file at once,
replaces the result (`text`, or a change's lines with `change`) and with `markResolved` writes and
stages it (refused with the code `unresolved` while conflicts are left, unless `force`);
`vcs.conflict.close` closes the editor. `vcs.operation.continue` and `vcs.operation.abort` go on
with or abort any operation in progress (the same as `vcs.rebase.continue` and `vcs.rebase.abort`
for a rebase).
Comparing: `vcs.diff.commits` (`from`, `to`, the working tree without `to`; `path` for that file's
`diff` too; `show` to show the comparison in the Git tab) answers with the `files` that differ
(`relative`, `status`, `oldPath`); `vcs.workspace.select` with `wip: true, toggle: true` picks the
work in progress (`pickedWip` in `vcs.workspace.state`), and two points picked are compared in the
right panel (`panel` of `vcs.workspace.state`: `{ "kind": "compare", "from", "to" }`); the commit
menu's `compare` item shows them again; `vcs.workspace.openDiff` with `base` opens a file's diff
between `base` and `id` (or the working tree). A file's history: `vcs.file.history` (`path`,
`revision`, `skip`, `limit`; `show` for the right panel, `{ "kind": "fileHistory", "path" }`)
answers with the `commits` that changed it, each with the file's `path` and `relative` there, its
`status` and `oldPath` for a rename, and `more`. A file's menu in the right panel:
`vcs.workspace.fileMenu` (`path`, `id`; `fileMenu` in `vcs.workspace.state`, `get_window_state`
names it `fileMenu`) and `vcs.workspace.fileAction` (`path`, `action`: `history`, `blame`,
`open`).
