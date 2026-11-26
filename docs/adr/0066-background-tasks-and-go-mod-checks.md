# ADR-0066: Background tasks, and go.mod checks for every common case

- **Status:** Accepted
- **Date:** 2026-10-19
- **Relates to:** [ADR-0059](0059-go-mod-dependency-checks.md) (extended, not replaced),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0050](0050-products-are-plugin-bundles.md);
  AGENTS.md golden rule 10

## Context

The owner (2026-10-19), after using the go.mod checks of ADR-0059 on a real project, asked for
background updates, deprecations, the Go version and every common case:

- updating many dependencies left the window unresponsive for a long time, and nothing showed what
  `go` was doing; a progress indicator with the live log of the update should open at once at the
  bottom right, be possible to hide and show again, and say how it ended;
- after the first problem popup, every other one appeared at once, even with the pointer on empty
  space of a line; a popup should show only over the marked text, after its usual delay each time;
- the Update dependencies dialog was far taller than its two rows, with its buttons in the middle;
- a deprecated module should be struck out with the reason given; retracted versions, the Go
  version, newer major versions and what `go` itself reports should be covered too.

## Decision

1. **Background tasks are a platform service** (`zen_workbench::tasks`). A feature starts a task
   (`Tasks::start`, with whether the panel opens at once) and reports through its `TaskHandle` from
   any thread: log lines, progress and its end (success, or the error). The window shows the running
   task in the status bar (title and progress bar; an outcome stays a moment, a failure until it is
   seen) and a panel above it with every task and the chosen one's live log; **Hide** and a click
   on the status bar hide and show it. Agents use `tasks.background.list`, `tasks.log.read`,
   `tasks.panel.show`, `tasks.panel.hide` and `tasks.finished.clear` (ADR-0033); `get_window_state`
   lists the panel as `backgroundTasks`. Logs keep their last 5000 lines and the window its last 20
   tasks.
2. **Users of it.** A quick-fix applied with `problems.fix.apply` is a task that opens the panel; its
   `FixJob` gives a title and the lines its programs print (`FixJob::title`, `drain_output`, both
   optional), and a failure is told in the panel (and as a notice only while the panel is hidden).
   A Git operation (`vcs.project.update`, fetch, push…) is a task that does not open the panel; the
   status bar's progress is the task's.
3. **Inspections grow** (`zen_project::inspect`): a finding may carry a `note` (a paragraph under
   its heading), be `deprecated` (struck out instead of underlined), and have the severity
   `information` (a hint quieter than a weak warning); a fix may be of kind `details` (shows the
   finding's details) or `action` (runs a window action, e.g. `view.settings.open` on the Go page;
   agents run it with `run_action`).
4. **Popups.** A problem's popup opens only while the pointer is on the drawn range itself, after
   egui's tooltip delay (the window's hint delay) every time, and closes when the pointer leaves
   (with a short grace to move into it). The Update dependencies dialog is as tall as its rows (a
   long list scrolls), has **Select All** and **Select None**, and its button counts the updates.
5. **The go.mod checks (in the Go module, `zen_lang_go::deps`)** add, with the same switches,
   caches and private-module rules as ADR-0059:
   - the `go.mod` of each module's latest version from the proxy (`<module>/@v/<latest>.mod`): its
     `// Deprecated:` comment (the path struck out, the notice in the popup and a dialog) and its
     `retract` directives (a retracted required version is a warning with the reason and an update
     to the newest version that is not retracted; retracted versions are never offered);
   - `exclude` directives of the file: an excluded required version is a warning, excluded versions
     are never offered;
   - newer major versions of direct requirements (`/v2`, `/v3`… from the proxy's `@v/list`, up to
     five past the required one; never for `gopkg.in`): an information hint without a fix;
   - the latest stable Go release from `https://go.dev/dl/?mode=json`, behind the same switch as the
     proxy (`go.dependencies.checkUpdates`) and kept an hour like the rest: an older `go` or
     `toolchain` directive offers `go mod edit -go=…` / `-toolchain=…`; a `go` directive newer than
     the Go SDK in use (`GOVERSION` of `go env` run with `GOTOOLCHAIN=local`) is a warning whose
     fix opens the Go settings;
   - the `go` command's own checks, run in the background with the project's SDK and
     `GOTOOLCHAIN=local`, changing nothing: `go list -m -json -e all` (a requirement it cannot load
     is an error with go's reason) and, from Go 1.23, `go mod tidy -diff` (an indirect requirement
     it would remove is a weak warning with **Run go mod tidy**); with the network switch off they
     run with `GOPROXY=off`. A requirement `go.sum` has no `go.mod` hash for is an error;
   - `go.work`: its `go` and `toolchain` directives as above, and `use` folders that do not exist or
     have no `go.mod`.
   Fixes run `go mod edit`, then one `go get` with every module, then `go mod tidy`, in that order,
   as one background task.

## Consequences

- No update or check blocks the window; what `go` does is visible while it does it, for users and
  agents alike. Other features (a build, a toolchain download) can use the same panel.
- GoGarden now also contacts `go.dev` (the list of Go releases) while a `go.mod` or `go.work` is
  open, and asks the proxy for one more document per module (its latest `go.mod`) and one per
  direct requirement (the next major version), all behind the existing switch; the settings page
  lists `go.dev` with the proxy.
- With the switch on, `go list` and `go mod tidy -diff` may download what they need into the module
  cache, as gopls and `go build` do; with it off they work offline only.
- Tests use the local HTTP server of ADR-0059 for the proxy and go.dev, and a fake `go` script for
  the local checks; none touches the network.
