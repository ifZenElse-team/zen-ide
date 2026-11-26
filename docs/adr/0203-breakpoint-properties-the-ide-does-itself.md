# ADR-0203: Breakpoint properties the IDE does itself

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

[ADR-0052](0052-debugger-zen-dap-and-delve-over-reverse-dap.md) left function, exception, data
and log breakpoints out of the UI. The IDE's breakpoints need more properties than the Debug
Adapter Protocol has: a suspend policy (all threads, the thread, or none), logging a "Breakpoint hit"
message, the stack or an expression, Remove once hit, a breakpoint that stays off until another one
is hit, user groups and descriptions, and Mute Breakpoints. The adapters the IDE uses (Delve for Go,
java-debug for Java, and lldb-dap or CodeLLDB for Rust once it is debugged) differ in what they
offer: log points, hit conditions, function and data breakpoints, exception filters, conditions on
exception filters and single-thread execution are each a capability of the `initialize` answer.

## Decision

1. **The model keeps every property; the adapter gets what it can do.** A line breakpoint keeps
   its condition, suspend policy, logging, Remove once hit, pass count (`hitCondition`), the
   breakpoint it depends on, its group and description, and the workspace file keeps them all
   (`workspace.json`, never `.zen/`). Function breakpoints, exception breakpoints (the adapter's
   `exceptionBreakpointFilters`, with a condition when the filter supports one) and the filters an
   adapter offered are kept too. What a session sends is planned from its adapter's capabilities: a
   condition, a hit condition, function or data breakpoints, and exception filter options go only to
   an adapter that takes them, and the dialog hides the fields of what the current adapter lacks.
2. **A logging breakpoint that does not stop is a log point when it can be.** With
   `supportsLogPoints`, suspend off, and only the hit message or an expression to log, it is sent
   as `logMessage` ("Breakpoint reached at main.go:12" and `{expression}`). Otherwise the IDE sends
   a plain breakpoint and, on the stop, logs the message, the stack and the expression's value
   (`evaluate` in the top frame) to the console itself, then resumes the program when suspend is
   off. A stop is matched to its breakpoint by `hitBreakpointIds`, or by the top frame's place.
3. **Suspend Thread** is offered only with `supportsSingleThreadExecutionRequests`: on the stop the
   IDE lets every other thread continue alone.
4. **Dependent breakpoints, Remove once hit and Mute are the IDE's.** A breakpoint that depends on
   another one is not sent until that one is hit in a session; after its own hit it waits again
   ("Disable again") or stays ("Leave enabled"); the waiting starts over when no session runs. A
   breakpoint marked Remove once hit is removed when it stops the program (Toggle Temporary Line
   Breakpoint). While breakpoints are muted every session gets empty lists of line, function,
   exception and data breakpoints; the breakpoints are kept, and muting is not saved.
5. **Data breakpoints last as long as their session**, since a `dataId` belongs to the session
   that gave it; they are added from a stopped program (`debug.dataBreakpoint.add`).
6. **⇧-click on a line number sets a non-suspending logging breakpoint**; it
   no longer selects the line (a click beside the numbers still does). ⌥-click enables or
   disables, a drag moves, ⌘-drag copies, and dragging a breakpoint to the editor or a middle click
   removes it only when Settings → Build, Execution, Deployment → Debugger → Remove breakpoint is
   set to Drag to the editor (then a click leaves it).

## Consequences

- Every adapter works with the same commands; a missing capability costs a stop and a resume, or a
  field that is hidden, never a wrong request.
- A non-suspending breakpoint that the IDE logs stops the program for a moment: other threads may
  run on a little later than with an adapter's log point.
- A breakpoint depending on a non-suspending one is turned on at the next sync after its master's
  hit, so a program that reaches it at once may pass it the first time.
- A Go panic entry among the exception breakpoints is not there yet: Delve offers no
  exception filters and stops on an unrecovered panic by itself.

## Alternatives considered

- **Hiding every property the adapter lacks:** logging and Suspend off would not exist for an
  adapter without log points, and dependent breakpoints for every adapter; the IDE has to offer
  them all for every debugger.
- **Sending the IDE's properties as conditions:** a condition that counts hits or logs depends on
  the debuggee's language and side effects in it.
