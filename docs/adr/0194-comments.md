# ADR-0194: No comments in code

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

By November 2026 the Rust sources carried about 39,700 comment lines in 592,000 lines of code:
paragraphs explaining how and why, references to ADRs, tasks and plans, comparisons with other
products, comments inside function bodies, and one-line docs on every public item that the
`missing_docs` lint required, most of them repeating the item's name (`/// The table.` above
`table`). They went out of date with the code, cost every reader (people and agents) time and
tokens, and recorded decisions where nobody reviews them as decisions. The crates are not
published (`publish = false`), so no one reads their docs on docs.rs. The owner asked to remove
them radically and to keep them from coming back.

## Decision

1. **Rust code has no comments:** no `//`, no `/* */`, no `///`, no `//!`, no commented-out code.
   Names and types say what code is; a decision that is not obvious from the code is written in
   `docs/` (an ADR when it is one).
2. **Exceptions:**
   - `// SAFETY:` above an `unsafe` block (ADR-0079);
   - `// WHY: <reason>`, one line on its own, above code whose reason no test can hold: an order
     threads depend on, a workaround of a tool's bug, a cost that decides the shape of the code.
     A rule a test can check is a test, never a `// WHY:`;
   - doc comments of types that derive `JsonSchema` or clap's `Parser`, `Args`, `Subcommand`,
     `ValueEnum`, and of their fields and variants. These docs are data: the descriptions agents
     read in command schemas and the text of `--help`. A description that only repeats the field's
     name is left out.
3. **Text inside string literals is not a comment** (test fixtures with Go or Rust code in them).
4. **The `missing_docs` lint is off** in the workspace and in every crate that copies the lints.
5. **The gate enforces it:** `cargo xtask comments`, a step of `cargo xtask ci`, fails on any
   comment outside these exceptions.

6. **Rules that protect behaviour live in tests.** A test whose name says what must not break
   (`carets_the_fold_hides_move_before_it`) replaces a warning comment: it fails when the rule
   breaks, cannot go stale, and costs no tokens until it fails. New tests go into an existing test
   module or test file (a new file in `tests/` is a new binary to link), check one function where
   they can instead of starting an IDE, and never wait on a clock.

## Consequences

- Code is shorter to read for people and agents; the code and `docs/` are the only two sources.
- Facts a one-line doc carried (what `None` means, a unit) must show in names and types, or in
  `docs/` when they are decisions.
- Command schemas keep their descriptions, so agents calling the IDE over MCP lose nothing.
