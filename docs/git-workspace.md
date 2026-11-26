# The Git workspace

> **Last updated:** 2026-11-11 · **Decisions:** [ADR-0057](adr/0057-git-workspace.md),
> [ADR-0187](adr/0187-git-workspace-on-the-workspace-host.md) ·

The Git workspace is a workspace on the workspace host (Git → **Git Workspace**, or its button at
the top of the right stripe; ADR-0187): it expands over the editor area with its graph as the
pinned first tab, a diff, the interactive rebase editor and the conflict editor as more inner tabs,
and a **Git** tab with × in the editor strip while it is open. It is for working with a repository
as a whole: the commit graph of every branch, the branches, remotes, tags and stashes, a commit's details
and the work in progress, and step by step the Git operations around them. Its UI is ours: our
layout, our icons, the theme's colours. It is built on the platform's Git engine, which the Git tool window (⌘9), the
Commit tool window, the branch popup and the diff viewer share.

This document is the **specification** of the workspace: what it does, what Zen IDE already has,
the order in which the rest is delivered, and what is left out.

## Clean room

ADR-0002 and the clean-room ADR apply. The layout and behaviour are specified in
this document only, from our own design and public Git documentation; no screenshot of another
product is kept as a reference. Names of Git operations (merge, rebase, cherry-pick, stash) are
Git's own words.

## The look

The tab's layout is ours: what it shows where, the proportions and the hierarchy of the type are
set out below. Every icon is drawn by us (`ide/glyphs.rs`), every colour is the theme's
(`IdePalette`, tints mixed from two of its colours in `git_workspace/look.rs`), every word is ours,
in Fluent.

| Part | What it is |
|---|---|
| Toolbar | From the left, caption-over-icon buttons all of one width (the project and the branch are the IDE title bar's, not repeated here): Undo, Redo (dimmed when there is nothing to take back or do again; their hints name the operation: "Undo Commit 'Fix login'"), Pull with a ▾ in its lower right corner, inside its width (Fetch All and the three pull modes, the default marked: `vcs.workspace.pullMenu`), Push, Branch, Stash, Pop; Search on the right; every hint names its shortcut. In a narrow tab the buttons keep their icons only. |
| Graph | Each row tinted in its lane's colour from its node to the right edge, stronger under the pointer and strongest when selected, with a mark of the lane's colour at the message's edge; lanes 2 points wide turning in rounded elbows; the first label of a commit filled in its lane's colour, with a check for the current branch, a laptop for local, a cloud for remote (one label for a local branch and its remote branch at the same commit) and a tag's mark, then "+N"; the WIP row's dashed ring; titles in small capitals; the columns moved by dragging their titles, resized by dragging their borders, switched by the titles' menu (`vcs.workspaceColumns.set`); rows outside a hovered branch's history dimmed, a hovered commit's nearest branch an outlined label. |
| Left panel | "Viewing N" (branches, tags, stashes, worktrees and submodules listed), the filter (⌥⌘F), sections LOCAL, REMOTE, TAGS, STASHES, WORKTREES and SUBMODULES in small capitals with an icon and a count, all folded at first (`vcs.workspace.section`), branches in folders by their `/`. |
| Right panel | Floating over the graph from the right while something is selected (×, ⎋ or a click on the graph's empty space close it; its left edge resizes it). A banner while the working tree has changes, with View Changes; "commit: id"; the subject large and the body under it in a frame; the author's picture or initials, name and "authored" date, the parents as links; "n modified, n added, n deleted"; Path or Tree and View all files (`vcs.workspace.view`, `vcs.commit.files`); a file's folder dimmed and its name bright. |

Left out, by rule 10 or until a stage brings it: accounts and cloud features (cloud patches, pull
requests, issues, teams), an "explain commit" feature (an opt-in AI feature, later at the earliest)
and co-authors. Authors' pictures come from hosting services while the setting is on
(on by default, ADR-0074 and ADR-0075).

## Features

"Have" means Zen IDE has it today (in the workspace or elsewhere in the IDE, through the same
commands); "Stage" is the step that brings it to the workspace (see [Delivery order](#delivery-order)).

### Layout and the commit graph

| Feature | Have | Stage |
|---|---|---|
| A repository opens as a tab; a toolbar on top, a left panel, the graph in the middle, a commit panel on the right | Yes: the Git workspace tab with the same three panels and a toolbar | 1 |
| The graph: one row per commit, newest on top, lines for branches and merges, each branch in its own colour | Yes: lanes over the whole history, colours of the theme's graph palette | 1 |
| Columns: branch/tag labels, graph, message; author, date and SHA can be switched on | Yes: all six, in any order; author, date and SHA switched on and off from the titles' menu, and giving way when the tab is narrow | 1; Graph columns (done) |
| Branch and tag labels at their commits, joined to the commit's node | Yes, with "+N" when there are more and the full list on hover | 1 |
| Commit nodes show the author's avatar | Yes, while the setting is on (on by default; Settings → Version Control → Git Workspace, ADR-0075): from the repository's GitLab, a GitHub no-reply address or Gravatar, kept on disk; otherwise and offline the author's initials in a circle of a colour from the theme (ADR-0074) | 1; Avatars (done) |
| The commit panel slides over the graph from the right, closing to give the graph the whole width | Yes: opens on a selection, closes with ×, ⎋ or a click on the graph's empty space; its left edge resizes it, kept per repository | Avatars (done) |
| A narrowed graph column gathers the nodes into one line | Yes: the lanes that fit stay, the others' nodes gather into a line of dots in their lanes' colours (the lanes scroll sideways); at the narrowest only the dots, and a graph icon for the title | Avatars (done) |
| A WIP node on top when the working tree has changes | Yes: the WIP row with the number of changed files | 1 |
| A node per stash | Yes: a row of its own right above the commit it was made on, in a lane of its own, joined to that commit | Stashes (done) |
| Scrolling histories of any length | Yes: the whole history is laid out in the background as it is read, the first rows at once and the rest below them; rows are drawn and read only in view (200 000 commits: first rows in about 20 ms, all in under a second in a release build, under 300 bytes a commit) | 1; Very large histories (done) |
| Hovering a branch highlights its commits and fades the others; the nearest branch of a commit shows on hover | Yes, a setting (Highlight Branch on Hover), for labels and the left panel's rows; the nearest branch: of those that have the commit, the one fewest commits above it | Graph columns (done) |
| Search commits by message, SHA or author; matches highlighted as you type | Yes: ⌘F in the tab, the Log's text rules (plain, Match Case, regex), ↑/↓ between matches, the count; 12 000 commits searched in milliseconds | Search (done) |
| Hide a branch from the graph, or "solo" some branches to show only them | Yes: for branches, remote branches and whole remotes, kept per repository | Search (done) |
| Column widths, order and choices saved per repository | Yes: kept per repository in `workspace.json` | Graph columns (done) |

### The left panel

| Feature | Have | Stage |
|---|---|---|
| Sections Local, Remote (by remote), Tags, Stashes, Worktrees and Submodules, with counts; sections fold | Yes for Local, Remote, Tags, Stashes, Worktrees and Submodules (`vcs.workspace.section` folds them) | 1; Worktrees, Submodules (done) |
| Click a branch to jump to its commit | Yes | 1 |
| Double-click a branch to check it out (a remote one: a local branch tracking it) | Yes (`vcs.branch.checkout`) | 1 |
| Ahead/behind counts of a branch | Yes (↑ ↓ next to local branches) | 1 |
| Branch context menu: checkout, rename, delete, merge, rebase, set upstream, push, pin | Yes, and fast-forward to the upstream, on local and remote branches, in the left panel and on the graph's labels; deleting a remote branch asks first; pin not yet | Branch actions (done) |
| Tags: create, delete, push, checkout | Yes: create from the commit menu; checkout (asks first), merge, rebase, push and delete from the tag menu | Commit actions, Branch actions (done) |
| Stash context menu: apply, pop, delete, rename, hide | Yes for Apply, Pop, Drop (asks first) and Rename, in the left panel and on the graph's rows; hide not yet | Stashes (done) |
| Filter the left panel by name | Yes: a filter field on top of the panel (branches, remotes, tags, stashes by name or message) | Search (done) |

### Commit details and the work in progress

| Feature | Have | Stage |
|---|---|---|
| A commit's summary, description, author, date, SHA, parents and changed files | Yes; parents are links | 1 |
| Click a file to see its diff; hunk, inline and split views | Yes: the diff tab, side by side or unified | 1 |
| The WIP panel: unstaged and staged files, a summary and a description, Commit | Yes: both lists (a file changed after staging is in both), the summary with a character count, the description, Commit and Commit and Push of what is staged; Stash with a message (all changes or the staged ones) | 1; Stashes (done); Staging (done) |
| Stage and unstage files, hunks and lines; discard | Yes: a file (its button or a double click) or all of them in the panel; a hunk or the lines chosen by their numbers in the diff tab; discarding asks first | Staging (done) |
| Amend the last commit | Yes, in the panel and in the Commit tool window | Staging (done) |
| Compare two commits, or a commit with the WIP | Yes: ⌘-click two commits (or a commit and the WIP row) and the right panel lists the files that differ, each opening its diff between the two (from where it was, when renamed in between); Compare in the commit menu | Compare (done) |
| File history and blame (colour per author) | Yes: History in a file's menu in the right panel lists the commits that changed it through renames, each opening its diff there (Show in Log for the Log's tab); Blame opens the file with the gutter's annotations, shaded by the commit's age rather than coloured per author, and a click on one selects its commit in the workspace | Compare (done) |

### Operations from the graph

| Feature | Have | Stage |
|---|---|---|
| Commit context menu: create branch here, create tag here, checkout (detached), cherry-pick, revert, reset (soft, mixed, hard), copy SHA | Yes, and Copy Message; checkout and hard reset ask first; conflicts are left as git leaves them, with a dialog saying what to do | Commit actions (done) |
| Drag a branch onto another: merge, rebase, fast-forward | Yes: a label in the graph or a row of the left panel dropped on another branch (highlighted while dragging, ⎋ cancels) offers Fast-forward (when possible), Merge, Rebase and Interactive Rebase (the editor); a merge into or a rebase of a branch that is not checked out asks first | Drag and drop (done) |
| Interactive rebase: pick, reword, squash, drop, reorder; start it from a branch or a commit; reset to start over | Yes: an editor tab in the workspace's look, from a commit's menu, a branch's menu or the drop menu; Pick, Reword, Squash, Fixup, Drop (P, R, S, F, D), dragging or ⌥↑/⌥↓, the message typed in place; Start, Reset, Cancel; conflicts hand over to the work in progress with Continue and Abort (ADR-0065) | Interactive rebase (done) |
| Cherry-pick several commits with the same editor | Yes: commits picked with ⌘-click, then Cherry-Pick *N* Commits… | Interactive rebase (done) |
| Undo and redo the last operation (checkout, commit, discard, delete branch, reset, rebase...) | Yes: checkout, commit, amend, reset, branch create and delete, merge, rebase, cherry-pick, revert, fast-forward, discard and rollback, from the workspace, the Git and Commit tool windows and agents alike; ⌘Z and ⇧⌘Z in the tab; refused with a reason when the repository moved since; discarded changes kept in the data folder (ADR-0064) | Undo (done) |

### Remotes and the toolbar

| Feature | Have | Stage |
|---|---|---|
| Fetch, Pull (fast-forward if possible, fast-forward only, rebase; a default), Push, Branch, Stash, Pop | Yes: Fetch All and Pull fast-forwarding if possible, fast-forward only or rebasing from Pull's ▾, the default chosen there and kept per repository; Push (with force), Branch, Stash and Pop, each with a shortcut | 1; Stashes (Stash, Pop); The look (Pull's ▾); Toolbar (done) |
| Progress of long operations | Yes: the status bar and a notification | 1 |

### Conflicts

| Feature | Have | Stage |
|---|---|---|
| Conflicted files listed in the WIP panel; a merge tool with your side, their side and the output; take a side for a hunk or a whole file; save and commit | Yes: listed first in the work in progress (and in the Commit tool window's Merge Conflicts group), a banner while an operation is in progress with Resolve…, Continue and Abort; the conflict editor tab with ours, the result (editable) and theirs lined up, » and « per change (both when pressed in turn), Accept Ours and Accept Theirs for the file, Mark Resolved; Continue and Abort for a merge, rebase, cherry-pick, revert and stash (ADR-0072) | Conflicts (done) |
| Warn about conflicts with other branches before they happen | No | Later (needs a design) |

### Repository-level features

| Feature | Have | Stage |
|---|---|---|
| Worktrees: create from a branch, open, remove, lock; a WIP node per worktree | Yes: Create Worktree… from a branch's menu, Open in New Window, Lock and Unlock, Remove (asks first, with its branch when chosen, which Undo brings back); a WIP row per other worktree with changes, above its commit | Worktrees (done) |
| Submodules: list, add, update, open | Yes for the list (whether each is at the recorded commit), Update, Initialise and Open as Project; adding one not yet | Submodules (done) |
| LFS, hooks, signing | Hooks and signing work already, because writes run the user's `git` (ADR-0047) | Later |
| Gitflow helpers | No | Later |

## Delivery order

The stages are done in this order; each adds commands first
(ADR-0033), then the UI, the guide and the changelog.

1. **Stage 1 (done):** the tab, the graph, the left panel, commit details, the WIP row.
2. **Commit actions from the graph (done):** the commit menu,
   `vcs.workspace.menu` and `vcs.workspace.commitAction`, and the operations `vcs.commit.cherryPick`,
   `vcs.commit.revert`, `vcs.branch.reset`, `vcs.tag.create` and `vcs.commit.copy`.
3. **Branch and tag actions in the left panel and on the graph's labels (done,****):** the branch and tag menu, `vcs.workspace.refMenu` and
   `vcs.workspace.refAction`, and the operations `vcs.branch.rebase`, `vcs.branch.fastForward`,
   `vcs.branch.rename`, `vcs.branch.setUpstream`, `vcs.branch.deleteRemote`, `vcs.tag.delete` and
   `vcs.tag.push`; `vcs.branch.merge` stops on conflicts as a cherry-pick does, and
   `vcs.remote.push` takes any local branch.
4. **Stashes (done):** Stash and Pop on the toolbar, Stash with a
   message (all changes or the staged ones) in the WIP panel, the stash menu (Apply, Pop, Drop, Rename),
   `vcs.workspace.stashMenu` and `vcs.workspace.stashAction`, stash rows in the graph, and the
   operations `vcs.stash.create`, `vcs.stash.apply`, `vcs.stash.pop`, `vcs.stash.drop` and
   `vcs.stash.rename`; a stash that does not apply cleanly stops on conflicts as a cherry-pick does.
5. **Staging and committing in the WIP panel (done):** the
   unstaged and staged lists (`stagedStatus` and `unstagedStatus` of `vcs.status.list`), Stage,
   Unstage and Discard of files in the panel and of hunks and lines in the diff tab (`part` of
   `vcs.diff.file` and `vcs.workspace.openDiff`), the commit form, and the operations
   `vcs.index.stage`, `vcs.index.unstage`, `vcs.changes.discard` (asked first through
   `vcs.workspace.discard`) and `vcs.commit.create` with `staged`. Partial staging writes the new
   staged text into the index (`zen_vcs::diff::combine`), so it never depends on patches applying.
6. **Search, filter, hide and solo (done):** the search bar
   (⌘F) over the graph with `vcs.graph.search`, matching with the Log's rules
   (`zen_vcs::filter::TextMatcher`, shared with `vcs.log.filter`) over texts folded once when the
   graph is laid out; the left panel's filter (`vcs.workspace.filter`); Hide and Solo of branches,
   remote branches and whole remotes (`vcs.graph.hide`, `vcs.graph.solo`, `vcs.graph.focus`),
   which lay the graph out again from the history already read and are kept per repository in
   `workspace.json`.
7. **Drag and drop: merge, rebase, fast-forward (done):** a branch or tag
   label dragged from the graph or the left panel onto another branch opens the drop menu
   (`vcs.workspace.dropMenu`, `vcs.workspace.dropAction`): Fast-forward, Merge, Rebase, Interactive
   Rebase (greyed out until its stage); `vcs.branch.merge` takes a `target` and `vcs.branch.rebase` a
   `branch` that is not the current one, checked out first after asking (`zen_vcs` `merge_into`,
   `rebase_branch`), and refused while tracked files have changes.
8. **Undo and redo of Git operations (done):** the toolbar's
   Undo and Redo and ⌘Z/⇧⌘Z in the tab run `vcs.history.undo` and `vcs.history.redo`;
   `vcs.history.list` lists the journal. The commands that change the repository record what they
   did (`zen_workbench::vcs::history`): `HEAD` and every local branch before and after, git's
   reflog lines, and for discard, rollback and a hard reset the files' contents, kept in the data
   folder. The engine (`zen_vcs` `ref_state`, `move_refs`, `file_states`, `restore_files`) moves
   back with `git reset --keep` or `--soft` and refuses when the repository moved since.
   [ADR-0064](adr/0064-undo-git-operations.md) says what can be undone and how.
9. **Interactive rebase and cherry-picking several commits (done):**
   the editor tab (`git_workspace/rebase.rs`) with `vcs.rebase.plan`, `vcs.rebase.edit`,
   `vcs.rebase.reset`, `vcs.rebase.cancel`, `vcs.rebase.start`, `vcs.rebase.state`, and
   `vcs.rebase.continue` and `vcs.rebase.abort` for a rebase stopped on conflicts, shown in the
   work in progress panel; ⌘-click picks commits (`vcs.workspace.select` with `toggle`). The engine
   (`zen_vcs` `rebase_commits`, `interactive_rebase`, `rebase_continue`, `rebase_abort`,
   `rebase_progress`) runs `git rebase -i` with a prepared todo list and never an editor
   ([ADR-0065](adr/0065-interactive-rebase-without-an-editor.md)); a rebase that stopped is recorded
   for Undo once continued to its end.
10. **The merge conflict editor (done):** the conflict
    editor tab (`git_workspace/conflicts.rs`, drawn with the diff tab's highlighting, change marks
    and sideways scroll) with `vcs.conflict.list`, `vcs.conflict.open`, `vcs.conflict.get`,
    `vcs.conflict.resolve` and `vcs.conflict.close`, the banner and the work in progress's
    operation box with `vcs.operation.continue` and `vcs.operation.abort` for every operation (the
    interactive rebase's box generalised), the Commit tool window's Merge Conflicts group and
    Resolve… in the conflicts dialog (also after Update Project). The engine reads git's index
    stages (`zen_vcs` `conflicts`, `blob`), merges three ways itself for the panes
    (`zen_vcs::merge`, as git's merge: overlapping or touching changes conflict, the lines both
    sides share taken out) and resolves, continues and aborts through git (`resolve_with`,
    `resolve_with_side`, `continue_operation`, `abort_operation`); a stash that stopped keeps the
    files it touched in the Undo journal for Abort. A merge, cherry-pick, revert or rebase that
    stopped is recorded for Undo once continued (or committed) to its end
    ([ADR-0072](adr/0072-merge-conflict-editor.md)).
11. **Compare commits; file history and blame in the workspace (done):**
    `vcs.diff.commits` (two commits, or a commit and the working tree: the files that differ, and a
    file's diff), `vcs.file.history` (the commits that changed a file, following renames; a merge
    that took the file from one side is left out, as git's history simplification does), the
    right panel's comparison and file history (`panel` of `vcs.workspace.state`), the work in
    progress picked with ⌘-click (`vcs.workspace.select` with `wip` and `toggle`), Compare in the
    commit menu, `vcs.workspace.openDiff` with `base`, and a file's menu in the right panel
    (`vcs.workspace.fileMenu`, `vcs.workspace.fileAction`: History, Blame, Open File). The engine
    (`zen_vcs` `diff_revisions`, `file_at`, `file_history`) reads through gitoxide; the working
    tree is compared file by file with the commit, a file moved counted once.
12. **Graph columns and view settings (done):** the column
    titles dragged to reorder the columns, every border but the message's dragged to resize them,
    the titles' menu (Author, Date, SHA, Highlight Branch on Hover, Reset Columns) as
    `vcs.workspace.columnsMenu`, all of it `vcs.workspaceColumns.get` and `vcs.workspaceColumns.set`
    (command ids have three parts, hence not `vcs.workspace.columns.get`), kept per repository in
    `workspace.json` with what is hidden and soloed. Hovering a branch (a label or a row of the
    left panel) highlights the rows of its history and dims the others; hovering a commit without
    labels shows its nearest branch, the one fewest commits above it; both are
    `vcs.workspace.hover`, worked out from the graph read (`HistoryStore::ancestors` and
    `distances_to`), never by asking git.
13. **Toolbar: pull modes (done):** Pull's ▾ lists Fetch All,
    Pull (fast-forward if possible), Pull (fast-forward only) and Pull (rebase), each
    `vcs.workspace.pull` with its `mode`; the mark beside one makes it what Pull (and ⌘T in the tab)
    does, `vcs.workspace.pullDefault`, kept per repository. `vcs.project.update` gained
    `fastForwardOnly`; the engine spells the mode out to git (`zen_vcs::pull_args`), so that
    `pull.rebase` and `pull.ff` do not change it. Every toolbar button's hint names its shortcut:
    ⌘Z, ⇧⌘Z, ⌘T, ⇧⌘K, ⌥⌘N, ⌥⇧⌘S (Stash, `vcs.stash.create`), ⌥⇧⌘P (Pop, `vcs.stash.pop`), ⌘F.
14. **Worktrees (done):** the WORKTREES section and its menu
    (`vcs.workspace.worktreeMenu`, `vcs.workspace.worktreeAction`), Create Worktree… in the branch
    menu, and the operations `vcs.worktree.list`, `vcs.worktree.create`, `vcs.worktree.remove`,
    `vcs.worktree.lock` and `vcs.worktree.open`; each other worktree with changes is a row of the
    graph above its commit (`worktree` in `vcs.graph.page`), its changes counted in the background
    (`zen_vcs` `worktrees`, `add_worktree`, `remove_worktree`, `lock_worktree`, `worktree_changes`).
    [ADR-0070](adr/0070-worktrees-and-submodules.md) says how they are read, opened and undone.
15. **Submodules (done):** the SUBMODULES section and its menu
    (`vcs.workspace.submoduleMenu`, `vcs.workspace.submoduleAction`) with `vcs.submodule.list`,
    `vcs.submodule.update` (with `init`, in the background) and `vcs.submodule.open` (`zen_vcs`
    `submodules`, `update_submodules`); with the two stages came `vcs.workspace.section`, which
    folds the left panel's sections.
16. **Very large histories (done):** the first rows before the
    whole history is read. `Repository::read_history` walks every branch, tag and `HEAD` newest
    first and hands each commit over as it is read; the workspace adds them in batches (the first
    after 500 commits or 30 ms, then every 100 ms) to a compact store (`zen_vcs::history::HistoryStore`:
    20-byte ids, authors once each, the messages in one block of text) and lays them out as they
    come (`zen_vcs::graph::Layout`, a row never changes once laid out; rows packed in
    `zen_vcs::graph::PackedRows`). A page's rows are made when asked for. `vcs.graph.page` says
    `loading` while rows come in, `rowsRevision` (raised only when rows given may change) and
    `reading` (first rows, whole reading, memory). A later reading (after a branch moved) replaces
    the graph only when it is done. Should the walk by date meet a commit after one of its parents
    (a clock set wrong), the graph is laid out once more, children first, at the end. The branches
    and tags that contain a commit (`vcs.commit.details`) come from the graph read
    (`HistoryStore::descendants`) instead of `git for-each-ref --contains`, which takes most of a
    second on such a history. The benchmark and its numbers are in
    [performance.md](performance.md#the-git-workspace-on-very-large-histories): 200 000 commits,
    first rows in about 20 ms, all in under a second, under 300 bytes a commit.

Between the stages, the tab took **its own look (done)**,
described in [The look](#the-look): with it came `vcs.workspace.view` (column widths, Path or Tree,
View all files), `vcs.commit.files` (every file of a commit, `Repository::commit_files`) and
`vcs.workspace.pullMenu` (Pull's ▾). The "Graph columns" stage saved the widths per repository and
added reordering and switching the columns; the toolbar stage brought the pull modes and the
default.

## Out of scope

Golden rule 10 (no accounts, no telemetry, no network by default) and our product boundaries leave
these out of the workspace:

- **Accounts, cloud and sign-in**: accounts, cloud workspaces, team views, profiles synced through
  a service, deep links into a hosted service.
- **Hosting services and issue trackers**: pull requests, GitHub/GitLab/Bitbucket/Azure
  integrations, issue trackers, CI services. They need tokens and network access; if wanted, they
  come later as an opt-in plug-in with its own ADR.
- **Built-in AI features** (commit messages, explanations, "commit composer"): in Zen IDE the agent
  is the user's own, and it drives the workspace through MCP like any other feature.
- **Telemetry** of any kind, and anything that goes online without the user asking (Fetch, Pull
  and Push are explicit).

## The next stages

The stages follow in this order. Every stage's acceptance criteria also include: every action a command
with typed parameters reachable over MCP (ADR-0033), Fluent messages in English and Russian, colours
only from the theme, tests for the commands, the guide page (docs/guide/14-version-control.md) and
the changelog.

1. **vcs: Commit actions in the Git workspace**
   - Goal: act on a commit from the graph's context menu.
   - Acceptance criteria: right-click a commit → Create Branch Here, Create Tag Here, Checkout
     (detached, with a confirmation), Cherry-pick, Revert, Reset current branch to here (Soft,
     Mixed, Hard, Hard asks first), Copy SHA, Copy Message; commands `vcs.commit.cherryPick`,
     `vcs.commit.revert`, `vcs.branch.reset`, `vcs.tag.create`; a failure (conflict) leaves the
     repository as git does and says what to do; tests on temp repositories for each operation.
2. **vcs: Branch and tag actions in the Git workspace**
   - Goal: manage branches and tags from the left panel and the graph's labels.
   - Acceptance criteria: context menus on a branch (local and remote) and a tag: Checkout, Merge
     into Current, Rebase Current onto, Fast-forward to upstream, Rename, Delete (local, remote with
     a confirmation), Set Upstream, Push (branch, tag), Delete Tag; commands `vcs.branch.rename`,
     `vcs.branch.rebase`, `vcs.branch.setUpstream`, `vcs.tag.delete`, `vcs.tag.push`; the left panel
     and the graph follow at once.
3. **vcs: Stashes in the Git workspace**
   - Goal: put work aside and bring it back from the workspace.
   - Acceptance criteria: Stash and Pop on the toolbar; Stash with a message from the WIP panel
     (all files or the staged ones); a stash's menu: Apply, Pop, Drop (asks first), Rename; stash
     nodes in the graph next to the commit they were made on; commands `vcs.stash.create`,
     `vcs.stash.apply`, `vcs.stash.pop`, `vcs.stash.drop`, `vcs.stash.rename`.
4. **vcs: Staging and committing in the WIP panel**
   - Goal: commit from the workspace without leaving it.
   - Acceptance criteria: stage and unstage a file (button, double click, all), a hunk and lines
     from the diff tab; discard a file or a hunk (asks first); summary and description fields with
     a character counter, Amend, Commit and Commit and Push; staged state read from the index
     (staged and unstaged parts of one file both listed); commands `vcs.index.stage`,
     `vcs.index.unstage`, `vcs.changes.discard`, and `vcs.commit.create` with staged-only commits.
5. **vcs: Search and filter in the Git workspace**
   - Goal: find commits and focus the graph.
   - Acceptance criteria: a search field (⌘F in the tab) matching message, SHA and author, matches
     highlighted, ↑/↓ to the previous and next match, the count shown; a filter field for the left
     panel; Hide and Solo for branches and remotes (kept per repository); commands
     `vcs.graph.search`, `vcs.graph.hide`, `vcs.graph.solo`; searching 10 000+ commits answers in
     under 200 ms in a release build.
6. **vcs: Drag and drop merge and rebase in the graph**
   - Goal: merge or rebase by dragging one branch label onto another.
   - Acceptance criteria: dragging a branch label (graph or left panel) onto another branch shows
     a menu: Fast-forward (when possible), Merge, Rebase, Interactive Rebase (after that stage);
     the drop target is highlighted; Escape cancels; the same through commands with typed
     parameters (`vcs.branch.merge` with source and target, `vcs.branch.rebase`).
7. **vcs: Undo and redo Git operations**
   - Goal: take back the last Git operation and do it again.
   - Acceptance criteria: Undo and Redo on the toolbar (⌘Z and ⇧⌘Z while the tab has the focus)
     for checkout, commit, amend, reset, branch create and delete, merge without conflicts, rebase,
     cherry-pick, discard of tracked changes; each recorded with what it changed (refs before and
     after, from the reflog), refused with a reason when the repository moved since; commands
     `vcs.history.undo`, `vcs.history.redo`, `vcs.history.list`; an ADR records what can be undone.
8. **vcs: Interactive rebase editor**
   - Goal: rewrite a branch's recent history in a list.
   - Acceptance criteria: start from a branch (onto another; also the drop menu's Interactive Rebase,
     greyed out until then) or a commit (its descendants on the current branch); a list of
     commits with Pick, Reword, Squash, Drop (keys P, R, S, D) and
     reordering by dragging or ⌥↑/⌥↓; Start runs it, Reset starts over, Cancel leaves everything as
     it was; conflicts hand over to the conflict editor and Continue/Abort; the same editor for
     cherry-picking several commits; commands `vcs.rebase.plan`, `vcs.rebase.start`,
     `vcs.rebase.continue`, `vcs.rebase.abort`.
9. **vcs: Merge conflict editor**
   - Goal: resolve conflicts of a merge, a rebase, a cherry-pick or a stash.
   - Acceptance criteria: conflicted files listed first in the WIP panel; a three-pane editor tab
     (ours, theirs, the result, editable) with each conflict's hunks; take ours, theirs or both per
     hunk and per file; Mark Resolved; Continue and Abort of the operation in progress; a banner on
     the workspace while one is in progress; commands `vcs.conflict.list`, `vcs.conflict.get`,
     `vcs.conflict.resolve`, `vcs.operation.continue`, `vcs.operation.abort`.
10. **vcs: Compare commits, file history and blame in the workspace**
    - Goal: see what changed between any two points and the story of a file.
    - Acceptance criteria: ⌘-click two commits (or a commit and WIP) to list the files that differ
      and open their diffs; a file's History (the commits that changed it, with its diff at each)
      and Blame (lines coloured per commit, click to select the commit) from a file's menu in the
      right panel; commands `vcs.diff.commits`, `vcs.file.history`.
11. **vcs: Graph columns and view settings**
    - Goal: arrange the graph as the user likes it.
    - Acceptance criteria: resize and reorder the columns, switch Author, Date and SHA on and off
      (the header's menu), saved per repository in the project settings; hovering a branch label
      highlights that branch's commits and dims the others (a setting); the nearest branch of a
      hovered commit is shown; commands `vcs.workspace.columns.get`/`set`.
12. **vcs: Pull modes, Stash and Pop on the workspace toolbar**
    - Goal: choose how Pull works, and stash, from the toolbar.
    - Acceptance criteria: Pull with a menu (fast-forward if possible, fast-forward only, rebase,
      Fetch All) and a remembered default; Stash and Pop buttons (done with the Stashes stage);
      Undo and Redo after that stage;
      every button with its tooltip and shortcut; commands extended with the pull mode.
13. **vcs: Worktrees in the Git workspace**
    - Goal: work on several branches at once.
    - Acceptance criteria: a Worktrees section in the left panel; Create Worktree from a branch,
      Open (in a new project window), Remove (and delete its branch, asks first), Lock and Unlock;
      a WIP row per worktree with changes; commands `vcs.worktree.list`, `vcs.worktree.create`,
      `vcs.worktree.remove`, `vcs.worktree.lock`.
14. **vcs: Submodules in the Git workspace**
    - Goal: see and update submodules.
    - Acceptance criteria: a Submodules section with each submodule's path and whether it is at the
      recorded commit; Update, Initialise, Open (as a project); commands `vcs.submodule.list`,
      `vcs.submodule.update`.
15. **vcs: Very large histories in the Git workspace**
    - Goal: open repositories with hundreds of thousands of commits at once.
    - Acceptance criteria: the first rows appear before the whole history is read (the graph is
      laid out as it is read, in the background); reading 200 000 commits does not block the
      window and takes under 5 s in a release build on a recent laptop; memory per commit measured
      and kept under 1 KB; a benchmark in the tests (ignored by default).
16. **vcs: Avatars, a floating details panel and a collapsing graph (done)**
    - Authors' pictures on the nodes and in the details panel, from the repository's GitLab, a
      GitHub no-reply address or Gravatar, kept in the data folder, only while the setting is on
      (ADR-0074, `vcs.avatar.get`); the details panel floats over the graph from the right, opens on
      a selection, closes with ×, ⎋ or a click on the graph's empty space, and is resized by its
      left edge (`vcs.workspace.details`, `detailsWidth` of `vcs.workspace.view`, kept per
      repository), never wider than the graph's area so that it and its shadow stay off the left
      panel in a narrow window; the graph column is resized by its border along the rows, and
      narrower than its
      lanes it gathers the nodes of the lanes that do not fit into one line of dots coloured by
      their lane, the lines of those lanes going with them and the lines joining them running into the dots (the lanes scroll sideways with a scroll bar), at the narrowest only the dots and a
      graph icon for its title (`graphStage`, `graphScroll` of `vcs.workspace.view`, `graph` of
      `vcs.workspace.state`).
