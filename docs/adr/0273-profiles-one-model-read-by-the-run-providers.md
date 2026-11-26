# ADR-0273: Profiles: one model, read by the run providers

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0208](0208-code-coverage-model-and-providers.md) (the same shape for coverage),
  [ADR-0048](0048-run-configurations-and-the-run-service.md) (run configurations and providers),
  [ADR-0050](0050-products-are-plugin-bundles.md) (capabilities for every IDE),
  [ADR-0033](0033-ai-access-to-every-api.md) (every feature is a command)

## Context

Go programs are profiled with pprof; the JVM and Rust have profilers of their own. People want to
see any of them the same way, and the shared Profiler tool window is separate work, on hold. The
Go half can still be built now, if what it produces is the model that window will show.

## Decision

1. **One model** in `zen-run` (`profile.rs`): a `Profile` has `metrics`, `frames` (a function, its
   file and the line it starts at) and `samples` (a stack from the root, one value per metric).
   `functions(metric)` gives each function's own and cumulative cost, a recursive function counted
   once per sample; `call_tree(metric, depth, least)` gives the call tree, which is also what a flame
   graph draws. Frames are functions, not lines.
2. **Providers record, the platform keeps.** `RunProvider::profiler(configuration)` answers
   `Some(&dyn ProfileProvider)` when the configuration can run with the profiler. As with coverage,
   the platform names a report file in a private folder, starts the run as any run (trust and
   before-launch tasks included), reads the file through the provider on a thread of its own once
   the run ended, then removes the folder. A rerun records again. The console says what was recorded.
3. **The parser of the public format lives in the platform:** `zen_run::pprof`, pure Rust: gzip by
   flate2's Rust backend and a protobuf reader of its own, with no generated code. It refuses a
   broken file and never panics; a file may not decode to more than 1 GB.
4. **Go** runs `go test -json -cpuprofile=<file>` or `-memprofile=<file>`, and `-o=<folder>/profiled.test`,
   because those flags otherwise leave the test binary in the project. Only test configurations can
   run with the profiler: a plain program records nothing unless its own code calls `runtime/pprof`,
   and the file it writes opens with `profile.file.open`. `go test` takes these flags for one package
   only; with several it refuses, the console shows why and no profile is shown.
5. **Commands first.** `profile.run.start`, `profile.file.open`, `profile.report.get`,
   `profile.tree.get` and `profile.frame.open` work without a window. One profile is shown at a time.
   The tool window, the Run menu's entries and the flame graph belong to a later step and read `Profiles`
   and these answers; nothing here draws.

## Consequences

- Agents can record and read a Go profile today; people see a line in the console and have no
  window until that step is taken off hold.
- A JVM or Rust provider adds `profiler()` and converts its data to `Profile`; the platform does not
  change.
- Line-level costs inside a function are dropped by the parser; a later view that needs them adds
  lines to the model.
