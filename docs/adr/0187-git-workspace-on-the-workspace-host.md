# ADR-0187: The Git workspace on the workspace host, and a tab in the editor strip for every open workspace

- **Status:** Accepted
- **Date:** 2026-10-31
- **Supersedes:** decision 1 of [ADR-0057](0057-git-workspace.md) ("an editor tab, like the diff
  tab"), which [ADR-0163](0163-workspaces-full-tools-in-the-editor-area.md) said would be superseded
  when the Git workspace moves onto the host
- **Relates to:** [ADR-0163](0163-workspaces-full-tools-in-the-editor-area.md) (the workspace host),
  [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md)
- **Decided by:** the owner, 2026-10-25 (one model for Git and Database) and 2026-10-31 (an
  expanded workspace shows its own tab in the editor strip)

## Context

The Git workspace (ADR-0057) was a fake editor tab: a "Git" tab appended to one split's tab strip
(the "extras" of that group), with its rebase and conflict editors as more such tabs next to it.
The Database workspace (ADR-0163) is a different model: a full tool that expands over the editor
area, keeps its own inner tabs, collapses without closing anything and comes back after a restart.
Two models for the same idea meant two sets of rules for what is in front, for keys, for agents and
for the restart.

The owner asked (2026-10-25) that the Database workspace work "like the Git tab on the right", and
on 2026-10-31, after trying the Database workspace: an expanded workspace must still show a tab of
its own ("Database", "Git", with its icon and ×) in the editor tab strip, as the Git tab looks
today, so that people see what is open.

## Decision

1. **Git is a workspace on the host.** It registers as workspace `git` ("Git", the branch icon) in
   every window, whether or not the project has a repository (opening it without one is refused
   with the reason, as `vcs.workspace.open` was). It expands over the editor area, collapses with
   ⇧⎋, its stripe button or opening a code file, and is restored after a restart like any workspace
   (ADR-0163, decisions 2–4 and 7). Its stripe button stays first on the right stripe, above the
   other workspaces' buttons, and runs `workspace.host.toggle { id: "git" }`.
2. **Inner tabs.** The graph (with its toolbar and panels) is the pinned first tab `graph`, which
   does not close. A diff opened from the workspace while it is expanded is the inner tab `diff`;
   the interactive rebase editor is `rebase` and the conflict editor `conflict`. Opening a rebase
   plan or a conflicted file expands the Git workspace on its tab; closing that tab is Cancel or
   Close, as the × on the old tab was. A diff opened from anywhere else (the Commit tool window,
   the Project tool window) stays the editor's diff tab. Only the graph tab is kept across a
   restart; the others hold live state and are not restored.
3. **An open workspace has a tab in the editor strip.** A workspace is *open* from its first
   expansion until it is closed. While it is open, the tab strip of the main editor group it
   returns to shows its tab after the code's tabs: its title, its icon and ×. The tab is active
   while the workspace is expanded; a click on it expands the workspace, × closes it. While the
   workspace is expanded, that strip stays visible above the host, so the code's tabs are one click
   away (a click on one collapses the workspace and shows the file).
4. **Collapsed but open keeps the tab.** Collapsing (⇧⎋, the stripe button, opening a code file)
   leaves the workspace open and its tab in the strip, as the Git tab stayed when another tab was
   picked. × *closes* the workspace: it collapses it and takes its tab out of the strip. Closing is
   not losing work: the inner tabs, consoles and their text stay and come back when the workspace
   expands again (for Git, closing is `vcs.workspace.close`, as before).
5. **Commands.** `workspace.host.close { id }` closes a workspace (what × runs); `workspace.host.list`
   and `workspace.host.state` report `open`. The project's `workspace.json` keeps `open` per
   workspace, so a collapsed workspace's tab comes back after a restart. A feature may run its own
   step when its workspace opens or closes (the Git workspace reads its graph in the background when
   it opens). The Git workspace's own commands (`vcs.workspace.*`, `vcs.rebase.*`, `vcs.conflict.*`)
   keep their parameters and answers: `vcs.workspace.open` now expands the workspace on its graph,
   `vcs.workspace.close` closes it, and `get_window_state` still reports `gitWorkspace`, `diff`,
   `rebaseEditor` or `conflictEditor` for the Git workspace's tab in front.

## Consequences

- One model for every large tool: Git and Database expand, collapse, restore and answer agents the
  same way, and the IDE view tracks only the diff and Compare Directories tabs as non-document tabs
  of the main editor (an earlier change made that one value).
- The editor's "extras" group no longer carries the Git, rebase or conflict tabs; it keeps only the
  diff and Compare Directories tabs, which stay in the split they opened in. Making those real
  kinded tabs of the main editor is left for later (ADR-0163, "A kinded tab model").
- Inside the Git workspace nothing changes: ADR-0057's decisions 2 to 9 stand (its own design, one
  Git engine, every action a command).
- The strip above an expanded workspace takes one tab row of height from the host; with the tabs
  hidden (Distraction Free, or tabs turned off) the host has the whole area.
- Rebase and conflicts expand the Git workspace: someone resolving a conflict from the Commit tool
  window sees the Git workspace, not a tab beside their code.

## Alternatives considered

- **Keep the Git tab as it was and give Database a fake tab too.** Two tab models stay, the
  restart and the agents' view differ; and the owner wants one model. Rejected.
- **No tab in the editor strip, only the stripe button** (ADR-0163 as written). People did not see
  that a workspace was open or how to get back to it. Rejected by the owner, 2026-10-31.
- **× collapses but keeps the tab.** Then nothing takes the tab away; a workspace once used would
  sit in the strip for good. Rejected: collapsing has three other ways, × closes.
- **The diff, rebase and conflict tabs stay in the main editor.** They belong to Git work and were
  already shown as part of it; the requirement asked for them as inner tabs. Only diffs opened outside the
  workspace stay in the editor.
