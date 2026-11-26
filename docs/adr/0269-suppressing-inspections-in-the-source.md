# ADR-0269: Suppressing inspections in the source

- **Status:** Accepted
- **Date:** 2026-11-06
- **Relates to:** [ADR-0258](0258-inspection-profiles-and-code-cleanup.md) (extended),
  [ADR-0256](0256-inspect-code-over-a-scope.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

A profile turns an inspection off everywhere. People also need to say "not here" about one place,
and the place to say it is the source, where the rest of the team and the command-line tools see it.
This is the second part of the work on the Inspection profiles page, suppression and Code
Cleanup. The requirement was a suppression comment of the IDE's own; this work writes the
language's own mechanism instead, so that what is written also means something outside this IDE.

## Decision

1. **The language's mechanism, nothing of ours.** Go gets `//nolint:<id>`, Rust `#[allow(<id>)]` and
   `#![allow(<id>)]`. A language server declares how, in `suppression` of its declaration: the text
   for a statement, a function and a file, each optional, with `{id}` for the inspection, whether it
   ends the line (`lineEnd`) or goes above, the line it goes `before` (Go's `package `), the
   `separator` of several ids, and the `sources` it applies to when not all of the server's. A new
   language is therefore a few lines of data.
2. **One form per scope, and none where it cannot be placed safely.** Rust has no statement scope: an
   attribute is only valid at the start of a statement, and a problem's line does not say where that
   is. ESLint is not declared, as its server offers its own disable fixes.
3. **Where a line already says it, it is added to.** A directive of the same form on the line or
   above takes the new id; one that names it already changes nothing. A line with a comment, or
   one that ends inside a raw string, gets the directive above it, since `//nolint` only counts as
   the start of a comment and text added inside a string would change the program.
4. **The IDE honours what a tool does not.** The Rust compiler honours its attributes by itself. gopls
   does not read `//nolint`, so `Outliner::suppressions` gives the lines a directive covers, and the
   same place that applies a profile (`inspections::joined`, the end of an Inspect Code run) leaves
   those problems out, before the profile, so raising a severity does not undo a suppression. The Go
   reader follows the common meaning of `//nolint`: at the end of a line, or alone on the line right
   above a statement, it covers that statement with its block; above `package`, the file. `//nolint`
   alone or `:all` covers everything.
5. **Errors stay.** An error is never hidden and no suppression is offered for one: a directive does
   not make the compiler accept the code. A typo is not suppressed either; spelling has its dictionaries.
6. **The id written is the code a finding reports, or its source when the code is empty or gopls's
   `default`.** A directive matches a problem by its source, code or both, as a profile does.
7. **Everywhere a problem is acted on, one list.** `inspect.suppress.list` gives what can be done at a
   place, `inspect.suppress.apply` does it. ⌥⏎ shows them after the quick-fixes, as context actions
   when the problem has none of its own and as window actions when it has; a row of the Problems tool
   window has them in its context menu, and an item of Inspection Results as buttons through
   `inspect.results.fixes`. `inspect.results.suppress` also takes out of the results the items of the
   same inspection the suppression covers. The change is an ordinary edit: an open file keeps it
   unsaved and one Undo reverts it.
8. **Directives are read once per version of a file.** Problems are asked for every frame while the
   tool window shows, so an open file is read again when its version changes, a closed one when its size
   or time changes, checked at most every two seconds.

## Consequences

- golangci-lint reads `//nolint:<id>` with linter names, and an id such as `simplifyrange` or `S1010`
  is not one; its `nolintlint` may report it. The IDE honours it either way.
- A file with a stale directive shows nothing; nothing reports that a directive is no longer needed.
- `.idea/inspectionProfiles` import and scope-specific severity are not part of this decision and
  remain open. Reading another IDE's own suppression comments is not done: their
  inspection ids mean nothing here.
