# Run and test

GoGarden runs and tests your code with run configurations: named settings for what to build, run or
test, shown in the run widget in the toolbar and kept with the project.

## The run widget

The right of the main toolbar shows the selected configuration, then **Run**, **Debug** and, while
something runs, **Stop**.

- Click the configuration's name to open its list: click another configuration to select it, or
  **Edit Configurations…** at the bottom to open the Run/Debug Configurations dialog. Temporary
  configurations are greyed.
- Without configurations the widget says **Add Configuration…**; clicking it opens the dialog.
- **Run** (**⌃R**) starts the selected configuration. While that configuration runs, the button is
  **Rerun**: it stops the run and starts the configuration again. GoGarden first asks "… is already
  running" with **Stop and Rerun** and **Cancel**; turn the question off in **Settings → Advanced
  Settings → Run/Debug → Ask before a rerun stops the running process**. A configuration with **Allow
  multiple instances** keeps **Run**: each start runs another instance in a tab of its own, named
  `name (2)`, `name (3)`…
- **Stop** (**⌘F2**) is red and shows only while a run is running; it stops the selected
  configuration's run, or the run started last.
- **⌃⇧R** runs what is at the caret (Run Context Configuration): the test or `main` function around
  it. It reuses a saved configuration with the same settings or makes a temporary one and selects it.
- **Debug** (**⌃D**, the bug) starts the selected configuration in the debugger and opens the Debug
  tool window; **⌃⇧D** debugs what is at the caret. Go code is debugged with Delve: when it is not
  installed, a notification says so, and **Install Delve** in it downloads Delve with `go install`.
  Nothing is downloaded unless you click it.
- Hovering a button shows what it does and its shortcut.

The widget's selection and the temporary configurations come back when GoGarden reopens the
project.

## Run Anything

Press **⌃** twice (**Ctrl Ctrl** on Windows and Linux) to open **Run Anything**: one field that runs a
configuration or a command.

- With nothing typed it lists the commands you ran from it before, newest first, then every run
  configuration: yours, the adopted ones and the temporary ones (greyed). Typing filters them by
  name. The recent commands are kept with the project's window and come back when it reopens.
- The run modules suggest their tools' commands as you type their first word: `go` (`go run .`,
  `go test ./...`, `go build ./...`, `go get`, `go mod tidy`, `go generate ./...`), `cargo`, `mvn`
  and `gradle`. A Go or Cargo command runs in the Run tool window; `mvn` and `gradle` run as Maven
  goals and Gradle tasks of the module of the folder ([Maven and Gradle](26-maven-and-gradle.md)).
  **⇥** puts the chosen suggestion into the field to change it.
- Anything else runs as typed in a new Terminal tab.
- `open <name>` lists your recent projects and opens the one you pick. `?` lists the prefixes.
- **⏎** runs the chosen item, **⇧⏎** starts a configuration in the debugger (the title says
  **Debug:** while **⇧** is held), and **⌥⏎** runs a command in the current file's folder (**Run in
  Context:**). The list at the top right sets where commands run: **Project** (the project's folder),
  **Module** (the module of the current file) or **Current File** (its folder).

The shortcut is **Other → Run Anything** in Settings → Keymap. **Turn off shortcuts made
of a modifier pressed twice** in Settings → Advanced Settings → User Interface turns off **⌃⌃** and **⇧⇧** (Search
Everywhere) both.

## Run… and Debug…

**Run → Run…** (**⌃⌥R**) and **Run → Debug…** (**⌃⌥D**) open a list of the run configurations, with
the configuration at the caret last when it is not saved yet. **⏎** runs or debugs the chosen one and
makes it the run widget's selection; **⇧⏎** does the other one (debugs from Run…, runs from Debug…).
Typing filters the list, a digit picks the item with that number, and **0** or the first item,
**Edit Configurations…**, opens the Run/Debug Configurations dialog.

## Run configurations

A configuration has a name, a kind (for example building a package or running its tests) with the
kind's own settings, and settings every kind has: the working directory, environment variables,
the program's arguments and the tool's arguments (`go` or `cargo`). The Cargo kinds of a Rust
project are in [Rust](06-rust.md#run-and-test); with the [Node.js plug-in](06-web.md#nodejs), **Node.js** and **Node.js Tests** configurations run
files and Jest or Vitest; every IDE also has **npm** configurations, which run
a `package.json` script or a program of the project's packages with its package manager
([npm scripts](06-web.md#npm-scripts)), **Maven** and **Gradle** configurations, which run goals
or tasks with the project's `mvnw` or `gradlew` first ([Maven and Gradle](26-maven-and-gradle.md)),
**JUnit** and **TestNG** configurations, which run Java tests with Maven or Gradle, or on the JUnit
Platform by the IDE when the Gradle settings ask ([Java](06-java.md#run-tests),
[Gradle settings](26-maven-and-gradle.md#gradle-settings)), Java **Application** and **JAR Application** configurations
([Run a Java program](06-java.md#run-a-java-program)), **Spring Boot** configurations with the Spring
Boot plug-in ([Spring Boot](31-spring-boot.md#run-an-application)), and a plug-in may add configurations to the projects it
supports (Nuxt's `nuxi dev`, [Nuxt](06-web.md#nuxt)). Running a context (⌃⇧R, a gutter icon) makes a
temporary configuration; at most five are kept (**Settings → Advanced Settings → Run/Debug →
Keep at most this many temporary configurations**), and the oldest goes first. Save one to keep it.

Saved configurations are kept in `.zen/runConfigurations.json` in the project; commit the file to
share them.

A project shared with a JetBrains IDE also brings its shared Go configurations (Go Application and
Go Test, from `.idea/runConfigurations/` and `.run/`; Application and JAR Application for Java). They are listed after yours, run and can be
selected, but GoGarden never changes the JetBrains files: in the dialog their settings are greyed
out, **−** is off, and **Save as Zen Configuration** (the save button) copies one into
`.zen/runConfigurations.json` under the same name, where you can change it; the copy then takes its
place in the list.

## The Run/Debug Configurations dialog

**Run → Edit Configurations…**, or **Edit Configurations…** in the run widget's list, opens the
dialog.

- On the left, the configurations grouped by kind. **+** lists the kinds to add one of, **−**
  removes the selected configuration, **Copy Configuration** duplicates it, and **Save
  Configuration** keeps a temporary one (**Save as Zen Configuration** for one from a JetBrains IDE).
  **Create New Folder** puts the selected configuration into a new folder under its kind; select the
  folder to rename it, drag configurations onto it to move them in (onto the kind to move them out),
  and **−** on a folder removes the folder but keeps its configurations. The **Search** field above
  the list shows only the configurations whose names contain what you type.
- On the right, the selected configuration's settings: **Name**, **Allow multiple instances**, the
  kind's own fields (text, paths with a folder button to choose one, choices, check boxes,
  environment tables), **Working directory**, the environment, **Go tool arguments**, **Program
  arguments**, then **Activate tool window** (on: the Run tool window opens when it starts) and
  **Focus tool window** (the Run tool window also takes the keyboard).
- **Environment variables:** takes them inline as `A=1;B=2`, and the table below shows the same
  variables (**+** adds a row, **−** removes one) with the system's variables, read-only, under
  **System environment variables**. A value can use the IDE's own variables as `$NAME$`, for
  example `PATH=/opt/tool/bin:$PATH$` (the name is case-sensitive).
- **Your login shell's environment:** a run gets the variables your login shell sets (`PATH`,
  `KUBECONFIG`, `JAVA_HOME`, a version manager's folders...), read once when the IDE starts, so a
  program behaves as in your terminal even when the IDE was started from the Finder or Zen Toolbox.
  The variables of the configuration and its `.env` files win over them. Turn it off in
  **Settings → Tools → Terminal → Environment** ([The terminal](07-terminal.md#environment)).
- **Environment files:** `.env` files, separated by `;`; the folder button (**Browse for .env files
  and scripts**) adds files. At each start they are read in order, then the variables above apply on
  top. A file that is missing stops the start with a message naming it.
- **Working directory**, **Go tool arguments** and **Program arguments** take macros such as
  `$ProjectFileDir$`, `$FilePath$`, `$FileDir$`, `$GOROOT$`, `$ClipboardContent$` and `$Prompt$`: the
  **+** button after the field (**Insert Macro**) lists them with their values now and inserts the
  one you click. They expand at every start, in configurations shared by a JetBrains IDE too; a value
  with spaces stays one argument. `$Prompt$` and `$FilePrompt$` ask for the value when the run
  starts.
- Changes stay in the dialog until **Apply** or **OK**; **Cancel** (or Escape) discards them. **OK**
  also selects the configuration shown in the run widget.
- A configuration without a name, or with the name of another one of its kind, is shown in red with
  the reason under its settings, and the dialog does not save until it is fixed.

## Compound configurations

A **Compound** configuration (**+ → Compound** in the dialog) starts several configurations at once.
Its page lists them: **+** adds one of the saved configurations, **−** removes the selected one.
**Run** starts every one of them, each in a tab of its own in the Run tool window; a compound inside
a compound starts its configurations too, and a configuration listed twice runs once. **Debug**
starts the ones that can be debugged in the debugger and runs the others. While a compound runs, the
run widget's **Stop** stops every configuration it started, and **Rerun** starts them all again. A
compound that contains itself, directly or through another compound, is shown in red with the
reason, and the dialog does not save it. Compound configurations from the `.run` files of a JetBrains
IDE are adopted too, with the configurations they name. In a project you have not trusted, a
compound starts none of the project's saved configurations, as they would not start alone.

## Configuration templates

**Edit configuration templates…** at the bottom of the dialog's list shows one template per kind.
A template has the settings of a configuration without its name: what you set there is what a new
configuration of that kind starts with, from **+** and from running a context (**⌃⇧R**, the gutter
icons). Configurations that exist already do not change. **Back to configurations** returns to the
list; **Apply** or **OK** saves templates and configurations together. Templates are kept in
`.zen/runConfigurations.json`, with the configurations; a project you have not trusted runs a
context without them.

## The Services tool window

**View → Tool Windows → Services** (**⌘8**, **Alt+8** on Windows and Linux) shows run
configurations and, below them, the HTTP Client's responses ([HTTP Client](20-http-client.md)).

- **Add Service** (**+**) lists the kinds of run configuration under **Run Configuration Type**:
  choose one to show all its configurations, grouped under the kind, each with where its latest run
  is (**Running**, **Finished**, **Stopped**, **Failed** or **Not started**).
- Select a configuration to see its latest run's console on the right. **Run**, **Debug**, **Stop**
  and **Rerun** on the toolbar act on it.
- **Hide Configuration** hides the selected configuration; **Restore Hidden Configurations** shows
  the hidden ones again. With a kind selected, the same button is **Remove from Services**.
- What the window shows is kept with the project's window and comes back when it reopens.

## Before launch

Under its settings, every configuration has a **Before launch** list: tasks that run, in order, each
time the configuration starts, before its own programs.

- The toolbar above the list has **+** (Add, also ⌘N or Alt+Insert once you have clicked the list),
  **−** (Remove, also Alt+Delete), **Edit** (Enter, or double-click a task), **Move Up** (⌥↑) and
  **Move Down** (⌥↓). **+** lists the kinds of task: **Run Another Configuration** (it runs in a tab of
  its own, and the launch waits for it to end), **Run External Tool** (one of your external tools),
  **Activate Tool Window**, **Save Files**, **Build Artifacts** (tick one or more of the project's
  [artifacts](05-project.md#artifacts); the launch waits for them to build), **Run File Watchers**
  (see [File watchers](#file-watchers)), and what the language modules and plug-ins add: **Run Go
  Command** (`generate ./...`, `vet ./...` or any words after `go`, in a folder of the project you
  choose), **Run Maven Goal** and **Run Gradle Task**. A new task opens its settings under the list;
  the list names each task with what it runs, such as Run Go Command “generate ./...”.
- **Show this page** opens this dialog on the configuration each time you run or debug it from the
  window; its **Run** (or **Debug**) button then starts it. Starts by AI agents never wait for it.
- Changes are kept with **Apply** or **OK**, as for the other settings.
- When the configuration starts, its Run tab shows each task's `#before-launch` line and output
  first. A task that fails (a program that exits with another code than 0, a configuration or tool
  that is not there) stops the launch: the tab says which task failed and why, and the
  configuration does not start. **Stop** during the tasks stops the running one and starts nothing.
- A configuration adopted from the run configuration files of another IDE keeps its tasks where
  the IDE knows them; a task it does not know is listed as not supported and skipped.

## The Run tool window

Every run opens the Run tool window (**⌘4**, or the Run button on the left stripe, which shows a dot
while something runs). Each run has a tab with its status icon and name; running a configuration
again reuses its tab, and **×** on a tab closes it, stopping the run if it still runs.

- The toolbar on the left has **Rerun** (stops the run if it still runs, then starts it again in the
  same tab) and **Stop** (**⌘F2**). For tests it also has **Rerun Failed Tests**, **Show Passed** and
  **Show Ignored**.
- The console shows the command line, the program's output (standard error in red) and how the run
  ended: `Process finished with exit code 0`, or `Process stopped`. It follows the end of the output
  unless you scroll up. Drag to select text, **⌘C** copies it, **⌘A** selects everything. The
  console keeps the last 1024 KB of output and drops the oldest lines beyond that (**Settings →
  Editor → General → Console → Override console cycle buffer size** changes the size); a line longer
  than 64 KB is shown in pieces.
- File positions such as `calc_test.go:17` or `/path/main.go:12:5` are links: they underline when you
  point at them, and a click opens the file at that line and column.
- A test run shows the tests as a tree on the left: packages, tests and subtests, each with how it
  went (passed, failed, skipped, running or terminated) and how long it took. Above the console a
  line says how many tests passed and failed. Click a test to see only its output; double-click it
  (or press **Enter**) to open the place it failed, or where it is written when no failure names a
  place (for Go tests; a subtest opens its test function). When the run ends, failed tests move to
  the top of each level and the first one is selected.
- **Rerun Failed Tests** runs only the tests that failed, with the same settings. Right-click a test
  or a package and choose **Run '…'** to run just that one in the same tab.
- **Test History** (the clock button under the filters, or **Run → Test History…**) lists the
  project's earlier test runs, newest first, each with how it went, its configuration and when it
  ended. **↑** and **↓** choose, **Enter** or a click reopens the run in a tab of its own with its test
  tree and console as they were; **Rerun** there starts its configuration again. The last 20 runs
  that ran tests are kept across restarts, in the IDE's data folder, not in the project.

## Go to Test and Create Test

**Navigate → Test** (**⇧⌘T**) jumps between code and its tests, by the naming rules of the file's
language (Go and Rust are built in; a plug-in adds its language's rules):

- From a function, method or type it goes to the tests named after it, from a test to the code it
  tests. With nothing at the caret it goes to the paired file: from `cart.go` to `cart_test.go` and
  back. One target: it goes there; several: a list "Tests for … (n)" opens, as for Go to
  Implementation.
- When code has no test, the list offers **Create New Test…**, which opens the **Create Test**
  dialog: the **Testing library** (the frameworks the project can use), the test's **Name**, the
  **Destination** folder (typed, or one of the usual ones from the button beside it), and the
  functions and methods to **Generate test methods for** (the one at the caret checked). **OK**
  writes the test file, or adds the stubs to the one there is as an unsaved change, skips members
  that have a test already, and puts the caret on the first stub. The destination is a folder of
  the project: one outside it is refused, and an AI agent cannot write a test where files are hidden
  from agents.
- Go: `cart.go` pairs with `cart_test.go`; `Total` is tested by `TestTotal`, `Cart.Total` by
  `TestCart_Total`, `price` by `Test_price`, and `Benchmark…`, `Example…` and `Fuzz…` count too, in
  any test file of the package. A new test is a table-driven `TestXxx` per member, with Go's testing
  package or Testify when `go.mod` requires it.
- Rust: the tests of `src/cart.rs` are in its `#[cfg(test)] mod tests`, in `src/cart/tests.rs` or in
  the crate's `tests/cart.rs`; `price` is tested by `price…` or `test_price…`, `Cart::total` by
  `cart_total…` or `test_cart_total…`. The dialog's destination is the subject's own folder (the stubs
  go into its `tests` module, which is made when there is none) or the crate's `tests/` folder; the
  library is `#[test]`, or `#[tokio::test]` when the crate uses Tokio.
- Java and Kotlin (with their plug-ins): `src/main/java/…/Cart.java` pairs with `CartTest`,
  `CartTests`, `CartIT` or `TestCart` in `src/test/java/…` of the same module and package (Kotlin in
  `src/main/kotlin` and `src/test/kotlin`); `total` is tested by `total…` or `testTotal…`. The library
  is JUnit 5, JUnit 4 or TestNG (Kotlin adds kotlin.test), the ones the build file names first, and a
  new test class has `setUp` and `tearDown` ([Java](06-java.md#go-to-test-and-create-test),
  [Kotlin](06-kotlin.md#go-to-test-and-create-test)).

For agents: `navigate.test.goTo`, `navigate.test.options` (what the dialog offers) and
`navigate.test.create` (what OK does); `run.history.list` and `run.history.open`.

## Run icons in the gutter

When a file has something to run, the gutter shows a green ▷ next to it: `func main`,
tests, benchmarks, examples and fuzz tests, in a Java file each test class and test method (JUnit 5
and 4, TestNG), in a Kotlin file its mains, test classes and test functions
([Kotlin](06-kotlin.md#run-test-and-debug)), and in a `package.json` each of its scripts. Click the icon to open its menu. To keep the gutter narrow, turn them off in **Settings → Editor →
Gutter Icons → Show run icons in the gutter** (for agents, the setting `gutterRunIcons`); then run and
debug through the run widget, the **Run** menu, **⌃⇧R** and **⌃⇧D** at the caret.

- **Run '<name>'** (⌃⇧R) runs it at once and opens the Run tool window. It runs the same
  configuration as Run Context Configuration on that line: a saved one with the same settings, or a
  temporary one, which it selects.
- **Debug '<name>'** (⌃⇧D) starts the same configuration in the debugger and opens the Debug tool
  window.
- **Run '<name>' with Coverage** runs it and then shows which lines the tests ran (see
  [Code coverage](#code-coverage)). It is there only when the configuration can measure coverage.
- **Modify Run Configuration…** selects that configuration (saving it as a temporary one when there
  is none yet) and opens Edit Configurations.

After a test run, the icon shows how that test last went: a check mark when it passed, a red cross
when it failed, an amber mark when its subtests went different ways or some were skipped. The mark
comes from the latest run that covered the test in that package. With the caret inside a test or
`main`, **⌃⇧R** (Run → Run Context Configuration) runs it without the mouse.

A scratch file gets the same ▷ when its language can run one file on its own (Go, Rust and Java):
it runs that file alone with the project's toolchain, as **Go Build** with Run kind File, **Rust
File** or **Single-File Java Program** ([Running a scratch file](25-scratches.md#running-a-scratch-file)).

## Profile Go tests

Record where Go tests and benchmarks spend processor time or allocate memory with the command
`profile.run.start`: it runs a test configuration with `go test -cpuprofile` or `-memprofile`, reads
the profile and says in the run's console how many samples it has and which function cost the most.
A test configuration must name one package, because `go test` records one package at a time. A
program that writes its own profile with `runtime/pprof` opens with `profile.file.open`.

There is no Profiler tool window yet, so this works through commands, which AI agents and scripts
can call: `profile.report.get` lists the costliest functions, `profile.tree.get` gives the call tree
and `profile.frame.open` goes to a function in the code ([Automation](../automation.md#profiles)).

## Code coverage

Run tests with coverage to see which lines they ran. **Run → Run with Coverage** runs the selected
configuration, the run icon's menu in the gutter runs the test at that line, and the run widget's
**More Actions** (⋮) runs the selected one. The run shows in the Run tool window as any run; when it
ends (failing tests too), its console says how much it covered, and the coverage shows:

- **In the gutter**, a mark next to every measured line: green when it ran, amber when it ran only
  partly (some of its statements or branches did not run), red when it did not run. Point at a mark
  to see how many times the line ran and, when the tool counts them, how many of its branches ran.
  The colours are the theme's (`editor.coverage.covered`, `editor.coverage.partly`,
  `editor.coverage.notCovered` in [themes](../themes.md)).
- **In the Coverage tool window** (on the right; View → Tool Windows → Coverage), a tree of the
  folders (packages) and files that were measured, each with the share of its lines that ran and
  how many of how many. Click a folder to fold it, a file to open it. **Hide Coverage** (×) removes
  the marks and empties the window until the next run with coverage; **Generate Coverage Report**
  asks for a folder and writes `index.html` there, with every file's percentage and its source
  with the lines marked, and opens it in the browser.
- **In the Project tree**, next to every measured file and the folders that hold them, the share
  of lines that ran (`85% lines`). **Hide Coverage** removes these too.

Each new run with coverage replaces what is shown. Coverage is measured by the language's own tools:

- **Go**: test configurations run as `go test -coverprofile … -covermode=count` (`atomic` with
  `-race`); the files of every module of the project are shown.
- **Rust**: Cargo Test configurations run with `cargo llvm-cov`, which must be installed
  (`cargo install cargo-llvm-cov`; it uses the toolchain's `llvm-tools-preview` component). The IDE
  never installs it: without it, Run with Coverage says what is missing. Documentation tests cannot
  be measured (they need a nightly toolchain).
- **Java and Kotlin**: JUnit and TestNG configurations (Maven and Gradle) and Application and JAR
  Application configurations run with JaCoCo. A build that already uses JaCoCo's Maven or Gradle
  plug-in runs with it; otherwise the IDE attaches the JaCoCo agent from the local Maven repository
  or the Gradle cache. Without either, **Download JaCoCo** (Find Action) fetches it from Maven
  Central once you ask ([Java](06-java.md#coverage)).

Agents use the commands `coverage.run.start`, `coverage.report.get`, `coverage.file.get`,
`coverage.report.hide`, `coverage.report.export` and `java.jacoco.download`
([automation](../automation.md#code-coverage)).

## Breakpoints in the gutter

Click a line number to set a line breakpoint there: a red dot takes the number's place, and a
second click removes it. Pointing at a line number shows a faint dot where a click would put one.
**⌘F8** (Run → Toggle Line Breakpoint) does the same at the caret; **⇧-click** on a line number
selects the line instead. A disabled breakpoint is a red ring; one the debugger could not set is a
red dot with a cross, and pointing at it shows why.

While the debugger is stopped in the file, the line it stopped at is highlighted and an arrow marks
it in the gutter. Step with **F8** (Step Over), **F7** (Step Into) and **⇧F8** (Step Out), continue
with **⌥⌘R** (Resume Program), run to the caret with **⌥F9**; **⌘F2** stops the session as it stops
a run. In a diff, F7 still goes to the next difference.

## External tools

An external tool is a program you set up once, such as a linter, a formatter or a script, and then
run on the current file or the project from a menu or a shortcut.

- Set tools up in **Settings → Tools → External Tools**. The list shows the tools by group, each
  with a check box that switches it on or off; the toolbar has **+** (Add), **−** (Remove), **Edit**,
  **Move Up**, **Move Down** and **Copy**, and double-clicking a tool edits it. With no tool the list
  says "No external tools configured". The tools belong to the IDE, not to a project: they are kept
  in `externalTools.json` in the IDE's data folder and every project has them.
- **Create Tool** and **Edit Tool** ask for the **Name**, the **Group** (`External Tools` by
  default), a **Description**, the **Program** (a path, or a name found on `PATH`), the
  **Arguments** and the **Working directory** (the project's folder when empty, relative to it
  otherwise). **+** (Insert Macro) next to each field lists the macros by name, each as "Name - what
  it stands for", shows the selected one's value now, and inserts the one chosen at the caret; under
  each field a preview shows it with the macros expanded. **Advanced Options**, folded until you
  open them: **Synchronize files after execution** (on by default) rereads the open files and the
  project's folders when the program ends; **Open console for tool output** gives the run a Run tool
  window tab; **Make console active on message in stdout / stderr** brings that tab forward when the
  program writes there; **Output filters** are regular expressions, one per line, in which
  `$FILE_PATH$`, `$LINE$` and `$COLUMN$` mark a file, a line and a column, so that
  `$FILE_PATH$:$LINE$:$COLUMN$` turns `/src/main.go:12:5` into a link.
- The macros: `$FilePath$`, `$FileDir$`, `$FileName$`, `$FileNameWithoutExtension$` (the current
  file: the editor's, or the one chosen in the Project view), `$ProjectFileDir$`, `$ProjectName$`,
  `$SelectedText$`, `$LineNumber$` and `$ColumnNumber$` (the caret, from 1), `$Prompt$` (a text
  asked for when the tool starts), `$FilePrompt$` (a file asked for when it starts),
  `$ClipboardContent$`, `$GOROOT$`, `$GOPATH$`, `$GoExecPath$` (the Go SDK's `go` program),
  `$GoFileImportPath$` (the import path of the current file's package, from the nearest `go.mod`),
  `$OSName$` (`macos`, `linux` or `windows`) and `$TempDir$` (the folder for temporary files). Each
  argument gets a macro's value as one argument, spaces and quotes included, and the program never
  runs through a shell. A tool that uses a macro without a value now (`$FilePath$` with no file
  open, `$SelectedText$` with nothing selected) does not start, and a notice says which macro and
  why.
- Run a tool from the context menu of the Project view (the file macros then name the file you
  clicked) or of the editor: each group of tools is a submenu named after the group, **External
  Tools** for the default one, in the Project view before **Mark Directory As**. The menus have no
  such submenu while no tool of the group is on, and the Tools menu has none. Each
  tool is also an action in **Settings → Keymap → External Tools**, where it can get a shortcut, and
  in Find Action (⇧⌘A).
- A run saves the files first, then opens a Run tool window tab named after the tool, with **Stop**
  and **Rerun** as for any run: the command line first, then the program's output, then "Process
  finished with exit code 0" or the code it ended with. Rerun starts the same command line again.

## File watchers

A file watcher runs a program by itself when files change: when you save a file, or when another
program changes it. Use one to format, compile, transpile or check files as you work.

- Set watchers up in **Settings → Tools → File Watchers**. The table lists them with the columns
  **Enabled**, **Name** and **Level**; the toolbar has **+**, **−**, **Edit**, **Move Up**,
  **Move Down**, **Copy**, **Import** and **Export**, and double-clicking a watcher edits it. With
  no watcher the table says "No file watchers configured". **+** offers **<custom>** and the
  templates: go fmt, goimports and golangci-lint in an IDE with Go, and Prettier, TypeScript and
  SCSS in every IDE. **All actions on save…** leads to **Tools → Actions on Save**, whose **File
  Watcher** row names the enabled watchers.
- A **Project** watcher is kept in `.zen/fileWatchers.json`, so it travels with the project
  through Git; a **Global** one in `fileWatchers.json` in the IDE's data folder, for every project.
  **Export** writes the watchers into a file and **Import** adds those of such a file.
- **New Watcher** and **Edit Watcher** ask for the **Name**, the **Level**, the **File type** it
  watches (**Any** for every file), the **Scope** (**Project Files**: the project's files outside
  excluded folders; **Open Files**: the files open in tabs, even outside the project; **Changed
  Files**: the files Git sees as changed), the **Program**, its **Arguments**, the **Output paths to
  refresh** (separated by `:`, relative to the working directory), the **Working directory** (the
  project's folder when empty) and the **Environment variables** (`NAME=value;OTHER=value`). The
  same macros as external tools work in all of them (**+** inserts one), with the changed file as the
  current file: `$FilePath$`, `$FileDir$`, `$FileName$`, `$FileNameWithoutExtension$`,
  `$ProjectFileDir$` and the others.
- **Advanced Options**: **Auto-save edited files to trigger the watcher** (on by default) saves a
  file a second after you stop typing in it so that the watcher runs; off, the watcher runs only
  when you save. **Trigger the watcher on external changes** (on) runs it when another program
  changes the file. **Trigger the watcher regardless of syntax errors** (off) runs it even while the
  file has syntax errors. **Create output file from stdout** writes what the program prints into the
  first output path. **Show console**: **Always** opens a Run tool window tab for every run, **On
  error** (the default) only for a run that failed, **Never** for none. **Output filters** are
  regular expressions, one per line, with `$FILE_PATH$`, `$LINE$`, `$COLUMN$` and `$MESSAGE$`: each
  match becomes a problem of that file in the Problems tool window and in the editor, so
  `$FILE_PATH$:$LINE$:$COLUMN$: $MESSAGE$` turns a linter's `main.go:12:5: unused x` into a
  warning on line 12.
- A watcher runs a moment (0.3 s) after the last save or change of a file, so that a burst of
  changes makes one run. Runs of one watcher go one after another; each shows as a background task
  in the status bar. After a run, the file and the output paths are read again: an open file without
  unsaved changes shows what the program wrote. The changes a watcher makes do not start it again.
- A watcher whose program cannot start, or that ends with an exit code other than 0 without its
  output filters finding anything, is switched off, and a notification says why; **Enable again**
  switches it back on.
- A project's own watchers (the ones in `.zen/fileWatchers.json`, which come with a cloned
  repository) run only once you trust the project with **Trust Project**; global watchers always
  run.
- To run watchers each time a run configuration starts, add **Run File Watchers** to its **Before
  launch** list ([Before launch](#before-launch)). Its settings choose the **File watcher** (**All
  file watchers**, the default, or one of them) and the **Files**: **Changed files** (the files Git
  sees as changed, the default), **Open files** or **All files**, each only in the configuration's
  working directory (the project's folder by default). Every enabled watcher whose file type and
  scope match a file runs on it, whatever its trigger options say, and the launch waits for the runs:
  a run that fails, or a watcher that is switched off, stops the launch.

## Shortcuts

| Shortcut | Action |
|---|---|
| ⌃R | Run the selected configuration |
| ⌃⇧R | Run Context Configuration: what is at the caret |
| ⌃⌃ | Run Anything |
| ⌃⌥R / ⌃⌥D | Run… / Debug…: choose a configuration to run or debug |
| ⌘F2 | Stop |
| ⌘4 | The Run tool window |
| ⇧⌘T | Go to Test: from code to its test and back |
| ⌃D | Debug the selected configuration |
| ⌃⇧D | Debug Context Configuration: what is at the caret |
| F8, F7, ⇧F8 | Step Over, Step Into, Step Out |
| ⌥⌘R | Resume Program |
| ⌥F9 | Run to Cursor |
| ⌘F8 | Toggle Line Breakpoint |
| ⌘5 | The Debug tool window |
