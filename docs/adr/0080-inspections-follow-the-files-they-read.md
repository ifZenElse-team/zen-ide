# ADR-0080: Inspections follow the files they read; every failed fix is told

- **Status:** Accepted
- **Date:** 2026-10-20
- **Relates to:** [ADR-0059](0059-go-mod-dependency-checks.md),
  [ADR-0066](0066-background-tasks-and-go-mod-checks.md) (point 2 superseded in part: a failed fix
  is told as a notification always, not only while the panel is hidden)

## Context

The owner reported (2026-10-20) that after a Git reset `go mod tidy` does nothing and the update
fix is gone: after updating dependencies in GoGarden and a `git reset` that brought `go.mod` and
`go.sum` back, a `go.mod` line showed
**Missing go.sum entry for github.com/prometheus/procfs v0.21.1** and
**Newer version is available: v0.22.0**, the popup offered only **Run go mod tidy** and **More
actions…**, and running it changed nothing.

Reproduced on a throwaway module: the findings of an open file were kept until its text or the
inspector's generation changed. The missing `go.sum` entries are read from `go.sum`, which the
inspection of `go.mod` does not otherwise see: `git reset` writes `go.mod` first, the window
reloads it and inspects it against the `go.sum` not yet written back, and the mark stayed after
`go.sum` was restored. `go mod tidy` then had nothing to do, nothing changed in `go.mod`, and the
stale mark stayed again. A `go.sum` line removed by hand never showed at all. The popup showed only
the fixes of the worst problem under the pointer, so the newer version's update was hidden behind
the `go.sum` one, and "More actions…" did not list it either.

## Decision

1. **An inspector names the other files its findings depend on** (`Inspector::inputs`, none by
   default); the Go module names `go.mod`, `go.sum`, `go.work` and `go.work.sum` of the module's
   folder. The window keeps an open file's findings only while those files have the same size and
   modification time, so a change on disk by any program refreshes the marks the next time they are
   drawn.
2. **A finished fix always inspects its file again**, whether or not it changed a file.
3. **A failed fix is told in the background task's log and as a notification**, the panel shown or
   not, so that it is never missed.
4. **The problem popup offers the main fix of every problem under the pointer** (the worst one's
   first, with ⌥⇧↵; of the others, the first fix that applies at once rather than a dialog), and
   **More actions…** lists every fix of every one of them once.

## Consequences

- A `git reset`, a checkout or an edit of `go.sum` in another program updates the go.mod marks at
  once; `problems.file.list` answers what is on disk now.
- One `stat` per input file each time the findings are asked for; nothing is read unless something
  changed.
- Agents see the same (ADR-0033): the fixes are those `problems.fixes.list` returns, and a failure is
  in `problems.fix.status`, `tasks.log.read` and the notifications.
