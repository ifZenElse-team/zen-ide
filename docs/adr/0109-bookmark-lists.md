# ADR-0109: Bookmark lists are ordered, the default one first; the Bookmarks tool window reads them through commands

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

Line bookmarks carry the name of their list, and the lists existed only as those names. The
Bookmarks tool window needs more: empty lists made with
Create Bookmarks List, a default list that new bookmarks go to and that Mark List as Default moves to
the top, an order of the bookmarks in a list (Move Up, Move Down, a drag between bookmarks), and the
breakpoints shown as a list of their own. The project's `workspace.json` must keep all of it.

## Decision

1. `zen_workbench::bookmarks::Bookmarks` keeps the **lists in their order; the first one is the
   default**. There is no separate "default" flag: Mark List as Default moves the list to the top.
   A project starts with one list named after its folder; with every list deleted, the next
   bookmark makes that list again.
2. Every bookmark names its list and has a **rank** in it; Move Up, Move Down and a drop at an
   index give the bookmarks of the list new ranks in their new order.
3. The commands keep their answers' meaning for agents: in `navigate.bookmark.list` a bookmark of
   the default list has no `list`; `groups` adds every list in order with its bookmarks in order.
   The new actions are commands too: `navigate.bookmarkList.create`, `.rename`, `.setDefault`,
   `.delete`, `navigate.bookmark.moveUp`, `.moveDown`, `navigate.bookmark.open`, and `index` of
   `navigate.bookmark.moveToList`; `navigate.bookmark.options` takes and answers both ⋮ options.
4. `workspace.json` keeps `bookmarks.lines` by list and rank, each of another list than the
   default naming its list (the default list's without `list`, as before lists had an order), and
   `bookmarks.lists` in their order, plus `groupByFile`. A file without `lists` (written before)
   makes the list named after the project the default, first, puts the bookmarks without a list in
   it, and the other lists follow by name, as they were shown before.
5. The tool window draws what `navigate.bookmark.list` and `debug.breakpoint.list` answer, read again
   when the bookmarks' or breakpoints' revision changes, and changes nothing itself: each gesture
   dispatches a command. Breakpoints are a list of the window only, not a bookmark list.
6. Inside the tool window both ⌃⌥↓ / ⌃⌥↑ (as asked) and ⌥⌘↓ / ⌥⌘↑ (Next / Previous Occurrence of
   the macOS keymap) open the next and previous row; they
   are window actions `view.bookmarks.next` and `.previous`, without global keys, so that the
   editor keeps ⌃⌥↓ / ⌃⌥↑ for Next / Previous Highlighted Usage. While the tree has the keyboard
   these four keys go to it before the window's shortcuts, so a global binding of ⌥⌘↓ / ⌥⌘↑ (Next /
   Previous Occurrence of the TODO tool window) does not take them.

## Consequences

- An older `workspace.json` loads unchanged, its project list the default; a newer one read by an
  older build loses only the order of the lists and the empty lists, and a default list renamed
  comes back under the project's name.
- Delete Bookmark List asks first and cannot be undone.
- Bookmarks of files, folders and open tabs (later work) can join the same lists and ranks.

## Alternatives considered

- **A default flag on a list.** The window shows the default list on top with a tag; a flag would
  allow a default list in the middle, which the window never shows.
- **Ordering by file and line.** Simple, but Move Up / Down and dropping between bookmarks would
  have nothing to change.
