# ADR-0057: A Git workspace tab

- **Status:** Accepted; decisions 3, 4 and 7 amended by [ADR-0280](0280-clean-room-process.md)
- **Date:** 2026-10-19
- **Relates to:** [ADR-0047](0047-version-control-gitoxide-and-git.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0002](0002-open-source-clean-room-pure-rust.md)

## Context

The owner (2026-10-19): a big Git feature, a Git workspace shown as a tab, with its Git functions
implemented step by step.

Zen IDE already has Git (ADR-0047): file status colours, the branch widget and
popup, the Commit tool window, diffs, the Git tool window's Log, blame, Update Project, Fetch and
Push. What it lacks is one
place where the whole repository is visible (every branch in a colourful graph, branches, remotes,
tags and stashes beside it) and where Git operations are done on what you see: drag a branch onto
another to merge it, right-click a commit to cherry-pick it, undo the last operation.

Questions this decision answers: can we reuse an existing Git client; where the workspace lives; how
it fits the rest of the IDE's UI; how it relates to the existing Git UI; and how it stays
clean-room.

## Decision

1. **A Git workspace tab, in the platform, for every IDE.** The workspace is an editor tab (like the
   diff tab), opened with Git → **Git Workspace** or Find Action (`vcs.workspace.open`). It is part
   of the platform (`zen-workbench` commands, `zen-ui` view), so every Zen IDE has it; no product
   owns it (ADR-0050).
2. **One Git engine.** The workspace adds no second Git implementation. It reads through `zen-vcs`
   (gitoxide, ADR-0047) and writes through the same `git` program runs; it reuses the existing
   commands (`vcs.branch.checkout`, `vcs.commit.details`, `vcs.diff.commit`, `vcs.diff.file`,
   `vcs.status.list`, `vcs.remote.fetch`, Update Project, Push, New Branch) and the existing diff tab
   and Commit tool window. New engine capabilities (the stashes, the refs stamp, revision lookup)
   go into `zen-vcs` for every view to use; the graph layout (`zen_vcs::graph`) is the one the Log
   uses.
3. **Our own design inside the tab.** The UI specification covers
   the IDE's frame, menus, actions, tool windows and dialogs. The workspace tab is a new surface
   beside them, with its own *workflow* (three panels, the WIP row,
   labels on the graph, double-click to check out, and, in later stages, drag-and-drop merges,
   context menus, undo) and our own layout and drawing. It uses what the IDE has where it touches
   the rest of the IDE: the menu entry, Find Action, the tab strip, our icons, the bundled fonts, and **colours only from the theme** (`IdePalette`: the graph palette, selection, hover,
   status colours), so it follows every theme, imported ones included. No existing component is
   changed.
4. **The Git tool window stays.** ⌘9's Log keeps its layout and behaviour;
   the workspace is the richer view of the same data. Both use the same
   commands, so a fix in the engine fixes both.
5. **Every action is a command** (ADR-0033): `vcs.workspace.open`, `.close`, `.state`, `.select`,
   `.openDiff`, `vcs.graph.page`, `vcs.graph.find`, `vcs.stash.list`, with typed parameters. The
   tab's state (open, selection, a diff to show) lives in the session, so agents drive the tab and
   see what it shows exactly as people do.
6. **The graph is laid out over the whole history once**, in the background, and served page by
   page; it is read again only when a branch, a tag, `HEAD` or the stashes move (a refs stamp that
   ignores the index). The view reads only the rows in sight, so histories of any length scroll at
   once.
7. **Clean-room, by observation of public material only.** The feature map
   ([docs/git-workspace.md](../git-workspace.md)) is built from public help pages and
   feature pages only. Nobody installs another Git client to study it,
   and no other product's source
   is read, not even parts under an open licence. Icons, images, colours and texts are ours.
8. **Out of scope:** accounts, cloud workspaces, telemetry, hosting-service integrations (pull
   requests, issues) and built-in AI features (golden rule 10). Hosting integrations, if wanted, come
   later as an opt-in plug-in with their own ADR.
9. **Delivered in stages**, in the order of docs/git-workspace.md. Stage 1: the tab,
   the commit graph, the left panel (local and remote branches, tags, stashes), commit details with
   diffs, and the WIP row leading into the Commit tool window.

## Consequences

- Users get an overview of a whole repository without leaving the IDE, in every Zen IDE.
- The workspace is a new UI surface; `docs/behaviour.md` lists it as our
  own design.
- Each later stage needs engine work in `zen-vcs` (cherry-pick, revert, reset, stash operations,
  interactive rebase, conflicts, undo); the Git tool window and the branch popup can use it too.
- Undo of Git operations (a later stage) is a decision of its own: it needs an ADR on what is
  recorded and what can be undone safely.
- Very large histories are read in full before the first rows show; a later stage reads them
  incrementally.

## Alternatives considered

- **Embed an existing desktop Git client.** Closed source, with no plug-in or embedding API, and
  a licence that forbids redistributing, modifying or reverse engineering. Not possible.
- **Embed an existing commit-graph component.** Under a proprietary licence, and TypeScript for
  webviews anyway. Not possible, and we do not read it.
- **Use a vendor's command-line tool.** Proprietary, tied to an account and cloud
  services. Against golden rule 10.
- **The `git-graph` crate.** It draws commit graphs in Rust, but depends on `git2` (libgit2, C),
  which breaks pure Rust (ADR-0002). Our layout (`zen_vcs::graph`) already exists and is tested.
- **Grow the Git tool window instead.** Its Log keeps its layout; a bottom tool window
  is also too small for a three-panel workspace.
- **A separate window or application.** A tab keeps the workspace next to the code, with the
  editor's diff tab and the Commit tool window one click away, as the owner asked.
