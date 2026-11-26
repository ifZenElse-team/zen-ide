# ADR-0189: Splits inside a workspace, scripts moving between a workspace and the editor, and Local History of consoles

- **Status:** Proposed
- **Date:** 2026-10-31
- **Amends:** [ADR-0163](0163-workspaces-full-tools-in-the-editor-area.md), which deferred splitting a
  workspace's area and moving scripts in and out (its decision 6 and its consequences) to this record
- **Relates to:** [ADR-0187](0187-git-workspace-on-the-workspace-host.md) (a tab in the editor strip
  for every open workspace),
  [ADR-0060](0060-local-history-store.md), [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

A workspace (ADR-0163) owned exactly one editor group, so its tabs could not sit side by side, and
`editor.split.moveTab` refused every move between a workspace and the main editor. Database work
often wants two things at once (a console beside a table's data, two consoles). A project's `.sql`
script can be run from the code editor into the workspace, but could not be moved there to stay,
or back. Consoles live in the IDE's data folder, outside the project, so Local History, which
keeps only the project's files, did not keep their versions.

## Decision

1. **A workspace owns a split tree of editor groups.** `EditorGroups` keeps per workspace a tree of
   its own (`OwnedGroup { workspace, root, focused }`) beside the main one, with the same node type
   (`SplitNode`) and ids from 2³² up; `focused` is its group that last had the keyboard, which
   expanding focuses. The workspace is expanded exactly when the focused group is one of its groups,
   as before. Its first group is never removed when empty; another one is removed once it holds no
   tab at all (no file and no tab the feature draws).
2. **The editor's split commands act on the area in front.** While a workspace is expanded, Split
   Right, Split Down, Unsplit, Unsplit All, Goto Next/Previous Splitter and resizing act on the
   workspace's area, as they act on the main editor otherwise; the commands keep their ids and
   parameters. `workspace.tab.split` names the tab by id. A text tab stays in its old group too
   unless moved, as in the main editor (one document, carets per group). A tab the feature draws
   cannot be shown twice, so splitting it always moves it, and is refused when it is alone in its
   group.
3. **Each inner tab belongs to one group of its workspace** (`InnerTab.group`), and each group shows
   one of its tabs. The workspace's active tab, which `get_window_state`, ⌘W and the refusals use,
   is the tab of its group that has, or last had, the keyboard. A feature tab whose group goes falls
   into the group with the keyboard. The host draws each group with its own tab strip, the dividers
   of the main editor and a click that moves the keyboard; the feature's toolbar and left panel stay
   once for the whole workspace. A feature's view keeps one results panel, so a console's results
   show under the console that has the keyboard only.
4. **A workspace says which project files it takes.** `Workspaces::accept_files` registers a check;
   the Database workspace takes the open project's `.sql` files and never a console. Such a file
   moves both ways between the main editor and the workspace: dragged onto the workspace's tab in
   the editor strip (ADR-0187) or into the expanded workspace, and dragged onto the editor strip
   above it or moved with Move to Editor. Agents use `editor.split.moveTab { to | workspace }` and
   `workspace.tab.move { group }`. A move is a move of the tab: the document, its unsaved text,
   carets and undo stay. Everything else between a workspace and the main editor, and anything
   between two workspaces, is refused with code `notMovable` and the reason. Opening a file it does
   not host there (`workspace.tab.open` with a code file) is refused with code `notHosted`; the one
   exception is a code file whose SQL runs from its strings (`db.injection.run`, ADR-0183), shown
   there with its results below through an internal path that no command opens.
5. **Kept across restarts.** `workspace.json` keeps a split workspace's layout (the tree without
   tabs), each tab's group and the tab each group showed; restoring lays the tree out again with new
   ids and puts every tab in its group. Older files load as one group.
6. **Local History records consoles.** The folder of consoles (`<data folder>/consoles`) is a
   recorded root beside the project: every save of a console keeps a version (the saves after a
   second of quiet included), and `localHistory.*` and Show History accept a console's path. The
   project's excluded folders and patterns do not apply to it; the limits and the cleaning do.

## Consequences

- `editor.split.list` reports a workspace's groups as `workspaces: [{ workspace, root, focused }]`
  instead of `{ workspace, group }`; `workspace.host.state` adds `panes` and each tab's `group`;
  `workspace.tab.move` takes `to` or `group`; `editor.split.moveTab` takes `workspace` in place of
  `to`. Older callers that pass `to` keep working.
- Every feature view may now be asked to draw several of its tabs in one frame, one per group; its
  state per tab must be keyed by the tab, not assumed single.
- Local History's store holds a second root per data folder; the consoles of every project share it.

## Alternatives considered

- **At most two parts per workspace.** Simpler, but not what the main editor does, and the tree code
  exists already. Rejected.
- **A feature tab shown in two groups at once.** Its view keeps state per tab (scroll, edits staged
  in a grid), and two copies would fight over it. Rejected; it moves.
- **Moving consoles into the project when dragged out.** A move that changes where a file lives on
  disk is Save As, not a drag; it would also put data folder files into Git. Rejected: consoles
  stay, and the refusal says how to make a script.
- **A results panel under every console.** Needs the feature's view to keep one results state per
  console; left for later.
