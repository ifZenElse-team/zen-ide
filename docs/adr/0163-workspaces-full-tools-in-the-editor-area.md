# ADR-0163: Workspaces: full tools that expand over the editor area and come back as they were

- **Status:** Proposed
- **Date:** 2026-10-26
- **Relates to:** [ADR-0057](0057-git-workspace.md) (its
  decision 1 is to be superseded when the Git workspace moves onto the host),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0122](0122-scratch-files-and-buffers.md)

## Context

The owner (2026-10-25) wants database work to be a full tool inside the IDE window, like the Git
workspace: a button on the right expands it; all database work happens there, with its own tabs;
it collapses to go back to the code, and expanding it again brings back every tab and all the
work in place.

The Git workspace (ADR-0057) is an editor tab with an "extras" group beside the split editors. That
was enough for one view, but a database tool needs many tabs of its own (consoles, object editors,
data, history), a navigator beside them, and a way to put all of it away at once without closing
it. Today the IDE has no notion of a tool that owns a set of tabs and can be shown or hidden as a
whole. The right stripe already shows a Database button that only says "In development"
(`tool_stripes.rs`).

Questions this decision answers: where an expanded workspace is drawn; how it is shown and hidden;
what its tabs are; where its consoles live; what is remembered; how agents drive it; and how it
fits the rest of the IDE's window.

## Decision

1. **A workspace replaces the editor area, and nothing else.** While a workspace is expanded,
   `editor_area` draws the workspace host in place of the split editors. The tool windows, both
   stripes, the main toolbar, the navigation bar and the status bar stay where they are and keep
   working. The split editors are not touched: their tabs, carets, folds and scrolling are exactly
   as they were when the workspace collapses.
2. **Expand and collapse slide, but the state flips at once.** The host slides in from the right
   (or out to the right) in about 150 ms. The state (`expanded`) and the keyboard focus change at
   the start of the slide, so a key pressed during the animation already goes to the new side; the
   editor beneath takes no input while it slides. With Reduce motion on, there is no slide.
3. **Collapse is not close.** Collapsing hides the workspace with everything in it: its tabs, their
   text, their results, running queries and open connections keep going. Only closing a tab closes
   it. The ways to collapse: the workspace's stripe button, **⇧⎋** while the workspace has the
   keyboard, and opening any code file from anywhere (Project, ⌘E, Go to File, Find in Files,
   Problems, a link). **⎋ never collapses**: inside a workspace it cancels what it cancels in an
   editor (a popup, a selection, the find bar). ⇧⎋ keeps its meaning of Hide Active Tool Window
   whenever a tool window, not the workspace, has the keyboard.
4. **"Expanded" is a fact about focus, not a second flag.** A workspace owns one editor group,
   kept outside the main split tree, never removed when empty and never
   renumbered. The workspace is expanded **exactly when the focused editor group is its own
   group**. Expanding focuses that group; collapsing focuses the main group that had the keyboard
   before; any command that focuses a main group collapses the workspace. One invariant, tested
   headless through the registry, so the UI, the keymap and agents can never disagree about what
   is in front.
5. **Two kinds of inner tabs.**
   - **Text tabs are real documents** in the workspace's group: a console or a script is a file,
     opened as any file is, so typing, undo, ⌘F, completion, highlighting, Local History and
     `get_document_text` work as in code. New tabs never go into a workspace's group unless the
     caller names it (`file.document.open { group }`).
   - **Other tabs are drawn by the feature** (an object editor, a data grid, Query history,
     Sessions): the workspace state keeps each such tab's id, kind, title, closable flag and a small
     piece of kind data, and the feature registers the view that draws that kind.
   The host gives a feature a toolbar slot and a left panel slot (the Database Navigator), and draws
   the inner tab strip with the editor tabs' widget (icon, title, ×, middle click, overflow ▾,
   drag to reorder) plus an optional colour strip per tab supplied by the feature.
6. **Consoles live in the IDE's data folder**, like scratch files (ADR-0122), never in the
   project: `<data folder>/consoles/<project key>/<connection>/console.sql`, `console_1.sql`, …
   Closing an empty console deletes its file. Scripts a person wants to share are ordinary `.sql`
   files of the project; moving them between the workspace and the code editor comes later.
7. **Kept across restarts, without data and without connecting.** The project's `workspace.json`
   gains `workspaces`: per workspace whether it was expanded, the main group to return to, its
   tabs in order (kind, title, path for text tabs, kind data), the active tab, each console's caret
   and first shown line, and the panel sizes the feature reports. It holds **no data and no
   secrets**: no result rows, no cell values, no passwords. Restoring reads it leniently (a bad
   entry drops itself, older files load) and **connects nowhere**: every database tab comes back
   disconnected and connects at the person's first action in it (AGENTS.md rule 10). Consoles are
   saved on idle even when autosave is off, so a killed IDE keeps their text.
8. **Commands, for people and agents alike** (ADR-0007, ADR-0033), in `zen_workbench::workspaces`:
   `workspace.host.list`, `workspace.host.state`, `workspace.host.expand`, `workspace.host.collapse`,
   `workspace.host.toggle`, `workspace.tab.open`, `workspace.tab.select`, `workspace.tab.close` and
   `workspace.tab.move`. The stripe button runs `workspace.host.toggle { id }`. `get_window_state`
   reports the workspace in front (`workspace: { id, expanded, activeTab }`), so an agent always
   knows whether its keys reach a console or hidden code beneath.
9. **Our own surface, inside the IDE's frame.** As for the Git workspace (ADR-0057, decision 3),
   what is inside the host is our design. Where it touches the frame it uses the frame's own
   parts: the stripe button and its tooltip, Find Action, the tab strip, our icons, the fonts, and
   colours only from the theme (`IdePalette`). No component of the frame changes;
   `docs/behaviour.md` lists the Database workspace as our own design.
10. **Every IDE, where the feature is on** (ADR-0050). The host is platform code. A workspace's
    button shows only while its feature is switched on: the Database workspace where database
    support is on ([ADR-0164](0164-database-engine-zen-db-runtime-and-drivers.md), decision 1),
    the Git workspace where the project has a repository.
11. **The modes keep a workspace.** Distraction Free, Zen and Presentation modes hide the stripes
    and tool windows as before; an expanded workspace stays expanded in them, and Find Action
    reaches its toggle while the stripe is hidden.

## Consequences

- A second large tool (Git first) can move onto the same host; ADR-0057's "a tab
  with an extras group" is then superseded by a new ADR, and the fake Git editor tab goes.
- `EditorGroups` gains owned groups, and every place that walks the groups (splits, next and
  previous splitter, `editor.split.list`, the workspace file) must skip them or report them apart.
- The IDE view's parallel "what is in front" flags (`diff_active`, `git_active`, and their copies)
  become one value before the host adds a fifth.
- Keys and agents have to learn the workspace: ⌘W closes the active inner tab, never a hidden code
  tab; the Switcher and ⌘E list inner tabs; an editor command on a non-file tab is refused with a
  reason.
- A console is a file outside the project, so the language features reach it as they reach scratch
  files (the SQL plug-in highlights it; the language server of a project does not see it).
- Splitting the workspace's own area and dragging scripts in and out are deferred;
  until then a tab dragged between a workspace and the main editor is refused with a reason.

## Alternatives considered

- **Cover the whole centre of the window** (tool windows and all). It hides Terminal, Run and
  Problems while one works with a database, and clashes with Distraction Free and Zen modes, which
  already own "everything but the text". Rejected.
- **A drawer that slides over the editor** and leaves part of it visible. Two editors are then
  visible at once: which one has the keyboard, where popups and completion belong, and what ⌘W
  closes all become ambiguous. Rejected.
- **Hidden documents** (keep the consoles as ordinary editor tabs and hide them while collapsed).
  Every command that walks the tabs would need to know about hidden ones, and agents would edit
  text nobody sees. Rejected.
- **A kinded tab model for every editor tab** (every tab, code or not, carries a kind and a view).
  It is the cleaner end state, but changes the main editor for a feature that does not need it yet.
  Deferred to the move of the Git workspace, which is where both models meet.
- **A tool window.** A bottom or side tool window is too small for a navigator, a console and a
  result grid at once, and would not keep its own tabs. Rejected, as for the Git workspace.
- **A separate window.** Loses the code, Terminal and Run one keystroke away, and is not what the
  owner asked for.
