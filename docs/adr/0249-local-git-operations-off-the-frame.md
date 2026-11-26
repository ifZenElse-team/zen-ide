# ADR-0249: Local Git operations off the frame

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

Checkout, merge, rebase, cherry-pick, revert, reset and commit ran inside command dispatch, on the
frame: a large checkout or a slow commit hook froze the window. These commands also answer agents
with typed results (`HeadMoved`, `CommitResult`, the code `conflicts`), so making them background
for everyone would break the contract of ADR-0033.

## Decision

1. Each command is split in three: checks and the undo recording on the session, the Git work on
   a `Repository` alone, and finishing (undo entry, conflicts, refresh) on the session.
2. `zen_workbench::vcs::off_the_frame(session, dispatch)` marks a dispatch as coming from the
   window. Only then does the Git work run on the path of remote operations (`remote::start`), so
   it shows in the status bar and in `vcs.operation.status`, and the command answers at once with
   an empty result. Every other caller, agents included, gets the answer when the work is over, as
   before.
3. Finishing needs the session, so it runs on the first `vcs.operation.status` after the work
   ends; the window asks on every frame. Conflicts go to the Git workspace, not to a notification.
4. One Git operation runs at a time: a second one started from the window is refused as busy.
5. The checks before the work still run on the frame.

## Consequences

The window must not assume the operation is done when dispatch returns; the commit forms clear
only when the operation ends well. Local operations cannot be cancelled: killing one half-way
would leave the repository in a state the user has to repair.
