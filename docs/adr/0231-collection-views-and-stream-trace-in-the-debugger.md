# ADR-0231: Collection views and the stream trace in the debugger

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0052](0052-debugger-zen-dap-and-delve-over-reverse-dap.md),
  [ADR-0153](0153-java-debugging-and-tests.md), [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

The debugger is to show a Java list, set or map by its elements, map entries as key and value, with
a way back to the object's raw fields, and to trace a stream chain operation by operation. The Debug
Adapter Protocol has no request for either. java-debug, the Java debugger inside Eclipse JDT LS, has
a logical structure for collections and maps (its documented setting `showLogicalStructure`) that it
reads when it answers `variables`, so the same `variablesReference` lists elements or fields by the
setting at the time of the request; the setting reaches it through the language server's
`vscode.java.updateDebugSettings` command, with the settings as one JSON string (observed through
JDT LS 1.63 with java-debug 0.53.1). It also marks some values `lazy` (DAP's
`VariablePresentationHint.lazy`): a boxed `Integer` reads `Integer@79` until its single child, which
holds `Integer@79 "3"`, is listed. Map entries in the logical view have the children `key` and
`value`.

## Decision

1. **An adapter declares how it switches views.** `AdapterLine.views` (zen-run) is an optional
   `{ command, logical, raw }`: with a server adapter (ADR-0153) a `workspace/executeCommand` of the
   language server with `logical` or `raw` as its arguments, otherwise a custom DAP request with the
   first argument. Java's adapter line declares java-debug's settings command with
   `{"showLogicalStructure": true}` and `false`. An adapter without it shows what it gives.
2. **The session switches lazily and never mixes views.** The view the adapter is in is unknown at
   the start. `debug.variables.list` with `raw` (or the global choice) switches it first when it
   differs; a switch waits until the `variables` requests sent in the other view have answered, and
   each answer is cached under its view, so a request never gets the other view's children. With
   `waitMs: 0` every step answers `pending` instead of waiting. The global choice is the setting
   `debugger.collectionViews` (Settings → Debugger → Data Views → Enable alternative view for
   Collections classes), switched by `debug.variables.raw`; one row's choice is the UI's.
3. **Lazy values are fetched.** `debug.variables.list` lists the child of a lazy variable and shows
   its value and children in the variable's place, for every adapter. A variable whose children are
   exactly `key` and `value` (looked at only for entry-like types or texts) carries `entry` with
   their values; the UI shows `key → value`, and a value of the form `Type@id "text"` as
   `{Type@id} text`.
4. **The stream trace evaluates prefixes.** `debug.stream.trace` finds the stream chain of the
   statement on the stopped line (a parser of Java text in the Java module, `java::stream`) and
   evaluates in the frame, one after another, `<source and operations so far>.toArray()` for each
   column and the whole chain for the terminal operation, with DAP `evaluate` in the `watch`
   context, so the recent expressions stay the user's. It refuses a chain whose stream is not made
   in the statement (a stream in a variable can be consumed once), `peek` and `onClose` (side
   effects run again), and an infinite source without `limit` or `takeWhile`; columns before the
   bound are not evaluated, a `forEach`-like terminal operation is not run. The lambdas run once
   more per column; the guide says so. Kotlin sequences and other languages are not traced.

## Consequences

- No bytecode instrumentation and no agent in the debuggee: the trace shows the values after each
  operation, not which element became which, and a lambda with side effects runs again.
- Two Java sessions share java-debug's settings in one JDT LS, and each session remembers only the
  view it set itself: with two sessions stopped at once and different choices, children not read
  yet can come in the other session's view until it switches again.
- The Variables tree now reads children with `waitMs: 0`, as the Evaluate tree does, because lazy
  values and map entries take several requests per row.
- Another language's debugger gets collection views by declaring `views`, and the trace by a chain
  parser for its language beside `java::stream`.
