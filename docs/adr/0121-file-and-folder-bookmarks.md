# ADR-0121: Bookmarks of files and folders share the line bookmarks' lists; the workspace file keeps them apart from the lines

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0109](0109-bookmark-lists.md)

## Context

The requirement: bookmarks of whole files and folders as well as of lines, from the Project
tool window (F3, ⌥F3, a Bookmarks › submenu, on several rows at once) and from an editor tab, and
Bookmark Open Tabs… makes a list of every open tab. These bookmarks sit in the same lists as the
line bookmarks (ADR-0109) and are kept with the project. A `workspace.json` written by this build
must not cost an older build its line bookmarks.

## Decision

1. A bookmark of `zen_workbench::bookmarks` is a place: a file and a line, or a file or folder
   with no line. Every place has at most one bookmark; the lists, ranks, mnemonics and
   descriptions work the same for both kinds, and one mnemonic is used once among all of them.
2. The commands name a file's or folder's own bookmark with `whole: true` beside `path`
   (`toggle`, `toggleMnemonic`, `setMnemonic`, `removeMnemonic`, `rename`, `remove`, `moveToList`,
   `moveUp`, `moveDown`); answers give it without `line`, with `folder` for a folder.
   `navigate.bookmark.open` without `line` opens the file. `navigate.bookmark.toggleFiles` and
   `navigate.bookmark.removeFiles` take the Project tool window's selection as `paths`: Toggle
   removes the bookmarks when every path has one, and otherwise bookmarks those without one.
   `navigate.bookmarkList.fromOpenTabs` makes a new list holding every open tab in tab order. A file
   already bookmarked moves into the new list, because a place has only one bookmark.
3. Opening a folder's bookmark (and `navigate.bookmark.goTo` to one) is refused with the code
   `folderBookmark` and the folder's path: only the window can show a folder. The window selects
   the folder in the Project tool window instead of reporting the error.
4. `workspace.json` keeps the line bookmarks in `bookmarks.lines` as before and the others in
   `bookmarks.files`. Each entry of `files` has an `index`, its place among all the bookmarks of
   its list, and reading puts it back there. An older build ignores `files` and still reads every
   line bookmark. It loses only the file and folder bookmarks, and only when it writes the file
   again.
5. The Project tool window draws a bookmark icon after a bookmarked row's name, with the mnemonic
   when the bookmark has one. Its F3 and ⌥F3 act on the tree's selection while the tree has the
   keyboard. The Bookmarks tool window shows a file's or folder's bookmark as a row of its list,
   among the line bookmarks in its rank order. With Group Line Bookmarks by File it stays a row of
   the list and is not put under the file's line bookmarks.

## Consequences

- Line-only consumers (the gutter, Next/Previous Line Bookmark, Show Line Bookmarks with its
  setting on) skip bookmarks without `line`.
- Renaming, moving and deleting a folder in the IDE carries or removes its bookmark like a file's.
  A folder deleted by another program loses its bookmark the next time the bookmarks are read.
- An agent that took `line` as always present must allow its absence.

## Alternatives considered

- **Bookmarks without a line in `bookmarks.lines`.** An older build fails to read an entry without
  `line` and would lose every bookmark of the project.
- **A separate list kind for files.** Files and lines are mixed in the same lists, and Move Up, Move
  Down and drag-and-drop work across both.
- **Several bookmarks per file, one per list.** Bookmark Open Tabs… would then copy and not move,
  but every command would also need the list to name a bookmark.
