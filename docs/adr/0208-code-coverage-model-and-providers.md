# ADR-0208: Code coverage: one model, read by the run providers

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0048](0048-run-configurations-and-the-run-service.md) (run configurations and
  providers), [ADR-0050](0050-products-are-plugin-bundles.md) (capabilities for every IDE)

## Context

People want to run tests with coverage and see which lines ran, in every language that can measure
it: Go with `go test -coverprofile`, Rust with `cargo llvm-cov`, Java and Kotlin with JaCoCo. The
tools write different formats (Go's cover profile counts blocks of statements, LCOV counts lines and
branches, JaCoCo's XML counts instructions and branches per line), and they are started differently.
The gutter, the Coverage tool window, the HTML report and the commands for agents must not care
which tool measured the data, and the JVM half must plug in later without changing the platform.

## Decision

1. **One model** in `zen-run` (`coverage.rs`): a `CoverageReport` is a list of `FileCoverage`, each
   file its measured lines (`LineCoverage`: the line from 1, its hit count, its status, and branch
   counts when the tool gives them). A line is **covered** when every part of it ran, **partly**
   when some parts or branches did not, **not covered** when nothing on it ran. A tool's data goes
   in through `CoverageBuilder`: `part(path, lines, hits)` for a block or a line,
   `branch(path, line, taken)` for a branch. `ranges()` gives lines in a row with the same status;
   `CoverageCounts` sums lines (and branches) per file, per folder and in all. The percentage is the
   share of measured lines that ran, covered or partly.
2. **Providers measure, the platform shows.** `RunProvider::coverage(configuration)` answers
   `Some(&dyn CoverageProvider)` when the configuration can run with coverage, else `None` (no Run
   with Coverage for it). A `CoverageProvider` gives the command lines that run the configuration
   and write the data to a report file the platform names, and reads that file into the model.
   The platform makes the file's folder, starts the run as any run, and on a thread of its own,
   once the run ended (failed tests too, not a stopped run), asks the provider to read the file,
   then removes it. The UI thread never parses coverage data.
3. **Parsers of public formats live in the platform:** Go's cover profile and LCOV
   (`parse_go_profile`, `parse_lcov`), for any provider to use. Go: `go test -json -coverprofile=<file>
   -covermode=count` (`atomic` with `-race`); profile entries are import paths, resolved through the
   project's modules, `command-line-arguments` to the tested folder; a block that ends at column 1
   leaves that line out. Rust: `cargo llvm-cov` with the test's arguments and `--lcov --output-path
   <file>`, only when `cargo-llvm-cov` is installed (beside cargo, on `PATH` or in Cargo's `bin`);
   the IDE never installs it nor the LLVM tools (it sets `CARGO_LLVM_COV_SETUP=no`), and the run is
   refused with a message that says what to install. Documentation tests are refused (they need a
   nightly toolchain).
4. **JaCoCo plugs in the same way.** The Maven and Gradle run providers answer `coverage()` for test
   configurations; their `CoverageProvider` adds the JaCoCo agent and its report to the build's
   command line and reads the XML into the model (`part` for a line's instructions, `branch` for each
   of its covered and missed branches). No platform change is needed.
5. **One suite is shown at a time**: the newest run with coverage replaces what is shown; Hide
   Coverage drops it. Commands: `coverage.run.start` (as `run.process.start`, with `waitMs` waiting
   for the data too), `coverage.run.supports`, `coverage.report.get` (summary and files, `path`
   narrows it), `coverage.file.get` (one file's lines, with a `known` revision so the editor asks
   every frame for nothing), `coverage.report.hide`, `coverage.report.export` (an `index.html`).
6. **UI.** Run with Coverage is in the Run menu, the run icon's menu in the gutter (only for
   configurations that support it) and the run widget's More Actions. The gutter marks lines green,
   amber and red with the theme keys `editor.coverage.covered`, `editor.coverage.partly` and
   `editor.coverage.notCovered`; pointing at a mark shows its hit count and branches. The Coverage
   tool window (right) lists folders and files with their percentages, opens a file on a click, and
   has Hide Coverage and Generate Coverage Report. The HTML report takes its colours from the Zen
   Light theme.

## Consequences

- A language that can measure coverage adds a `CoverageProvider` to its run provider and nothing
  else; the gutter, the tool window, the report and the commands come for free.
- Lines are kept by number: editing a file after the run moves its code under the marks until the
  next run.
- Only one suite is shown; choosing among several earlier runs is not there yet.
