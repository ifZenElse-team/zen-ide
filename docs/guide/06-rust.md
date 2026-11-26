# Rust

RustRonin reads Rust projects the way Cargo does: the members of a workspace are the project's
modules, and the crates they use are listed under External Libraries with their sources.
rust-analyzer, the Rust language server, gives problems as you type, completion, navigation,
usages, rename, formatting with rustfmt, the Structure, inlay hints and quick fixes.

Everything on this page is the **Rust** plug-in, built into every Zen IDE and on in RustRonin. In
another Zen IDE, such as GoGarden, switch it on in **Settings → Plugins → Built-in** and restart
the IDE ([Built-in plug-ins](10-plugins.md#built-in-plug-ins)).

## The Rust toolchain

RustRonin finds the Rust toolchains on your computer: rustup's `~/.cargo/bin` (or `CARGO_HOME/bin`),
the folders on `PATH`, and Homebrew's Rust; a folder counts when it holds both `cargo` and `rustc`.
The first one found is used. In a project with a `rust-toolchain.toml`, rustup's `cargo` and
`rustc` follow it, and so does RustRonin: the version and the standard library shown are the
project's.

Looking at the toolchain never downloads anything: rustup is asked not to install a toolchain a
`rust-toolchain.toml` names. Agents read what was found with `toolchain.rust.info` and look again
with `toolchain.rust.refresh`.

## rust-analyzer

RustRonin uses the rust-analyzer of the toolchain (rustup's `rust-analyzer` component, as
`rustup which rust-analyzer` finds it), else one on `PATH` or in `~/.cargo/bin` that answers
`rust-analyzer --version`; rustup's own `rust-analyzer` launcher, which fails while the component
is not installed, does not count.

- When it is missing, a notification above a Rust file offers **Install**: it runs
  `rustup component add rust-analyzer` and shows how it goes. **Settings → Rust** offers the same.
  Without rustup, it says so and links to [rustup.rs](https://rustup.rs/).
- It starts with the toolchain first on its `PATH`, and checks the code with `cargo check` into
  `target/rust-analyzer`, a folder of its own, so that it never waits for your `cargo build`, or
  makes it wait.
- Saving a file checks the package the file belongs to (`cargo check -p <package>`), not the whole
  workspace, and nothing is checked when the project opens: on this repository (70 packages) the
  first lookups after opening it answer in a fraction of a second instead of waiting for a
  workspace-wide check, and a save's problems come in about half a second. Problems in other
  packages that a change breaks show in their files as you open them (rust-analyzer's own
  analysis), and in full after a save there; **What a save checks → The whole workspace** in the
  Rust settings checks everything on each save. The measurements are in
  `docs/performance.md` in the repository ("rust-analyzer on a large workspace").
- On a large workspace it works for a while after starting (loading the workspace, building proc
  macros, indexing, `cargo check`): the status bar says what, for example "rust-analyzer: Indexing
  18%". Agents see the same in `lsp.servers.list` (`busy`, `progress`, `readyAfterMs`) and wait
  for `busy: false` before trusting usages and problems.
- It is one of the IDE's [language servers](language-servers.html): restart or stop it, read its
  log, or start another rust-analyzer program from the status bar.

What it gives, as for Go: problems as you type (rust-analyzer's own analysis, a moment after each
change) and `cargo check`'s after a save, with their quick fixes and context actions (**⌥⏎**);
completion; **⌘B**, **⌥⌘B**, **⇧⌘B** (Type Declaration: from a variable or an expression to its
`struct`, `enum` or `trait`), **⌘U** (Super Method: from a method of an `impl Trait for Type`, on its
name or in its body, to the trait's method; a method of a trait, an inherent method or a function
has none), **⌥F7** and Show Usages; Rename (**⇧F6**) across the workspace;
Reformat Code (**⌥⌘L**) with rustfmt; quick documentation; the Structure; the colours of names;
inlay hints for parameter names, the types of variables and of method chains, and the type arguments
of generic calls, each switched in
[Settings → Editor → Inlay Hints](09-settings.md#editor--inlay-hints). **⇧⌘T** (Navigate → Test) goes from a function to its tests in the
`tests` module, a `tests.rs` beside the file or the crate's `tests/` folder, and back; with no test it
offers Create New Test…
([Go to Test and Create Test](15-run-and-test.md#go-to-test-and-create-test)).

## Rust settings

**Settings → Languages & Frameworks → Rust** has five parts. Changes apply at once, in every
window: rust-analyzer restarts with them and the project is read again.

- **Toolchain**: the toolchain in use, chosen from those found (each with its `rustc` version and
  folder). **Add Toolchain → Local…** uses another folder that holds `cargo` and `rustc`. Under it,
  where rustup is, or that there is none.
- **Standard library**: where its sources are, or **Download via rustup** when they are not
  installed (`rustup component add rust-src`); External Libraries and rust-analyzer need them.
- **rust-analyzer**: the one in use, its version and path. **Custom path** uses another, once it
  answers `rust-analyzer --version`; empty the field to go back to the one found. **Install
  rust-analyzer** or **Update rust-analyzer** runs `rustup component add rust-analyzer`, only when
  you click it; **Look again** searches for toolchains and rust-analyzer once more.
- **Performance**: how rust-analyzer works on a large workspace, each with RustRonin's default:
  - **Check on save**: run the check after each save (on).
  - **Check command**: `check` or `clippy` (`check`; Clippy finds more and takes longer).
  - **What a save checks**: **The package of the saved file** (the default) or **The whole
    workspace**.
  - **Check into**: `target/rust-analyzer`, a folder of its own that never waits for `cargo
    build` (the default), or `target`, shared with your builds: less disk, but the check and your
    builds wait for each other.
  - **Build scripts (build.rs)** and **Procedural macros**: run and expand them (on). Turned off,
    rust-analyzer loads faster but knows nothing that a `build.rs` or a derive such as
    `#[derive(Serialize)]` generates, and reports false errors.
  - **Index every crate on start** (on): off, the project is ready sooner, and the first search
    pays for it instead.
  - **Syntax trees kept in memory**: 128, rust-analyzer's own default, up to 1024.
  - The folders rust-analyzer leaves out: `.claude/worktrees` (the copies of the repository AI
    agents work in), besides `target`, which it always leaves out.
- **On Save**: **Reformat code on save**, the option of **Tools → Actions on Save**.

Your choices are kept in `toolchain-rust.json` in RustRonin's data folder: the toolchain's folder
(`toolchain`), the rust-analyzer you chose (`rustAnalyzerPath`) and your rust-analyzer settings
(`rustAnalyzerSettings`). Agents read the page with `toolchain.rust.settings` and change it with
the commands it names: `toolchain.rustAnalyzer.install`, `toolchain.rustSrc.install` and
`toolchain.rustAnalyzer.configure`. `toolchain.rust.choose`, `toolchain.rustAnalyzer.choose` and a
setting that names a program (`check.overrideCommand`, `server.path`, `server.extraEnv` and the like)
are yours alone: they choose which program runs, so an agent gets `agentNotAllowed`, before anything
in the folder is started.

### A project's own rust-analyzer settings

A repository can carry rust-analyzer settings in **`.zen/rust-analyzer.json`** at its root. They
go over yours and over RustRonin's defaults, for everyone who opens the project. The settings are
rust-analyzer's own, as [its manual](https://rust-analyzer.github.io/book/configuration.html)
names them without the `rust-analyzer.` prefix, nested as JSON objects:

```json
{
  "check": { "command": "clippy", "workspace": true },
  "files": { "exclude": ["vendor", "generated"] }
}
```

Any of rust-analyzer's settings can be set this way, not only those on the page. rust-analyzer reads
the file when it starts: after editing it by hand, restart rust-analyzer from the status bar.
Agents read every layer with `toolchain.rustAnalyzer.settings` (`project` names the project's
folder; `effective` is what rust-analyzer starts with there) and set one setting with
`toolchain.rustAnalyzer.configure` (`key`, dotted, such as `check.workspace`; `value`, or null to
remove it; `project` writes the project's file instead of your settings), which restarts
rust-analyzer.

## Postfix completion and statements

After an expression and a dot, [postfix templates](02-editor.md#postfix-completion) wrap it:
`x.let`, `ok.if`, `ok.else`, `x.not`, `x.while`, `items.for`, `x.match`, `opt.ifl`
(`if let Some(value) = opt`), `x.ref`, `x.refm`, `x.some`, `x.ok`, `x.err`, `x.box`, `x.dbg`,
`x.println`, `x.return`, `x.par`; rust-analyzer's own postfix snippets of the same name give way to
them. **⇧⌘⏎** ([Complete Current Statement](02-editor.md#complete-current-statement)) closes the
brackets, adds the block of an `if`, a loop, a `match`, an `fn`, `struct`, `enum`, `trait`, `impl`
or `mod`, or the `;` of a statement. `///` typed above a declaration puts a documentation comment
stub, with an `# Errors` section above a function that returns a `Result`.
**⌃⇧Space** ([Type-Matching completion](02-editor.md#type-matching-completion)) keeps the items
rust-analyzer preselects for the place, or else its values, functions and types.

## Cargo projects

A folder with a `Cargo.toml` is read with `cargo metadata`, offline:

- **Modules.** Every member of the workspace is a module: its folder is drawn in bold in the
  Project tool window, with the package's name after it when the folder is named otherwise
  (`core [zen-core]`). A single package is its own module.
- **External Libraries.** **Rust SDK 1.x.y** browses the standard library's sources (rustup's
  `rust-src` component), and **Crates** lists every other crate the workspace uses for this
  computer, as `name@version`, with its sources in Cargo's registry. See
  [External Libraries](05-project.md#external-libraries).
- **Following changes.** Saving a `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml` (in the IDE
  or anywhere else) reads the project again; nothing is read again while they stay the same.
- **Problems.** A manifest Cargo cannot read is an error with its file and line, for example a
  value without quotes in a member's `Cargo.toml`: the **Problems** tool window lists it on
  **Project Errors**, open or not, and the editor underlines its line, until the file is fixed and
  saved; while you edit, the mark moves with the lines. When only the dependencies are missing (never
  downloaded) the members are still listed, and Cargo's message is a warning on the line that
  declares the dependency: run `cargo fetch` or a build once, then **Reload Project Model** (Find
  Action, **⇧⌘A**).

Agents read the modules (with each package's edition, `rust-version`, targets and features) and
the problems with `project.module.info` (and, with the other problems, with `problems.file.list`
and `problems.project.list`), the libraries with `project.libraries.list`, and read the
project again with `project.model.refresh`.

## Run and test

RustRonin runs Rust code with Cargo, as [run configurations](15-run-and-test.md) of three kinds:

- **Cargo Run** (`cargo run`): a binary or an example of a package.
- **Cargo Test** (`cargo test --no-fail-fast`): every target, or the library, a binary, an
  integration test, a benchmark, an example or the documentation tests of a package; **Test
  names** narrows it to tests whose names contain the words given, or are them with **Exact
  names**.
- **Cargo Bench** (`cargo bench`): the benchmarks, narrowed the same way.

Each has the package (`-p`), the target, **Features** (`--features`), **All features**, **No default
features** and the **Profile** (`--profile`, for example `release`; the command's own when empty).
The tool's arguments go to Cargo and the program's arguments after `--`.

A green arrow in the gutter runs what is on its line: `fn main` of a binary or an example, a
`#[test]` function (also `#[tokio::test]` and the like), a `#[bench]` function, and a
`#[cfg(test)]` module with all its tests. **⌃⇧R** runs the test, the module of tests or the program
around the caret, or the whole integration test file. The configuration it makes knows the
package, the target and the test's full path, for example `cargo test -p calc --lib --
tests::adds --exact`.

The Run tool window shows the tests as a tree: one node per test binary (`calc` for the library,
`outside (tests/outside.rs)` for an integration test, `calc (doc-tests)` for the documentation
tests), and the tests under it, with how each went and the time each binary took. A failed test
shows its own output, and its `panicked at calc/src/lib.rs:24:9` is a link to the line. **Rerun
Failed Tests** runs only those again, by their exact names. Agents do the same with
`run.targets.list`, `run.context.get`, `run.process.start`, `run.tests.get` and
`run.tests.rerunFailed`.
