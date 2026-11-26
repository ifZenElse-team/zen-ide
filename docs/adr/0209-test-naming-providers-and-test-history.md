# ADR-0209: Test naming providers and the test history

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

Navigate → Test (⇧⌘T) leads from code to its tests and back, offers Create New Test… when there is
none, and the Create Test dialog writes a test with stubs for the members chosen. All of it depends on
the language: Go pairs `foo.go` with `foo_test.go` and names tests `TestFoo` and `TestType_Method`;
Rust keeps tests in a `#[cfg(test)] mod tests` of the same file, in a `tests.rs` beside it or in the
crate's `tests/` folder; JVM languages pair `src/main/java/…/Foo.java` with `src/test/java/…/FooTest.java`
and write JUnit or TestNG classes. Every IDE needs the feature (golden rule 11, ADR-0050), the
languages come from built-in modules (ADR-0071) and from plug-ins, and the JVM half
must come later without platform changes.

The Run tool window also forgets a test run once its tab is reused or closed; the requirement is for
the earlier runs to be listed and reopened with their tree and output.

## Decision

1. **One provider interface, `zen_run::test_support::TestSupport`**, in the run layer (L2), which both
   language modules and the workbench see. A provider says which files it handles, which are test
   files, the members of a file (name, owner type, lines, whether it is a test), the test files paired
   with a subject and the subject files paired with a test (plus files to search for named tests, such
   as the other files of a Go package), whether a test tests a member, the frameworks a project can
   use, the Create Test dialog's first name and destinations, and how to generate a test file or add
   stubs to one. The platform does the rest in one place (`navigate.test.goTo`, `navigate.test.options`,
   `navigate.test.create`): which targets to offer, the chooser, Create New Test…, writing the file.
2. **Built-in modules register a provider in code** (`ModuleHost::test_support`): Go (`GoTests`:
   `_test.go` files, `Test`/`Benchmark`/`Example`/`Fuzz` names, the testing package and Testify when
   `go.mod` requires it) and Rust (`RustTests`: the `tests` module, `tests.rs`, the crate's `tests/`,
   `#[test]` and `#[tokio::test]` when the crate uses Tokio).
3. **Plug-ins declare a provider as data**, `"tests"` in plugin.json (`TestDeclaration`, docs/plugins.md):
   path pairs with `{path}` and `{name}`, a regular expression for members, test name patterns, the
   package line, where methods go, and frameworks with the files that detect them and the text of a
   test file and a test method. The platform turns each into a `DeclaredTests` provider. The manifest
   key travels like the other declarations: the catalog, the installed plugin.json, the release
   tooling. This is how the JVM plug-ins will add JUnit and TestNG without a platform change.
4. **The Create Test dialog's destination is a folder and its name the test's**: the file is
   `<destination>/<name>.<the subject's extension>`. For Rust a destination of the subject's own folder
   with its own name means the subject file itself, so the stubs go into its `tests` module.
5. **The test history is kept per project in the IDE's data folder**, `test-history/<project key>/`:
   an `index.json` of the entries, newest first, and one JSON file per entry with the configuration,
   the command lines, the final state, the test tree and the console (at most 20 000 lines). It keeps
   the last 20 runs that ended with a test tree; older entries and their files are removed. A run is
   recorded when it ends (`zen_workbench::test_history::settle`, called each frame by the Run tool
   window and by `run.history.list`), written on a background thread in order. `run.history.open`
   reopens an entry as a new ended run of the session, without a configuration id so it gets a tab of
   its own; Rerun on it starts its configuration again.

## Consequences

- A new language gets Go to Test and Create Test by registering a provider or by a `tests`
  declaration in its plug-in; nothing in the UI or the commands changes.
- A declared provider finds members with a regular expression, not a parser: enough for naming rules
  and stubs, and the language plug-in can move to its own outline later without changing the format.
- The history survives restarts and is never in the project folder, so nothing is committed by
  accident; it is bounded by count and console length.
