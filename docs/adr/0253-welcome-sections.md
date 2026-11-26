# ADR-0253: The sections of the welcome window

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

The welcome window's left column showed Remote Development, Customize, Plugins and Learn as stubs.
They are to hold first-run choices, the Plugins page and training material, and
Help → Welcome is to bring the window forward. Three things have nothing behind them: we have no
remote
development, no training projects, and no switch for screen readers (the guide's
"Accessibility" page: the description is built whenever a screen reader asks for it).

## Decision

1. **Sections.** Projects, Customize, Plugins and Learn. Remote Development is hidden until it
   exists, rather than kept as a stub.
2. **Customize** dispatches the commands Settings uses (`ui.theme.*`, `ui.keymap.*`) and applies the
   same per-IDE settings at once; there is no draft and no OK. **Support screen readers** is drawn on
   and disabled with a line that says why: a switch that changes nothing would be a lie, and adding
   a mode only to have a switch would make the IDE worse for the people who need it.
3. **Plugins** is the Settings → Plugins page itself, the page's state shared with Settings.
4. **Learn** lists the chapters of the guide and the keyboard shortcuts page and opens them on the
   website in the browser. It is a click by the person, so rule 10 of AGENTS.md holds; the IDE fetches
   nothing itself. An offline copy of the guide is not bundled.
5. **Help → Welcome** (`view.welcome.open { section? }`) focuses the main window when it shows the
   welcome window. When a project shares the main window, the welcome window opens as a window of
   its own, and it closes when a project is opened from it.
6. **Import Settings takes one file** as well: a JetBrains keymap `.xml`, a colour scheme `.icls` or
   `.xml`, or a theme `.json`, told apart by extension and root element, as a new source kind `file`.
   It adds a keymap or a theme through the existing importers, as a JetBrains ZIP does (ADR-0157),
   and is limited to the size of an archive entry, 8 MB.

## Consequences

- With no project open, agents reach the welcome window's commands through `run_action` and
  `dispatch_command`, and `get_window_state` describes it.
- Every chapter of the guide needs a title in the Fluent files; a test fails when `docs/guide`
  changes without the list.
