# Zen UI: names that screen readers read for controls drawn without visible text. English is the source language (ADR-0024).

a11y-close-tab = Close { $name }
a11y-pinned = Pinned
a11y-unpin-tab = Unpin { $name }
a11y-workspace = { $title } workspace
a11y-hide = Hide
a11y-options = Options
a11y-terminal-tabs = Terminal tabs
a11y-modified = modified
a11y-problem = { $severity ->
    [error] Error
    [warning] Warning
    [information] Information
    [typo] Typo
   *[other] Hint
}: { $message }, line { $line }
a11y-editor = Editor: { $file }
a11y-editor-split = Editor: { $file }, group { $split } of { $count }
a11y-editor-divider = Editor group divider
a11y-gutter = Gutter
a11y-run-icon = Run { $name }
a11y-fold = Folding region at line { $line }
a11y-tests = Tests
a11y-console = Console
a11y-sticky-lines = Sticky lines
a11y-change = { $kind ->
    [added] Added lines
    [deleted] Deleted lines
   *[other] Modified lines
}
a11y-threads = Threads
a11y-navigation-bar = Navigation bar
