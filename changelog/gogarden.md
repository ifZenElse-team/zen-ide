# GoGarden changelog

Every release of GoGarden, the Go IDE, newest first. Each section is the release notes
users see as "What's new"; the format is in [README.md](README.md).

## 26.11.11 - 2026-11-28

### Fixed

- The buttons of the **Trust and Open Project** dialog, and the outlined buttons of the other
  dialogs, keep their size and place under the pointer: only their colour changes.

## 26.11.10 - 2026-11-27

### Changed

- The Kotlin language server is listed as **Kotlin LSP** in **Settings → Languages & Frameworks →
  Language Servers**; the notice beside it still says whose software it is.
- The **About** window, the website and the application's copyright line say that Zen IDE is an
  independent project, not affiliated with any other IDE vendor, without a list of products.
- The link of an internal error report opens the project's issues on GitHub.

### Fixed

- An Oracle connection whose SQLcl finds no Java says "Java is missing" every time: on a busy
  machine that has another Java installed it could say only "SQLcl ended".

## 26.11.9 - 2026-11-17

### Added

- Programs the IDE starts get your login shell's environment: runs, language servers, plug-in
  tool windows and terminal tabs see the same `PATH`, `KUBECONFIG`, version managers and proxies as
  your terminal, even when the IDE was started from the Finder or Zen Toolbox. The shell is read
  once, in the background, when the IDE starts; a run configuration's own variables still win.
  Turn it off in Settings → Tools → Terminal → Environment.
- The Kubernetes window's header names the kubeconfig files its contexts come from, with a missing
  one marked.
- Plug-in tool windows have icons of their own on the tool window bar, in your theme's colours, and
  are listed in View | Tool Windows. The Docker and Kubernetes windows open on the right; you can
  move them, and they stay where you put them. If you had them somewhere else, the IDE says once
  how to move them.
- **Diff with Cluster** and **Apply to Cluster** for a Kubernetes manifest file, from its run icon,
  and for a folder of manifests, from the Project view's context menu. Both use the context and
  namespace chosen in the Kubernetes window. Diff opens each changed object in the diff viewer,
  with the cluster's object now on the left and the file's result on the right. Apply, and every
  other kubectl command that changes a cluster, runs only when you start it and confirm the whole
  command line; AI agents can never start one.
- Plug-in tool window actions can ask for values you type: the Kubernetes window's **Port
  Forward…** (pods and services, on 127.0.0.1 only) and **Scale…** (deployments). Values are
  checked before anything runs, and an AI agent never types them: it asks, and you enter them.
- **Settings → Languages & Frameworks → Go → Go Modules** sets the project's Go environment
  (`GOPROXY`, `GOPRIVATE`, `GOFLAGS`…): shared with the project in `.zen/go.json`, or only on this
  computer, where your values win. It reaches every `go` command the IDE runs for the project
  (runs, tests, debugging, the go.mod checks, `go vet`, installing gopls and Delve) and gopls. The
  shared file applies only in a trusted project and never holds a password.

### Changed

- The New Project wizard's **Environment** is saved with the new Go project in `.zen/go.json`
  instead of being used for `go mod init` only.

### Fixed

- The Kubernetes window lists your kubeconfig's contexts and the cluster's resources: before, it
  showed no context at all ("There is nothing to choose for Context"). The run icon on a shell
  script's first line appears too.
- A background task that starts or ends no longer moves the status bar's widgets (line and column,
  encoding, indentation) under the pointer: its progress shows to their left.
- A security fix in database connections over SSH: the IDE took the tunnel as open as soon as
  anything accepted a connection on its local port, so another program listening there could have
  received the database's credentials. It now waits for ssh itself to report the forward on that
  port, and fails the connection if ssh exits or never reports it.

## 26.11.8 - 2026-11-14

### Added

- **Two new Zen IDEs, SqlScroll and DevopsDaruma**, are in Zen Toolbox: SqlScroll for SQL and
  databases, DevopsDaruma for DevOps work. They share this IDE's platform, version and plug-ins.

## 26.11.7 - 2026-11-14

### Added

- **Run a shell script** from the run icon on its first line: with the shell its hashbang names
  (and the options after it), or by its extension (`.sh` and `.command` with `sh`, `.bash`, `.zsh`,
  `.ksh`), in the script's folder. Only in a trusted project; AI agents only when you allow them in
  Settings → Plugins → Shell Script.
- A guide page on shell scripts: which files are scripts (hashbangs too), running them, and
  ShellCheck's problems through the Bash Language Server when ShellCheck is installed.
- `vcs.commit.create` with `push` takes `wait`: it waits for the push and answers its outcome in
  `push`.
- `search.text.find` takes `group: true` (each file's path once, its matches under it) and
  `compact: true` (rows of text), cheaper answers for AI agents; the default answer is unchanged.
- **Show Kotlin Bytecode**: View → Show Bytecode works in Kotlin files too, for the class at the
  caret (the file's class, such as `CartKt`, for top-level code), from the classes Gradle or Maven
  built. Decompiling to Java is not available yet.
- **Code → Convert Java File to Kotlin File** (⌥⇧⌘K) with the Kotlin plug-in 0.4.0: a `.kt` file
  beside the `.java` one, opened, with the Java file moved to the Trash; Java copied in the IDE and
  pasted into a Kotlin file is offered as Kotlin. What could not be carried over is reported with
  its line in the Java text and kept in a comment.
- **Track coroutines** in a Java or Kotlin Application configuration (off by default): debugging
  starts the program with the `kotlinx-coroutines-debug` agent of the project's own dependencies,
  taken from the classpath the run resolved, only in a trusted project, and shown in the Debug
  console's command line; without that jar the console says so and the program starts without it.
- Debugging a Java or Kotlin application turns on the coroutine debug mode
  (`-Dkotlinx.coroutines.debug`), so the threads show the coroutine they run (`main @coroutine#1`).
  Never for Run; off in Settings → Build, Execution, Deployment → Debugger → Kotlin coroutines, or
  for one configuration with Coroutine debug mode.
- The Debug tool window of a Java or Kotlin run has a **Coroutines** tab: with the coroutine debug
  agent in the program (Track coroutines), every coroutine with its name, state, dispatcher and the
  frames where it last ran, read once at each stop by a breakpoint or a step.
- **Settings → Languages & Frameworks → Schemas** lists the schemas of the plug-ins and lets you map
  your own: a shipped schema, a JSON Schema file of the project or a web address, to file patterns.
- **Allow downloading schemas a file names** (off by default, yours alone) lets the IDE download a
  schema a YAML file names with an `http` or `https` address, never in a project you have not trusted,
  with a notification naming the address before the download starts.
- YAML files are completed and checked offline against schemas that come with the plug-ins:
  `.gitlab-ci.yml` (GitLab CI), GitHub workflows and Kubernetes 1.34 manifests (YAML).
  A file can name one of them, or a schema file of the project, in a comment.
- `get_window_state` tells an agent `headless` and, in `session`, the project, its branch, how many
  files changed, the language servers' states and whether the index is ready, in one call.
- Terminal tabs come back after a restart of the computer or a crash of the terminal host: each
  pane shows its old output dimmed under a line saying why the session ended, starts a new shell
  in the same folder with the environment it was given, and offers its last command again with
  Run Again and Dismiss; nothing runs by itself. Settings → Tools → Terminal → Sessions → Restore
  terminal tabs after a restart turns it off. macOS and Linux.
- AI agents in the Terminal continue their conversation after a restart of the computer: a tab
  whose agent reported its conversation id runs the agent's resume command in its new shell.
  Adapters for Claude Code, Codex and Qwen Code come with the new base plug-in AI Agents in the
  Terminal; Settings → Tools → Terminal → AI Agents adds one for any other agent and shows the
  session-start hook each agent needs. Every terminal has `ZEN_TERMINAL_ID` in its environment.
  An agent without an adapter gets the offer to run its command again, and so does an agent whose
  adapter comes from a plug-in you installed yourself. macOS and Linux.
- A terminal that the IDE attaches to again shows its screen and history as they are now, with what
  was printed while the IDE was away, then goes on with the live output; full-screen programs such
  as AI agents' interfaces are shown as they were and redraw at the pane's size.
- **Help → Open-source Licences** (on macOS also under **About** in the application menu) shows the
  licences of everything the IDE contains: Zen IDE's own, every library's notices and the fonts'
  licences.
- Terraform's `init`, `validate`, `plan` and `apply` from the run icon of a module's `terraform`
  block (Terraform plug-in). Commands that change state or infrastructure (`apply`, `destroy`,
  `import`, `state`...) are the person's: the IDE shows the whole command line and folder and asks
  every time, and AI agents never start them, whatever the plug-in's agent setting.
- The **Kubernetes** plug-in: a tool window over your clusters through `kubectl`. It lists pods,
  deployments, services, config maps, secrets (by name only), events, namespaces and contexts, in
  the context and namespace you choose in its header. That choice is never written into your
  kubeconfig. It shows a pod's log and opens a shell in it, describes, restarts a rollout and
  deletes, and every change asks first, naming the context.
- The Terminal selects text: drag, double-click for a word (a path or a web address whole),
  triple-click for a whole line, Shift-click to extend and ⌥-drag for a rectangle. The selection
  stays on its text while programs print and while you scroll, and takes its colour from the theme.
- **⌘C** copies the terminal's selection without blanks at line ends and with wrapped lines put back
  together; with nothing selected it still sends Ctrl+C. The context menu has **Copy**, **Copy as
  Plain Text**, **Paste** and **Select All**.
- Agents read the terminal's selection and copy a range of its lines (`terminal.selection.get`,
  `terminal.selection.copy`, `terminal.selection.selectAll`, `terminal.lines.copy`).
- Updating the IDE never ends its terminals: the IDE runs its own copy of `zen-pty-host` from the
  data folder (`terminal/bin/<protocol>/<build>/`), so replacing the installation leaves the shells
  and agents in them running, and the updated IDE attaches to them again. A tab whose shell runs in
  an older terminal host the new version cannot read says so and closes when that host stops; new
  tabs open in a new host. macOS and Linux.
- Every IDE carries the licence notices of everything it contains: Zen IDE's licences, the texts of
  every library and font, in `Contents/Resources/Licenses/` (`Licenses/` on Linux and Windows).
- Terminal sessions outlive the IDE: quitting, closing the project window or a crash leaves the
  shells and everything in them (builds, servers, AI agents) running in a small helper,
  `zen-pty-host`, and the next start attaches the same tabs to them. Closing a tab still ends its
  shell. macOS and Linux.
- The Go type of New Project asks for GOROOT (the SDKs found, or a folder of your own) and an
  environment for `go mod init`; Create stays disabled, saying why, without a Go SDK.
- The Docker plug-in completes the keys of Docker Compose files and the values of keys that take one
  of a few, and reports keys the Compose Specification does not have, values a key does not take and
  the obsolete `version`, without the network.
- Plug-ins can offer run icons in their files: the IDE runs the programs they declare, as run
  configurations of the kind **Plug-in target**, never through a shell and never in a project you
  have not trusted. The Docker plug-in runs a Compose file's services this way.
- **Settings → Plugins** lists the programs a plug-in runs, and **Agents may run its targets** says
  what AI agents may do: no (the default), ask each time, or yes.
- The Docker plug-in completes the instructions, flags and parser directives of Dockerfiles and
  reports their problems (an unknown instruction or flag, an instruction before `FROM`, missing
  arguments, a deprecated `MAINTAINER`), in pure Rust and without the network.
- A Docker tool window in every IDE with the Docker plug-in: containers, images, volumes and
  networks, with start, stop, restart, a container's log and a shell in it, pull, and delete and
  prune, which ask you every time. It runs the `docker` command, so your Docker context and logins
  apply, and nothing runs in a project you have not trusted.
- Plug-ins can have tool windows of their own: a button on the tool window bar opens a window with
  sections of rows (for example a container's name, image and status, with a dot for its state),
  refreshed every 5 seconds while it is open, with the section's actions on the toolbar and in
  each row's context menu. Anything that cannot be undone asks you first.

### Changed

- An AI agent's edit through `editor.text.replace`, `editor.text.write`, `editor.text.batch` and
  `refactor.rename.apply` is saved by the time the call answers, unless it passes `save: false`, without the on-save actions (an explicit `save: true` runs them);
  a file changed on disk under unsaved edits makes the call fail instead of being overwritten.
- The YAML language server no longer downloads schemas from the network: YAML files are checked for
  their syntax only, without schemas from the public schema catalog, until schemas that come with
  the IDE and work offline arrive.
- The YAML language server no longer downloads a schema a file names with a web address; the file
  says that schema downloads are off. Plug-ins can ship schemas that the IDE gives to the server,
  offline.
- `project.module.info` answers the modules in a few words (`name folder` rows, their count and the
  module files' names), the workspace file without its member list, and the problems; `module` gives
  one module in full and `all` every one.
- The TypeScript Language Server no longer installs `@types` packages from npm by itself (automatic
  type acquisition is off): the types of a library come from the project's `node_modules`.
- The JSON language server no longer downloads a schema a file names with a web address unless you
  allowed schema downloads, and Taplo no longer reads the public schema catalog or downloads schemas:
  JSON and TOML files are checked offline.
- The run mark in the editor's gutter is our own drawing: a play sign in a ring.
- Bookmark commands (`navigate.bookmark.*`, `view.bookmark.*`) and `search.text.edit` take and give
  lines and columns from 1, as search, navigation and problems do; 0 is refused.
- Reworded messages, dialog titles and settings labels in English and Russian, among them the power
  saving notice, the question when a search reaches its result limit, spelling problems, the
  macro dialogs, Clipboard History, Keep My Edits, the lists of implementations, super methods and
  tests, and many Advanced Settings and Editor options. Their meaning, defaults and shortcuts stay.
- The details of a Go dependency's vulnerabilities name the licence of the Go vulnerability
  database's reports, CC BY 4.0.
- `http.requests.list` gives a request's `line`, `startLine` and `endLine` counted from 1, as every
  other answer an AI agent reads does.

### Removed

- The Docker plug-in no longer uses the Dockerfile language server (a Node.js program from npm);
  a server installed before is no longer started, and its settings break nothing.

### Fixed

- The outcome of a push, fetch, update, tag push or submodule update reads in English for an agent
  and in the window's language in the IDE, whoever started it.
- Page Down, Page Up, Home and End scroll a Settings page, for a person and through `press_keys`.
- In a project that is not trusted, the problems and searches of a file whose language server does not
  start say so (`notTrusted`, trust the project) instead of asking to open one of its files.
- A file edited through the IDE counts as checked two seconds after the change when its language
  server published nothing new (rust-analyzer publishes only diagnostics that changed) and has no work
  under way, instead of staying "checking".
- Go to Symbol and Go to Class wait the whole `waitMs` an AI agent gives, up to 120 s, also after the
  language server has loaded, instead of answering `pending` after 10 s.
- Find Usages asks the language server again, up to three times, when the server dropped the search
  for a change (`content modified`, `server cancelled`), instead of answering an error.
- The schemas the YAML and GitLab CI plug-ins ship reach the IDEs that bundle them: the bundled
  plug-ins' manifests had left them out.
- An AI agent connected with `<ide> mcp` keeps its tools when the IDE restarts: a call waits up to
  15 seconds for the IDE to come back before the bridge starts it, and fails with a clear error when
  none comes.
- Notifications and a run's before-launch failure read in English for an AI agent and in the window's
  language in the IDE, whichever language they came in.
- An AI agent reads the paths of Go to Declaration's targets (and Implementation, Type Declaration
  and Super Method), of a rename's preview and result, and of `file.fs.delete` relative to the
  project, as every other answer gives them.
- A Find in Files search run as a background task ends with the number of occurrences it found; it
  could say a few fewer when the files were searched in parallel.
- Renaming a file whose class the language server renames no longer leaves another file naming the old
  class without a word: the IDE asks the server again, and names the files it still left so that you can
  check them. Java's server sometimes left a usage unchanged when it was busy.
- `language.servers.status`, `lsp.servers.list` and the problems of a file tell an agent why a
  language server failed in English, and the window in its own language, whichever language the IDE
  was in when the server failed.
- Find in Files for AI agents (`search.text.find`) right after a project opens waits for the
  project's files to be listed instead of answering that nothing matches.
- Go to Symbol names a Rust method, associated function, constant or field with its type
  (`IdeApp.route_call`), as it does for Go, so two methods of the same name tell apart.

## 26.11.6 - 2026-11-09

### Changed

- A dragged editor or project tab lifts with a shadow and stays under the pointer where you took
  it; the other tabs slide aside with an easing and no longer flicker when the pointer goes back and
  forth over the middle of a tab, and on release the tab glides into place.

## 26.11.5 - 2026-11-09

### Changed

- Dragging an editor tab or a project tab along its row is a real drag and drop: the tab follows
  the pointer, the other tabs slide aside to open its place, and it settles there on release. With
  Reduce motion on in the system settings, tabs move without sliding.

## 26.11.4 - 2026-11-09

### Added

- Editor tabs move along their row by dragging; an unpinned tab never goes
  before a pinned one, and dragging a tab onto another split or out of the window works as before.
- Project tabs in the main window move along their row by dragging.

### Fixed

- The Commit and Commit and Push buttons of the Commit tool window stay in view in a low window: the
  list of changes and the message field get smaller first, a long message scrolls inside its field,
  and the line between the list and the message can be dragged; its place is remembered.

## 26.11.3 - 2026-11-07

### Added

- A panic inside the IDE shows as a red count in the status bar. Clicking it opens **IDE Internal Errors**: the message, the stack, a description field, **Clear All**, **Copy for Report** and **Open Issue**. The IDE sends nothing itself.
- A new Go scratch starts as a program, with the caret inside `main`.
- A Go scratch that imports more than the standard library runs against the project's Go module, through `-overlay`, so `go.mod` and the project's packages resolve.
- gopls checks such a scratch in the project's Go module too, so its imports resolve in the editor.
- **Refactor → Extract/Introduce → Variable… (⌥⌘V), Constant… (⌥⌘C), Field… (⌥⌘F), Parameter… (⌥⌘P), Method… (⌥⌘M)** and **Refactor → Inline… (⌥⌘N)**, done by the file's language server (Java, Go, Rust, TypeScript and JavaScript): a popup picks the expression when several qualify at the caret, and the new name is typed in place at every occurrence, with suggested names.
- **Navigate → Test** (**⇧⌘T**) jumps from a function, method or type to its tests and back, with a
  chooser when there are several; with no test it offers **Create New Test…**, whose dialog picks the
  testing library, the name, the destination in the project and the members to write test stubs
  for. Go and Rust come built in; a plug-in adds its language's rules.
- **Run → Test History…** and a clock button in the Run tool window list the project's last 20 test
  runs; one reopens in a tab of its own with its test tree and console.
- **Settings → Editor → Inspections** lists every inspection by group, gopls analyzers included,
  with a check box and a severity, a filter, Modified only and Cleanup only, and what you changed
  in blue.
- Inspection profiles: Project Default and your own, in the project (`.zen/inspectionProfiles/`) or
  the IDE, with Copy to IDE or Project, Duplicate, Rename, Delete, Export, Import and Restore
  Defaults. Inspect Code asks which one to use.
- A profile applies to the editor, the Problems tool window and Inspect Code, and turns gopls
  analyzers on and off in gopls.
- **Code → Code Cleanup…** and **Code → Analyze Code → Silent Code Cleanup** apply the fixes of the
  cleanup inspections in the background, as one undoable change per file.
- Suppress a problem for its statement, function or file from **⌥⏎**, the context menu of a row in
  the Problems tool window, or Inspection Results. The IDE writes the language's own mechanism:
  `//nolint:<id>` in Go and `#[allow(<id>)]` in Rust, and honours `//nolint` in the editor, Problems
  and Inspect Code.
- An agent reads and writes no suppression of a hidden file, and changes no inspection profile in a folder hidden from it.
- **Run with Coverage** for Java and Kotlin: JUnit and TestNG configurations of Maven and Gradle
  builds, Application and JAR Application configurations run with JaCoCo. A build's own JaCoCo
  plug-in is used when it has one; otherwise the JaCoCo 0.8.16 agent, checked against its SHA-512,
  from the IDE's data folder, the local Maven repository or the Gradle cache is attached. Lines are marked in `.java` and `.kt` files, partly when some of
  their branches did not run. **Download JaCoCo** fetches JaCoCo from Maven Central (checked against
  its SHA-512) only when you run it.
- **Run with Coverage** (Run menu, the run icon's menu in the gutter, the run widget's More Actions):
  Go tests with `go test -coverprofile`, Cargo tests with `cargo llvm-cov` when it is installed.
  Lines that ran, ran partly and did not run are marked green, amber and red in the gutter, with
  their hit counts on hover; the **Coverage** tool window lists folders and files with their
  percentages, hides coverage and generates an HTML report. A run with coverage starts as any run:
  it asks before starting again, runs its before-launch tasks, and waits until you trust the
  project for the configurations the project comes with.
- The Markdown preview typesets math: inline `$…$` in the line, display `$$…$$` and fenced `math`
  blocks centred, with fractions, scripts, roots, limits, growing delimiters, matrices and aligned
  rows, in the theme's colours. A formula that cannot be typeset shows its source with the reason;
  selecting and copying a formula gives its TeX.
- Text in the Markdown preview can be selected and copied: drag across blocks, double-click a word,
  triple-click a block, **⌘A** in the preview; **⌘C** copies plain text with list markers, task
  boxes and table cells kept readable, and the context menu adds **Copy as Markdown** (the source
  lines) and **Select All**. A click without a drag still moves the caret and follows links.
- Agents read the preview's selection with `markdown.preview.getSelection` and select or copy with
  `view.markdownPreview.selectAll`, `view.markdownPreview.copy` and
  `view.markdownPreview.copyMarkdown`.
- **Settings → Editor → Code Style** has **Hard wrap at**, **Wrap on typing** and **Visual guides**, and a **Formatter** tab with **Do not format** and **Turn formatter on/off with markers in code comments**.
- **Show Scheme Actions** has Copy to Project…, Copy to IDE…, Duplicate…, Reset, Rename…, Delete, Export → IntelliJ IDEA Code Style XML and Import Scheme…; the **Project** scheme is written to `.idea/codeStyles`.
- **Keep Tab Open** in the tab menu and in Window → Editor Tabs.
- **Copy Path/Reference…** in the tab menu also has File Name, Path From Project Root and Path From
  Repository Root.
- Editor tabs: **Pin Tab** (a pinned tab shows a pin, stays at the start of the row, survives Close
  Other Tabs, Close Tabs to the Left and Right and Close All but Pinned, and is kept with the project;
  click the pin to unpin), **Reopen Closed Tab** (the last closed file back in its group and place,
  pinned again if it was; no default shortcut, as ⇧⌘T is Go to Test), **Close All but Pinned**,
  **Close Tabs to the Left**, **Close Tabs to the Right**, **Close All Read-Only Tabs**, **Move to
  Opposite Group** and **Select Next / Previous Tab** (⇧⌘] or ⌃→, ⇧⌘[ or ⌃←).
- ⌥-click on a tab's × closes the other tabs.
- **Run File Watchers** in a run configuration's **Before launch** list runs every file watcher, or
  one, on the changed, open or all files of the configuration's folder before it starts; a failed
  run stops the launch.
- The Markdown preview draws ```` ```mermaid ```` diagrams in the theme's colours, offline: flowcharts
  and sequence, class and state diagrams among others. One that cannot be read shows its source with
  a note saying why.
- Selecting text in a Markdown file shows a floating toolbar: Text and H1–H6, Bold, Italic,
  Strikethrough, Code, Create Link, and bullet, numbered and check lists. **Advanced Settings →
  Markdown → Hide floating toolbar** turns it off.
- In Markdown files **⌘B** is Bold, **⌘I** Italic, **⇧⌘S** Strikethrough, **⇧⌘C** Code and **⇧⌘U**
  Create Link (on Linux and Windows with Ctrl).
- **Refactor → Pull Members Up…** and **Push Members Down…** in Java: tick the fields and methods to move into a superclass or interface of the project, or into every subclass, optionally leaving an abstract declaration, with a preview and a conflict for every use the move would break.
- **Refactor → Extract/Introduce → Superclass…** and **Interface…** in Java: a new superclass in the class's package, or an interface from the public methods you tick, in the package you choose.
- **Refactor → Encapsulate Fields…** in Java: the fields become private, get a getter and a setter with the names you give, and their uses in other classes call them.
- **Refactor → Extract/Introduce → Parameter Object…** in Java: the parameters you tick become one new class or record, read in the method's body and built at every call.
- The shared refactoring dialog shows lists of members to tick.
- The Node.js plug-in runs and debugs JavaScript and TypeScript files with Node.js and runs Jest and
  Vitest with the test results tree, run icons in test files and Rerun Failed. WebWave comes with
  it; the other IDEs offer it for a project with a `package.json`.
- **Code → Override Methods…** (**⌃O**) and **Implement Methods…** (**⌃I**) open the member chooser
  of the language's generator directly, and say so when there is nothing to override or implement.
- **View → Show Bytecode** shows the bytecode of the Java class at the caret (the project JDK's
  `javap -c -p -v` of its compiled class) in a read-only tab beside the editor that follows the
  caret; a class that is not compiled yet offers **Build**, and a scratch file or a file outside
  Maven and Gradle is compiled alone first.
- **Tools → JShell Console…** opens the project JDK's JShell with the module's classpath in a
  Terminal tab, with **Restart JShell** and **Stop JShell**; in a project not trusted it is refused,
  since building and loading the classes runs the project's code.
- **Tools → Kotlin REPL** and runnable Kotlin scratch files use the `kotlinc` installed on the
  computer (never downloaded); without it they say what is missing.
- Collections, maps and arrays open to their elements in the debugger's Variables, watches and Evaluate, map entries as **key → value**, with a debugger that can show them so (java-debug for Java); **Show Raw Fields** in a row's menu shows what is inside one of them, **Show Raw Fields of Every Collection** and Settings → Debugger → Data Views → **Enable alternative view for Collections classes** all of them.
- Values a debugger sends lazily, such as a boxed `Integer` in a Java list, are fetched at once and shown as `{Integer@79} 3`.
- **Trace Current Stream Chain** (Run → Debugging Actions) shows the values after each operation of the Java stream on the stopped line, one column per operation, filled without blocking the window.
- Agents get `debug.variables.raw` and `debug.stream.trace`; `debug.variables.list` takes `raw` and gives map entries as `entry`.
- The welcome window's **Plugins** section shows Settings → Plugins, and **Learn** lists the guide's chapters and the keyboard shortcuts and opens them in the browser.
- **Customize** on the welcome window: color theme with Sync with OS, IDE font size, keymap, screen reader support, Import Settings… and All settings…; changes apply at once.
- **Help → Welcome** brings the welcome window forward.
- **Import Settings…** takes a single keymap, colour scheme or theme file as well.
- The Maven tool window's context menus: on a project **Reload Project**, **Unlink Maven Projects**,
  **Ignore Projects**, **Open settings.xml** (or **Create settings.xml**), **Download Sources and
  Documentation**, **Show Effective POM**, which Maven computes in the background and the IDE
  opens read-only, and **Analyze Dependencies**, which opens the Dependency Analyzer on that project
  or module; on a phase or a goal **Run**, **Debug**, **Create Run Configuration**, **Execute
  Before Build**, **After Build**, **Before Rebuild**, **After Rebuild**, **Execute Before
  Run/Debug** and **Assign Shortcut…**.
- A **Run Configurations** node under each Maven project; double-click runs one.
- **Execute Maven Goal** completes phases and plug-in goals and lists the command lines run before.
- Speed search in the Maven tool window, and its **View Options**: Group Modules, Show Ignored
  Projects, Show Basic Phases Only.
- **Add Maven Project** for a `pom.xml` outside the project's folder.
- Build triggers: a goal set to run before or after a build runs in the run of Build Artifacts and
  Rebuild Artifacts.
- The Spring Boot plug-in: `application.properties` and `application.yml` (and their profile
  files) complete, document and check their properties from the configuration metadata in the
  project's own jars, so they match the project's Spring Boot version and work offline; unknown
  keys are weak warnings and deprecated ones are struck through with their replacement.
- **Spring Boot** run configurations: a `@SpringBootApplication` class in Java or Kotlin runs from
  its gutter icon or **Run Context Configuration**, with active profiles and overridden
  configuration properties.
- Spring beans and their injection points are marked in the gutter of Java and Kotlin files and lead
  to each other; the Endpoints tool window lists the project's request mappings with their HTTP
  method and full path and opens their handlers.
- A Spring Boot version past its open-source support end is marked in `pom.xml`, `build.gradle(.kts)`
  and `libs.versions.toml`, with the dates spring.io publishes (asked once a day; off by default,
  turned on in Settings → Build Tools).
- **Analyze Dependencies** in the Maven and Gradle tool windows: a module's dependencies as a list and a tree with their scope or configuration, conflicts marked with the version that won and why, filters to conflicts and by scope, Go to Declaration, and **Exclude**, which adds the exclusion to `pom.xml` or the Gradle script as written and loads the project again.
- **Edit Starters…** in the context menu of a `pom.xml` or Gradle build script and on the Endpoints
  tool window's toolbar: add and remove the starters of an open Spring Boot project in the list of the
  New Project wizard, for the project's own Spring Boot version. The build file is edited in place,
  with the BOMs and repositories a starter needs, and the project loads again.
- Java and Kotlin (with their plug-ins) get live templates (`psvm`, `sout`, `fori`, `iter`,
  `itar`, `ifn`, `inn`, `psf`… in Java; `main`, `sout`, `fun0`–`fun2`, `void`, `ifn`, `inn`,
  `iter`… in Kotlin), postfix templates (`x.var`, `ok.if`, `x.nn`, `items.for`, `n.fori`,
  `x.try`, `x.cast`…), Surround With constructs (if, while, for, try / catch, try / finally,
  synchronized, Runnable…), Javadoc and KDoc stubs on `/**` and Enter with `@param`, `@return`
  and `@throws`, and Complete Current Statement rules.
- **Code Style → Java** sets the Eclipse formatter of the Java language server: indents, spaces, hard wrap, brace placement, wrapping, new lines, blank lines, and the import order, blank lines between import groups and the class counts that make an import use `*`. With the Project scheme the options are kept in the project's `.zen/java-formatter.xml`.
- **Code Style → Kotlin** sets the `.editorconfig` properties Kotlin LSP reads: spaces, hard wrap, trailing commas, new lines, blank lines and the import layout, kept in the project's `.editorconfig` with the Project scheme.
- Files in other encodings open editable and save back in their encoding: every web-standard encoding (windows-1251, KOI8-R, ISO-8859-x, Shift_JIS, GBK and others), US-ASCII and UTF-16.
- **Settings → Editor → File Encodings**: Global Encoding, Project Encoding, the Path / Encoding table and Create UTF-8 files, kept in `.idea/encodings.xml`.
- The status bar's encoding opens a searchable list; a change that would lose something asks **Reload** or **Convert**.
- **File → File Properties → File Encoding** for the open file; **Remove BOM** and **Add BOM** on files and, recursively, on folders.
- **Generate** in Go files: **Getter**, **Setter** and **Getter and Setter** inside a struct type, and
  **Implement Methods…** (⌃I), which searches the interfaces of the module and the standard library
  and writes stubs for the missing methods with the imports they need.
- After a run with coverage, the **Project tree** shows the share of lines that ran next to every
  measured file and folder, until you hide coverage.
- `gogarden --wait file` opens a file, starting the IDE if it is not running, and ends when the file's tab is closed, so that the IDE can be Git's editor: `git config core.editor "gogarden --wait"`.
- Go templates (`.tmpl`, `.gotmpl`, `.gohtml`) are highlighted: actions, comments, and the HTML around
  them.
- **⌘-click** (Go to Declaration) on a `//go:embed` pattern opens the file, or lists the files when it
  matches several.
- **New Project** has a **Spring Boot** generator, from the Spring Boot plug-in: every choice comes from
  the Spring Initializr service, starters are grouped and searchable with their guides, starters that
  do not fit the chosen version are disabled with the versions they need, and the files are shown
  before anything is written. The generated project opens and imports through Maven or Gradle.
- **Settings → System Settings → Project Generators:** turn the network off for project generators, and
  set a generator's own options, such as your company's Spring Initializr URL.
- The Properties plug-in for `.properties` files: highlighting, folding, the keys in the Structure
  view, duplicate keys and malformed `\u` escapes reported, and each escaped value shown unescaped
  as an inlay. Keys and bundle names complete, and Go to Declaration finds them, in the string
  literals of `getString(…)`, `getBundle(…)` and the like in Java and Kotlin.
- Resource bundles: the files `name.properties`, `name_ru.properties`, `name_de_DE.properties`… of
  a folder show as one **Resource Bundle 'name'** node in the Project tool window, and open in the
  Resource Bundle editor, a table with a column per locale where missing translations are marked
  and a key is added, renamed or removed in every file at once, undone in one step.
- Go tests and benchmarks can record a CPU or memory profile, and pprof files open, through the
  `profile.*` commands for agents and scripts; the run's console names the costliest function. The
  Profiler tool window comes later.
- **Refactor → Safe Delete… (⌘⌦)** in the editor and on the Project tool window's selection: a declaration or file nothing else uses is deleted at once (a declaration with its attributes, annotations and doc comment, one Undo brings it back); with usages, **Usages Detected** lists them with **View Usages**, **Delete Anyway** and **Cancel**.
- A shared refactoring dialog with the refactoring's fields, **Preview** on the Refactoring Preview tab, and a list of conflicts before anything changes, which language plug-ins use for their refactorings.
- **Settings → Editor → General → Editor Tabs**: Tab placement (Top, Bottom, Left, Right, None), one
  row that scrolls or squeezes, multiple rows, pinned tabs in a separate row, file icon, file
  extension, directory for non-unique names, Mark modified, full path on hover, close button
  position, alphabetical order, Open new tabs at the end, Tab limit (30) with Close unchanged or
  Close unused, and which tab becomes active after closing the current one.
- **Enable preview tab**: a single click in the Project tool window and the files the debugger opens
  reuse one italic tab, which an edit, a double click or Keep Tab Open makes permanent.
- **Advanced Settings → Editor Tabs**: Keep pinned tabs on the left and Open declaration source in
  the same tab.
- **Configure Editor Tabs…** in the tab menu and in Window → Editor Tabs.
- **Set Value…** (**F2** or the row's menu in the Debug tool window's Variables) changes a variable of the stopped program in place when the debugger can; a refusal is shown under the variable.
- While the program is stopped, the editor shows the values of the frame's variables at the ends of its lines, a value changed since the previous stop in another colour (Settings → Debugger → Data Views → Show values inline).
- **Run → Debugging Actions** gets **Force Step Over** (⌥⇧F8), **Force Step Into** (⌥⇧F7) and **Force Run to Cursor** (⌥⌘F9), which pass the breakpoints on the way.
- A **Run to Cursor** button at the start of a line under the pointer while the program is stopped (Advanced Settings → Debugger → Show inlay Run to Cursor popup).
- Agents get `debug.variable.set`, `debug.inlineValues.get`, `debug.execution.forceStepOver`, `debug.execution.forceStepInto` and `debug.execution.forceRunToCursor`.
- **Settings → Appearance & Behavior → Menus and Toolbars** edits the main menu, the main toolbar and every popup menu of the editor, Project view, terminal, Git, debugger and Database: Add…, Edit Icon, Move Up, Move Down, Remove, Restore and Restore All, with a search field. Built-in popup items can be moved and removed, and any command can be added to the main menu. Right-click the main toolbar for **Add Action to Main Toolbar…** and **Customize Toolbar…**. Agents use the `ui.customization.*` commands.
- Struct tags complete inside the backticks after a field: keys such as `json`, `yaml` and `db`, the
  field's name in each case, and options such as `omitempty`.
- **Generate → Struct Tags** adds tags to the chosen fields, in the chosen case.
- **Reformat Code** lines up the values of struct tags. **Code Style → Go → Line up the values of
  struct tags** turns it off.
- **Settings → Editor → General → Console → Override console cycle buffer size**: a run or debug
  console keeps the last 1024 KB of output by default and drops its oldest lines beyond that.
- **Settings → System Settings → Server Certificates:** an accepted list with **+** and **−**, the details of the selected certificate, and **Accept non-trusted certificates automatically**, off by default and with a warning.
- **Untrusted Server's Certificate** asks to Accept or Reject when the HTTP Client or Check Connection meets an unknown issuer; Accept holds for that host and certificate only.
- Dependencies of `package.json` are under External Libraries, as `name@version` with the version
  installed in `node_modules`, in every IDE.
- The npm plug-in: the npm tool window lists the `package.json` of the project and its workspaces
  with their scripts, runs them, installs dependencies and has an offline mode. WebWave comes with
  it; the other IDEs offer it for a project with a `package.json`.
- In the npm tool window and its commands, an AI agent sees nothing of a `package.json` hidden from it
  and can turn offline mode on but not off. A `package.json` reached through a link out of the
  project is not listed.
- **Trust and Open Project** asks before a project you have not trusted opens: **Trust Project**,
  **Preview in Safe Mode**, **Don't Open**, and **Trust all projects in '<parent>'**.
- Safe mode: files open and can be edited, and no Git call, language server, go.mod check, `cargo metadata`,
  download, formatter from the project, run or debug session starts; **Run** and **Debug** say why.
  The editor banner's **Trust project…** starts all of it without reopening the project. A scratch
  file still runs, in its own folder.
- Settings → Appearance & Behavior → **Trusted Locations**: every project under these folders is
  trusted without asking, with a warning for the home folder.
- **Settings → Editor → TODO**: patterns (regular expression, case sensitivity, icon, own colour),
  filters, and Treat indented text on the following lines as part of the same TODO.
- TODO text is highlighted in the editor with the colour scheme's `todo` colour or the pattern's own,
  and marked along the scroll bar.
- **Filter TODO Items** in the TODO tool window.
- **Check TODO** in the Commit tool window, with Configure: TODO items in the changed lines stop the
  commit with Review, Commit Anyway and Cancel.
- **Settings → System Settings → HTTP Proxy:** No proxy, Auto-detect (the proxy variables, then the Mac's system proxy) and Manual (HTTP or SOCKS, No proxy for, login with the password in the Keychain), with **Check Connection**. Proxy configuration scripts are reported as not supported.
- `# @no-proxy` in an `.http` file of a trusted project connects a request directly.
- Maven and Gradle already on the computer are found: the distributions wrappers left in
  `~/.m2/wrapper/dists` and `~/.gradle/wrapper/dists`, the newest complete one. The Maven and Gradle
  tool windows show which tool runs and where it was found.
- A Maven or Gradle project without a chosen JDK uses the installed JDK of the Java its build asks
  for, and the Java language server compiles with it too. When none is installed, a notification
  offers **Download JDK** or **Choose a JDK**.
- One banner in the Maven and Gradle tool windows says why a project does not load: the tool is not
  found, the project is not trusted, or the JDK is not the one the build asks for, with the button
  that fixes it. Sync says the same.
- **Reload All Maven Projects** and **Reload All Gradle Projects** start the Java language server
  with no file open. Sync's import step waits for its real result; when the server is not installed
  it says so and offers **Install**.
- **New → Go File** in a Go module or a folder with Go files asks for the name and the kind (Go File or Simple Application) and opens the file with the caret in place; **Go Test File** is beside it, and the templates can use `${GO_PACKAGE_NAME}` and `${GO_PACKAGE_NAME_WITH_TEST_SUFFIX}`.
- Every IDE writes a rotating log on your computer, with crashes and their backtraces. **Help → Show
  Log in Finder** opens it.
- **Help → Collect Logs and Diagnostic Data** writes a ZIP for a bug report, with secrets hidden;
  nothing is uploaded.
- **Help → Diagnostic Tools** gains **Debug Log Settings…** and **Special Files and Folders…**.
- **Open Tab in New Window** (tab menu, **⇧F4**, or dragging a tab out of the window) moves an editor tab into a window of its own; drag it back to dock it. Detached windows come back with the project.
- Advanced Settings → Editor Tabs → **Open declaration source called from a detached window in the main IDE window**.
- **Settings → Tools → Actions on Save** is a table with Activated On; Reformat code and Optimize imports choose their files, and in a project under version control Reformat code can take only the changed lines. Rearrange code, Run code cleanup, Update copyright notice, Run eslint --fix and Run stylelint --fix have their rows.
- **File → New Projects Setup → Settings for New Projects…** holds the Actions on Save of projects that have not chosen their own.
- **Refactor → Move… (F6)** in Java: classes from the editor or the Project tool window and packages move to another package, with their `package` lines and the imports of the code that uses them updated, a preview of every change and file moved, and a conflict for a class that is not public and still used from its old package.
- **Refactor → Change Signature… (⌘F6)** in Java: a method's visibility, return type, name and parameters (add with a default value, remove, reorder) change with every call, previewed call by call, optionally keeping the old method as a delegate.
- The shared refactoring dialog has tables whose rows can be added, removed and reordered.
- **Change Split Orientation**, a button beside the preview switch of SVG and Markdown files, puts
  the editor and the preview one above the other or side by side, kept for each file.
- **Settings → Languages & Frameworks → Markdown**: **Preview layout** for files you never switched,
  **Sync scroll in the editor and preview** and **Preview font size**.
- Postfix completion: type an expression, a dot and a key (`ok.if`, `err.nn`, `items.for`, `x.let`)
  and choose it in the completion list or press Tab; Go and Rust have their sets, and Settings →
  Editor → General → Postfix Completion switches each one off.
- Code → Code Completion → Complete Current Statement (⇧⌘⏎) closes the brackets, adds the block
  of an `if`, a loop or a declaration, or the `;` of a Rust statement.
- Code → Code Completion → Type-Matching (⌃⇧Space) keeps the completion items the language
  server marks as fitting the place.
- Typing `///` above a Rust declaration or `//` above a Go one puts a documentation comment stub.
- Settings → Live Templates: Duplicate, variables computed by expressions (`decapitalize(TYPE)`,
  `fileNameWithoutExtension()`, `date()`…) with Skip if defined, and where a template applies
  (code, comments, strings); plug-ins bring templates of their own languages, read-only until
  duplicated.
- **New Project** has **Java** and **Kotlin** types when their plug-ins are installed, in every IDE:
  Build system, JDK with Download, Add sample code, and GroupId and ArtifactId under Advanced
  Settings. The chosen JDK becomes the project's; with no JDK, Create stays disabled and says why.
- **Code Style → Go** and **Code Style → Rust** have the tabs Tabs and Indents, Spaces, Wrapping and Braces, Blank Lines, Imports and Arrangement with a preview. Go offers gopls's gofumpt and local import prefix (gofmt itself has no options); Rust offers the stable rustfmt options, kept in the project's `rustfmt.toml` with the Project scheme.
- **Code → Rearrange Code** puts Go's top-level declarations in the order of Code Style → Go → Arrangement; Reformat Code can do it first.
- `go.mod` and `go.work` complete a module's versions with **⌃Space**, newest first, from what the
  go.mod checks already know.
- **Go to Declaration** on a module in `go.mod` or `go.work` opens its `go.mod`: in the module cache,
  a `replace` folder or a workspace module.
- A run icon on every `//go:generate` line runs that line alone with `go generate`, with a new **Go
  Generate** run configuration kind.
- Benchmarks run with `-benchmem`, and the Run tool window shows a table of their results; the
  numbers are in `run.tests.get` for agents.
- The Gradle tool window fills by itself after import: tasks by Gradle's groups, without Reload.
  Gradle describes the build through an init script, and the answer is kept per build file hash, so
  an unchanged build opens without running Gradle.
- A **Dependencies** node per Gradle project, by configuration, as Gradle resolved it.
- Builds added with `includeBuild` and `buildSrc` show as projects, and their tasks run from the
  composite.
- A **Run Configurations** node at the root of a Gradle build.
- The Gradle task menu: **Run**, **Debug**, **Create Run Configuration**, **Execute Before Sync**,
  **After Sync**, **Before Build**, **After Build**, **Before Rebuild**, **After Rebuild**, **Execute
  Before Run/Debug** and **Assign Shortcut…**.
- The Gradle toolbar: **Add Gradle Project**, **Execute Gradle Task** with completion and history,
  **Toggle Offline Mode** and **Build Tool Settings**.
- Speed search in the Gradle tool window.
- `go vet` and `staticcheck` check the package of an open Go file when it is opened and saved, or
  with **Run go vet and staticcheck**. Their problems join gopls's in the editor and in **Problems**,
  with the quick-fixes `go vet` gives.
- **Settings → Languages & Frameworks → Go → Inspections** turns each tool off, and **Install
  staticcheck** downloads staticcheck with `go install`, only when clicked.
- A Go, Rust or Java scratch file runs with one click: the ▷ in its gutter, **⌃⇧R** or **Run** on
  its context menu under Scratches and Consoles build and run that one file with the project's
  toolchain (Go Build of the file, `rustc`, Java's single-file source launcher with the project JDK,
  optionally with the module's classpath), with the output in the Run tool window.
- **File** has **Recent Projects**, **Close Project**, **Close All Projects**, **Close Other Projects**
  and **Plugins…**.
- The **New** popup (⌘N) has a **Search file type** field. In a source folder it lists **Java Class**,
  **Kotlin Class/File**, **File**, **Package** (dots make nested folders), **package-info.java** and
  **module-info.java**; **Java Class** chooses the kind in its name dialog.
- **New Project**: a search over the types, ↑ and ↓ move through them, **More via plugins…**, the line
  **Project will be created in**, and a description for **Empty Project**.
- **Kotlin** projects have **Use compact project structure**: sources directly in `src`.
- **File → Invalidate Caches…** removes the IDE's derived data for all projects and restarts; the
  checkboxes add Local History and the Git log's caches, **Just Restart** only restarts.
- **File → Repair IDE** fixes a broken project state step by step without a restart: a notification
  reports each step and offers **Everything Works Now**, **Continue** and **More**. **Repair IDE on
  File** in the Project tool window does the first two steps for one file.
- Background tasks can be cancelled with **×** in the status bar and in the panel: a Git fetch or
  push, a dependency update (`go get`), a Maven or Gradle reload and a file watcher. Their programs
  are stopped with their whole process group, and a cancelled dependency update puts `go.mod` and
  `go.sum` back.
- **Indexing…** can be paused and resumed with **‖** and **▷**.
- A language server's work shows as a background task with its percentage and message, and can be
  cancelled when the server allows it.
- **Window → Processes** shows and hides the background tasks panel.
- **File → New → Project from Version Control…** and **Clone Repository** on the welcome screen open
  the Clone Repository dialog: Version control, URL with the addresses cloned before, Directory and
  Shallow clone. The clone leaves submodules until you trust the project, shows its progress, can be cancelled without
  leaving anything, then asks to open the project, which is not trusted until you trust it.
- **File → New → Project from Existing Sources…** takes a folder or a build file, says whether it is
  Maven, Gradle or plain sources, and opens the folder.
- **File → New → Module…** is the wizard for a module of the open project: its folder under
  **Location**, the project's JDK, and the build lists it (`<modules>`, `include`) and loads again.
- **File → New → Module from Existing Sources…** lists an existing folder in the project's build.
- Forms of project types can have notes with a link, tables and lists kept with **Manage…** or
  **Add…**, and generators can show a build tool's output while it runs.
- **Smart Step Into** (**⇧F7**, Run → Debugging Actions) lists the calls on the stopped line in a popup, frames the chosen one in the editor and steps into it, with a debugger that lists them.
- **Reset Frame** (formerly Drop Frame) on the Debug toolbar, in a frame's menu and in Run → Debugging Actions runs the selected frame again from its start, with a debugger that can restart frames.
- Async stack traces: the frames of the awaited or scheduled call follow the stack below a separator in the Debug tool window when the debugger gives them.
- Agents get `debug.execution.stepInTargets`, `debug.execution.smartStepInto` and `debug.frame.reset`; `debug.frames.list` tells separators, async frames and frames that can restart.
- **Code → Generate…** (**⌘N** in the editor) lists what can be written at the caret: the
  language's own generators and the language server's source actions. A member chooser picks the
  fields or methods to use, with Select All, Sort Alphabetically and the generator's options, all
  from the keyboard; the code is written formatted, as one undo step.
- **Generate → Constructor…** inside a struct type writes `NewT(...)` for the chosen fields, in
  gofmt's layout, returning a pointer or a value; gopls' **Add test for …** is offered on a
  function.
- **Dependencies tool window** for Maven and Gradle projects: modules on the left, each module's declared dependencies on the right; search repositories by name or coordinates, add a dependency with a scope or configuration and a version, upgrade and remove it. The build file is edited in place, keeping its formatting and comments, and the project loads again.
- Vulnerable versions show their advisories from OSV and an upgrade to the nearest safe version; outdated versions show what is newer.
- `pom.xml`, Gradle scripts and `gradle/libs.versions.toml` complete dependency coordinates, and underline outdated and vulnerable versions with a quick fix.
- Three settings under Build Tools turn search and the checks off, each naming the servers it contacts; they are off by default.
- **Help → What's New in <product>** opens the notes of the running version in an editor tab, offline too, and once by itself after an update (**Settings → System Settings → Show What's New after an update**).
- **Help → Keyboard Shortcuts PDF** makes a reference of your active keymap, grouped as the menus, and **Export Keyboard Shortcuts…** saves it as PDF or HTML.
- **Help → Tip of the Day**, with tips that show the shortcuts of your keymap; off on startup by default.
- **Help → My Productivity** lists the features with how often and when you used them, sortable, counted on this computer only; **Reset** clears the counts.
- **Code → Inspect Code…** checks a whole project, a directory, the selected files, the uncommitted
  files or a custom scope in the background, and shows the results in a tab of the Problems tool
  window: a tree by severity, inspection and file, with quick-fixes, Fix All in File, Exclude,
  Rerun, Group by Directory and export to HTML and XML.
- **Code → Analyze Code → Run Inspection by Name…** (⌥⇧⌘I) runs one inspection or gopls analyzer
  over a scope and a file mask.
- In a project that is not trusted, Inspect Code runs no program of the project and a fix that would run one says why.
- An agent sees nothing of hidden files in inspection results, and exports them only inside the project.
- A rename or quick fix of a language server can create, rename and delete files: renaming a Java
  class renames its file, renaming a Rust module moves its file, and "create class" fixes work. The
  rename preview lists the files created, renamed or deleted, and one Undo reverts everything.
- Renaming, moving or deleting a file in the Project tool window lets the language servers update
  what refers to it first (imports, `mod` declarations, class names); one Undo in a changed file
  reverts their changes and the rename or move.
- The Run tool window's test tree: **Run '…'** in the menu of a test or a package reruns just that one, and the commands `run.tests.rerun` and `run.tests.source` do the same for agents.
- A double-click on a Go test that passed opens where it is written.
- **File → New → Project…**, **File → New → Empty Project…** and **New Project** on the welcome screen
  open the New Project wizard: project types on the left, Name, Location, the type's fields and
  **Create Git repository** on the right. A failure leaves nothing behind, a project you create opens
  trusted, and a folder that holds something asks first.
- The Go plug-in adds the **Go** project type: module path and sample code.
- Plug-ins can add project types, listed under **Generators**: fields as data, a preview of the files,
  and forms loaded in the background with a spinner, the error and **Retry**.
- **Settings → Appearance & Behavior → System Settings:** **Default project directory**.
- `project.new.*` commands and the actions `view.newProject.open` and `view.newProject.empty` let
  agents create projects, also with no project open.
- **Compound** run configurations start several configurations at once, each in a tab of its own;
  **Debug** debugs the ones that can be debugged, **Stop** stops them all, and a compound that
  contains itself is shown as an error.
- **Edit configuration templates…** in the Run/Debug Configurations dialog: new and temporary
  configurations of a kind start from its template, kept in `.zen/runConfigurations.json`.
- The Run/Debug Configurations dialog has folders (**Create New Folder**, drag configurations onto
  a folder) and a search field.
- The **Services** tool window (⌘8) shows run configurations: **Add Service → Run Configuration
  Type**, each configuration's run state and console, **Run**, **Debug**, **Stop**, **Rerun**,
  **Hide Configuration** and **Restore Hidden Configurations**.
- Compound configurations and folders are adopted from the `.run` files of JetBrains IDEs.
- **Build Artifacts** in a run configuration's **Before launch** list builds the artifacts you tick
  before the configuration starts; a build that fails stops the launch.

### Changed

- Keys an agent sends with `press_keys` now count as the agent's: a command, button or dialog they reach
  is refused where the agent would be refused, and a change in Settings passes what `set_settings` refuses.
- The tab context menu and Window → Editor Tabs list the close, pin and reopen actions,
  with Copy Path/Reference… in the tab menu; Close Other Tabs keeps pinned tabs.
- Copying a folder keeps the symbolic links inside it as links.
- The profiles ticked in the Maven tool window are kept across restarts.
- **Uninstall** on the Plugins page asks first.
- With the Project scheme, Code Style options a project's formatter file does not keep (as server settings) are now your own values instead of their defaults.
- A file inside a `.git` folder opens in the repository's project, not as a project of `.git`.
- Closing the active tab activates the tab on its left by default; it was the
  tab on the right.
- An editor group keeps 30 tabs by default: past that, the least recently used one that is not
  pinned closes, its changes saved. Reopen Closed Tab brings it back.
- The **Create Test** dialog lists the testing libraries a project uses first and the others after
  them, so another one can still be chosen.
- Connections trust the macOS keychain's roots and the accepted list as well as Mozilla's roots, so updates and the plug-in catalog work behind a corporate TLS proxy whose root is installed.
- In a project that is not trusted, running the code at the caret waits for trust too.
- The TODO tool window, the completion in comments and the highlighting share one set of patterns.
- **Import Settings** and **Restore Default Settings** are a person's only: the commands
  `settings.ide.import` and `settings.ide.restoreDefaults` refuse an agent, because both replace
  settings wholesale, including what an agent may not change, such as the files hidden from agents.
- An agent can no longer write into the IDE's data folder through the IDE's commands (scratch files stay
  open to it), nor add, remove or choose a JDK; `toolchain.jdk.download` stays.
- **New Project:** a type that asks a service shows the address first and connects only after **Connect**;
  with **Let project generators use the network** off, a generator's build tool does not start either.
- Nor can an agent add, remove or choose a Go SDK, gopls, Delve, a Rust toolchain or rust-analyzer, or set
  a rust-analyzer setting that names a program. Listings and the IDE's own installs stay open.
- **Edit Starters** shows the service's address first too, and connects only after **Connect**.
- Every network request of the IDE follows the one proxy setting. The HTTP Client used to ignore proxies.
- Actions on Save are kept per project in `.zen/actionsOnSave.json` once a project changes them.
- When Gradle fails to load, the Gradle tool window keeps the last good model, and the Sync view
  shows why.
- **Toggle Offline Mode** for Gradle is the **Offline work** setting, kept with the project; it used
  to last until the IDE closed and apply only to loading the tasks.
- A problem an inspection finds is listed once when the language server reports the same one on its
  line.
- **File → New** has **Project…**, **Project from Existing Sources…**, **Project from Version
  Control…**, then **Module…** and **Module from Existing Sources…**, then the file items.
- **New Module** and **Maven Archetype** have the look of the New Project wizard: the search and the
  groups on the left, rows of buttons for choices, **More via plugins…**. **New Module** has no
  **Create Git repository**.
- **File → New** is the same for every folder and creates no classes: those are in the **New** popup.
  **Empty Project…** is no longer in the menu; choose it in the wizard.
- **Build system** and similar choices in New Project are rows of buttons, and **Maven** is the Java
  type's default. **Location** is the parent folder.
- **Spring Boot** has two pages: the project's fields under **Server URL**, with **JDK**, then the
  Spring Boot version and the dependency tree, with a description pane and **Added dependencies**.
  **Version** and **Description** are left to the service's defaults.
- **Restart IDE…** in Find Action asks first, listing what is running, as Exit does.
- An AI agent's **Restart IDE** and **Restore Default Settings…** only ask: the person answers, and the agent's key presses do not reach those dialogs, **Invalidate Caches…** or **Import Settings…**.
- Go to Class and Go to Symbol say they are not available while indexing.
- **Create Git repository** moved under **Location** in the New Project wizard.
- When a test run ends, failed tests move to the top of each level of the tree.
- **Open project in** moved from Appearance to **System Settings** and gained **Ask**: **New window**,
  **Current window** (a new tab of the main window) or **Ask**.

### Removed

- Remote Development, which never worked, is hidden from the welcome window until it exists.
- The page of generated files in New Project. Agents still have `project.new.preview`.
- **Edit File Templates…** from the New lists: it is in Settings → Editor → File and Code Templates.
- **File → Save** from the menu: the action stays in the keymap and Find Action.

### Fixed

- With Use "CamelHumps" words on, the first **⌥↑** (Extend Selection) selects the hump at the caret
  (`Amount` in `totalAmount`), the next one the whole word, and **⌥↓** walks the same steps back.
- An update installed while the IDE runs no longer restarts over files that could not be saved: the IDE stays open, keeps your changes and says why.
- A damaged settings file is no longer replaced by the defaults: it is kept as `settings.json.corrupt` and a notification names it. A failed save of the settings is now reported too.
- The active tab comes back after a restart even when an earlier file in the row was deleted.
- Moving to a detached tool window or another window of the IDE no longer counts as switching to a different application for autosave.
- The continuation indent of `.idea` and `.editorconfig` applies when Enter follows `(` or `[`, and the `max_line_length` of `.editorconfig` moves the right margin.
- Completion in a breakpoint's condition or log expression no longer leaves its hidden copy of the file open in the language server when the dialog is closed before the answer arrives.
- A failed step of Shelve no longer loses your changes: they stay in the shelf and Undo restores the files.
- Renaming a stash happens in one step, so a failure cannot lose stashes.
- Conflicts in files with non-ASCII names are listed under their real names.
- Shelves and patches of files with a text conversion filter in Git apply again.
- **Force Push** no longer overwrites commits you fetched but did not bring into your branch, or commits a fetch brought in while the Push dialog was open.
- A stash with an empty message no longer makes other stash actions hit the wrong stash.
- Git output that is not UTF-8 is no longer read as empty.
- **Save**, **Save All**, closing a tab, Commit and the agents' saves never write over a file changed outside the IDE: the save stops, your changes stay unsaved, and the editor offers **Load File from Disk** or **Keep Memory Changes**.
- Saving an unmodified file that changed on disk reads it instead of writing the old text back.
- Two IDEs on one Local History (a window and a headless IDE) no longer drop each other's versions or reuse their numbers.
- Renaming a file to another case on a case-sensitive volume no longer replaces a file of that name.
- Copying a folder with a symbolic link that leads back up no longer fails.
- Saves from two IDEs no longer share a temporary file, a private file stays private while it is saved, and a file with the longest allowed name saves.
- Find in Files refuses a path that climbs out of the project with `..`.
- A big checkout that overflows the file watcher reloads the open files; a watcher that cannot start is tried again and reported once.
- `editor.text.batch` with `save` saves every file or none.
- Changing the line separator never writes a read-only document or a file changed outside the IDE.
- Find in Files reports the right lines in files that use carriage returns only.
- Typing at several carets is one undo step.
- A search in Settings scrolls the page to the first match once per query; scrolling by hand stays.
- The dependency tree of a multi-module Maven project keeps every module: it held only the last one,
  so the Maven tool window and External Libraries showed the other modules without it.
- A plug-in pre-release such as `1.0.0-rc1` is older than `1.0.0`, so it is no longer offered as an update to it.
- A plug-in's memory is limited to 1 GiB.
- A password added to the system keychain outside the IDE is seen within 30 seconds, without a restart.
- Drawing a frame no longer reads the disk: the offer to install a plug-in, the Welcome screen's recent projects and problems in unopened files no longer slow typing or scrolling.
- A change on disk re-reads only the open files it touched, not all of them.
- Enabling and uninstalling a plug-in, and the search of Replace in Files, no longer freeze the window.
- The Search Everywhere preview skips files over 2 MB, and comparing a dropped file skips files over 10 MB.
- A busy Local History no longer loses the version from before an edit.
- A branch named like an option (`-D`) or a start point like `--discard-changes` is refused instead of running as an option; so are a checkout of `-` and a remote named like an option.
- Discarding a file outside the repository, by `../` or through a symbolic link, is refused instead of deleting it.
- File names with `[`, `*` or `?` are taken literally: Rollback of `app/[id]/page.tsx` no longer touches `app/i/` or `app/d/`, and the same goes for Accept Yours, partial commits, staging and shelving.
- A UTF-8 byte order mark no longer shows up in the text, shifts columns or reaches language servers; the status bar shows `UTF-8 BOM`.
- New UTF-8 files follow the project's Create UTF-8 files setting.
- Go's **Constructor** no longer lists a struct's methods as fields.
- A preview that failed in the New Project wizard is tried again the next time, instead of showing the
  same error.
- Checkout, merge, rebase, cherry-pick, revert, reset and commit (with its hooks) run in the background: the window stays responsive, and the status bar shows them.
- Staging, unstaging and discarding no longer freeze the window while the status is computed.
- Submodules and avatars in the Git workspace are read in the background.
- A Go test that prints `PASS`, `FAIL` or an `ok` line no longer ends its package with a wrong
  result.
- Rust tests are no longer put under the wrong test binary when the output of two binaries arrives
  before their names.
- A very long line without a line break no longer makes a run use ever more memory and time; it is
  shown in pieces of 64 KB.
- **Stop** and **Kill** of a run that has ended never signal another program that got its process
  group number.
- Breakpoints sent to a debugger that does not answer no longer freeze the window, and a refused set
  is not sent again until the breakpoints change.
- Killing a debug session asks the debugger to end the program being debugged.
- Asking Go or Cargo for their settings, versions and the project's modules no longer hangs when a
  program they start keeps their output open; finding a Rust run configuration from the caret no
  longer waits for `cargo metadata`.
- A debugger or language server that announces an enormous message can no longer exhaust memory.
- Run scratch folders have unpredictable names and only their owner can read them.
- A long error in the Maven and Gradle tool windows wraps instead of being cut off.
- Notifications and tool windows show what went wrong, not the id of the command that failed.
- The tree of a key-value connection (Redis) kept for working offline no longer loses the keys just
  read, and its cache file no longer ends up unreadable, when two reads of the tree finish close
  together.
- **Settings → Editor → Natural Languages** no longer shows a message's internal name above the list of languages, and
  saving a database schema over an existing file says the file exists instead of showing one.
- **Assign Shortcut…** in the Maven tool window's menu no longer fails.
- Running a Go scratch file no longer builds every Go scratch together as one package.
- **Install** in a language server's notification shows what it does: the links go away, **Installing ESLint…** appears, and a second notification says when it is installed or why it failed. The installation always ran, but nothing said so.
- **Database history** settings changed just after the IDE starts are no longer lost: history stays off once it is turned off.
- **Run with Coverage** answers an agent only after the coverage line is in the console.
- Pasting in the Project tool window pastes what is on the clipboard now. A file copied in the tree earlier no longer comes back as `image copy.png` when you have since copied a new image, some text or anything else.
- Gradle library and source scripts are kept once in the temp folder, instead of one new file per IDE process.
- **Local History** finishes its queued work when the IDE closes it, and unpacking JDK sources stops when the window closes.

### Security

- Git is off in a project that is not trusted: the IDE makes no Git call, so a repository's filter
  drivers and hooks do not run. The Git tool windows say why they are empty, Git commands are refused
  for you and for agents, and trusting the project starts Git without reopening it.
- Language servers in a project that is not trusted get `GOTOOLCHAIN=local`, and so does every
  `go version` and `go env` the IDE runs, so a `go.mod` cannot make them download a toolchain.
- **Safe mode:** a plug-in's setup step (Nuxt's `nuxi prepare` and the like) no longer starts in a project that is not trusted; it asks for trust as Run does.
- **Safe mode:** agents can no longer run an external tool, rerun one, or run a database migration in a project that is not trusted. You still can.

## 26.11.2 - 2026-11-04

### Added

- **Project Structure** has the sections Project, Modules, Libraries, SDKs and Problems: the
  project's SDK, language level and compiler output, each module's source, test and resource
  folders, excluded folders, output and dependencies, the libraries with the modules that use them,
  the JDKs and other SDKs, and what is wrong with the setup. For Maven and Gradle the values come
  from the build, are marked so, and open the build file to change them.
- External Libraries in a Maven or Gradle project lists the JDK with its sources and every library
  as `Maven: group:artifact:version` or `Gradle: group:artifact:version`, with the contents of its
  jar: Maven's resolved tree once it is loaded, Gradle's resolved classpaths once the build is
  reloaded, else the declared dependencies.
- A library class opens with its sources when the library has them, otherwise as a read-only
  decompiled view whose banner offers **Download Sources** (through Maven or Gradle, only when you
  click it). Any `.class` file opens that way.
- **Go to Declaration** into a JDK or library class opens it read-only, and navigation goes on from
  inside it.
- **File watchers**: Settings → Tools → File Watchers runs a program by itself when matching files are saved or changed on disk, per project (`.zen/fileWatchers.json`) or for every project, with templates (go fmt, goimports, golangci-lint, Prettier, TypeScript, SCSS), macros, output paths, output filters whose matches become problems, and Import and Export. A watcher that fails is switched off with a notification that can switch it back on. Tools → Actions on Save has a File Watcher row.
- Run configurations take environment files: **Environment files** lists `.env` files (the folder
  button adds them), read in order at each start before the configuration's own variables. The
  **Environment variables** field takes `A=1;B=2` inline, the table shows the system variables
  read-only, and a value can use the IDE's variables as `$NAME$`.
- Program arguments, tool arguments and the working directory of run configurations expand macros
  such as `$ProjectFileDir$`, `$FilePath$` and `$Prompt$`; **Insert Macro** lists them with their
  values.
- **Allow multiple instances**: such a configuration starts another instance in a tab of its own,
  named `name (2)`. **Activate tool window** and **Focus tool window** choose whether the Run tool
  window opens and takes the keyboard.
- **Settings → Advanced Settings → Run/Debug**: **Confirm rerun with process termination** and
  **Temporary configurations limit**.
- Configurations shared by a JetBrains IDE keep their "Allow multiple instances".
- **Load Changes** after editing `pom.xml`, `build.gradle(.kts)` or `settings.gradle(.kts)`: a
  button in the editor's top-right corner, or **⇧⌘I**, saves the files and reloads the Maven or
  Gradle project; it hides once the project is loaded. Changes made outside the IDE load by
  themselves, and **Settings → Build, Execution, Deployment → Build Tools** chooses whether edits in
  the IDE do too (**Any changes** / **External changes**).
- The **Build** tool window with its **Sync** tab: each load of the project with its steps, how long
  they took, and their errors and warnings; an error opens its file at the line and offers the
  obvious fix (**Download Maven**, **Choose a JDK**, **Work Offline**). A failed load opens it.
- Gradle builds can load offline (`gradle.options.set`).
- Run → Debugging Actions → Evaluate Expression… (⌥F8) opens an Evaluate dialog that does not block
  the window: the selection or the name at the caret is in its field, Enter evaluates it in the
  selected frame, the result is a tree read as you open it, with completion from the language
  server, the project's recent expressions and Add to Watches.
- Quick Evaluate Expression (⌥⌘F8, or ⌥-click on a name) shows a value in a popup at the
  expression, and resting the pointer on a name while the program is stopped shows its value in a
  tooltip; Settings → Build, Execution, Deployment → Debugger → Data Views turns the tooltip on or
  off and sets its delay.
- Show Execution Point (⌥F10) goes back to the line where the program stopped.
- Advanced Settings → Debugger → Maximum number of recent expressions (50 by default).
- **Run Anything** (⌃⌃, Ctrl Ctrl on Windows and Linux) runs a run configuration, a tool's command
  (`go test ./...`, `cargo build`, `mvn clean install`, `gradle build`) or any command line in a new
  Terminal tab, with the recent commands first; ⇧⏎ debugs a configuration, ⌥⏎ runs in the current
  file's folder, `open <name>` opens a recent project and `?` lists the prefixes.
- **Run → Run…** (⌃⌥R) and **Run → Debug…** (⌃⌥D) choose a configuration to run or debug.
- Settings → Advanced Settings → User Interface → **Disable double modifier key shortcuts** turns off
  ⌃⌃ and ⇧⇧.
- Run → View Breakpoints… (⇧⌘F8): every breakpoint by kind and your groups, with check boxes, + for
  function and exception breakpoints, −, Move to Group, Edit Description, the properties and the
  code around the line.
- Logging breakpoints ("Breakpoint hit" message, stack trace, an expression), a pass count, and
  "Disable until hitting the following breakpoint" with Disable again or Leave enabled; a debugger
  without log points still logs, and fields a debugger cannot use are hidden.
- Exception breakpoints from the debugger's own kinds (Java: Caught and Uncaught Exceptions), with
  a condition where the debugger takes one, function breakpoints, and data breakpoints for the
  debuggers that have them.
- Mute Breakpoints, a toggle in the Debug toolbar.
- Breakpoints keep every property, and function and exception breakpoints, with the window's state.
- A right click on a breakpoint opens its properties: Enabled, Suspend (All or Thread when the
  debugger can), Condition, logging and Remove once hit, with More (⇧⌘F8) and Done; changes apply
  at once, in a running session too.
- ⌥-click (or a middle click) enables or disables a breakpoint, a drag moves it to another line and
  ⌘-drag copies it; Settings → Build, Execution, Deployment → Debugger → Remove breakpoint can make
  dragging it to the editor remove it.
- Run → Toggle Breakpoint with Toggle Line Breakpoint, Toggle Temporary Line Breakpoint (⌥⇧⌘F8,
  removed once it stops the program) and Toggle Breakpoint Enabled; the gutter's menu has Remove
  Other Breakpoints and Disable Other Breakpoints; Add Conditional Breakpoint… and Add Logging
  Breakpoint… are in the keymap.
- The Condition and Evaluate and log fields of a breakpoint complete from the language server, as
  if the expression were typed at the breakpoint's line.
- Breakpoint marks for a condition, a breakpoint that does not stop, and muted breakpoints.
- **Settings → Build, Execution, Deployment → Build Tools → Gradle**: the Gradle user home, offline
  work, and for the project's build: build and run using and run tests using (Gradle or the IDE),
  the distribution (wrapper, local installation or a folder), the Gradle JVM, VM options and
  downloading sources on reload. They are kept per project, and every Gradle run uses them: tasks,
  reload and the task list, the classpath of Java applications, and tests. With the IDE, Java
  applications are compiled with `javac` and tests run on the JUnit Platform by the IDE.
- **Download Gradle** in the Gradle tool window and the Gradle settings when no Gradle is found: the
  current release from services.gradle.org, checked against its published SHA-256, only when asked.
- **Before launch** in the Run/Debug Configurations dialog: an ordered list of tasks that run before
  a configuration starts (Run Another Configuration, Run External Tool, Activate Tool Window, Save
  Files, Run Go Command, Run Maven Goal, Run Gradle Task), with Add (⌘N), Remove, Edit and Move
  Up/Down. Their output shows in the run's tab under `#before-launch` lines, and a failing task stops
  the launch. **Show this page** opens the dialog before each start.
- Configurations adopted from another IDE's run configuration files keep their before-launch tasks
  where they are known and list the others as not supported.
- **Settings → Build, Execution, Deployment → Build Tools → Maven**, with **Importing**, **Runner**
  and **Running Tests**: the Maven home (wrapper, Maven on this computer or a folder), user settings
  file, local repository, work offline, thread count, always update snapshots, output level,
  checksum and fail policies; the importer's JDK and VM options and downloading sources and
  documentation on reload; the runner's VM options, JRE, environment, properties, skip tests and
  delegating the build of Java applications to Maven; and which Surefire and Failsafe arguments
  reach a debugged test. They are kept per project, and every Maven run uses them; Toggle Offline
  Mode and Toggle 'Skip Tests' Mode in the Maven tool window are the same settings.
- **Lombok** works in Java projects: when the project depends on it, the Java language server runs
  with the Lombok jar the build has downloaded as its agent, so getters, builders and `@Slf4j`'s
  `log` complete and navigate and show no false errors. The language server restarts, with a
  notification, when the project's Lombok version changes.
- **Settings → Build, Execution, Deployment → Compiler → Annotation Processors**: enable annotation
  processing, obtain processors from the build or a processor path, the generated sources folder and
  processor options, with what the build configures shown under each. They reach `javac` when the
  IDE compiles a Java application, and the language server's processing of Gradle projects.
- Kotlin code runs, is tested and is debugged like Java code, with the Kotlin plug-in: a ▶ in the
  gutter and Run Context Configuration (⌃⇧R) for a top-level `main`, a `@JvmStatic main` of an
  object, and JUnit 5, JUnit 4, kotlin.test and TestNG tests; they run with Maven or Gradle, with
  the results tree, links to the `.kt` lines and Rerun Failed Tests.
- Breakpoints in `.kt` files stop the program and the tests under the debugger, and the frames show
  the Kotlin files. The debugger starts the Java language server for it when no Java file is open.
- With the Ant plug-in, the **Ant** tool window lists a project's build files and their targets,
  the default one marked; **+** adds a build file, Filter Targets keeps the described ones, and a
  double-click runs a target with the project's JDK, its output in the Run tool window.
- Run configurations of kind **Ant**, and Go to Declaration from a target's name in a `build.xml`
  to the target.
- **Build → Build Artifacts…** builds, rebuilds or cleans one artifact or all of them, in the Run tool
  window.
- **Project Structure → Artifacts**: a jar the IDE writes from the modules' compiled classes and
  resources, with a main class in its manifest and the libraries extracted inside it or copied next
  to it, or the build's own packaging. Maven and Gradle projects are offered their own packaging
  first (`mvn package`, `gradle jar` or `bootJar`).

### Changed

- The folder tree and marks of Project Structure moved to **Modules → Sources**; the page opens on
  **Project**.
- Enter in the field at the top of the Debug tool window's variables evaluates the expression and
  shows its value under the field; ⇧⌘Enter or + adds it to the watches.
- ⇧-click on a line number sets a logging breakpoint that does not stop the program, instead of
  selecting the line.
- SQLite needs `sqlite3` 3.37 or later (macOS 13 and later have one), and `ATTACH` is no longer
  run.
- A module with Kotlin sources is built by Maven or Gradle even when the IDE was chosen to build,
  and the Run console says why.
- **Paste Image** offers a free name: when `image.png` already exists in the folder, the name
  field starts with `image_1.png` (then `image_2.png`…), its name selected. Typing a name that
  exists still shows that the image will be saved under a free name.

### Fixed

- SSH tunnels to databases open on Linux.
- Starting a configuration that is running from an agent (MCP `run.process.start`) stops the running
  instance and starts it again, as the Run button does, instead of running a second copy.
- SQL the database tools write quotes every schema, table and column name, so saving to a column
  named `to` or `key` works and a column named `current_date` is read, not the function.
- SQLite: in manual commit, saving the grid or releasing a savepoint no longer commits the open
  transaction behind the toolbar's back, and a savepoint that opened a transaction ends it when
  released.
- Grid edits saved inside an open transaction count as changes not committed, so Disconnect,
  Reconnect, closing a console and switching to Auto ask before they are lost.
- PostgreSQL: Commit of a transaction in which a statement failed says the server rolled it back,
  instead of reporting success; a schema or read-only switch made inside a transaction survives its
  Rollback.
- MySQL: a DDL statement that fails after its implicit commit shows the transaction as ended.
- Redis: a database chosen with `SELECT` in the console stays chosen after a reconnect.
- Changing only the isolation level keeps the open transaction, also one begun with `BEGIN` in
  auto-commit, and Commit and Rollback no longer wait behind a result that is still reading.
- Cancelling Count, Refresh or a navigator read asks the server to stop and closes a session that
  does not, and a change that finished while its cancel was on the way shows its result — in a CSV
  import and an EXPLAIN ANALYZE that is not rolled back too, so a cancelled import counts the rows
  it did commit.
- Statements run after connecting that change data are refused on a read-only connection.
- A `SELECT` of a function is not run again after a lost connection, since it may have changed data.
- SQLite: a trigger cut short after a `CASE … END` is refused instead of leaving `sqlite3` waiting.
- PostgreSQL: a statement stops reading at its row limit, even when its rows never end.
- **Run Again** in the Query History refuses a statement whose connection is no longer in the
  project instead of running it on another connection, and a console in the folder of such a
  connection asks for a connection rather than taking the project's only one.
- A run icon in the gutter of a `.sql` file runs its own statement when it follows the one before it
  without a space (`SELECT 1;SELECT 2;`), not the statement before.
- A negative parameter value written into a statement after a minus (SQLite, ClickHouse) is
  separated by a space, so `5-:n` never becomes a `--` comment.
- A connection URL's `?password=` goes to the Keychain like the password before the host, and neither it nor a client key's `sslpassword` is kept in `data-sources.json` or shown in the connection's settings; connections saved with one are answered without it and lose it from the file at their next change.
- Pasting rows whose header names the columns in another order puts each value under the column it names instead of by position.
- **Discard** and **Discard All** in the Recovered tab ask before dropping recovered work, and so does `db.recovery.discard` for agents: unsaved text or edits are dropped only with `confirmed: true` (code `confirmDiscard` otherwise), and every item only with `all: true`.
- Reverting a cell, the changes or a deleted row while those changes are being saved is refused.
- Editing a table's rows on a PostgreSQL connection that shows several databases picks the table of the connection's own database, not one of another database with the same id.
- The data grid no longer reads the connection, type and custom key files on every frame; they are read again when they change.
- Sorting a result by a SQLite column that holds values of several types no longer closes the IDE;
  numbers sort first, then text, then NULL.
- Typing binary data with non-ASCII characters into a cell is refused instead of closing the IDE.
- Copying from a table in an object tab after its contents shrank no longer closes the IDE.
- Running or splitting a `.sql` file that is not open uses its current text, so a file that got
  shorter no longer closes the IDE or runs its old statements.
- Reading more rows of a result, resetting its filter or exporting it no longer runs a statement that changes data again: past the first 10 000 rows of an `UPDATE … RETURNING` or a Redis `LPOP`, the IDE says that more rows need running it again, and an export of such a statement, or Export From Query on `DELETE FROM orders`, is refused before anything runs.

### Security

- ClickHouse and Redis through an SSH tunnel check the server's certificate and host name as
  without a tunnel, instead of turning the checks off.
- With jump hosts, the connection's own or a `ProxyJump` in `~/.ssh/config` (read with `ssh -G`),
  a saved SSH password is sent only to the SSH host itself, named by its alias or its `HostName`; a
  jump host's password is asked and never saved in its place.
- A SQLite connection runs only SQL on its own file: a line of only `/` or `GO` can no longer
  smuggle a `sqlite3` command such as `.shell` after a statement, and `sqlite3` runs in its safe
  mode, so `readfile()`, `writefile()`, `ATTACH` and `VACUUM INTO` never reach other files, also on
  a read-only connection.
- **Trusted projects:** a Maven, Gradle or Ant project runs nothing of its build by itself until you
  trust it (the notification's **Trust Project**). Until then the Java language server starts
  without Lombok, annotation processors and Gradle import, changed build scripts wait for **Load
  Changes**, the run configurations it comes with (and any whose before-launch tasks run a
  program) and its artifacts wait for trust, and AI agents can neither run its builds nor trust it.

## 26.11.1 - 2026-11-01

### Added

- **The Database workspace takes on more of your daily work** (still a preview):
  - **SQL in your code:** SQL in strings of Go, Rust, Python, JavaScript, TypeScript, Java and
    Kotlin files is highlighted, completed and checked against the connection's structure, and runs
    from the caret (⌘⏎) with its results in the Database workspace. **Find Usages** (⌥F7) of a
    table, view, routine or column lists where it is used in `.sql` files, code, consoles and the
    database itself.
  - **See and move data:** read-only **ER diagrams** of a schema or table, exported as an image;
    **Backup and Restore** with the `pg_dump` and `mysqldump` you have installed; **transfer
    tables** between databases and export Parquet; and **Migrations** that recognise the project's
    migration tool and run it from a tab of its own.
  - **A richer navigator and grid:** Properties on a double click, renaming,
    deleting and truncating objects with a preview, sizes, more PostgreSQL objects, a grid that says
    why a value cannot be edited, F2 and typing to edit, and the workspace's own keys and toolbar.
  - **Splits in the workspace:** consoles and scripts can sit side by side, and `.sql` scripts move
    between the editor and the workspace.
- **Clickable links in the Terminal:** hold ⌘ and point at a web address or a file path (with the
  line and column compilers and test runners print, such as `src/main.rs:12:5`) to underline it;
  ⌘-click asks what to do: Open in Browser or Copy Link for a web address; Open in Editor at the
  line, Reveal in Project, Open in Finder or Copy Path for a file. "Remember my choice" skips the
  question next time; Settings → Tools → Terminal → Links asks again. Agents list and open links
  with `terminal.link.list` and `terminal.link.open`, but only you open a web address in the
  browser.

### Changed

- **English by default.** The IDE starts in English whatever your system's language. Choose
  Русский (or System, to follow your system) in Settings → Appearance & Behavior → Appearance →
  Language or with the welcome window's language switch; the choice is remembered across restarts
  and updates. If you never chose a language and your Mac is set to Russian, choose Русский once.
- **TypeScript and JavaScript coloured by meaning** (ADR-0184): calls in the colour
  of function names, globals and module variables purple and italic, properties purple where they
  are declared; locals, parameters and properties in use stay plain. The language server's
  semantic tokens refine what the editor's own reader sees.

### Fixed

- Connecting no longer waits forever when the Keychain does not answer, and says what it is
  waiting for.
- Making a connected connection Production takes effect at once, without reconnecting.
- Removing a connection closes its tabs and tells its consoles that it is gone.
- SQLite: `$1` and `?1` parameters, `sqlite:///` URLs keep their path, empty tables keep their
  columns, and quick re-reads no longer fail.
- Result tab numbers start again with each run, and messages no longer show command names.
- **F4** opens the file from the Commit window and from the diff viewer.
- A database driver that comes as a program (DuckDB) says what went wrong: a query on a connection
  whose connect failed tells why, such as the `duckdb` program missing, and a new connection of a
  driver that asks no password starts without password authentication.
- A database driver program that a plug-in names at a full `https://` address is downloaded from
  that address; before, the IDE looked for it next to the plug-in catalog and failed.

### Security

- AI agents can no longer choose the program a database connection runs (the `sqlite3`, `duckdb` or
  SQLcl program, or a Java home): only you set it.

## 26.10.4 - 2026-10-27

### Added

- **The Database workspace grows into a daily tool** (still a preview):
  - **Write SQL faster:** completion and navigation that know your schema, **Reformat Code** with
    Code Style → SQL, bind parameters and script variables with a Bind Parameters dialog, and project
    `.sql` files that run with the connection and schema you choose for each.
  - **Find and follow:** database objects in Search Everywhere and Go to Object, foreign keys and
    references followed with Back/Forward, text found in a schema's tables on the server, and a
    searchable query history.
  - **Move data:** Export results and tables in one streamed dialog, Copy As and pasting from a
    spreadsheet, CSV import into tables (with `COPY` on PostgreSQL), and one script run on several
    connections in turn.
  - **Understand and change the structure:** Explain plans as a tree with the slow nodes marked
    (`ANALYZE` is rolled back), a Sessions tab with locks, cancel and terminate, structure edits with
    an SQL preview saved as migrations, and schema and data compare with sync scripts.
  - **Production safety:** Production connections are read-only or ask before changing anything,
    for you and for AI agents alike; a type that forbids agents everything
    hides its connections' structure and history from them too.
  - **Nothing lost after a crash:** unsaved console text, data edits not saved yet and open
    transactions survive a crash or a power loss and come back for review; nothing is run for you.
  - **More databases:** wire-compatible servers (CockroachDB, TiDB and others) have their own
    profiles with the right defaults, and plug-ins can add databases through driver programs.
  - **Work beside the IDE:** open a connection's own client (psql, mysql, sqlite3, redis-cli,
    clickhouse) in the Terminal with its settings, the password passed privately, and generate Go,
    Rust or TypeScript types from a table.
  - **See spatial data:** PostGIS, MySQL and GeoPackage geometries, WKT and GeoJSON drawn as shapes
    in the value panel, offline.

### Fixed

- **Double-clicking the title bar zooms the window on every macOS.** Double-clicking the toolbar's
  empty space (or the Welcome window's title strip) now does what System Settings → Desktop & Dock
  → "Double-click a window's title bar to" says — Zoom or Fill, Minimize, or nothing — also on the
  macOS versions where it did nothing before.

## 26.10.3 - 2026-10-27

### Added

- **Settings → Project Structure (⌘;), the first page of Settings.** The project's content root as
  a tree of folders: mark a folder **Excluded** or as a **Resource Root**, take a mark off, and
  set **Exclude files**, name patterns such as `*.log;tmp?` that leave matching files and folders
  out of the project. Apply or OK saves them in the project's module file, where JetBrains IDEs keep
  them, and the searches follow at once.
- **Mark Directory As → Resource Root** and **Unmark as Resource Root** in the Project tool
  window; resource roots get a purple folder, also those a JetBrains IDE marked.
- **Settings search finds actions and plug-ins.** A text that is part of an action's name, as
  Keymap shows it or in English, or that has a word beginning one of its words, keeps Keymap in
  the list of sections and opens it with its own search set to the text; one that matches an
  installed plug-in keeps Plugins and opens it on Installed, filtered by the text. The page you
  are on stays while it is in the list; otherwise Settings shows the first section named by the
  text, else the first one with an option of that name, else the first one found. Keymap's own
  search finds the English names too. Agents search with `view.settings.open { query }`, which
  answers the pages found.
- **Compare Directories.** Select two folders in the Project tool window and press **⌘D**: the
  folders open side by side, compared in the background, every file only on the left, only on the
  right, different or equal, with a toggle for each state and a filter
  (`*.go & !*_test.go | README*`). Each row copies its file to the right or the left, or deletes it;
  **Synchronize Selected** and **Synchronize All** apply them after asking before anything is
  overwritten or deleted, and **⏎** or a double-click opens a file's diff. AI agents use
  `file.compare.folders`, `file.compare.synchronize` and `view.folderDiff.*`; files hidden from
  agents are left out.
- **Reload Changed Classes while debugging Java.** Saving a changed method body while a Java program
  is debugged reloads the changed classes into it at once (hot code replace through java-debug), and
  **Run → Reload Changed Classes** does it on request; a notification names the classes replaced or
  says why the JVM refused the change.

### Changed

- **Fix or accept a typo with ⌥⏎.** On an underlined word, **Fix typo** lists the closest words in
  the word's case and replaces it with the one you pick, **Typo: Rename to '…'** renames a misspelt
  name you declared everywhere through the language server (`parseUrlHadnler` to
  `parseUrlHandler`), and **Save '<word>' to dictionary** stops marking it, asking which dictionary
  (**Select Dictionary**: application-level or project-level); **⌘Z** right after a save takes the
  word out again. The project-level dictionary is `.idea/dictionaries/project.xml`, the same file
  JetBrains IDEs read and write, so the team shares the words and nothing else
  in the file changes. **Settings → Editor → Natural Languages → Spelling** saves every word to one
  dictionary when you turn on Use single dictionary for saving words, lists the Accepted words, and
  adds Custom dictionaries: plain word lists and Hunspell `.dic`/`.aff` files. AI agents use `spelling.typo.get`, `spelling.typo.change`, `spelling.word.save`,
  `spelling.word.remove` and `spelling.dictionaries.get`.
- **Quick Switch Scheme.** ⌃` (also View → Quick Switch Scheme… and Find Action) opens the Switch
  popup: numbered rows Theme, Editor Color Scheme, Code Style Scheme, Keymap,
  View Mode and Zoom. Enter, a click or a row's digit shows that list in the popup's place, with an
  arrow at what is in use; typing filters the rows. Choices apply at once and are saved exactly as
  in Settings; View Mode reads Enter or Exit for Presentation, Distraction Free, Full Screen and Zen
  Mode, Keymap ends with Configure Keymap…, and Zoom sets the IDE zoom. Editor Color Scheme lists
  the themes, which carry the editor's colours.
- **Code Style Scheme.** Settings → Editor → Code Style → Scheme chooses **Project** (the default:
  the code style a project keeps in `.idea` wins where it sets one) or **Default (IDE)** (only your
  own code style); agents set `codeStyleScheme`.
- **Tool window view modes and saved layouts.** Every tool window has a View Mode (⋮ in its header,
  Window → Active Tool Window → View Mode): Dock Pinned, Dock Unpinned (hides when you click
  elsewhere), Undock (over the editor), Float and Window (a window of its own, on any screen).
  Window → Layouts → Save Current Layout as New… keeps the whole layout, modes included, for every
  project; each saved layout has Restore, Save Changes, Rename and Delete…, ⇧F12 restores the
  current layout and Default puts everything back. Stretch to Left, Right, Top and Bottom are now
  ⌃⌥←→↑↓, and a stripe button's menu has Hide and Move to.
- **New (⌘N) and File and Code Templates.** ⌘N in the Project tool window (or its **+**) opens the
  New popup: File, Scratch File, Directory, the file templates that fit the folder, Edit File
  Templates… and EditorConfig File; **File → New** and the tree's **New ›** list the same. A template
  that asks for a name or variables opens a dialog for them, and the new file opens with the caret
  at the template's `$END$`. **Settings → Editor → File and Code Templates** edits the templates: the Scheme (Default for the IDE, Project in `.idea/fileTemplates`), the tabs Files,
  Includes and Code, Create Template, Create Child Template File, Remove Template, Copy Template
  and Revert to Original Template, and the variables described under the editor. Built-in
  templates cannot be removed; changed ones are shown in blue. Agents use `file.new.list` and
  `file.templates.useScheme`.
- **File → Manage IDE Settings.** **Export Settings…** writes your settings, keymaps, themes, live
  templates and plug-in list (each with a check box, and Select All) into a ZIP file;
  **Import Settings…** reads such a ZIP, or one exported from a JetBrains IDE (its keymaps and colour
  schemes are taken), shows Select Components to Import and lists every entry it skips, with why;
  **Restore Default Settings…** backs your settings up to a folder, then restarts with the defaults,
  and Import Settings… brings the backup back. A project's `.zen` and `.idea` are never touched, and a
  ZIP that is too large or names a path outside its folder is refused. Agents use the
  `settings.ide.*` commands.
- **External Tools.** Set up a program once, such as a linter, a formatter or a script, in
  **Settings → Tools → External Tools**, and run it on the current file or the project from
  **External Tools** in the Project view's and the editor's context menus, or a shortcut of your
  own (Keymap → External Tools). Arguments take macros such as `$FilePath$`, `$SelectedText$`
  or `$Prompt$`, with a live preview; the output shows in a Run tool window tab named after the
  tool, with Stop and Rerun, and output filters turn file positions into links.
- **Power Save Mode.** File → Power Save Mode pauses checks as you type, the completion popup that
  opens by itself, go.mod's network checks and reindexing of changed files; a battery icon in the
  editor's top right corner and a notification say it is on. Turning it off brings the highlighting back in every open editor, and
  the IDE remembers it across restarts.
- **Status bar widgets and the Memory Indicator.** Right-click the status bar to choose which
  widgets it shows; the new Memory Indicator shows the memory the IDE takes.
- **Activity Monitor.** Help → Diagnostic Tools → Activity Monitor… lists the processor use of the
  IDE, the index, each language server and Git, the busiest first.
- **The Database workspace, a preview.** The **Database** button at the top of the right tool window
  bar (under Git) expands a database tool over the editor area: an **Overview** tab and SQL
  consoles. **New Console** creates `console.sql` in the IDE's own folder, a real SQL editor whose
  text is kept across restarts. Connections to PostgreSQL 12 to 18, MySQL 8.0 and later, MariaDB
  10.6 and later, SQLite (through your system's `sqlite3`, 3.35 or later), Redis and Valkey (also Sentinel and
  Cluster) and ClickHouse are made in the
  **connection dialog** (by host or by pasting a URL; Test Connection shows the server's version or
  the likely cause of a failure), with the Development, Test and Production connection types and
  their safety defaults, every SSL mode, and **SSH tunnels** through your own OpenSSH and
  `~/.ssh/config` (an unknown host key is always asked, never accepted for you).
  - **Run SQL:** ⌘⏎ runs the statement at the caret, ⌥⌘⏎ the script; results open below in a fast
    grid (a million rows scroll smoothly), with Output, the Execution Log, the time and rows of each
    statement, and errors underlined where the server points. Reading stops at 10 000 rows and never
    leaves a lock open on the server; Fetch Next and Fetch All read on.
  - **Database Navigator** on the left: databases, schemas, tables, views, functions and their
    columns, keys, indexes and triggers, read in the background and kept, so it shows at once on
    the next start, with Generate SQL.
  - **Databases your project already names** are offered on the Overview: `DATABASE_URL`-style
    values of `.env` files and the database services of Compose files, added with one click (a
    password found there goes to the Keychain only then). **Import Connections…** brings your
    DBeaver and JetBrains connections, without their passwords.
  - **Filter, sort and edit data:** a WHERE filter above every grid with completion and history,
    sorting from the column headers, and editing cells, adding and deleting rows, with the SQL
    shown before it is saved in one transaction; a table without a key stays read-only until you
    choose one, and a save that would change several equal rows changes none.
  - **Objects open in tabs:** a table or view opens on its rows in key order, with Properties and
    DDL pages; a Redis key opens its value with its type, time to live and size. The **value panel**
    (⇧⌘P) beside every grid shows a value as text, pretty JSON, XML, hex or an image, and the
    selection's count, sum (exact for decimals), average, minimum and maximum show as you select.
  - **Transactions you can see:** Auto, Manual or Smart commit per console, Commit and Rollback in
    the toolbar with a counter of pending changes, and a question before anything would drop them.
  Passwords and SSH passphrases are kept only in the macOS Keychain, read and written
  in the background so a Keychain prompt never freezes the IDE; nothing connects until you ask. ⇧⎋
  or the button collapses it; your code tabs are untouched underneath.

### Fixed

- View → Appearance on macOS reads **Enter Presentation Mode** (and the other modes) while no mode
  is on, instead of the keymap's Toggle names.
- AI agents see the language servers the IDE installed itself in `lsp.servers.list`; they were left
  out of the list as if they were the IDE's private files.

## 26.10.2 - 2026-10-25

### Added

- **Kotlin, as a plug-in.** Opening a Kotlin file offers the Kotlin plug-in. It highlights Kotlin
  and Gradle's Kotlin DSL and lists their declarations, and it installs a Kotlin language server
  when you ask: the Kotlin team's kotlin-lsp, or fwcd's open-source server, chosen in Settings → Language
  Servers. A language served by several servers now offers that choice.
- **Completion follows what you type with every server.** When a language server says its list is
  not complete, the IDE asks it again as you type more of the word, so the item you want appears even
  if the first list stopped short of it.
- **AI agents drive tool windows without a window.** A headless IDE (`--headless`, `mcp --headless`)
  keeps each project's window state undrawn: agents open and close tool windows (Maven, Gradle,
  Problems, Run…), read what they show and run their actions, as in a window.
- **Maven and Gradle.** Run configurations of kinds Maven and Gradle, with the project's `mvnw`
  or `gradlew` first, and the Maven and Gradle tool windows in projects with a `pom.xml` or a Gradle
  build while the Maven or Gradle plug-in is on. The plug-ins are offered when such a project opens.
- **Run Java programs, in every Zen IDE.** Application configurations run a class's `main` (a gutter
  icon next to it, or ⌃⇧R), built first by the project's Maven or Gradle, or by `javac` without
  them, with the project's JDK; JAR Application runs `java -jar`. Stack traces link to the sources.
- **JDKs, in every Zen IDE.** The IDE finds the JDKs on your computer (JAVA_HOME, macOS's
  `java_home`, `/Library/Java/JavaVirtualMachines`, Homebrew, SDKMAN!, asdf, mise, Gradle's and
  Maven's toolchains) and lets each project choose its JDK and language level in **Settings →
  Languages & Frameworks → JDKs**. **Add JDK** adds one from disk or downloads Eclipse Temurin into
  the IDE's own folder, only when you ask. Language servers and run configurations of a Maven or
  Gradle project start with its JDK (`JAVA_HOME`).
- **Java, as a plug-in.** The Java plug-in, offered when you open a `.java` file, highlights,
  folds and outlines Java 25 (records, sealed types, switch patterns, text blocks) and adds **New →
  Java Class** and its siblings. On your word it downloads the Eclipse JDT Language Server into the
  IDE's own folder for problems, navigation, Find Usages, Rename, Reformat Code, Optimize Imports,
  quick fixes, Quick Documentation and the hierarchies, in Maven and Gradle projects.
- **Java tests and debugging.** Run a Java test class or method from the gutter (JUnit 5, JUnit 4,
  TestNG) with the project's Maven or Gradle: results as a tree with times and each test's output,
  failures linked to your code, Rerun Failed Tests. Debug tests and programs with java-debug, which
  comes with the Java plug-in's server: breakpoints with conditions, exception breakpoints, stepping,
  variables, watches and Evaluate Expression. When the server finds no Java 25 to run on, the banner
  offers to download one.

### Changed

- **About** names the trademarks of the languages every Zen IDE supports through plug-ins: Oracle's
  credit line for Java and the Kotlin Foundation's for Kotlin, with the notice that Zen IDE is not
  affiliated with either.

## 26.10.1 - 2026-10-25

### Changed

- **One version for every Zen IDE.** GoGarden, RustRonin and WebWave are now released together
  under one calendar version, `YY.M.N`: the year, the month and the release's number within the
  month, so 26.10.1 is the first release of October 2026. It follows GoGarden 0.38.49.

### Security

- **Agents neither read nor change files hidden from agents.** Patches an agent makes (of local
  changes, a commit or a shelf, on the clipboard or in a file) leave hidden files out; the IDE's
  own data folder (Git's undo history, shelves, Local History, settings) is closed to agents, but
  for the scratch files; and an agent's delete, rename, rollback, discard, checkout, merge, reset
  or new file that would change a hidden file, or a folder holding one, is refused, while a stash,
  a changelist's commit or Replace in Files leaves hidden files as they are and says how many.
- **Hidden files stay out of agents' HTTP requests, copies and Local History.** An agent's HTTP
  request whose body reads a hidden file (`< .env`) is refused before anything is sent, and a
  request you ran that read one shows an agent neither body; an agent cannot copy a hidden file, a
  folder holding one, or into a hidden place; a folder's Local History no longer lists hidden
  files' versions to an agent; `git show HEAD:.env` typed by an agent is refused like `cat .env`; and an agent's
  discard of all changes reverts tracked files only, never deleting untracked files or links.
- **The HTTP Client's private environment is hidden from agents.** `http-client.private.env.json`,
  where tokens and passwords go, is now among the files hidden from agents by default: an agent's
  request never uses its values, and one that needs them is refused before anything is sent.

## 0.38.49 - 2026-10-25

### Security

- **Comparing files with a hidden file is refused.** An agent could read a file hidden from agents
  by comparing it with another (Compare With…, three-file compare). Every path an agent's command
  takes is now checked, whatever the parameter is called.

## 0.38.48 - 2026-10-25

### Security

- **Hidden files stay hidden under any spelling.** Whether a file is hidden from agents is now
  decided on its real path: `.ENV` for `.env` on a Mac, `sub/../.env`, and a link to the file or to
  its folder are all refused, in reads, edits, searches, the terminal and the file in front. A
  command that works on the file in front is refused while a hidden file is in front even when the
  agent passes some other path it does not use.

## 0.38.47 - 2026-10-25

### Security

- **A hidden file in front stays hidden.** While a file hidden from agents is the active tab, an
  agent's commands that work on the file in front (its text, selection, structure, problems,
  history, keys and actions) are refused as reading it by name is; an agent opens another file
  first.

### Fixed

- **Files hidden from agents stay out of their searches.** An agent's Find in Files, Search
  Everywhere and TODO searches no longer look into the files of Settings → Tools → MCP Server →
  Files hidden from agents at all: a search for part of a secret answers the same whether or not
  the secret starts that way. The answer says only how many files are hidden.

### Changed

- **A shorter tool list for agents.** The list of MCP tools an agent reads at the start of a session
  is a third smaller (66 KB → 42 KB): `set_settings` lists its settings on request
  (`describe_command`), and the commands' own tools leave out what told an agent nothing.

### Added

- **Search Everywhere forgives the wrong keyboard layout.** A query typed with the other layout on
  still finds what it means, on every tab and in Go to File, Class, Symbol and Action: `ьфшт`
  typed on the Russian layout finds `main`, after what `ьфшт` itself finds. A pasted query is
  taken as it is.

## 0.38.46 - 2026-10-25

### Fixed

- **Markdown Preview Only scrolls again.** In Preview Only the preview jumped back to the top as soon
  as it was scrolled, so a README could not be read in that mode. It now scrolls freely and keeps its
  place when the text changes; Editor and Preview still scroll together, each following the side you
  scroll, and switching between the modes keeps the place.

## 0.38.45 - 2026-10-24

### Added

- **Shorter answers for AI agents.** Symbols, types, a file's structure and usages come as one line
  each when an agent asks for `compact` (15 symbols: 1 KB instead of 3 KB); problems can be narrowed
  to errors only, to some files or to a number; Find Usages takes a declaration's name instead of a
  place and can add the places only text knows of (comments, strings); an edit answers the errors it
  brought (`checkProblems`), so no separate check is needed.
- **Files hidden from agents.** Settings → Tools → MCP Server → **Files hidden from agents**:
  environment files, keys, certificates and credential files by default. Agents get an error instead
  of their text, search results show `[redacted]`, and the IDE tells you which file was kept from an
  agent. Only you can change the list.
- **`--no-restore`** opens a project without its last tabs in an IDE without a window
  (`mcp --headless --no-restore .`).
- **What an agent's calls cost.** Each MCP answer says how long it took, and `mcp.session.stats`
  counts the calls, bytes and time per tool.

### Changed

- **`next-env.d.ts`** in a Next.js project has the Next.js icon and counts as generated code:
  **Show Usages** leaves it out when **Show Results In Generated Code** is off.
- **The preview switch is kept per file**: switching one SVG or Markdown file
  to **Editor Only** or **Preview Only** leaves the others as they were, and a file you never
  switched opens as **Editor and Preview**.

## 0.38.44 - 2026-10-24

### Added

- **A full Markdown preview.** A Markdown file opens with its preview beside it (the same switch as
  SVG): GitHub Flavored Markdown drawn by the IDE in your theme, with tables, task lists whose boxes
  tick the text, footnotes, GitHub alerts, code highlighted as in the editor with a **Copy**
  button, links that open files and anchors, and pictures next to the file. Pictures from the web
  wait until you allow their site. The preview and the text scroll together, a click in the
  preview puts the caret on the line, and the **Structure** tool window lists the headings.

## 0.38.43 - 2026-10-24

### Added

- **SVG as text and as a picture.** An SVG file now opens as text you can edit, with its picture
  beside it. The switch at the editor's top-right corner shows **Editor Only**, **Editor and
  Preview** or **Preview Only**, kept for every SVG file; the line between text and picture can be
  dragged. The picture follows the text as you type, before saving, without slowing typing; an SVG
  that is not valid shows why. The preview zooms as the image viewer does. Other images still open
  in the image viewer.

## 0.38.42 - 2026-10-24

### Added

- **ESLint, Prettier and Biome from your project.** Plug-ins offered when a project uses them:
  ESLint's and Biome's problems appear as you type next to the language server's, with their quick
  fixes on ⌥Enter, **Fix Problems with Linters** and fixes on save; Reformat Code formats with your
  project's Prettier (respecting `.prettierignore`) or Biome. Settings → Languages & Frameworks →
  **Code Quality Tools** turns each on or off and chooses what runs on Reformat Code and on save;
  Tools → Actions on Save links to it.
- **Where a language server runs from** can now also be **In the project**, for a tool such as Biome
  that runs from the project's `node_modules`.
- **XML in every IDE.** XML files (`.xml`, `.plist`, `.xsd`, `.svg` as text, `.csproj`, `.xaml`,
  `.rss`…) are highlighted, folded, shown in the Structure tool window and checked as you type
  (end tags that do not match, elements not closed), and typing `>` or `</` closes tags.
- **File types by content.** A file whose name says nothing gets its type from what it starts
  with: an Apple property list saved as `timeweb-cloud.cyberduckprofile` (`<?xml`, `<!DOCTYPE
  plist`) is XML, with its icon, highlighting and folding. Only the first 4 KiB are read, your own
  patterns and Override File Type still win, and **Settings → Editor → File Types** shows these
  rules read-only.

## 0.38.41 - 2026-10-23

### Added

- **Components imported from TypeScript.** With the Svelte or Vue plug-in, `.ts` and `.js` files
  read the `.svelte` and `.vue` components they import: props typed, checked and completed, Go to
  Declaration into the component, Find Usages reaching it — with nothing to set up in the project.
  The plug-in adds its TypeScript server plug-in to the TypeScript Language Server's own folder when
  it is installed; if both were installed before, the banner above a `.ts` file of a Svelte or Vue
  project offers **Install**.
  The server restarts by itself to load it.

## 0.38.40 - 2026-10-23

### Added

- **Svelte from the Marketplace.** Opening a `.svelte` file offers the Svelte plug-in: components
  highlighted with their blocks, directives and runes, folded, outlined and completed, end tags
  added as you type, and the Svelte Language Server installed with it for problems, navigation,
  rename and formatting. `.svelte` files have their own icon in the project tree and the tabs.
- **SvelteKit.** Opening a SvelteKit project (`@sveltejs/kit` in `package.json`) offers the new
  SvelteKit plug-in, which installs with the Svelte plug-in: run configurations `vite dev`,
  `vite build` and `vite preview`, `svelte-kit sync` offered while `.svelte-kit/` is missing, and
  templates for the route files (`+page.svelte`, `+layout.svelte`, `+page.ts`, `+server.ts`…) and
  `hooks.server.ts`. `svelte.config.*` files have their own icon.

## 0.38.39 - 2026-10-23

### Added

- **React and Next.js.** Opening a React project (`react` in `package.json`) offers the new React
  plug-in, and a Next.js project (`next.config.*`, or `next` in `package.json`) the new Next.js
  plug-in, which installs with React. React adds `vite`, `vite build` and `vite preview` run
  configurations to Vite projects and templates for components, hooks and contexts; Next.js adds
  `next dev`, `build`, `start` and `lint` and templates for the App Router (`page.tsx`,
  `layout.tsx`, `route.ts`, `loading.tsx`, `error.tsx`, `not-found.tsx`) and the Pages Router.
  `next.config.*` files have their own icon.
- **JSX end tags.** In `.jsx` and `.tsx` files, typing the `>` of a start tag adds its end tag,
  as in HTML.

## 0.38.38 - 2026-10-23

### Added

- **TypeScript and JavaScript with a language server.** `.ts`, `.tsx`, `.mts`, `.cts`, `.js`,
  `.jsx`, `.mjs` and `.cjs` files get problems as you type, completion from types, parameter info,
  quick documentation, Go to Declaration, Type Declaration and Implementation, Find Usages, Go to
  Symbol, the Call Hierarchy, Rename across files, Reformat Code, Optimize Imports and context
  actions, from the TypeScript Language Server. The banner above the first such file installs it,
  with one click, into the IDE's own folder (Node.js with npm needed); your project's TypeScript 5
  or 6 is used when it has one. `.vue` files stay with the Vue Language Server.

### Changed

- Updating plug-ins when the IDE starts no longer installs language servers; installing or
  updating a plug-in yourself still does.

## 0.38.37 - 2026-10-23

### Added

- **A plug-in's language server installs with the plug-in.** Installing Vue, YAML, JSON, Shell
  Script or Docker from the Marketplace or from the banner above a file now installs its language
  server too, in the background, into the IDE's own folder; a notification says when it is ready,
  and a new plug-in's server works without a restart. A server you already have (on your `PATH` or
  at a path you set) is kept. Without Node.js, one notification says where to download it, and
  the server can be installed later from its banner. Uninstalling the plug-in removes the copy of
  the server the IDE installed.
- **Where each language server runs from.** Settings → Languages & Frameworks → Language Servers has
  an **Installed** column — Locally, Global (PATH) or Custom path — and the language server list in
  the status bar names it too.
- **Plug-ins for your project.** Opening a project that a plug-in of the Marketplace supports
  offers it in a notification: a Nuxt project (`nuxt.config.ts`, or `nuxt` in `package.json`)
  offers the new Nuxt plug-in, which installs with the Vue plug-in it needs, adds the run
  configurations `nuxi dev`, `build`, `generate` and `preview`, offers to run `nuxi prepare` while
  `.nuxt/` is missing and adds templates for pages, layouts, components, composables and server
  routes. `nuxt.config.*` files have their own icon.
- **npm scripts.** The scripts of a `package.json` have run icons in the gutter, and **Edit
  Configurations…** has **npm** configurations that run a script or a program of the project's
  packages with the project's package manager (npm, pnpm, Yarn or Bun).

## 0.38.36 - 2026-10-22

### Added

- **Vue from the Marketplace.** The Vue plug-in highlights, folds, outlines and completes `.vue`
  single-file components and runs the Vue Language Server for problems, navigation, rename and
  formatting; `.vue` files have their own icon in the project tree and the tabs.

### Changed

- **Language servers no longer touch your global npm packages.** Installing a server written for
  Node.js (YAML, JSON, Bash, Dockerfile, Vue) puts it into a folder of the IDE's own,
  `language-servers/<server>` in its data folder, instead of `npm install -g`; installing again
  updates it there. A server you installed globally yourself is still found on your `PATH`, and a
  path you set in the server's settings still wins. The Vue Language Server checks with your
  project's TypeScript 5 or 6 and otherwise with the TypeScript installed with it, so a global
  TypeScript 7 is no longer replaced.

## 0.38.35 - 2026-10-22

### Added

- **Kozō, the mascot of the Zen IDE family.** A young novice monk in pixel art meditates on the
  splash and at the bottom of the welcome screen's left column, in GoGarden's teal robe, watering
  his garden: drops fall from the can and a sprout comes up. He breathes and moves only while the
  window is visible; with macOS's Reduce motion he sits still.

## 0.38.34 - 2026-10-22

### Added

- **Macros.** Edit → Macros records what you do in the editor — typing, caret moves, selections,
  line actions, comments, Reformat Code and saving — and plays it back in any file: Start Macro
  Recording (the status bar shows Recording macro with a stop button), Stop Macro Recording with a
  name in Enter Macro Name (or none, to keep it only for Play Back Last Macro), Play Saved Macros…
  and Edit Macros to rename and remove macros and their actions. One ⌘Z takes a playback back.
  Saved macros belong to the IDE and are actions in Settings → Keymap under Macros and in Find
  Action, so they can have a shortcut; AI agents use the `editor.macro.*` commands, and what an
  agent changes while you record stays out of your macro.
- **Find in Files works.** The window no longer blocks the IDE: **Pin Window** keeps
  it open while you click and edit in the editor, with its results still there. The header shows
  "10 matches in 3 files" ("100+ matches in 29+ files" past the limit), the **File mask** check box
  with the masks used before, the **Filter** and the pin. The query field has the recent queries
  (⌥↓), **×**, **New Line** (⇧⌘⏎) and **Match Case** ⌃⌥C, **Words** ⌃⌥W, **Regex** ⌃⌥X; the scope
  row underlines its letters. The search starts a moment after you stop typing. The matches of one
  line share a row, a file name found in several folders shows its folder (`kucoin/client.go`), and
  rows from test files are tinted. The preview is coloured like the editor, titled "file · folder",
  and you can edit right there. **⌘⏎ Open in Find Window** lists every match in the Find tool window,
  on a new tab if you like, and asks **Too Many Results** past 1000. Both limits (100 and 1000) are
  in Settings → Advanced Settings → Find in Files.

- **The search bar.** ⌘F reads: the arrow that opens the replace row, the
  field with its history, New Line, Cc, W and .*, the count, ↑ ↓, the Filter, ⋮ and ×.
  - A selection of several lines turns on **In Selection**; **⌃⌥E** (or the Filter's first item)
    turns it on and off.
  - **⌥↓** (or the lens) lists recent queries and replacements, shared with Find in Files and kept
    across restarts. **New Line** (⇧⌘⏎) makes the field multi-line.
  - **⋮**: Open Results in Find Window (⌥F7) lists every match in the Find tool window; the Multiple
    Cursors actions; Scroll to Results During Typing.
  - With Regex on, the selected text taken as the query is escaped; pasted text is not.
- **Edit → Find → Next / Previous Occurrence of the Word at Caret.**

- **⌃⇧G (Unselect Occurrence)** takes back the occurrence ⌃G added last, also after ⌃G wrapped
  around to the top of the file.
- **The Find tool window lists Find in Files' results.** **Open in Find Window** now
  searches in the background ("Searching for '…' in <file>…" in the status bar, with **×** to stop)
  and asks **Too Many Results** — Abort or Continue — when it passes the limit. The tab
  "Occurrences of '…'" shows Targets, then "Found occurrences in Project" with the usages grouped
  by usage type (code, comments, string constants), module, folders, file and line.
  - **Group By**: Directory Structure ⌃D, File Structure ⌃S, Module ⌃M, Flatten Modules ⌃O, Group
    by Directory ⌃P, Production/Test, Usage Type ⌃T, Merge Usages from the Same Line ⌃L.
  - The toolbar: Rerun ⌘R, Previous/Next Occurrence, Settings ⇧⌘F (back to Find in Files with the
    same search), Show Usages In Comments, Expand/Collapse All, Preview Source, Pin Tab and Open
    results in new tab.
  - **⌫** excludes a node (struck through) and takes it back; Replace in Files' tab has **Replace
    Selected** and **Do Replace All**, which skip excluded nodes.

- **Replacing in files is safer.** Replace in Files and the Find window's replacements change
  nothing when a file is read-only, not in UTF-8 or changed since the search, and say which file;
  each open file changes in one undo step, the others get a Local History version, and CRLF line
  breaks stay. A file that cannot be written midway puts back the files written before it.
- **For AI agents:** `search.text.occurrences` and `view.findInFiles.openInFindWindow` answer at
  once with the new tab; pass `waitMs` to get the final count, as before.
- **Last Edit Location (⇧⌘⌫)** takes you to the place you last changed the text, in any file; press
  it again for the change before. **Next Edit Location** goes back toward the latest change. Both
  are in the Navigate menu; a closed file opens again, a deleted one is skipped.
- **Go to Type Declaration (⇧⌘B or ⌃⇧B, ⇧⌘-click).** From a variable or an expression to the
  declaration of its type: `f := &File{}` leads to `type File struct`; several types
  open a list to choose from.
- **Go to Super Method (⌘U).** From a method of a concrete type, on its name or in its body, to the
  interface method it implements, for example `(*File).Read` to `io.Reader.Read`; several open a
  list.
- Both are in the Navigate menu, remembered by Back and Forward, and available to agents
  (`navigate.typeDeclaration.goTo`, `navigate.superMethod.goTo`).
- **Select In (⌥F1).** Navigate → Select In… shows the file in the editor, or the one selected in
  the Project tool window, in another place, picked by its number, **Enter** or a click: **Project
  View** (the tree unfolds to the file and selects it, for a library's file too), **Structure** (the
  symbol at the caret), **Commit** (when the file has changes), **Finder** and **Terminal**. Agents use `view.selectIn.open` and `view.selectIn.target`.
- **Open In on editor tabs.** Right-click a tab: **Open In → Finder** shows the file in its folder,
  **Open In → Terminal** opens a terminal tab there.
- **Line bookmarks.** **F3** (Edit → Bookmarks → Toggle Bookmark) marks the
  caret's line with a bookmark icon in the gutter; **⌥F3** gives it a digit or letter and a
  description, chosen in a 0–9 / A–Z grid that marks the characters in use and asks before moving
  one. **⌃⇧1** toggles bookmark 1 on the line and **⌃1** jumps back to it; **⌥⌘F3** (Go to
  Mnemonic…) jumps to any of them, and Back returns. Previous and Next Line Bookmark can get
  shortcuts in Settings → Keymap. Hover an icon for its description, ⌥-click it to rename it, or
  right-click the gutter for the Bookmarks menu. Bookmarks move with their lines as you edit and are
  kept when the IDE restarts, in the IDE's own data, not in the project. Agents have all of it as
  `navigate.bookmark.*` commands.
- **The Bookmarks tool window and Show Line Bookmarks.** **⌘2** (View → Tool
  Windows → Bookmarks, at the left bottom of the stripe) shows your bookmark lists: the default
  one, named after the project and tagged Default, first, each with its bookmarks as `file:line`
  and their description, and the breakpoints as a list of their own. Right-click a list to create,
  rename, delete it (its bookmarks leave the code) or mark it as the default, where new bookmarks
  go; right-click a bookmark to open, rename, move it up or down or to another list, or drag it
  there. Double-click or Enter opens one, **⌃⌥↓** / **⌃⌥↑** go through them, Delete removes one;
  **⋮** has Group Line Bookmarks by File and Ask Before Rewriting Mnemonic. **⌘F3** (Edit →
  Bookmarks → Show Line Bookmarks…) lists the line bookmarks in a popup: type a mnemonic to jump at
  once, or pick one and press Enter; Advanced Settings → Bookmarks can add the breakpoints. Agents
  have it all as `navigate.bookmarkList.*` and `navigate.bookmark.*` commands.
- **The TODO tool window** (View → Tool Windows → TODO) lists every TODO and FIXME comment of the
  project: only comments count, by each file's language, and excluded folders are left out. Tabs
  Project, Current File, Scope Based (Find in Files' scopes, `.idea/scopes` among them) and, in Git,
  Current Changelist; a tree of files with each item's line; a double click, ⏎ or F4 opens it.
  Previous and Next Occurrence (⌥⌘↑ / ⌥⌘↓) walk the items, with Expand All, Collapse All, Preview
  Source and Navigate with Single Click. The list follows your typing and changes on disk; agents
  use `todo.items.list` and the `view.todo.*` actions.
- **"more…" in Search Everywhere.** On All, a group that found more than it shows ends with
  **more…**; **Enter** on it (or a click) lists the next 50 of that group in place, and a tab of its
  own ends with it too past 50 results. **⌘↓** and **⌘↑** jump to the end and the start of the list.
  The letters that matched stay marked on every tab, `main.go:42` and `main.go:42:7` open the file
  at that line and column, and **Enter** on a folder found on Files selects it in the Project tool
  window. In Find Action, an action found by its English name while the interface is in another
  language shows that name, the letters that matched marked. Agents: `more` of
  `search.everywhere.query`.
- **`/` commands and a calculator in Search Everywhere.** As the field says, **/** lists the
  groups of Settings (`/appearance`, `/keymap`, `/editor`, `/plugins`, `/vcs`, `/build`,
  `/languages`, `/tools`, `/advanced`); **Enter** opens the one chosen. `/plugins` also lists the
  built-in and installed plug-ins, each with a switch that **Enter** or a click turns on or off
  while the popup stays open; words after it filter them (`/plugins docker`). An arithmetic
  expression on All (`2 + 3 * 4`, `(1.5 + 2) ^ 2`, `17 % 5`) shows its value as the first row,
  `2 + 3 * 4 = 14`, and **Enter** copies it; in Russian a decimal comma works too (`1,5 * 2`).
- **Shortcuts, abbreviations and synonyms in Find Action.** **⌥⏎** on an action in Find Action or
  on All opens the Keyboard Shortcut dialog over the popup and saves the keys pressed in the keymap
  in use, taking them from another action if you choose; any command Find Action lists can get a
  shortcut this way. Settings → Keymap → **Add Abbreviation** gives an action a short name of your
  own, which lists it first when typed, and actions are found by synonyms in English and Russian
  (`format` finds **Reformat Code**, `preferences` **Settings…**, `grep` **Find in Files…**).
  Agents: `search.everywhere.query` answers the commands, switches and the calculator's value,
  `ui.keymap.setShortcut` (or `set_keymap`) assigns a shortcut, and `ui.keymap.abbreviations`,
  `ui.keymap.addAbbreviation` and `ui.keymap.removeAbbreviation` keep the abbreviations.

- **"more…" in Search Everywhere.** On All, a group that found more than it shows ends with
  **more…**; **Enter** on it (or a click) lists the next 50 of that group in place, and a tab of its
  own ends with it too past 50 results. **⌘↓** and **⌘↑** jump to the end and the start of the list.
  The letters that matched stay marked on every tab, `main.go:42` and `main.go:42:7` open the file
  at that line and column, and **Enter** on a folder found on Files selects it in the Project tool
  window. In Find Action, an action found by its English name while the interface is in another
  language shows that name, the letters that matched marked. Agents: `more` of
  `search.everywhere.query`.
- **Go to Text.** **⌥⇧⌘E** (Navigate → Text…) opens Search Everywhere on **Text**: the lines of your
  files that hold what you typed, found as Find in Files finds them, the match marked and
  `file:line` on the right; **Enter** opens the file at the match. **Cc**, **W** and **.\*** sit in
  the field, and the toolbar has the search scope and a file mask (`*.go`, `!*_test.go`) whose
  list of recent masks is the one Find in Files shows. On **All**, text comes at the bottom when
  the other groups found fewer than six results (Settings → Advanced Settings turns it off), and
  Text is in All's **Filter**. Every key typed stops the last search. Agents: `run_action`
  `view.gotoText.open`, `matchCase`, `wholeWords`, `regex` and `mask` of
  `search.everywhere.query`, `search.findHistory.*` and the setting `searchTextOnAll`.

- **Search Everywhere fits its toolbar.** The popup widens to its tabs and toolbar, so the Russian
  "Include non-project items" check box shows its label instead of the box alone.
- **Call and type hierarchy from gopls, for agents first.** Agents ask who calls a function or
  method and what it calls, and which interfaces a type implements or which types implement an
  interface (`navigate.hierarchy.call`, `navigate.hierarchy.type`). Each node has its name, kind,
  package, file and place, and the lines of its calls; deeper levels are asked for only when a node
  is expanded (`navigate.hierarchy.expand`), and requests no longer wanted are cancelled. After
  edits, `navigate.hierarchy.refresh` builds it again with the same nodes expanded. The scope shows
  the project without tests, only the tests (`_test.go`) or everything with the libraries. A gopls
  older than v0.19, which has no type hierarchy, is reported instead of failing. The
  Hierarchy tool window comes next.
- **Hierarchy tool window: Call Hierarchy (⌃⌥H) and Type Hierarchy (⌃H).** Put the caret on a
  function, method or type and choose Navigate → Call Hierarchy or Type Hierarchy: the Hierarchy
  tool window opens on the right with a tree that expands level by level as you open it. Its
  toolbar switches between callers and callees (supertypes and subtypes for types), sorts
  alphabetically, limits the scope to the project, its tests or everything, refreshes, navigates
  with a single click, expands or collapses everything, pins the tab and closes it. A new hierarchy
  replaces the current tab unless it is pinned; pinned tabs stay and keep working, and a click on a
  tab's title switches to it. Double-click, ⏎ or F4 opens the code, and Back returns. Recursive
  calls are marked and never expanded, and nodes in test files are tinted like test files in the
  Project view. View → Tool Windows → Hierarchy shows the window again. Agents drive every part of
  it with the `navigate.hierarchy.*` commands, by tab.
- **Always Select Opened File is kept.** The Project tool window's option (⋮ → Behavior) is saved
  for each project and is on again when the project opens; while it is on, the header hides Select
  Opened File. Agents use `view.project.selectOpened` and
  `view.project.alwaysSelectOpened`.
- **The Switcher (⌃⇥).** Hold ⌃ and press ⇥ to switch between files and tool windows: a quick
  ⌃⇥ goes back to the previous file at once; held, the popup lists every tool window with its
  shortcut on the left and the open and recent files on the right. ⇥ / ⇧⇥ and the arrows move,
  ←/→ switch columns, releasing ⌃ opens the selection; ⌃⇧⇥ starts moving up. ⌦ (or ⌫, ⌘⌫)
  deletes a file from the list and closes its tab, saving its unsaved changes first.
- **Recent Files (⌘E) is the same popup**, with every tool window on the left (Commit, Project,
  Bookmarks, Find, Run, Debug, Problems, Structure, Services, Git, Terminal, Notifications) and the
  same keys. Agents use `view.switcher.open` and `navigate.recent.remove`.
- **Extend Selection and Shrink Selection (⌥↑ / ⌥↓).** Each press of ⌥↑ selects the word at the
  caret, then the next larger piece of code around it: a string's text, the string, the argument,
  the argument list, the call, the statement, the block, the declaration with its doc comment and
  the whole file; in comments and text by word, sentence and paragraph. ⌥↓ steps back. Every caret
  grows on its own, the language server's selection ranges join in, and both are in the Edit menu.
  Advanced Settings → Editor → Select whitespace characters with 'Extend Selection' selects the
  spaces at the caret first. An imported JetBrains keymap keeps its keys for them; agents use
  `editor.selection.extend` and `editor.selection.shrink`.
- **CamelHumps words.** Settings → Editor → General → Smart Keys → **Use "CamelHumps" words** (off
  by default) makes ⌥← and ⌥→ (with ⇧, selecting), ⌥⌫ and ⌥⌦ stop at the humps of a name: at `Bar`
  and `Baz` in `fooBarBaz`, at `Server` in `HTTPServer`, after `_` in `foo_bar`. Under it, **Honor
  "CamelHumps" words settings when selecting on double click** (on by default, available only with
  the first) makes a double click select just the hump. Settings search finds both; agents use the
  settings `camelHumps` and `camelHumpsOnDoubleClick` and the command `editor.selection.word`.
- **TODO and FIXME as you type in a comment.** Start typing `to` or `fix` in a comment, in any case,
  and the completion list offers **TODO** or **FIXME** (or ⌃Space after `// `); Enter puts the word
  in upper case in place of what you typed, Tab also replaces the rest of the word, and with several
  carets every caret that typed the start of it gets it. Nothing extra is offered in code or
  strings. The words come from the TODO patterns (TODO and FIXME by default); agents read them with
  `todo.patterns.list`.
- **Cyclic Expand Word.** **⌥/** finishes the word left of the caret from words already in your
  open files, with no popup and no language server, in code, comments, strings and plain text
  alike: the nearest word above that starts with what you typed, then, pressed again, the next one
  above, the words below and those of the other open files, each once, and after the last one what
  you typed. **⌥⇧/** looks below first. Anything else you do ends the round. Both are in Code →
  Code Completion, an imported JetBrains keymap brings its own keys for them, and agents run
  `editor.completion.expandWord`.
- **Closing a tab no longer loses unsaved changes.** ⌘W, the tab's ×, closing the last tab of a
  split, and closing a project's window save the file first, as Save All does (with Optimize
  Imports and Reformat Code on save when they are on, and a version in Local History). A file
  that cannot be saved (a read-only folder, a deleted folder) keeps its tab and its changes, and a
  notification says why; quitting the IDE then stays too.
- **The tab's context menu opens again**, with Close Tab, the new **Close Other Tabs**, **Close All
  Tabs** and **Close Unmodified Tabs** (also in Window → Editor Tabs), the split items and Local
  History in one menu.
- **Save All (⌘S) saves every file it can** when one cannot be saved, and the notification names
  that file; before, it stopped at the first.
- **Autosave.** GoGarden saves your modified files by itself once you have
  not typed, clicked or scrolled for 15 seconds, and when you switch to another application or to
  the Terminal tool window. An autosave is an ordinary save: the Actions on Save run (a formatter
  that fails gets one notification, not one per autosave), the line separators are kept, and each
  autosave keeps one version in Local History. A file changed on disk by another program is never written over (the
  editor offers Load File from Disk or Keep Memory Changes), and read-only files and files no
  longer on disk are left alone. Set it in **Settings → Appearance & Behavior → System Settings →
  Files** (Save files if the IDE is idle for [15] seconds; Save files when switching to a different
  application or a built-in terminal; both on), also reached from **Tools → Actions on Save →
  Configure autosave options…**. Agents use `set_settings` (`autosaveOnIdle`,
  `autosaveIdleSeconds`, `autosaveOnFrameDeactivation`) and the command `file.document.autosave`.

- **Saving a file opened through a symbolic link keeps the link.** The file the link points to is
  written; before, the save replaced the link with a copy and the original file kept the old text.
- **An interactive navigation bar.** The path in the status bar is now a navigation bar:
  `project › folders › file`, and with Show Members the declarations around the caret. A click on a
  segment opens a popup of what is inside it (folders and files, a file's declarations) with speed
  search: typing filters it, Enter or a click opens a file, goes into a folder or goes to a
  declaration. **Navigate → Jump to Navigation Bar** (⌘↑ or ⌥Home) gives it the keyboard: ← and →
  move between segments, ↓ opens the popup, Escape returns to the editor. **View → Appearance →
  Navigation Bar** puts it at the **Top** (a bar under the toolbar), **In Status Bar** (the default)
  or **Don't Show** (⌘↑ then shows it over the editor), and **Show Members** turns the declarations
  on and off; the choice is saved with the IDE's settings. Agents use `view.navigationBar.jump`,
  `navigate.navigationBar.get` and `navigate.navigationBar.children`.

- **⌘↑ is Jump to Navigation Bar**; Move Caret to Text Start is ⌘Home (fn⌘←).
- **Unwrap/Remove (⇧⌘⌦).** Code → Unwrap/Remove… lists the constructs around the caret,
  innermost first — `if`, `else`, `for`, `switch`/`select` cases, function literals, braces and calls
  with one argument — and takes the one you choose away: **Unwrap** keeps its body, **Remove** drops
  an `else` branch or a `case` with its body (those entries have a background of their own). The
  code that stays is highlighted while you choose; when only one entry applies it runs at once. The
  body is re-indented, the caret stays on its code and one ⌘Z undoes it. Imported JetBrains keymaps
  keep their own shortcut for it; agents use `editor.unwrap.list` and `editor.unwrap.apply`.
- **Surround With (⌥⌘T) and Surround with Live Template (⌥⌘J).** Code → Surround With… wraps the
  selection, or the statement at the caret (a whole `{ ... }` block with it), in a construct of the
  file's language: in Go `if`, `if / else`, `for`, `func() { … }()`, and for a selection `(expr)` and
  `!(expr)`; every file with comments also gets `<editor-fold…> Comments` and `region…endregion
  Comments`. The items are numbered: a digit or letter picks one, and beside the list the hovered or
  selected item shows the lines as they will read. The code is indented inside the construct, the
  caret lands in the condition (Tab goes on), and one ⌘Z undoes it all. Live templates can now hold
  `$SELECTION$`: they come at the end of the list, and ⌥⌘J (in Find Action and Settings → Keymap →
  Other) lists only them. An imported JetBrains keymap keeps its keys for both; agents use
  `editor.surround.list` and `editor.surround.apply`.
- **The editor keeps a file's own indentation.** When a file is already indented with tabs, or with
  2, 3, 4 or 8 spaces, **Tab**, **Enter** and **⇧Tab** indent it the same way instead of with your
  Code Style settings; comments, strings and continuation lines are not counted, and a file with
  fewer than three indented lines keeps the configured style. Reformat Code still uses the
  configured style. The indent in the status bar opens a menu that switches the file between its
  own and the configured indent, and opens the Code Style settings. Settings → Editor → Code Style →
  **Detect and use existing file indents for editing** (on by default) turns it off for every open
  file. AI agents: `editor.indent.get`, `editor.indent.useDetected`, `editor.indent.useConfigured`
  and `set_settings` with `detectIndents`.
- **Zoom IDE.** View → Appearance → **Zoom IDE (Current: 100%)…** scales the whole IDE at once, the
  editor included: − and + step through 70 % to 200 %, Reset goes back to 100 %. Settings →
  Appearance & Behavior → **Accessibility** → **Zoom** offers the same presets and any value from 50
  to 300 %, with **Reset to default** while the zoom is changed. The zoom works on top of the UI font
  size and is saved for the IDE. **Zoom In**, **Zoom Out** and **Reset** have no shortcut by default; a JetBrains keymap imported in Settings → Keymap brings its own (agents:
  `view.ideZoom.in`, `view.ideZoom.out`, `view.ideZoom.reset`, `view.ideZoom.open` with `run_action`,
  or `set_settings` with `ideZoom`).
- **Read-only files.** A lock at the right of the status bar shows whether the open file can be
  edited: open for a writable file, closed for one that is read-only on disk or cannot be edited
  for another reason (an encoding other than UTF-8, which the hint names). A click on it, or
  **File → File Properties → Make file read-only** (**Make file writable**), turns the file's write
  permission on disk off or on; unsaved changes are saved first, and the file gets back the write
  permission it had. Typing into a read-only file, replacing in it or renaming in it changes nothing,
  and typing shows a notification with **Make Writable**. Agents use
  `file.document.toggleReadOnly` and `file.document.readOnlyState`.
- **Line numbers your way.** Settings → Editor → Appearance → **Show line numbers** turns them off
  and chooses **Absolute**, **Relative** (the distance from the caret's line) or **Hybrid** (the
  caret's line by its number, the others by their distance). **View → Active Editor →
  Show Line Numbers** and the gutter's **Appearance** menu show or hide them for the file shown.
- **Method separators.** **Show method separators** draws a thin line above each top-level
  function, method and type, in the theme's colour (imported from JetBrains schemes too).
- **Whitespaces, indent guides and the right margin.** Thin lines mark each indent
  level, the block the caret is in brighter; a vertical line marks the right margin (the project's
  `.idea` code style, 120 by default) and the code style's visual guides. Settings → Editor →
  Appearance has **Show hard wrap and visual guides** and **Show indent guides** (on) and **Show
  whitespaces** with Leading, Inner, Trailing and Selection (off): spaces then show as dots and tabs
  as lines, or as arrows with Advanced Settings → Editor → **Tab character rendering**. **View →
  Active Editor → Show Whitespaces** and **Show Indent Guides**, and the gutter's menu **Appearance →
  Show Indent Guides**, change the file shown. Themes have four new colours (`whitespace`,
  `indentGuide`, `indentGuide.selected`, `rightMargin`), also read from JetBrains and VS Code
  themes. Agents: `editor.whitespaces.toggle`, `editor.indentGuides.toggle`,
  `editor.appearance.get`, and `set_settings` with `editorAppearance`.
- **Line separators.** The status bar shows the open file's real line
  separator (LF, CRLF or CR; a file that mixes them shows its first one) instead of always "LF".
  Click it, or use **File → File Properties → Line Separators**, to rewrite the file with CRLF, LF
  or CR: one **⌘Z** brings the old ones back, the file is saved at once and Local History keeps the
  text before and after; from the Project tool window it changes the selected files, and a folder
  changes all its text files. Enter, paste, completion and formatting now write new lines with the
  file's separator, so a Windows file stays CRLF when you save it.
- **Settings → Editor → Code Style → General → Line separator** chooses the separator of new files
  (System-Dependent by default); an `end_of_line` in the project's `.editorconfig` wins. Agents use
  `file.lineSeparator.get`, `file.lineSeparator.set` and the `lineSeparator` setting.
- **EditorConfig sets the code style.** The project's `.editorconfig` files now set the
  indentation (`indent_style`, `indent_size`, including `tab`), the tab width (`tab_width`) and the
  right margin (`max_line_length`, `off` for none) of the files they match, above the project's and
  your own Code Style: the nearest file wins up to the project's root or
  `root = true`, sections match by EditorConfig globs, `unset` hands a property back, and
  `ij_continuation_indent_size` and `ij_visual_guides` are read in their `ij_go_`, `ij_any_` and
  general forms. **Tab**, **Enter**, **⇧Tab**, the width of a tab, the status bar's indent and
  Reformat Code follow it, and editing or adding an `.editorconfig` applies to open files at once.
  **Settings → Editor → Code Style → General → Enable EditorConfig support** (on by default) turns it
  off. Agents read what applies to a file with `project.editorConfig.get`, the lines that cannot be
  read in `project.settings.get`'s problems, and switch it with the `editorConfig` setting.
- **EditorConfig on save, on open and in the status bar.** An `end_of_line` in the project's
  `.editorconfig` now also rewrites a modified file's line breaks when you save it (one **⌘Z** takes
  it back), and `charset` (`utf-8`, `utf-8-bom`, `latin1`, `utf-16be`, `utf-16le`) is the encoding
  the file is opened and saved in; a character the encoding cannot hold refuses the save with its
  line and column instead of being lost. `trim_trailing_whitespace` and `insert_final_newline` win
  over Editor → General → On Save for the files they match. When an `.editorconfig` sets the open
  file's style, the status bar's indent shows the EditorConfig mark, with **Open .editorconfig**,
  **Show Files Related to Project…** and **Disable EditorConfig**. `.editorconfig` files are
  highlighted and complete their properties and values. **File → New → EditorConfig File** (also
  in the Project tool window's **New**) creates one with the stubs you choose, and **Code Style →
  Show Scheme Actions → Export → EditorConfig File** writes your code style as one. Agents use
  `project.editorConfig.open`, `.list`, `.create`, `.export` and the `save` of
  `project.editorConfig.get`.
- **Font size from the keyboard and the mouse wheel.** View → **Increase / Decrease Font Size in
  All Editors** (⌃⇧. / ⌃⇧,) and **Reset Font Size in All Editors** change the editor's font size
  everywhere and keep it in Settings → Editor → Font; View → Active Editor → **Increase / Decrease /
  Reset Font Size** change only the file in front until it is closed. Settings → Editor → General →
  Mouse Control → **Change font size with Command+Mouse Wheel in:** Active editor / All editors
  (off by default) lets ⌘ and the wheel do it. A label at the bottom of the editor shows the new
  size and the default with **Reset** (Advanced Settings → Editor → **Show zoom indicator**). Keymaps
  imported from JetBrains IDEs keep these shortcuts; agents use `editor.fontSize.*` and
  `view.editorFontSize.*`.
- **The whole Code → Folding menu.** Expand Recursively (⌥⌘= or ⌥⌘+) and Collapse
  Recursively (⌥⌘−) act on the region at the caret and every region inside it; Expand to Level and
  Expand All to Level unfold the region at the caret, or the whole file, down to levels 1 to 5 and
  fold what is deeper, from submenus or with two keystrokes: the numeric keypad's `*` with ⌘ (or
  ⌥⌘), then the digit. Expand Doc Comments and Collapse Doc Comments act on every doc comment of
  the file, Toggle Folding unfolds or folds at the caret, and Fold Code Block (⇧⌘.) folds the
  block around the caret. With a selection, Collapse All and Expand All act only inside it. Agents
  use `editor.fold.expandRecursively`, `editor.fold.collapseRecursively`,
  `editor.fold.expandToLevel`, `editor.fold.expandAllToLevel`, `editor.fold.expandDocComments`,
  `editor.fold.collapseDocComments`, `editor.fold.toggleAtCaret` and `editor.fold.collapseBlock`.
- **Shortcuts of two keystrokes.** A keymap can bind an action to two keystrokes in a row
  (`⌘NumPad*, 1`), and a JetBrains keymap's two-keystroke shortcuts and its numeric keypad `*`,
  `+` and `−` are now imported instead of left out. The editor waits two seconds for the second
  keystroke; any other key does what it always does, so nothing typed is lost.
- **Fold Selection and custom folding regions.** Fold Selection / Remove Region (⌘.,
  Code → Folding) folds the selected text, part of a line too, and removes such a region again from
  its line. Comments `// region Name` … `// endregion` and `//<editor-fold desc="Name">` …
  `//</editor-fold>`, in the comment syntax of any language (`#` in Python and YAML, `--` in SQL,
  `<!-- -->` in HTML), fold whole and show their name; Custom Folding… (⌥⌘., Navigate) lists them
  and goes to the one you choose. Selected regions are remembered with the other folds of the file
  when it closes and at the next start. Agents use `editor.fold.selection`, `editor.fold.regions`
  and `view.customFolding.open`.
- **What saving does to whitespace.** Settings → Editor → General → On Save:
  **Remove trailing spaces on: Modified lines** (on by default; or **All lines**), **Keep
  trailing spaces on caret line** (on), **Remove trailing blank lines at the end of saved files**
  (off) and **Ensure every saved file ends with a line break** (off). Modified lines are the ones
  changed since the last save; Markdown hard line breaks and spaces inside multi-line strings stay;
  CRLF files stay CRLF; one ⌘Z undoes it; read-only files are never touched. A project's
  `.editorconfig` `trim_trailing_whitespace` and `insert_final_newline` win for the files they
  match. Agents read what applies with `editor.onSave.options` and change it with `set_settings`.
- **Settings → Editor → File Types.** One list of file types decides how every file is highlighted
  and indented, which language server opens it and which icon it gets. The tab Recognized File
  Types shows each type (Go, Go Module, Rust, the plug-ins' types, Text) with its File name
  patterns and HashBang patterns, which you can add, remove and edit with + − ✎; a pattern another
  type has moves, and the page says so first. Give `*.gotmpl` to Go and those files are Go files
  at once, open ones included, without opening them again; a script without an extension whose
  first line is `#!/usr/bin/env bash` is highlighted as a shell script. The tab Ignored Files and
  Folders lists the names the IDE does not see (`.git`, `.DS_Store`, `__pycache__`,
  `*.pyc`, ...): the project tree, Go to File, Search Everywhere and Find in Files leave them out,
  and a change indexes the project again in the background; `.git`, `.hg` and `.svn` are never
  indexed or watched, even when you show them. The patterns are kept for the whole IDE; agents use
  the `editor.fileTypes.*` commands.
- **Caffeine ends on time.** Choosing a time while Caffeine was already on could keep the computer
  awake up to half a minute longer while the IDE sat idle in the background; now it ends at the
  chosen time.
- **A broken go.mod or go.work is in Problems.** A `go.mod` or `go.work` the IDE cannot read for
  the project's modules (an unknown directive, a `use` folder without a `go.mod`) was only told to
  agents; now the Problems tool window lists it on Project Errors, open or not, the editor
  underlines its line (the mark moves with the lines while you edit), and `problems.file.list`
  and `problems.project.list` give it, until the file is fixed. When gopls already reports an
  error on that line, it is listed once.
- **gopls hears of changed go.mod and go.work files.** A `go.mod`, `go.sum`, `go.work` or Go file
  changed on disk (saved, or changed by git or another program) is now told to gopls, so its
  problems follow: fixing a `go.work` no longer leaves its old error in Problems until gopls is
  restarted.
- **File templates.** New File from Template… (in Find Action) creates a file from a template:
  a text with variables in a subset of Velocity (`${NAME}`, `#set`, `#if`/`#else`/`#end`,
  `#parse` of an include, `${DS}`), with predefined variables (`NAME`, `FILE_NAME`,
  `DIR_PATH`, `PROJECT_NAME`, `PRODUCT_NAME`, `USER`, `DATE`, `TIME` and the parts of the date and
  time). Every other variable, also one in the template's file name, is asked for in the dialog;
  what a template has that cannot be used is shown with its line and column. **File → Save File as
  Template…** makes a template of the file in front, with Reformat according to style and Enable
  Live Templates. Templates are kept for the IDE, or for the project in `.idea/fileTemplates` as
  JetBrains IDEs keep them; agents use the `file.templates.*` commands.
- **Spelling.** Misspelt words in comments, string literals and the names you declare get a green
  wavy line in the theme's new Typo colour: names are split at capitals, `_` and digits, so in
  `parseUrlHadnler` only `Hadnler` is marked, and a library's names you only use are left alone.
  Short words, words in capitals, URLs, e-mail addresses, file paths, dotted names such as
  `fmt.Sprintf` and hexadecimal numbers are skipped, and American and British spellings are both
  correct. Typos are never errors: F2 and ⇧F2 step through them, the editor's counter and the
  Problems tool window's File tab show them as typos, and Project Errors leaves them out. The English
  dictionary is bundled and works offline; the check runs in the background, so typing in a large
  file is never slowed down. **Settings → Editor → Inspections → Proofreading → Spelling** turns it
  off or chooses Process code, Process literals and Process comments; AI agents read typos with
  `spelling.typos.list`.
- **Create Patch and Apply Patch.** Local changes, a commit (the Git workspace's commit menu) or
  shelved changes become a patch in Git's format, saved to a file or copied to the clipboard, with a
  base path, reversed and in another encoding; `git apply` and `git am` take it, author and message
  included. Git → Patch → Apply Patch…, Apply Patch from Clipboard…, or a `.patch` dropped onto the
  editor open the Apply Patch dialog: each file with how it applies and its diff (⌘D, conflicts in
  red), Map base directory and Strip leading directories, the changelist to put the changes in, and
  Import to shelf. Hunks that do not apply leave their file to merge in the conflict editor, and a
  patch made from a commit fills the commit message and author in. Paths in a patch never reach
  outside the project or into `.git`, not even through a symbolic link; hunks that apply only where
  `git apply` would refuse them, and a file made executable, are pointed out; the files keep their
  text before the patch in Local History. Agents use `vcs.patch.*` and `vcs.shelf.import`.
- **Find Usages opens a tab of the Find tool window like Find in Files'.** ⌥F7 lists "Usages of
  <name>" with Targets ("Variable Version") and "Usages in Project Files", and the same **Group By**
  as text search (Usage Type, Module, Directory Structure, File Structure, Production/Test…, and
  **Merge Usages from the Same Line** ⌃L), **Preview Source**, **Rerun** ⌘R, **Pin Tab**, **Open
  results in new tab**, previous/next usage and exclusion. The language server's read and write
  access puts usages under **Value read** and **Value write**, and **Show Read Access** / **Show
  Write Access** filter them; usages it does not classify stay listed. The tab opens at once and
  the server is asked about the files in the background.
- **Find Usages Settings (⌥⇧⌘F7)** opens the Find Usages dialog: **Scope** (Project Files, All
  Places, Open Files, Current File), **Open in new tab** and **Skip results tab with one usage**,
  kept for the window. From the Show Usages popup it shows the popup again in the chosen scope.
- **Edit → Find Usages → Recent Find Usages** lists the earlier searches and runs one again; it and
  **Rerun** find the symbol on its line after edits, not a namesake nearby.

- **Edit → Find Usages** is a submenu of its own (Find Usages, Settings, Show Usages, Find Usages
  in File, Highlight Usages in File, next/previous highlighted usage, Recent Find Usages).
- Show Usages' **Open in Find Tool Window** and the search bar's **Open Results in Find Window**
  open such a tab too, in the popup's scope.
- **For AI agents:** `navigate.usages.find` and `navigate.usages.list` tell the tab (`tab`), read
  with `search.results.tree`; `navigate.usages.settings` opens the dialog
  (`navigate.usagesSettings.*`); `navigate.usages.recent` and `navigate.usages.openRecent` give the
  recent searches.

- **The Find tool window lists Find in Files' results.** **Open in Find Window** now
  searches in the background ("Searching for '…' in <file>…" in the status bar, with **×** to stop)
  and asks **Too Many Results** — Abort or Continue — when it passes the limit. The tab
  "Occurrences of '…'" shows Targets, then "Found occurrences in Project" with the usages grouped
  by usage type (code, comments, string constants), module, folders, file and line.
  - **Group By**: Directory Structure ⌃D, File Structure ⌃S, Module ⌃M, Flatten Modules ⌃O, Group
    by Directory ⌃P, Production/Test, Usage Type ⌃T, Merge Usages from the Same Line ⌃L.
  - The toolbar: Rerun ⌘R, Previous/Next Occurrence, Settings ⇧⌘F (back to Find in Files with the
    same search), Show Usages In Comments, Expand/Collapse All, Preview Source, Pin Tab and Open
    results in new tab.
  - **⌫** excludes a node (struck through) and takes it back; Replace in Files' tab has **Replace
    Selected** and **Do Replace All**, which skip excluded nodes.

- **Replacing in files is safer.** Replace in Files and the Find window's replacements change
  nothing when a file is read-only, not in UTF-8 or changed since the search, and say which file;
  each open file changes in one undo step, the others get a Local History version, and CRLF line
  breaks stay. A file that cannot be written midway puts back the files written before it.
- **For AI agents:** `search.text.occurrences` and `view.findInFiles.openInFindWindow` answer at
  once with the new tab; pass `waitMs` to get the final count, as before.
- **Settings → Editor → General → Code Folding.** Show code folding arrows chooses
  On mouse hover (as before) or Always, or hides the arrows; Show bottom arrows adds an arrow on the
  last line of each open region, which folds it. **Fold by default** chooses what starts folded when
  a file opens for the first time: Imports (on, as before), File header, Documentation comments,
  Method bodies and Custom folding regions, and for Go One-line 'if' blocks for error handling
  (`if err != nil { return err }`), One-line functions with a single 'return', One-line case
  clauses, Empty functions, Empty struct or interface type definitions, and Formatted strings
  (`fmt.Printf("{name}: {done} of {total}\n")`). A file you opened before keeps the folds you left.
  The settings search finds every option. Agents read the options with `editor.folding.options` and
  change them with `set_settings`.

- Fold imports when a file opens moved from the Editor page to Code Folding as Fold by default →
  Imports; the setting is kept.
- **Associate with File Type and Override File Type.** Opening a file GoGarden does not know and
  that is not text (a dump, an archive) asks how to open it: a pattern (`*.ext` or the
  file's name), then as text with the type detected by content, as a file type chosen in a list, or
  in the application macOS opens it with. The same dialog is in File → File Properties → Associate
  with File Type… and in the Project tool window's menu; the choice is kept for the whole IDE and
  shows in Settings → Editor → File Types under "File type auto-detected by file content" and "Files
  opened in associated applications". A text file of no type still opens as text. **Override File
  Type** in the Project tool window's menu gives only the selected files another type (Text: no
  highlighting, no language server), and **Revert File Type Override** undoes it; overrides are kept
  for the whole IDE by the file's path and follow the file when you rename or move it in the IDE. A
  UTF-16 text file opens read-only as text instead of asking. Agents use
  `editor.fileTypes.associate`, `.override`, `.revertOverride` and `.overrides`.
- **`gogarden` opens in the GoGarden that runs.** With GoGarden already running, `gogarden .`,
  `gogarden main.go:12:5` or `gogarden` alone from a terminal (or Zen Toolbox's shell script) no
  longer starts a second GoGarden: the folder or file opens in the running one, in the window of the
  project that holds the file or in a new one for a new project, at that line and column, and the
  command returns at once. A file inside an open project opens in that project, also at a start.
  Only you reach your running GoGarden this way: the command finds it through a socket in its data
  folder that other users cannot open. A headless GoGarden that agents use never takes these, unless
  started with `--accept-open`.
- **Compare files, the clipboard and blank texts.** In the Project tool window, **⌘D** with two
  files selected compares them at once, and with three opens a three-side viewer (the middle file
  compared with the other two); with one file it still asks for the other. **View → Compare
  With…** and **View → Compare with Clipboard** are in the View menu: the clipboard is compared with
  the file in the editor, or with its selection, and the file's side can be edited there and saved
  with ⌘S. **Open Blank Diff Window** (Find Action) opens two empty panes to paste, type or drop
  files into.
- **The diff tab ignores whitespace and swaps sides.** Next to ↑ ↓ (F7, ⇧F7) and the side-by-side
  or unified viewer, the toolbar chooses **Do not ignore**, **Trim whitespaces** or **Ignore
  whitespaces**, and **Swap Sides** puts the left side on the right. AI agents do all of this with
  `file.compare.*` and `view.diff.*`.

- **⇧F7 in a diff** goes to the previous difference again instead of the next one.
- **Scratch files and buffers.** File → New → Scratch File (⇧⌘N) lists the languages, the ones you
  used last first: type to filter and press Enter to get `scratch.go`, `scratch_1.go`, ... in the
  IDE's own folder, shared by every project and never in version control. With text selected, ⌥⏎
  offers Create new scratch file from selection, in that file's language. New Scratch Buffer (in
  Find Action) opens `buffer1.txt` to `buffer5.txt` in turn and asks before it clears a reused one;
  Show Scratch Files lists them all. Scratch files are highlighted and completed like any file of
  their language, and a Go scratch gets gopls even outside any module. The Project tool window
  shows them under Scratches and Consoles, with Change Language, Rename, Delete and Move to the
  project (F6); Advanced Settings → Project View turns the node off. Closing an empty scratch
  deletes it.
- **Bookmarks of files, folders and open tabs.** In the Project tool window, **F3** bookmarks the
  selected files and folders (several at once) and **⌥F3** gives one a digit or letter; the
  context menu's **Bookmarks** has Toggle Bookmark, Toggle Bookmark Mnemonic… and Delete, and a
  bookmarked file or folder shows the bookmark icon after its name. Right-click an editor tab for
  the same actions on its file, and for **Bookmark Open Tabs…**, which puts every open tab into a
  new named list. They are rows of their lists in the Bookmarks tool window: opening one opens the
  file, or selects the folder in the Project tool window. Agents use
  `navigate.bookmark.toggleFiles`, `navigate.bookmark.removeFiles`,
  `navigate.bookmarkList.fromOpenTabs` and `whole: true` in the other bookmark commands.

- **Local History from an editor tab.** Right-click a tab → **Local History** → Show History or
  Put Label now works: the submenu was a second menu on the same tab and could not be opened.
- **Selecting many files in the Project tool window no longer freezes the IDE.** With hundreds of
  rows selected (⌘A in a large folder) every frame took seconds; it now stays responsive.
- **Mark Directory as → Excluded.** Right-click a folder in the Project tool window, choose
  Mark Directory as → Excluded, and Go to File, Search Everywhere and Find in Files stop finding its
  files at once; on an excluded folder the same menu has Cancel Exclusion. The exclusion is written
  into the project's `.iml` file, where a JetBrains IDE keeps it, so both IDEs see the same
  folders; a project without one gets `.idea/modules.xml` and a module file. Project ⋮ → Excluded
  Files (on by default) hides excluded folders from the tree, remembered per project. A Go module
  with vendoring now finds its files in `vendor` in Go to File, Search Everywhere and Find in
  Files: `vendor`, `build` and `dist` are no longer excluded by their names alone.
  What Git ignores (a build's output, such as a Cargo project's `target`) stays out of the index
  and Local History while a build writes it, as it stays out when the project opens.

### Fixed

- Context actions (⌥⏎), the quick-fix list, completion, parameter info, quick documentation, the
  chooser of declarations and Show Usages open next to the caret after it moved off the screen,
  as when an AI agent moves it and opens one at once; they appeared where the caret had been. Near
  the bottom of the window they open above the caret, and a file opened with the caret far down
  scrolls to it.
- **Typing right after switching popups goes into the new one.** A letter typed at once after
  closing one popup and opening another (**⎋** in Search Everywhere, then **⇧⌘F**) went into the
  file at the caret, or was lost; now it goes into the field of the popup just opened, in Find in
  Files and in every popup that filters as you type, keys such as **⌫**, **←** and **⏎** in the
  order they were typed. A dialog that opens (Settings, Confirm Exit) takes the keyboard from the
  file under it, so typing no longer edits the file behind the dialog.
- **Find in Files opens with the last query selected.** Opened again without a selection, Find in
  Files and Replace in Files show the last query selected, so typing replaces it instead of adding
  to it.

## 0.38.33 - 2026-10-22

### Added

- **Maximum Caffeine.** A button next to Turn On and Turn Off in the Caffeine tool window, with the
  red-steaming cup, keeps everything awake (every "keep awake" option) until turned off, in one
  click; it stays pressed while Caffeine is at maximum, and a second click or Turn Off ends it. The
  settings and the battery rules are left as you set them. Agents use `caffeine.awake.maximum`, or
  `maximum: true` of `caffeine.awake.start`; the status says `maximum`.

### Fixed

- **The Caffeine tool window no longer jumps.** The long sentence under the coffee drinker wrapped
  differently with each state and the time left came and went, so every control under it moved.
  It is now two short lines ("Keeping awake: everything · until you turn it off") with the whole
  story in their hint, and the time left, the state and the summary keep a fixed height in every
  state, frame and language.

## 0.38.32 - 2026-10-22

### Changed

- **Caffeine is a tool window with a coffee drinker.** Caffeine moved from the status bar to the
  right stripe, right under Git: a tiny pixel coffee cup shows whether the computer is kept awake
  (cold with a z when off, steaming when on, steaming red at maximum), and opens the Caffeine tool
  window with every control (on and off, for how long, while what, what to keep awake, the battery
  rules, the settings). A pixel-art coffee drinker of our own shows the state: asleep with zZ when
  off, waiting with the cup down when paused, sipping steadily (`-i`), eyes wide and sipping more
  (`-d`), and with every option on bloodshot red eyes, jitters and gulping very often; a timed run
  shows the time left. He moves only while the tool window shows, holds still with Reduce motion,
  and agents open the tool window with `view.toolWindow.caffeine` (the status bar widget's
  `view.caffeine.menu` is gone).
- **AI agents edit through the IDE.** Besides replacing exact text, `editor.text.replace` replaces
  by a regular expression (`regex`, with line starts and ends and groups); `editor.text.write` gives
  a file its whole text and creates a new one; `editor.text.batch` changes several files in one
  call, all of them or none. Agents are told to edit this way rather than with scripts, so every
  change is one undo step and kept in Local History; creating, renaming and deleting files take
  paths relative to the project. Measured on the IDE's own repository: 38 calls became 21.
- **Places by line and column.** Find Usages, Show Usages, Go to Declaration and Rename take a
  one-based `line` and `column`, as every answer gives them, instead of only a character offset.
- **Shorter symbol answers.** Go to Symbol and Go to Class name each file relative to the project,
  and name a missing language server beside what they found once per session instead of with every
  search: one symbol of the IDE's own repository went from 605 to 242 bytes.

### Fixed

- A language server's own problems for an edit (those the IDE asks for, as rust-analyzer's) came
  no more once the server had cancelled a request while reloading files, and a file's problems said
  "checked, no problems". A cancelled request is asked again a moment later, and the file counts as
  being checked until the answer comes.
- A symbol search that waited while the language server loaded the project no longer answers
  "searching" at once when the loading ends: the query the server is asked again gets its own wait,
  so the first Go to Symbol in a large project takes one call instead of several.

## 0.38.31 - 2026-10-22

### Changed

- **AI agents read whole sentences.** `list_commands` with a group or a query gives each command's
  first sentence whole; it was cut at "e.g." or after 120 characters.
- **Clean or not checked yet.** A file's problems (`problems.file.list`) tell a clean file
  (`checked: true`) from one the language server has not checked yet or is checking again after a
  change (`checked: false` with the reason), instead of an empty list for both.
- **Usages name each file once.** Find Usages and Show Usages answers group the usages under their
  file, relative to the project: 1 436 usages of one function in this repository went from 530 KB
  to 294 KB.
- **Short run targets, counted from 1.** What a file can run is listed as line, kind and name,
  without each test's whole configuration (`full: true` adds it): a file of 600 tests went from
  360 KB to 83 KB. Its lines, and the line `run.context.get` takes, are counted from 1 like every
  other line agents read.
- **Waiting for a large project.** Go to Symbol and Go to Class wait up to two minutes when asked
  while the language server is still loading the project, instead of answering "pending" after
  10 seconds; the IDE's windows go on meanwhile.
- **The main commands are tools of their own.** Replace text, Find in Files, Go to Symbol, Find
  Usages, a file's problems, rename, run and test results are also MCP tools
  (`editor_text_replace`, `search_text_find`, `navigate_symbol_goTo`, `navigate_usages_find`,
  `problems_file_list`, `refactor_rename_apply`, `run_process_start`, `run_tests_get`), so agents
  see their parameters without looking them up.

### Fixed

- A symbol or class search no longer answers "no match" when a language of the project has no
  language server while others run: it names the missing server beside what the others found, and
  the project's problems say so too.
- A Rust test, benchmark, example or binary file added while the IDE runs gets its run icons at
  once, as its own target, instead of none or another file's.

## 0.38.30 - 2026-10-22

### Changed

- **AI agents run tests without polling.** `run.process.start` with `waitMs` answers once the run
  has ended, with its state and test counts, and `run.tests.get` answers only what is asked:
  `summary` (a few hundred bytes), `failedOnly` (each failed test with the file and line where it
  failed and its message) or one `test`'s output. Measured on the IDE's own 605 tests, an answer an
  agent polled was 365 KB; the summary is 279 bytes and the failures under 1 KB.
- **One way to count lines for agents.** Find in Files answers count lines and columns from 1, as
  navigation, problems and file reads already did, and name each match's file once, relative to the
  project (absolute outside it), which makes a search answer about 70 % smaller. Numbered file reads
  are `12|text` lines. Agents that read `relative` or counted search lines from 0 must adapt.
### Fixed

- **A missing language server says so.** When the language server a file needs is not installed
  or does not run, the Problems tool window says so instead of "No problems", and AI agents get
  why instead of an empty answer: problems, usages, symbols, the file structure and rename answer
  `serverMissing`, `serverNotRunning` or `starting` with the server's name and the command that
  installs it; `language.servers.status` lists the missing servers too, and a headless GoGarden
  writes at its start which servers it found and which are missing.
- **Run targets for agents.** `run.targets.list` and `run.context.get` take a path relative to the
  project, and `run.context.get` takes a `line` (as `run.targets.list` gives it) instead of an
  offset; before, both were ignored or failed.

## 0.38.29 - 2026-10-21

### Changed

- **AI agents get English.** What agents get over MCP (errors, the reasons for empty answers,
  messages) is in English whatever the language of the system and the interface; Settings → Tools →
  MCP Server → **Language for agents** chooses another (agents: `set_settings` with `mcpLanguage`).
  Error codes stay the same in every language, and the interface keeps its own language.
- **MCP answers are half the size.** Every answer to an AI agent used to carry its result twice,
  as text and again as structured content; it now carries it once, as compact JSON text, which is
  what agents such as Claude Code read (typical answers are about 45 % smaller).

### Fixed

- **A very short agent session no longer leaves a headless IDE running for a minute.** A session
  through `mcp --headless` that ended within a fraction of a second (initialize, the tool list and
  done) could come and go between two of the IDE's looks at its clients, and the IDE then waited
  60 seconds for a first client. Now any session that ever connected counts, so the IDE ends about
  a second after the last one leaves, however short it was.
- **Unknown command-line options are refused.** `gogarden --bogus-flag` prints the usage and exits
  with code 2 instead of opening a window on a project named `--bogus-flag`; `--` before a path
  opens a folder whose name starts with `-`.
- **`zen --version` prints the real version.** The `zen-cli` inside the IDE printed 0.0.0; it now
  prints the version of the IDE it ships with.

## 0.38.28 - 2026-10-21

### Added

- **Language switch on the Welcome screen.** A globe with the current language's own name
  (**English ▾** / **Русский ▾**) sits in the top-right corner of the welcome window, right of New
  Project, Open and Clone Repository. Choosing a language applies it at once to the welcome screen
  and every open window, without a restart, and saves it as Settings → Appearance & Behavior →
  Language (agents: `set_settings` with `language`). The welcome window is a little wider, so the
  Russian buttons and the switch fit in one row; in a narrow window the search shrinks first and the
  buttons move under it instead of overlapping.

## 0.38.27 - 2026-10-21

### Added

- **Caffeine keeps the computer awake from the IDE.** A little coffee drinker in the status bar
  dozes while it is off; a click turns it on and he sips steaming coffee. Right-click (or ▾) for
  the menu: until turned off, 15 min to 4 h or a custom time (the time left shows next to him), or
  only while a run or test, a debug session, a background task, indexing or an agent over MCP is
  at work. Choose what to keep awake (the system, the display, the disk, the system on mains power,
  "user is active") and battery rules (pause on battery or below a level, go on when charging).
  It runs macOS's own `caffeinate` tied to the IDE, so nothing stays awake after the IDE ends.
  Settings → Tools → Caffeine holds every parameter and can turn it on when the IDE starts; agents
  use `caffeine.status.get`, `caffeine.awake.start` and `caffeine.awake.stop`.

## 0.38.26 - 2026-10-21

### Added

- **Modules in the Project tool window.** The folders of the modules a `go.work` uses are drawn in
  bold, with the module's path after the folder's name when they differ.
- **Agents can select a file of External Libraries** in the Project tool window (`select_in_tree`
  with a file of a module in the module cache).
- **The status bar says what a language server is busy with**, for example "gopls: Loading
  packages", and agents see it in `lsp.servers.list` (`busy`, `progress`, `readyAfterMs`), so they
  know when its answers are complete.
- **Code Vision counts usages again once gopls has loaded the project**, instead of keeping the
  "no usages" it counted before.
- **Go and Rust side by side.** With the Rust plug-in switched on (Settings → Plugins → Built-in),
  gopls keeps using the Go SDK and rust-analyzer the Rust toolchain; a change of one restarts only
  its own server.

## 0.38.25 - 2026-10-21

### Added

- **Agents edit in one call.** `editor.text.replace` replaces exact text in a file as the editor
  has it, unsaved changes included, as one undo step, and saves it with the on-save actions when
  asked; several edits of a file go in together or not at all. The answer is a few bytes (whether it
  changed and the line); text that is missing or found more than once is an error with the count.
  A small edit now takes one call and 40 bytes instead of five calls and 17 KB.
- **Agents read part of a file.** `get_document_text` takes a range of lines (`startLine`/`endLine`
  or `offset`/`limit`) and `withLineNumbers`: 40 lines of an 800-line file are 1 KB instead of the
  whole 16 KB. A file over 32 KB is read only by ranges.
- **The command list on demand.** `list_commands` answers the command ids by group (8 KB instead of
  364 KB); `group` or `query` gives those commands with a one-line description, and
  `describe_command` one command's parameters. The old full answer is `list_commands {full: true}`.
  Agents are told the main commands and the groups when they connect.

### Changed

- **No empty answers without a reason for agents.** Go to Symbol, Go to Class, Find Usages and
  Find in Files now say why they have nothing (`noMatch`), or why more is coming: the project is
  being listed, the language server is starting, searching, or still loading the project (with its
  progress, for any language server). An answer the language server gave while loading is asked
  for again once it is done.
- **Paths relative to the project.** Agents may give files and Find in Files folders relative to
  the project or as full paths.

### Fixed

- **Go to Symbol right after opening a project answered "nothing, done"** while the project's
  files were still being listed and its language server had not started; it now answers that it is
  still waiting.
- **Find in Files with a full folder path found nothing.** An absolute folder of the project is
  searched; a folder outside the project or missing from it is an error that says so.

## 0.38.24 - 2026-10-21

### Added

- **GoGarden for agents in one line.** `claude mcp add -s project gogarden -- gogarden mcp
  --headless .` gives every Claude Code session of the project a GoGarden without a window: it
  starts with the session (or joins the one already running for the project), several sessions
  share it, and it ends by itself when the last session ends. Your GoGarden window is never used.
  **Settings → Tools → MCP Server** shows the command with a Copy button.
- **Tools → Create Command-line Launcher…** writes the `gogarden` script that starts GoGarden
  from a terminal, with the name and folder you choose (`~/.local/bin` by default); when that folder
  is not on `PATH`, the dialog shows the line that puts it there, and adds it to `~/.zprofile` only
  if you tick the box.
- **zen-cli comes with GoGarden** (`Contents/MacOS/zen`); Zen Toolbox puts it on your terminal as
  `zen`.
- **`gogarden main.go:12`** (and `main.go:12:5`) opens the file at that line and column.
- **Settings → Tools → MCP Server** shows `claude mcp add` commands with the short name,
  `gogarden mcp`, once a terminal finds the `gogarden` script; with the full path otherwise.

### Fixed

- **`gogarden --version` prints the version.** It opened another GoGarden window instead;
  `--version`, `-V`, `--help` and `-h` now print and exit without starting anything, in the
  language chosen in GoGarden's settings, or the system's.

## 0.38.23 - 2026-10-21

### Added

- **GoGarden without a window, for AI agents.** `gogarden --headless <project>` opens the project
  with gopls, Git, run configurations, tests and every command, but draws nothing: no window, no
  Dock icon, no GPU. Its MCP server is always on and prints its address; an agent keeps it open for
  a whole session and asks it for problems after an edit, usages, rename, test results and Git
  state. `exit_ide` ends it (`--exit-with-parent` also when the program that started it ends), a
  second start on the same project is refused with the running one's address, and what needs a
  window answers that it is not available. `zen-cli ide start`, `list`, `stop` and `call` manage
  them.

### Fixed

- **A covered window or a sleeping display no longer holds agents up.** gopls's problems, Git's
  status, new runs and usages now keep up while GoGarden's window is covered, minimised or the
  display sleeps, instead of waiting until the window is drawn again.
- **Go follows `brew upgrade go` without a restart.** GoGarden kept the versioned Homebrew folder
  it found at start (`/opt/homebrew/Cellar/go/1.27.2/libexec`), so after an upgrade **Settings → Go**
  still showed the old Go in a folder brew had removed. A Homebrew Go is now kept as
  `/opt/homebrew/opt/go/libexec`, which follows upgrades; within a few seconds the new version is
  shown, gopls restarts with it, and runs, tests and go.mod checks use it. An asdf, mise or goenv
  shim on `PATH` is asked for its `GOROOT`.
- **A chosen Go SDK that is gone is said so.** **Settings → Go** shows it in red with a one-click
  **Use go1.27.3 …** for the SDK now in use; `toolchain.go.info` reports it as `missingSdk`.

## 0.38.22 - 2026-10-21

### Added

- **Two ways to update the go line.** The **Newer Go version is available** popup on the `go`
  directive now offers both **Update go directive to 1.27.3** (the newest Go release) and **Update
  to 1.26.10** (the newest patch of the release you are on), on the spot. The `toolchain` line gets
  the same two. Agents see and apply both with `problems.fixes.list` and `problems.fix.apply`.
- **Why the go line was raised.** When updating dependencies makes `go` raise your `go` line (for
  example `go 1.26` to `go 1.26.0`), the task's log and a notification now say from what to what and
  which updated modules require it: "github.com/gofiber/fiber/v3 v3.6.1 and 6 more require go
  1.26.0". `problems.fix.status` reports it as the job's `notice`.

## 0.38.21 - 2026-10-21

### Fixed

- **⌫ and ⌦ in Recent Files and Show Usages no longer edit the file under them.** Typing letters was
  fixed in 0.38.20, but **⌦**, **⌥⌫** and **⌘⌫** in Recent Files (⌘E), and every delete key over
  the Show Usages popup (⌥⌘F7), still deleted from the code underneath. Every popup that filters as
  you type now keeps these keys for itself.

## 0.38.20 - 2026-10-21

### Fixed

- **Typing in Recent Files no longer types into the file.** Letters typed to filter Recent Files
  (**⌘E**) went into the open file too and left it modified; now what you type in any popup that
  filters as you type stays in the popup, and nothing typed over Show Usages reaches the file.
- **Search Everywhere shows every result.** When results came in late (Go to Symbol, which asks
  the language server, or **All**), the list stayed at its first height and cut the rows off, also
  after **↓**. The list now grows to its full height as results arrive, and **↓** scrolls to the
  selected row; Recent Locations, File Structure, Paste from History and Show Usages grow the same
  way.
- **An unused requirement in go.mod is only greyed out**, also while gopls runs: gopls's own "is
  not used in this module" no longer draws an amber wavy line over the grey nor shows up twice in
  the Problems tool window, and a direct requirement gopls finds unused is greyed out the same way.

## 0.38.19 - 2026-10-21

### Added

- **Smarter name matching.** Search Everywhere, Go to Class, File and Symbol, Find Action and
  Recent Files find names the same way: camel humps (`NPE` and `nullpoex` find
  `NullPointerException`), `*` for any characters, a space for a new word or, at the end, for the
  end of the name, and `main.go:42` to go to a line. The exact name comes first, then a prefix, then
  camel humps; your project's names before the SDK's. The letters that matched are marked.
- **Search Everywhere keeps still while you type.** The list keeps the last results until the new
  ones are ready (two seconds at most; Settings → Advanced Settings can make it wait for every
  search), and **Enter** pressed meanwhile opens the first result of what you typed. **Include
  non-project items** (or **⇧ twice** in the open popup) adds the Go SDK and the libraries, and
  **⌥↓** brings back the queries you used, also after a restart.
- **Faster Find in Files.** GoGarden keeps an index of your project's text, built in the
  background with its progress in the status bar, so Find in Files reads only the files that can
  match and shows the first results at once, also in large projects.
- **Search in comments or strings only.** The Filter of the search bar (**⌘F**) and of Find in
  Files offers Anywhere, In Comments, In String Literals, Except Comments, Except String Literals
  and Except Comments and String Literals.
- **Preserve Case and case conversion in Replace.** **Aa** next to the replace field keeps the case
  of each match (`fooBar` → `bazQux` turns `FooBar` into `BazQux` and `FOOBAR` into `BAZQUX`); with
  Regex, `\U…\E`, `\L…\E`, `\u` and `\l` change the case of what they surround, and each match
  shows what it becomes; both in the search bar and in Replace in Files. **Exclude** in the search
  bar skips a match, so Replace All leaves it alone.
- **Search scopes.** Find in Files searches In Project, a Module, a Directory (with or without its
  subfolders) or a Scope: All Places, Project Files, Project Test Files, Recently Changed Files and
  more, and the project's own named scopes, edited in **Scopes** with patterns such as
  `file:internal//*&&!file:*_test.go` (the named scopes of `.idea/scopes` too). The Classes, Files
  and Symbols tabs of Search Everywhere search in the same scopes, and the choice is remembered with
  the project.
- **Show Usages (⌥⌘F7).** A popup at the caret lists the usages of the name under it, reads and
  writes told apart, with toggles for what to show and a preview of the source; **Enter** goes to
  one, **⌥⌘F7** again widens the search up to All Places, and the usages count of Code Vision opens
  it too.

These came in earlier updates without a word in What's new:

- **Paste from History (⇧⌘V).** GoGarden remembers the texts you copy and cut in it while it runs;
  **Edit → Paste Special → Paste from History…** lists them, newest first, filters as you type, and
  pastes one or several at every caret.
- **Matching brackets.** With the caret next to a bracket, it and its match are highlighted, and a
  bracket without a match shows in red; **⌃⇧M** moves the caret to the matching one.
- **Local History.** Every save, and every change another program or Replace in Files, Rename or
  a Git rollback makes, keeps a version of the file for five days, without Git. **File → Local
  History → Show History**, also on a file or folder in the Project tool window, shows the versions
  with their differences and reverts to one; **Put Label…** marks a moment.
- **Inlay hints.** The language server's small grey labels in the code: parameter names before the
  arguments of a call, the types of variables declared without one, constant values. The caret
  steps over them; Settings → Editor → Inlay Hints chooses which you see.
- **Code Vision.** A grey row above each top-level declaration says how many usages it has; click
  it to find them. It can also show who changed the declaration last (**⌃⌥A**). Settings → Editor →
  Inlay Hints → Code Vision.
- **Breadcrumbs** under the editor show the declarations around the caret, such as `Server › Start`;
  a click goes to one. Settings → Editor → Breadcrumbs turns them off.
- **The HTTP Client.** Write requests in `.http` and `.rest` files and send them with the run icon
  next to each one; the response opens in the Services tool window (**⌘8**), with the history of
  earlier runs. Variables and environments come from `http-client.env.json`.
- **Screen reader support.** With VoiceOver on, buttons, tabs, lists, trees and fields have their
  names, the editor reads as a text field with its caret, and notifications are announced. Nothing
  to switch on, and it costs nothing without a screen reader.
- **Tool windows go where you want them.** Drag a stripe button to another side or group, or use
  **Move To** in its right-click menu; drag the edges to resize, **⌃⇧** with an arrow stretches the
  active one, **⇧⎋** hides it, **⇧⌘F12** hides them all, and **⇧F12** restores the default layout.
  Each project remembers its layout.
- **Notifications.** Messages come as balloons in the bottom right corner (errors stay until
  closed) and are kept in the Notifications tool window behind the bell on the right; Settings →
  Appearance & Behavior → Notifications turns balloons off for some or all of them.
- **Soft wraps.** **View → Active Editor → Soft-Wrap** wraps long lines at the editor's edge, and
  **↑**, **↓**, **Home** and **End** move by the rows you see. Markdown and text files open wrapped;
  Settings → Editor → Soft Wraps lists which files do.
- **Presentation, Distraction Free and Zen modes.** **View → Appearance** leaves the editor alone:
  in full screen with a large font for showing code, centred without anything around it for
  writing, or both. **Escape** leaves the mode; **⌃⌘F** switches full screen.
- **Live templates.** Type an abbreviation such as `fori` in a Go file and press **Tab** to get the
  code with its variables to fill in; **⌘J** lists the templates, and Settings → Editor → Live
  Templates edits them and adds your own.

### Fixed

- **⇧ twice** opens Search Everywhere again: pressing ⇧ itself no longer counts as typing.

## 0.38.18 - 2026-10-21

### Changed

- **Unused requirements in go.mod are greyed out, not underlined.** An indirect requirement that
  `go mod tidy` would remove is now drawn in grey across the whole requirement, as unused code is,
  instead of with a wavy underline. Its popup, **Run go mod tidy** (**⌥⇧↵**), **⌥↵** and the
  Problems tool window are as before. Code gopls marks as unnecessary is greyed out the same way
  (an unused import, which stops the build, keeps its red underline too). Themes have a new
  `editor.unused` colour, and JetBrains colour schemes bring theirs.

## 0.38.17 - 2026-10-20

### Fixed

- **go.mod marks a requirement whose go.sum has only one of its two lines.** When one line of a
  version was missing from `go.sum` (for example after deleting it by hand or a partial merge),
  `go build` failed with "missing go.sum entry" but `go.mod` showed nothing. Every requirement now
  needs its `go.mod` hash, and a direct one the hash of its files too, as `go` does; **Run go mod
  tidy** restores them.
- A **Run go mod tidy** (or another `go.mod` fix) that fails is now told by an error notification
  with a title, not a plain message.

## 0.38.16 - 2026-10-20

### Added

- **Type to find a project in the Projects popup, in either keyboard layout.** With the project
  widget's popup open, start typing: a search field appears and only the matching open and recent
  projects stay, the best match first and selected. Matching is fuzzy, on the name and then the
  path (`bs` finds billing-service, `fps` fast-path-search), with the matched letters marked;
  text typed in the wrong layout still matches (`ишд` finds billing-service). ↑ ↓ move, ⏎ opens,
  ⎋ clears and then closes, and "No matching projects" says when nothing is left. Each project now
  shows its Git branch under its path, and agents reach the popup with `view.projects.popup`.

### Fixed

- **go.mod marks follow a `git reset`.** After dependencies were updated and `git reset` brought
  `go.mod` and `go.sum` back, a line could keep **Missing go.sum entry** although `go.sum` had the
  entry, and **Run go mod tidy** seemed to do nothing: the marks were not checked again when only
  `go.sum` changed on disk. They now follow `go.mod`, `go.sum` and `go.work` as they are on disk,
  whatever changed them (a reset, a checkout, an edit by hand), and every finished fix checks the
  file again, also when `go` changed nothing.
- **Every fix of a spot is offered.** When a version has several problems, such as a missing
  `go.sum` entry and a newer version, the popup now offers **Run go mod tidy** and **Update to
  v0.22.0**, and **More actions…** lists the fixes of all of them; before, the update was hidden
  behind the `go.sum` problem.
- **A failed go.mod fix is always told.** When `go get` or `go mod tidy` fails, a notification says
  so with `go`'s error, also while the background tasks panel is open.

## 0.38.15 - 2026-10-20

### Fixed

- **Exiting stops Git operations and dependency updates too.** The Confirm Exit dialog names a
  running Git fetch, pull or push and a dependency update and says exiting stops them, but only
  runs and debug sessions were stopped: a fetch from a server that never answered went on as
  `git fetch` after GoGarden had quit. Their programs, and the helpers git starts for the remote,
  are now stopped as runs are (an interrupt, then a kill two seconds later), on exit and on restart.

## 0.38.14 - 2026-10-20

### Fixed

- **Quit from the Dock asks too.** Quit in the Dock icon's menu, logging out and other quit
  requests of macOS ended GoGarden at once, without **Are you sure you want to exit GoGarden?**,
  and left a running program behind. They now ask as ⌘Q does, naming the work still running, and
  exiting stops your runs and debug sessions; with **Confirm before exiting the IDE** off they
  exit at once, as before. An exit asked for while every GoGarden window is covered no longer
  waits until a window is shown again.

## 0.38.13 - 2026-10-20

### Fixed

- **Graph dots stay on their branches' lines while the graph column narrows.** Narrowing the Git
  workspace's graph column, a branch's dots went into the line of dots at the column's right while
  its line still ran further left, so dots and lines came apart. A branch's line and its dots now
  go into the line of dots together, at the right edge and, with the lines scrolled sideways, at
  the left one; a merge line into a gathered branch runs on into the line of dots instead of
  stopping short of it.

## 0.38.12 - 2026-10-20

### Fixed

- **The Terminal header keeps its buttons when the tabs fill it.** Three tabs with the shell's
  long `user@host:~/path` titles pushed **+**, **⌄** and **⋮** out of the header and the tab shown
  off-screen. Tabs are now named **Local**, **Local (2)**… (a short title the
  shell sets still shows), long names end in "…" with the full name on hover, only whole tabs are
  drawn with the tab shown always among them, the mouse wheel or a sideways swipe scrolls through
  them, and **⌄** lists every tab, the ones that do not fit too.
- **Terminal tabs of a hidden Terminal are there for AI agents.** When the Terminal was hidden as
  GoGarden closed, `terminal.session.list` answered nothing after reopening until the Terminal was
  shown; the remembered tabs now start as soon as an agent lists, reads or types into them. A tab
  opened just before quitting is remembered too.

## 0.38.11 - 2026-10-20

### Added

- **Several terminals and split panes.** The Terminal (⌥F12) opens tabs with **+** or **⌘T**;
  double-click a tab to rename it. **Split Right** and **Split Down**, in a pane's right-click menu
  and in **⋮**, run another shell next to the first; **⌥⇥** and **⌥⇧⇥** move between the panes, and
  the line between them drags to resize. **⌘W** closes the pane with the keyboard.
- Shells keep running while the Terminal is hidden, and the Terminal comes back with its tabs,
  their names, splits and folders when GoGarden reopens the project.
- In the terminal, keys without **⌘** (**⌃C**, **⌃D**, **⌃R**, the function keys) go to the shell.
- AI agents connected over MCP can open, split, list, read and type into terminal sessions with
  the `terminal.*` commands.

### Fixed

- **A filter in the Git workspace shows what it finds.** With the left panel's sections folded (as
  they start), typing in its Filter showed the count but kept LOCAL and the other sections folded,
  so the matching branches stayed hidden. Sections with matches are now shown open while you
  filter, and fold back as you left them when the filter is emptied.
- **Settings fit the dialog at its smallest size.** On a small window, Appearance & Behavior's Sync
  with OS, the Go page's GOROOT list, gopls' Custom path and the notes under them ran past the
  dialog's right edge. Rows now wrap, long choices end in "…", and tables scroll sideways inside
  the page.
- **`/* ... */` comments in Go files fold.** A block comment of several lines folds whole into
  `/.../` with ⌘− or its gutter arrow, like a run of `//` lines; agents see it in `editor.fold.list`.
- **Enter in File Structure (⌘F12) and Recent Locations (⇧⌘E) no longer changes the file.** It goes
  to the symbol or the place and closes the popup, as before, but no longer also breaks the line
  there. Escape no longer reaches the editor either.
- Switching the theme while GoGarden runs (Zen Dark and Zen Light, or an imported theme, in
  Settings → Appearance or over MCP) no longer leaves the code in the editor blank or drawn as
  broken pieces of letters until each line is edited; the code is redrawn in the new theme at once.
- **Exiting GoGarden stops what you ran.** After ⌘Q → Exit (or Quit, or an agent's exit), a
  program started with Run or a debug session no longer keeps running on its own once GoGarden has
  quit: it is stopped as the Stop button does, and killed if it has not ended two seconds later. A
  restart (for an update or a plug-in) does the same.

## 0.38.10 - 2026-10-20

### Added

- **Zen Light, a light theme for the whole IDE.** Choose it in **Settings → Appearance → Theme**:
  the editor and its code colours, the tool windows, the Git workspace, diffs, problems and the
  terminal's colours are all light. The theme changes at once, without a restart.
- **Sync with OS.** Next to the theme list, it makes GoGarden follow macOS: Zen Light while the
  system is light, Zen Dark while it is dark, switching the moment the system does. Choosing a
  theme while it is on makes it the theme for its own appearance.
- **Usages at the caret come from gopls.** With the caret on a name, its real usages in the file
  are highlighted: where it is written (declared, assigned, changed) in a reddish colour, where it
  is read in grey; a comment or another variable with the same name is no longer highlighted.
  Files without a language server keep the plain word match. Settings → Editor → General →
  Highlight on Caret Movement turns it off.
- **Highlight Usages in File (⇧⌘F7) and Find Usages in File (⌘F7)** keep the usages highlighted
  while you move around; **⌃⌥↓** and **⌃⌥↑** step through them, and Escape clears them. Both are in
  Edit → Find.
- Selecting text highlights the other places the same text occurs in the file, inside words and
  comments too.
- **Recent Locations (⇧⌘E)** lists the places in the code you visited and edited lately, newest
  first: each with its file, the declarations around it, how long ago, and five lines of its code.
  Type to filter them by code, file name or declaration; Enter goes there; ⌫ removes a place, from
  Back too; ⇧⌘E again shows the edited places only. It is in the View menu.
- **Recent Files (⌘E) has Show edited only**: ⌘E again keeps the files you edited. Hover over a file
  for Open in Right Split (⇧⏎); the footer shows the selected file's path, and Recent Locations is a
  click away under the tool windows.
- **File Structure (⌘F12)** shows the symbols of the file in a popup over the editor. Type a few
  letters, or camel humps, to find a symbol, the matched letters marked; Narrow down on typing
  keeps only what matches; Enter goes to it. It is in the Navigate menu.

### Fixed

- Light themes imported from VS Code or a JetBrains IDE stay light and readable: the colours they
  do not set come from Zen Light instead of Zen Dark (no more dark tree, current line or terminal
  tab in a light theme), the diff of a VS Code theme gets its modified-line and filler colours, and
  text too faint on its background is made readable on import. Import such a theme again to get
  the fix.
- Find Action names the Git workspace's new commands (columns, details, hover, pull modes) and
  the built-in plugins list.

### Changed

- **Typing in a large Go file is fast however long the file is.** The editor highlights again only
  the lines a keystroke changes, instead of the whole file: on a 100 000-line file a keystroke
  takes a fraction of the time it did. The folds of a Go file come from the same work, so they no
  longer read the whole file when you stop typing.
- **Typing never waits for gopls.** Changes and questions to a language server are written by a
  thread of their own instead of the window: while gopls is busy, typing in a 10 000-line file no
  longer stutters (95% of frames take under 1 ms, where they grew to 25–50 ms after half a
  minute of typing), and the changes made meanwhile reach it together. A server that stops
  reading is restarted.
- **Files keep their folds.** A file you close and open again, or open at the next start, has the
  same regions folded as when you left it, imports included. ⌘+ and ⇧⌘+ (as on the numeric
  keypad) unfold as well as ⌘= and ⇧⌘=.

## 0.38.9 - 2026-10-19

### Changed

- **Git workspace: authors' pictures are on by default.** The graph and the details panel show
  each author's picture from the repository's GitLab, a GitHub no-reply address or Gravatar
  without looking for the setting; settings saved by 0.38.8 have it turned on once. Turn it off
  in **Settings → Version Control → Git Workspace** and GoGarden contacts nothing and shows the
  initials. GitLab finds an author only by the e-mail address public on their profile, and shows
  a generated pattern otherwise.

### Fixed

- The Git workspace's graph scrolls in every direction at once: a diagonal swipe on the trackpad
  goes down the history and along the lines together, instead of keeping to one of them.
- The Git workspace's details panel: its × no longer covers View Changes in the banner above a
  commit.

## 0.38.8 - 2026-10-19

### Added

- **Git workspace: authors' pictures.** Turn on **Settings → Version Control → Git Workspace →
  Show the authors' pictures from Gravatar, GitLab and GitHub** and the commits' circles and the
  details panel show each author's picture, from the repository's GitLab, a GitHub no-reply
  address or Gravatar, kept on your Mac so that they show offline too. Off by default: GoGarden
  then contacts nothing and shows the initials, as before.
- **Git workspace: a details panel that floats over the graph.** Selecting a commit or the WIP
  slides it in from the right; **×**, **⎋** or a click on the graph's empty space closes it, and
  the graph keeps the whole width. Drag its left edge to make it wider or narrower; GoGarden
  remembers the width for each repository.
- **Git workspace: a graph column that collapses.** Drag the graph column's border anywhere along
  the rows. Narrower than its lines, the commits of the lines that do not fit gather into one line
  of dots in their branches' colours, and the lines scroll sideways with a scroll bar (or a
  sideways swipe, or ⇧ and the wheel); at the narrowest only the dots are left, under a graph icon.
  Widen it, or double-click its border, and the lines come back. The graph's scroll bars always
  show.

### Changed

- **Git workspace: a calmer toolbar.** The repository and branch pickers are gone from it (the
  title bar has them); the buttons start at the left and are all of one width, Pull's **▾** now in
  its lower right corner.

## 0.38.7 - 2026-10-19

### Added

- **Git workspace: arrange the graph's columns.** Drag a column's title to move it, drag its border
  to resize it; right-click the titles to switch **Author**, **Date** and **SHA** on and off or
  **Reset Columns**. GoGarden remembers the columns for each repository.
- **Git workspace: see a branch at a glance.** Rest the pointer on a branch's label (or its row on
  the left) and its commits stay bright while the others fade (**Highlight Branch on Hover** in the
  titles' menu turns it off). Rest it on a commit without a label to see the branch it is nearest to.
- **Git workspace: every way to pull.** Pull's **▾** now offers **Fetch All**, **Pull
  (fast-forward if possible)**, **Pull (fast-forward only)** and **Pull (rebase)**; click the circle
  beside one to make it what **Pull** (and **⌘T** in the Git tab) does, remembered for each
  repository. Every toolbar button's hint names its shortcut; **Stash** is **⌥⇧⌘S** and **Pop**
  **⌥⇧⌘P**.
- **Confirm before exiting.** ⌘Q and **Quit GoGarden** ask **Are you sure you want to exit
  GoGarden?** with **Exit** and **Cancel** (⏎ exits, ⎋ cancels), and name what is
  still running: a dependency update, a Git operation, a run or a debug session. **Settings →
  Appearance & Behavior → System Settings → Confirm before exiting the IDE** turns the question
  off. AI agents exit with the `exit_ide` tool and are never held up by the dialog.
- **Copy and paste files in the Project tool window, as in the Finder.** **⌘C** copies
  the selected files and folders themselves, so that the Finder can paste them, and **⌘V** pastes
  copies into the selected folder, named `main copy.go`, `main copy 2.go` when the name is taken;
  **⌘X** then **⌘V** moves them. Files copied in the Finder paste too. **⇧⌘C** copies the absolute
  paths, one per line.
- **A fuller context menu in the Project tool window**, with icons and shortcuts: Cut,
  Copy, Copy Path, Paste, **Rename…** (**⇧F6**), **Reformat Code**, Delete…, **Open in Right
  Split** (**⇧↵**), **Open In → Finder / Terminal**, Local History, **Git → Show History, Annotate
  with Git Blame, Show Diff, Rollback…, Add, Commit File…**, **Reload from Disk**, **Compare With…**
  (**⌘D**), **Compare File with Editor** and **Add to .gitignore**.
- **Areas of work.** An IDE can now ship plug-ins that stay off until you choose the areas you work
  in: on its first start a small window offers them, each with a line about what it gives, with
  **Continue** and **Skip**, and **Settings → Plugins → Built-in** has **Choose Areas…** for later.
  GoGarden has no such plug-ins, so nothing changes for you; this is the groundwork for TestTotami,
  the QA IDE. Agents use `plugins.areas.list`, `choose`, `skip` and `show`.
- **go.mod covers every common case.** A deprecated module's path is struck out:
  rest the pointer on it for **Deprecated module …** with its author's deprecation notice and
  **Show … deprecation notice**. A retracted version is a warning with the reason and an update to
  the newest version that is not retracted; an excluded one likewise. A newer major version
  (`/v2`, `/v3`…) of a direct requirement is a quiet hint. The `go` and `toolchain` directives are
  compared with the latest Go release (**Update go directive to 1.27.2**), and a `go` directive
  newer than your Go SDK is a warning with **Configure Go SDK…**. What `go` itself reports shows on
  its line: an unknown version, a requirement missing from `go.sum`, an indirect requirement nothing
  uses (with **Run go mod tidy**). `go.work` files get the Go version checks and an error for a
  `use` folder that is missing or has no `go.mod`.
- **Background tasks.** An update of dependencies runs in the background and never stops the window:
  a panel at the bottom right opens at once with what `go` prints as it prints it, then says it
  finished or shows the error, and the problems are checked again. **Hide** hides it; the task's
  title and progress bar in the status bar show it again. Git fetch, pull and push show their
  progress there too.
- **Git workspace: Undo and Redo of Git operations.** **Undo** on the Git tab's toolbar (or **⌘Z**
  while the tab is in front) takes back the last checkout, commit, amend, reset, branch created or
  deleted, merge, rebase, fast-forward, cherry-pick, revert, discard or rollback, wherever you did
  it in GoGarden; **Redo** (**⇧⌘Z**) does it again, and their hints name the operation ("Undo
  Commit 'Fix login'"). An undone commit leaves its changes staged; discarded changes, unversioned
  files deleted with them and what a hard reset threw away come back from copies GoGarden keeps.
  Undo never loses work: if the repository changed since (a commit made in a terminal, a file
  edited again), it says what changed and does nothing.
- **Changelists and the shelf in the Commit tool window**. Changed files are grouped
  in changelists: **New Changelist…**, **Edit Changelist…**, **Set Active Changelist** and **Delete
  Changelist** in a list's context menu, **Move to Another Changelist** (**⌥⇧M**) for files; new
  changes go into the active list, and checking only one list's group commits just that list.
  **Shelve Changes…** puts changes aside without a commit (with a preview of each file's diff) and
  rolls the files back; the new **Shelf** tab lists what is shelved, and **Unshelve…** brings it
  back into a changelist, merging with what changed since and marking conflicts if there are any.
  GoGarden remembers both per project; Undo in the Git workspace takes a shelve or an unshelve back.
- **Git workspace: interactive rebase.** **Interactive Rebase from Here…** in a commit's menu,
  **Interactive Rebase *current* onto *branch*…** in a branch's menu, or a branch dragged onto
  another opens the commits in an editor tab: choose **Pick**, **Reword**, **Squash**, **Fixup** or
  **Drop** for each (or press P, R, S, F, D), drag rows or press ⌥↑/⌥↓ to reorder, type a new
  message right in the row, then **Start Rebase**; **Reset** starts over and **Cancel** leaves
  everything as it was. The same editor cherry-picks several commits: ⌘-click them in the graph,
  then **Cherry-Pick *N* Commits…**. If Git stops on conflicts, the work in progress lists the
  conflicted files with **Continue** and **Abort**. A finished rebase is one step for Undo. No text
  editor ever opens.
- **Git Log: filter by folders and files, and the history of one.** The Log's **Paths** filter offers
  **Select Folders…**, a tree of the repository with checkboxes, and the sets of paths you used last.
  **Git → Show History** in the project tree's context menu opens a Log tab with the history of a
  folder or file. The Log's tabs keep their filters (text, branches, users, dates, paths) the next
  time you open the project.
- **Blame shows how old each line is.** Annotate with Git Blame shades every line by the age of its
  commit: the newest changes are bright and warm, older ones fade. The colours come from the theme
  (`editor.annotation.recent` and `editor.annotation.old`; imported JetBrains schemes bring theirs).
- **Git workspace: very large histories open at once.** The newest commits show as soon as they are
  read and the rest comes in below them, with how many are read so far at the bottom of the graph;
  the window never waits. A repository of 200 000 commits is read in about a second and takes about
  300 bytes of memory a commit, so histories of hundreds of thousands of commits scroll and search
  at once.
- **Built-in plug-ins: all of Go is one plug-in.** Everything for Go (gopls, run, test and debug,
  go.mod checks, the Go SDK and Settings → Go) is now the **Go** plug-in, built into every Zen IDE
  and on in GoGarden; **Rust** is another. The new **Settings → Plugins → Built-in** tab lists them
  with what each gives and whether this IDE has it on by default, switches each on or off, and
  offers **Restart IDE** to apply the change. **Restart IDE** is also in Find Action. Nothing
  changes in how Go works in GoGarden.
- **Git workspace: worktrees.** Work on several branches at once, each in a folder of its own. The
  new **WORKTREES** section lists the repository's worktrees with their branch, a lock when locked
  and how many files are changed in each. **Create Worktree…** in a branch's menu makes one (a new
  branch or the branch itself, in a folder next to the repository's) and opens it in a new window;
  a worktree's menu offers **Open in New Window**, **Lock** or **Unlock**, and **Remove…**, which
  asks first and can delete its branch too (Undo brings the branch back). Every other worktree with
  changes is a **WIP** row in the graph, right above the commit it is at.
- **Git workspace: submodules.** The new **SUBMODULES** section lists them with whether each is at
  the commit the repository records (a check), moved off it, or not initialised; **Update**,
  **Initialise** and **Open as Project** in its menu.
- **Resolve merge conflicts.** When a merge, rebase, cherry-pick, revert, stash or Update Project
  stops on conflicts, the Git tab says so in a banner with **Resolve…**, **Continue** and
  **Abort**, and the conflicted files come first in the work in progress and in a **Merge
  Conflicts** group of the Commit tool window. **Resolve…** opens a file in the conflict editor:
  ours on the left, the result in the middle, theirs on the right, every change lined up. **»**
  takes ours for a change, **«** theirs, and pressing the other one after that takes both; click
  the result's lines to edit them right there; **F7** and **⇧F7** go to the next and previous
  conflict. **Accept Ours** and **Accept Theirs** take a whole file, also for a file one side
  deleted; **Mark Resolved** writes the result and stages it. Then **Continue** finishes the merge,
  cherry-pick, revert or rebase (or drops a popped stash), and **Abort** puts everything back. A
  merge, cherry-pick or revert finished after conflicts is one step for Undo.
- **Git workspace: compare commits, a file's history and blame.** Click a commit and **⌘-click**
  another (or the **WIP** row) to see what changed between them: the right panel lists the files
  that differ, and a click opens a file's diff between the two points, from where it was when it
  was renamed in between; **Compare** in the commit menu shows them again. Right-click a file in
  the right panel for **File History** (the commits that changed it, through renames, each opening
  the file's diff at that commit; **Show in Log** for the Log's tab), **Blame** (the file in the
  editor with its annotations, a click on one selecting its commit in the Git tab) and **Open
  File**.

### Changed

- **Code authors are off by default.** Code Vision shows only the usages above declarations; the
  author line comes back with the new person button at the right end of the status bar (lit while
  it is on), **View → Appearance → Show Code Authors**, **⌃⌥A** or Settings → Editor → Inlay Hints
  → Code Vision → **Code author**. They are turned off once for everyone with this update; switch
  them back on with **⌃⌥A** or the status bar, and that choice is kept.
- The Git workspace's toolbar no longer has a Terminal button; the terminal stays one click away
  in the tool window bar (⌥F12).
- The Git workspace's left panel starts folded, sections and branch folders alike; click to open them, and a filter opens what matches.

### Fixed

- The Git workspace's section titles are no longer cut mid-word in a narrow left panel in Russian
  (**ОТЛОЖЕННЫЕ**, **ДЕРЕВЬЯ**); a title that still does not fit ends in "…" and its hint shows it
  whole.
- The Git Log's empty details text wraps inside its panel instead of running past the edge in
  longer translations.
- Problem popups open only while the pointer rests on the underlined text itself, never on empty
  space or elsewhere on the line, after the same short delay every time, and close when the pointer
  leaves.
- The **Update dependencies** dialog fits its rows, with its buttons at the bottom; a long list
  scrolls, **Select All** and **Select None** check or uncheck every row, and **Update (3)** counts
  the updates.
- The change marks on a diff's right edge have a strip of their own beside the scroll bar; pointing
  at one names the lines that changed, and a click goes to them.
- `go.mod`, `go.work` and `go.sum` show module paths in green; versions stay teal.

## 0.38.6 - 2026-10-19

### Added

- **The Git workspace has a new look.** The toolbar shows the repository and the branch under
  their captions (click either to switch), then Undo, Redo, **Pull** (its **▾** offers Pull with a
  merge or a rebase, and Fetch), Push, Branch, Stash, Pop and Terminal, each a caption over its
  icon, and Search. Every row of the graph is tinted in its branch's colour, the branch lines turn
  in rounded corners, and branch labels are filled in their branch's colour with a laptop for a
  local branch and a cloud for a remote one. Drag a column's border to narrow it. The left panel
  says how many items it is viewing, has a filter (**⌥⌘F**) and sorts branches like `feat/login`
  into folders. The commit panel shows how many files are changed in the working directory with
  **View Changes**, the subject in large letters, the author and parents, the counts of modified,
  added and deleted files, and the files by **Path** or as a **Tree**, with **View all files**.
- **Git workspace: drag and drop to merge and rebase.** Drag a branch's label in the graph, or its
  row in the left panel, onto another branch: the branch it would land on is highlighted, **⎋**
  cancels, and dropping it offers **Fast-forward** (when the other branch is behind), **Merge** and
  **Rebase** (Interactive Rebase comes later). Merging into, or rebasing, a branch that is not
  checked out asks first, then checks it out.
- **Git workspace: search, filter, Hide and Solo.** **⌘F** in the Git tab searches commit
  messages, SHAs and authors as you type: matches are highlighted, **↑**/**↓** (or **⏎**/**⇧⏎**)
  move between them and the bar shows "2/5"; Match Case and regular expressions work as in the
  Log. A **Filter** field narrows the left panel's branches, remotes, tags and stashes. Point at a
  branch or a remote to **Hide** it from the graph or **Solo** it (show only it); **Show All**
  brings everything back, and both are remembered for the repository.
- **go.mod.** Versions are teal and module paths plain. While a `go.mod`
  is open, an outdated version gets a quiet wavy underline (**Newer version is available: v1.2.6**)
  and a module with known vulnerabilities (from the Go vulnerability database) is underlined across
  its line, with its advisories, their scores and **Show details…**; the editor's counts include
  them. **Update dependencies…** (**⌥⇧↵**), **Update to unaffected version**, **More actions…**
  (**⌥↵**, also with `go mod tidy`) run `go get` with the project's Go SDK. Private modules are never
  asked about; **Settings → Languages & Frameworks → Go → Dependencies** turns the checks, and their
  network use, off.
- **Show Context Actions** (**⌥↵**) lists the quick-fixes of the problems at the caret when there
  are some (the language server's context actions otherwise), and **⌥⇧↵** applies the main one;
  hovering a problem shows a popup with its fixes.
- Diffs mark where their differences are on the right edge, in the colours of added, modified
  and deleted lines; click a mark to go to it.
- **Git workspace: stage and commit.** The WIP panel lists the unstaged and the staged files (a
  file changed again after staging is in both). Stage or unstage a file with its button or a double
  click, or all of them at once; discard a file's unstaged changes (asks first). Click a file to see
  that part in the diff tab, where **Stage Hunk**, **Unstage Hunk** and **Discard Hunk…** act on a
  difference, and clicking line numbers chooses single lines to stage, unstage or discard. Type a
  summary (with a character counter) and a description, then **Commit** or **Commit and Push**
  exactly what is staged, or amend the previous commit.

## 0.38.5 - 2026-10-19

### Added

- **Git workspace: act on a commit.** Right-click a commit in the graph for **Create Branch
  Here…**, **Create Tag Here…** (a message makes an annotated tag), **Checkout This Commit…**
  (asks first: HEAD will be detached), **Cherry-Pick Commit**, **Revert Commit**, **Reset *branch*
  to This Commit** (**Soft**, **Mixed**, or **Hard**, which asks first), **Copy Commit SHA** and
  **Copy Commit Message**. After a cherry-pick, a revert or a reset the new HEAD is selected. When a
  cherry-pick or a revert stops on conflicts, the repository is left as Git leaves it: a dialog
  names the conflicted files and says how to go on, and the WIP row lists them.
- **Git workspace: act on a branch or a tag.** Right-click a branch or a tag in the left panel, or
  the labels at a commit in the graph: **Checkout**, **Merge *branch* into *current***, **Rebase
  *current* onto *branch***, **Fast-forward *branch* to *upstream*** (without going online),
  **Push *branch*** (any local branch, not only the current one), **Set Upstream…**, **Rename…**
  and **Delete** for branches (deleting a remote branch asks first); **Checkout…** (asks first),
  **Merge**, **Rebase**, **Push Tag** and **Delete Tag** for tags. The left panel and the graph
  show the change at once. A merge or a rebase that stops on conflicts is explained in a dialog, as
  a cherry-pick is.
- **Git workspace: stashes.** **Stash** and **Pop** on the toolbar put every change aside and bring
  the newest stash back. In the WIP panel, stash with a message: **Stash All**, or **Stash Staged**
  for the staged changes only. Right-click a stash in the left panel or in the graph for **Apply
  Stash**, **Pop Stash**, **Rename Stash…** and **Drop Stash…** (asks first). Each stash is a row
  of the graph, right above the commit it was made on. A stash that does not apply cleanly is kept,
  and a dialog names the conflicted files.

### Fixed

- The Project tool window's title no longer runs under its buttons when the window is narrow: it
  is cut where the buttons begin.
- Diffs scroll sideways: long lines are no longer cut. Both sides of a side-by-side diff scroll
  together, by a sideways swipe, **⇧** and the wheel, or the bar under the text.
- **Git workspace**: clicking a commit that is already in view no longer scrolls the graph.
- Cherry-picking a merge commit (from the Git tool window's Log too) takes its changes against the
  first parent instead of failing; a commit the current branch already has is refused with a clear
  message.
- **Merge into Current** (the branch popup and the Log too) that stops on conflicts now says which
  files conflict and how to go on, in a dialog, instead of Git's raw message. The confirmation
  dialogs of the Git workspace take ⏎.

## 0.38.4 - 2026-10-19

### Added

- A button at the top of the right tool window bar opens the Git workspace; it is lit while the
  workspace is in front, and a second click closes it.

### Fixed

- The gutter's run icon has slightly rounded corners.
- **Git → Log**: branch and tag labels after a commit's subject are no longer cut off. When a row
  is too narrow, the subject is shortened first; then the extra labels collapse into "+N" (rest the
  pointer on it to see their names).
- **Git workspace**: a commit message too long for its column now ends in "…", and resting the
  pointer on it shows the whole message. Long branch names in the labels on the left end in "…"
  too.

## 0.38.3 - 2026-10-19

### Added

- The green ▷ next to `func main` and the tests is back in the gutter: click it to run
  or debug. **Settings → Editor → Gutter Icons** still turns it off.
- **Git workspace** (Git → Git Workspace): the whole repository in an editor tab. The commit
  graph of every branch with a colour per branch, branch and tag labels,
  authors, dates and ids; a **WIP** row for the changes in progress; the local and remote
  branches, tags and stashes on the left (click to jump to a commit, double-click a branch to check
  it out); the selected commit's message, author, parents and changed files on the right (click a
  file for its diff); Fetch, Pull, Push and Branch on top. Histories of tens of thousands of
  commits scroll at once. More Git operations come to it step by step.
- **The Git tool window's Log.** The branches panel with **Branch or tag** search,
  **Local**, **Remote** and **Tags** groups with folders for names with "/", favourites (★),
  commits to push and to pull, and its tool bar (New Branch, Update Selected, Delete, Compare with
  Current, Navigate Log to Branch Head, Fetch, Mark as Favorite…). The filter bar with **.\***
  (regular expression) and **Cc** (match case) for **Text or hash**, and **Branch**, **User**,
  **Date** and **Paths** filters, the sort order, Refresh, Cherry-Pick and Go to Hash/Branch/Tag.
  More Log tabs with **+**. Dates read **Today** and **Yesterday**; merge commits are dimmed and
  your own commits have the author in bold; the commit `HEAD` is at has a ring. The details show
  the changed files as a tree by directory and the message with the branches containing the
  commit. AI agents reach all of it through the `vcs.log.*`, `vcs.logTab.*` and new `vcs.branch.*`
  commands.

### Fixed

- The Log labels every branch and tag at a commit, local branches (the current one too), remote
  branches and tags; before, a long subject hid them and only some showed.

- Selecting a commit from blame scrolls the Git tool window's Log to that commit again, also far
  down a long history.

## 0.38.2 - 2026-10-19

### Fixed

- Closing a dialog or popup (Settings, Go to Line, Rename, Search Everywhere and the others, with
  **⎋**, **OK** or **Cancel**) gives the keyboard back to the editor, so its shortcuts such as
  **⌃G** work at once.
- AI agents: starting GoGarden with `ZEN_AUTOMATION` turns the MCP server on for that run only;
  saving a setting no longer turns it on for the next runs.
- Settings: the list of sections scrolls when the dialog is small, so **Advanced Settings** is no
  longer drawn below the dialog; opening Settings on a page scrolls the list to it.
- In Push Commits, a long commit subject ends in "…" before the author and date instead of
  running over them; the whole subject shows when you point at it.
- A project opened through a symbolic link (such as `/tmp/app`, where `/tmp` links to
  `/private/tmp`) behaves as its real folder: **Select Opened File** finds its files, the status bar
  shows the file's path inside the project, and opening it by either path shows the same window.
  The tree's first row shows the path you opened it by.
- Files changed by other programs reload while the window is covered, minimised or hidden, not
  only once it is drawn again.

## 0.38.1 - 2026-10-19

### Fixed

- Settings → Languages & Frameworks → Language Servers: the table of servers fits the dialog; a
  long path or argument list ends with "…" and shows in full when you point at it.
- **Language Servers** and **List Language Servers** in Find Action open the Language Servers
  settings page instead of running without showing anything.
- **Go to Line** and **Rename** no longer open unseen over a diff tab; opening a file brings its tab
  back in front of the diff, even the file the diff is over.
- AI agents: `press_keys` with keys and text in one call types the text into what the keys opened
  (such as Find Action after ⇧⌘A); Go to Line and Rename are refused with a clear message when the
  editor shows a diff, an image or no file.

## 0.38.0 - 2026-10-18

### Added

- **Basic web in every IDE**: HTML, CSS, JavaScript (with JSX) and TypeScript (with TSX) are
  highlighted, folded and outlined in the Structure tool window, with completion of tags,
  attributes, CSS properties and values, keywords and snippets, and **Emmet abbreviations** in
  HTML (`ul>li.item$*3`, then **Tab**). They come with GoGarden and every Zen IDE.
- **Language servers**: a widget in the status bar shows the project's language servers, their
  state, version and files, with Restart, Stop, Show Log and Install. When a file needs a server
  that is not installed, a banner offers to install it; Settings → Languages & Frameworks →
  Language Servers switches them on and off and sets their path and arguments.

### Fixed

- A project's `.idea` code style now applies to plain text and other files too.
- The status bar shows what Tab and Enter actually insert (`Tab` or `N spaces`).
- **Find Usages**, Go to Declaration and the other lookups work for a file that is not open.

## 0.37.3 - 2026-10-18

### Added

- **Clone Caret Above** and **Clone Caret Below**: press ⌥ twice and hold it, then ↑ or ↓
  (⌥⌥↑, ⌥⌥↓); also in Edit → Multiple Carets and Find Action.
- **Column Selection Mode** (⇧⌘8, Edit → Column Selection Mode): ⇧ with the arrows, and a drag,
  select a column, also past the ends of short lines.
- **Settings → Editor → Gutter Icons → Show run icons in the gutter**: the green ▶ next to `main`
  and the tests is off by default; run and debug from the run widget, the Run menu, ⌃⇧R and ⌃⇧D.
- For AI agents: `editor.caret.cloneAbove`, `editor.caret.cloneBelow` and
  `editor.selection.columnMode`; `editor.fold.list` can list only some lines (`fromLine`, `toLine`);
  the setting `gutterRunIcons`; the theme key `gutter.separator`.

### Changed

- **The editor's gutter**: rows 22 points high, the line numbers right-aligned, room
  for icons, the fold arrows in their own column between the numbers and the text, a thin line
  before the text, and the first row right under the tabs. The arrows are larger; those of regions
  you can fold show while the pointer is over the gutter, those of folded regions always. A
  breakpoint is a red dot in place of the line number.
- **Completion**: the selected item's documentation is in a panel beside the list.
- While you type in a large file, the folding, the run icons and the highlighting of the name at
  the caret wait until you pause.

### Fixed

- **Typing in large files is fast again**: about 11 ms per keystroke on a 100 000-line Go file
  (85 ms before), 4 ms on 10 000 lines; scrolling stays under 1.5 ms a frame at any size.
- **Completion keeps the arguments of a call**: choosing `Total` at `item.|Total(2)` with Tab or
  Enter gives `item.Total(2)`, not `item.Total()(2)`; a generic function gives `Map(xs)`.
- **Constants are coloured at every use**, in other files and packages too (`model.MaxItems`), from
  gopls.
- The wheel and the trackpad scroll the text with the pointer over the line numbers.
- **Find Action** (⇧⌘A) finds the image viewer's **Zoom In**, **Zoom Out**, **Actual Size** and
  **Fit Zoom to Window**.

## 0.37.2 - 2026-10-18

### Added

- **Go to Class** (⌘O, Navigate → Class…): Search Everywhere has a **Classes** tab that finds your
  project's types (structs, interfaces and other named types) by name through gopls, then those of
  the Go SDK and libraries; Enter opens the file at the type. For AI agents: `navigate.class.goTo`.
- For AI agents: `run_action` runs anything a shortcut, a menu item or Find Action runs, including
  what opens tool windows, popups and dialogs, and says what the window shows afterwards;
  `get_window_state` tells what is open; `press_keys` presses keys in GoGarden's window (Escape
  closes a dialog) without using your keyboard.

### Changed

- Shortcuts on macOS: **⌘S** is **Save All** (every modified file; **Save** keeps its
  File menu item without a shortcut), **⌘O** is **Go to Class**, and **File → Open…** has no shortcut.

### Fixed

- **View → Tool Windows** lists **Problems** (⌘6), between Debug and Terminal.
- **Find Action** (⇧⌘A) finds every command, not only those with a shortcut: **Install gopls**,
  **Reload Project Model**, **Restart Language Servers** and the rest. **Select Theme**, **Import
  Theme**, **Choose Go SDK** and other commands that need something from you open the Settings page,
  dialog or popup where you give it.
- For AI agents: `dispatch_command` runs the window's actions that `find_action` and `get_keymap`
  list, such as `view.settings.open` or `view.toolWindow.git`, instead of answering "unknown command".

## 0.37.1 - 2026-10-18

### Changed

- **Softer terminal colours**: the terminal's ANSI colours are pastel tones.
- **About GoGarden** says that Zen IDE is an independent project, not affiliated with, sponsored
  by or endorsed by any other IDE vendor; so does the application's copyright line in Finder.

## 0.37.0 - 2026-10-18

### Added

- **Debugging Go with Delve**: **Debug** (⌃D, the bug next to Run) debugs the selected
  configuration, ⌃⇧D what is at the caret, and **Debug '<name>'** in a run icon's menu the test or
  `main` on that line. GoGarden builds the program without optimisations, starts Delve on it and
  shows the build and the program's output in the session's console.
- **Line breakpoints**: click a line number or press ⌘F8. They move with your edits, show whether
  Delve could set them, change at once while a session runs, and come back when the project opens
  again.
- **Stepping**: F8 Step Over, F7 Step Into, ⇧F8 Step Out, ⌥F9 Run to Cursor, ⌥⌘R Resume Program,
  ⌘F2 Stop; the editor opens the line where the program stopped and marks it with an arrow.
- **The Debug tool window** (⌘5): a tab per session with its toolbar, the goroutines and their
  frames, the variables of the selected frame (long values read 100 at a time), watches, and the
  console with clickable file links.
- **Delve**: found next to the SDK, in GOBIN, GOPATH/bin or on PATH, or chosen in Settings →
  Languages & Frameworks → Go. When it is missing, a notification offers **Install Delve**, which
  downloads it with `go install` only when you click it. Known Delve errors come with what to do.
- For AI agents: the `debug.*` commands (sessions, stepping, frames, variables, evaluation,
  breakpoints and watches), and `toolchain.dlv.choose` and `toolchain.dlv.install`.

## 0.36.0 - 2026-10-18

### Added

- **Projects shared with JetBrains IDEs**: open a project that a JetBrains IDE also uses, and
  GoGarden reads its `.idea` settings and never changes them, so both IDEs keep working on it side by
  side.
  - The project's own code style (tabs, tab size, indent) is used by Enter, Tab and Reformat Code
    over your Editor → Code Style, and the status bar shows it.
  - A file the project keeps in another encoding (for example windows-1251), or a text file that is
    not valid UTF-8, opens read-only with a message and is never saved; the status bar shows its
    encoding.
  - Folders and name patterns the JetBrains IDE excludes are excluded here too: in the project tree,
    Go to File, Search Everywhere and Find in Files.
  - The shared Go run configurations (Go Application and Go Test) appear in the run widget and run as
    they are; **Save as Zen Configuration** in Edit Configurations makes a copy you can change.
- **Keymaps of your own**: Settings → Keymap has a keymap list with **Duplicate…**, **Rename…** and
  **Delete**, and **Import…** reads a keymap `.xml` exported from a JetBrains IDE, with a report of
  what could not be carried over.

## 0.35.0 - 2026-10-18

### Added

- **Search in Settings**: type in the field above the sections (⌘F) to find a setting by its name,
  in the language of the interface; the matching words are marked on the page.
- **UI font size**: Settings → Appearance → **Use custom font** and **Size** make the whole interface
  larger or smaller, menus and tool windows included, while the editor keeps its own font.
- **Editor → Font**: choose any monospaced font installed on your computer, its size and line
  height, and see a preview of Go code before you apply it.
- **Editor → Code Style**: tabs or spaces, the tab size and the indent for Go and for JSON, YAML,
  TOML, Markdown and other files, used by Tab, Enter and Reformat Code.
- **Languages & Frameworks → Go**: **Add SDK → Local…** adds a Go SDK from a folder and **Remove**
  takes it off the list; a **Custom path** chooses another gopls; the on-save options are there too.
  A new SDK or gopls applies at once in every window: gopls restarts, and the project's libraries
  and your next run or test use the new SDK.

## 0.34.4 - 2026-10-18

### Changed

- **The branches popup** looks and works: the actions with their icons and shortcuts
  (Update Project…, Commit…, Push…, New Branch… on ⌥⌘N), then the **Recent**, **Local**, **Remote**
  and **Tags** groups as a tree, with branch names containing "/" grouped in folders, the current
  branch marked with a yellow label, and the commits to pull and push right after each name. The
  popup keeps its width, collapses or expands all groups with one button, and works from the
  keyboard (↑ ↓ → ← Enter, type to search, Escape).
- **The branch widget** shows a green ↗ when there are commits to push and a blue ↙ when there are
  commits to pull.

### Added

- **Checkout Tag or Revision…** in the branches popup checks out a tag, a commit or any revision.
  For AI agents: `vcs.revision.checkout`; `vcs.branch.list` also gives the recent branches and the
  tags.

## 0.34.3 - 2026-10-18

### Fixed

- **Holding ⌘ over a keyword** (`return`, `func`), a string, a number or a comment no longer makes it
  a link.

## 0.34.2 - 2026-10-18

### Fixed

- **Holding ⌘ over a name** underlines it only when it leads somewhere (GoGarden found its
  declaration), and the underline sits right under the letters instead of low under the line.

## 0.34.1 - 2026-10-18

### Fixed

- **The branches popup**: a long branch name no longer runs under its upstream; it ends in "…".
  The menu of a branch stays compact, with long names shortened in the middle.
- **Undoing every change of a file** makes it unchanged in the Project tool window and its tab at
  once, before it is saved: the Git status follows the text in the editor.

## 0.34.0 - 2026-10-18

### Added

- **Run and test your Go code**. Run configurations build and run a package (Go Build)
  or run its tests (Go Test, for a package, a folder, files or one test), with the working
  directory, environment variables and arguments; save them with the project in
  `.zen/runConfigurations.json`.
- **The run widget** in the toolbar shows the selected configuration with Run (⌃R), Rerun and
  Stop (⌘F2); its list selects another one or opens **Edit Configurations…**, the Run/Debug
  Configurations dialog. The selection comes back when you reopen the project.
- **Run icons in the gutter** next to `func main`, tests, benchmarks, examples and fuzz tests run them
  in one click and show how each test last went; **⌃⇧R** runs the test or program at the caret.
- **The Run tool window** (⌘4) shows each run in a tab: the output with clickable file links, and
  for tests a tree of packages, tests and subtests with passed, failed and skipped tests, their
  durations and their own output. **Rerun Failed Tests** runs only the ones that failed.

## 0.33.0 - 2026-10-18

### Added

- **Themes.** Settings → Appearance → **Theme** chooses the colours of the whole IDE: the windows,
  the editor and the terminal change at once, and **Cancel** goes back to the theme you had. The
  look you know is the built-in **Zen Dark**, and it stays the default.
- **Load theme file…** imports a JetBrains colour scheme (`.icls`) or UI theme (`.theme.json`), a
  VS Code colour theme (`.json`) or a GoGarden theme, and switches to it; colours GoGarden has no
  place for yet are counted, and the ones the file leaves out come from Zen Dark.
- **Your own themes**: a theme file in the `themes` folder of GoGarden's data folder sets only the
  colours it changes and builds on another theme. AI agents can list, choose and import themes too.

## 0.32.0 - 2026-10-18

### Added

- **The Git tool window** (⌘9) shows the log: every branch's commits with the graph, their branches and
  tags, filters by text or hash, branch and user, and each commit's changed files and message; a
  double click on a file shows how the commit changed it.
- **Annotate with Git Blame** (in the gutter's context menu and the Git menu) shows who last changed
  each line and when; click a line's annotation to find its commit in the log.

### Changed

- Commit dates show in the author's time zone.

## 0.31.0 - 2026-10-18

### Added

- **The Commit tool window** (⌘0, or ⌘K to write the message): check the changed and new files to
  commit, write the message, Commit or Commit and Push…, or Amend the last commit.
- **Diffs** in an editor tab, side by side or unified, with the changed words marked and ↑ ↓ to go
  from one difference to the next.
- **Change markers** in the editor's gutter show the lines changed since the last commit; click one to
  roll those lines back or see the diff. **Rollback** (⌥⌘Z) discards the changes of files.

## 0.30.0 - 2026-10-18

### Added

- **Git**: file names in the Project tool window and on the tabs are coloured by their status: modified in blue, added in green, unversioned in red, ignored in olive.
- **The branch widget** in the toolbar opens the branches popup: search, check out, create, merge and
  delete branches, with the commits to pull and to push of each.
- **Update Project** (⌘T), **Push** (⇧⌘K) and **Fetch** run in the background with their progress
  in the status bar; the Push dialog lists the commits it sends. A new Git menu has them all.

## 0.29.1 - 2026-10-18

### Added

- **The name at the caret is highlighted** with every other place it occurs in the file, in every
  kind of file.

## 0.29.0 - 2026-10-18

### Added

- **External Libraries** in the Project tool window: the Go SDK and the Go modules your
  project depends on, from `go.mod` and `go.work`, to browse and open. Modules that are not
  downloaded yet are greyed out, and the list follows your edits to `go.mod` and `go.work`.
- **The Structure tool window** (⌘7) shows the types, fields, methods, functions, constants and
  variables of the file you are in. Click a symbol to go to it; sort alphabetically, hide fields, and
  let the tree follow the caret.

## 0.28.2 - 2026-10-18

### Changed

- **Comments fold whole** into `/.../`; a doc comment keeps the name it describes in
  view (`// TestLimitStopsWalk ...`), and that name is a little brighter than the comment.
- **Resting on a fold's mark** shows the folded lines.

## 0.28.1 - 2026-10-18

### Changed

- **Holding ⌘ over a name** colours it as a link and shows its declaration above it, highlighted.

## 0.28.0 - 2026-10-18

### Added

- **The Project tool window's header buttons work**: New File or Directory (+), Select Opened File,
  Expand All, Collapse All, and Options → Always Select Opened File.

## 0.27.0 - 2026-10-18

### Added

- **Rename**: ⇧F6 (or Refactor → Rename…) renames a name everywhere in the project. Refactor
  renames at once; Preview lists every change in the Find tool window first, with Do Refactor and
  Cancel. Every file changes or none does, and ⌘Z undoes the whole rename.
- **Reformat Code**: ⌥⌘L formats the file with gofmt.
- **Optimize Imports**: ⌃⌥O removes unused imports and sorts the rest, as goimports does.
- **Actions on Save**: Settings → Tools → Actions on Save can reformat the code and optimize the
  imports of a file each time it is saved.

## 0.26.1 - 2026-10-18

### Changed

- **Calls of functions and methods are sand-coloured** (`fmt.Sprintf`, `c.Log()`);
  declarations stay blue.

## 0.26.0 - 2026-10-18

### Added

- **Go to declaration**: ⌘B or ⌘-click on a name goes to where it is declared; holding ⌘ over a
  name underlines it. On the declaration itself, ⌘B lists its usages. When there are several
  places, a list opens to choose from.
- **Go to implementation**: ⌥⌘B goes to the implementations of an interface or its methods.
- **Find Usages**: ⌥F7 lists every place a name is used in the project in the new Find tool window
  (⌘3), grouped by file; click a line or press Enter to go there.
- **Quick documentation**: F1 (or ⌃J) shows the documentation of the name at the caret.
- **Parameter info**: ⌘P inside a call shows the function's parameters, the current one in bold,
  and follows the caret through the arguments.
- **Back and Forward**: ⌘[ and ⌘], the arrows in the toolbar and the mouse's side buttons return to
  where you were, across files.

## 0.25.3 - 2026-10-18

### Fixed

- **The bottom tool window keeps its height** when you switch between the Terminal and Problems.

## 0.25.2 - 2026-10-18

### Fixed

- **AI agents get answers while the window is hidden**: with GoGarden minimised, hidden or covered
  by other windows, MCP calls such as `list_windows` and `dispatch_command` waited until the window
  showed again and failed after 30 seconds. They are now answered at once.

## 0.25.1 - 2026-10-18

### Fixed

- **A block opened from its `...` folds again**: imports, unfolded with a click on their dots, could
  not be folded back. Folding now moves a caret it would hide to the start of the block.
- **Escape closes the search bar** (⌘F) again.

### Changed

- The `...` of a folded block is as large as the imports' one, and the pointer is a hand over it and
  over the fold arrows.
- **The active editor tab stands out**: blue, with an outline.

## 0.25.0 - 2026-10-18

### Added

- **Go code is coloured by meaning**: gopls tells GoGarden what each name is, so
  package names are olive, types teal, and built-in types such as `error` look like keywords. The
  colours follow your typing without flickering.

## 0.24.1 - 2026-10-18

### Changed

- **Test files stand out in the project tree**: `*_test.go` and other test files sit on a
  green-tinted row.

## 0.24.0 - 2026-10-18

### Added

- **Code folding**: blocks in brackets that span lines and runs of comment lines fold. Arrows next to the line numbers fold and unfold; a folded block shows as `{…}` and Go's
  imports as `import ...`; click the mark to unfold. ⌘− folds the block at the caret, ⌘= unfolds,
  ⇧⌘− and ⇧⌘= fold and unfold everything. Moving the caret into a folded block opens it.
- **Imports start folded** when a file opens; Settings → Editor can turn that off.
- **Settings → Editor** with Code Folding and Font.

### Changed

- **Ligatures are off by default**: `!=` and `<-` show as typed, not as `≠` and `←`. Settings →
  Editor → Enable ligatures turns them back on.

## 0.23.2 - 2026-10-18

### Fixed

- **Double-click selects a word and triple-click the line** in the editor again: the second and third
  presses count as soon as the button goes down, so holding and dragging extends by words or lines.
- **The completion list scrolls with the mouse or trackpad**: it no longer jumps back to the
  selected item; ↑ and ↓ still bring the selection into view.

## 0.23.1 - 2026-10-18

### Fixed

- **Errors and completion in files with non-English text**: in a file with Cyrillic (or any
  non-ASCII) text, such as a Russian comment, error underlines appeared in the wrong place and
  completion took the wrong word and did not insert. Positions from gopls are now counted the way
  the editor counts them.

## 0.23.0 - 2026-10-18

### Added

- **Code completion for Go**, from gopls: a list opens under the caret as you type a name or a `.`,
  and with ⌃Space. Each item shows its kind and its type or signature, the letters you typed in
  bold, and the selected item's documentation. ↑ and ↓ choose, Enter inserts, Tab replaces the rest
  of the word, Escape closes. Functions come with their parentheses and the caret between them, and
  an item from a package that is not imported yet adds the import.

## 0.22.0 - 2026-10-18

### Added

- **GoGarden shows Go errors as you type**, from gopls: errors and warnings are underlined with a
  wavy line, marked on the right edge of the editor, and counted in its top right corner. Hover over
  an underline or a mark to read the message. The changes you make are sent to gopls at once, so
  the problems follow your typing, saved or not.
- **The Problems tool window** (⌘6, or its button on the left): the current file's problems, or
  every file's on the Project Errors tab; click one to go there. It shares the bottom of the window
  with the Terminal.
- **F2 and ⇧F2** go to the next and previous problem in the file.
- If gopls stops, GoGarden starts it again and says so; after three stops in three minutes it
  leaves it stopped and tells you why.
- AI agents read the problems with `problems.file.list` and `problems.project.list`, and the
  servers' state with `language.servers.status`.

## 0.21.0 - 2026-10-17

### Added

- **GoGarden finds Go and gopls** on your computer: Go SDKs from `GOROOT`, `PATH`, Homebrew,
  `/usr/local/go`, `~/sdk` and the toolchains `go` downloads, and gopls from `GOBIN`, `GOPATH/bin`
  and `PATH`.
- **Settings → Languages & Frameworks → Go**: choose the SDK (GOROOT) among those found, see gopls
  and its version, install or update gopls.
- **When gopls is missing**, a notification above a Go file offers to install it with
  `go install golang.org/x/tools/gopls@latest`, showing its progress; without Go, it links to
  go.dev/dl. gopls is what code completion, navigation and errors will come from.
- AI agents reach all of it through the `toolchain.go.*` and `toolchain.gopls.install` commands.

## 0.20.1 - 2026-10-17

### Changed

- **Updates and the plug-in catalog come over HTTPS** from ide.ifzenelse.space, with a TLS stack
  written entirely in Rust. Every download is still checked against the publisher's signature.

## 0.20.0 - 2026-10-17

### Added

- **The main menu** in the macOS menu bar: File, Edit (with Find and Multiple Carets),
  View (Tool Windows), Navigate, Code, Window (Editor Tabs) and Help, with the usual GoGarden
  application menu. Items show the shortcuts from your keymap, follow the interface language, and
  are greyed out when they cannot run (an editor action without an open file). Choosing an item
  does exactly what its shortcut does. More menus appear as their actions arrive.

## 0.19.1 - 2026-10-17

### Fixed

- **The hover is visible in menus and popups.** It was almost the colour of the menu itself. In the
  project menu the hovered project is now blue, as menu items are, and the current one grey; Search
  Everywhere, Recent Files, Find in Files, the Settings buttons and context menus have a lighter hover.

## 0.19.0 - 2026-10-17

### Added

- **Search Everywhere** (press ⇧ twice, or the magnifier in the toolbar): one popup
  with the tabs All, Files and Actions. Tab and ⇧Tab switch tabs. Go to File (⇧⌘O) opens it on Files.
- **Find Action** (⇧⌘A): find any action by name, in Russian or in English, by words in any order
  (`line dup`) or by letters (`dls`), with its shortcut; Enter runs it. Actions you used lately come
  first; actions that cannot run now (an editor action without an open file) are dimmed.
- Actions without a shortcut are in the keymap too, so you can give them one: Reload All from Disk
  (⌥⌘Y), Select All Matches, Indent Selection.
- AI agents find actions over MCP (`find_action`).

## 0.18.1 - 2026-10-17

### Fixed

- **The editor no longer scrolls past the end of a file**: the last line stops at the bottom of the
  view, instead of scrolling up to the top and leaving an empty editor.

## 0.18.0 - 2026-10-17

### Added

- **Settings → Keymap**: every action with its shortcuts, in groups, with search by name or by
  shortcut. Double-click an action (or use its context menu) to add a shortcut: press the keys; if
  another action has them, remove them from it or leave both. Remove a shortcut or reset an action
  to its defaults. Changed actions are shown in blue. Changes are saved in `keymap.json`.
- AI agents read and change the keymap over MCP (`get_keymap`, `set_keymap`).

### Changed

- Every shortcut of the window and the editor now comes from the keymap, matched exactly: ⇧⌘F no
  longer depends on being checked before ⌘F. Hints show the shortcuts you set.
- Tab in the editor indents the selected lines when a selection covers several lines; Escape with
  several carets keeps only the primary one. Both are commands (`editor.edit.tab`,
  `editor.caret.keepPrimary`).

## 0.17.4 - 2026-10-17

### Changed

- **Narrower gaps between the tool windows and the editor**: one gap between two panels instead of
  two.

## 0.17.3 - 2026-10-17

### Changed

- **A hairline under the editor tabs** separates them from the text, so that they no longer merge.

## 0.17.2 - 2026-10-17

### Changed

- **The splash is a picture of its own**: a small window without a frame or window
  buttons, in the middle of the screen, with GoGarden's mark, name and version, shown for at least
  two seconds while the projects open. The IDE window stays hidden behind it at its remembered
  size and place, then shows (and goes full screen if it was). The running bar is gone.

## 0.17.1 - 2026-10-17

### Changed

- **A loader at start and restart**: while GoGarden reopens its projects and takes its place on
  the screen, a picture in GoGarden's colours with its mark, name and a running bar covers the
  window, then fades out. A window that was full screen now starts at the screen's size, so it no
  longer shows small before macOS takes it full screen.

## 0.17.0 - 2026-10-17

### Added

- **Find in Files** (⇧⌘F) and **Replace in Files** (⇧⌘R): the query with Match Case,
  Words and Regex, a file mask (`*.go, !*_test.go`), the whole project or the selected folder; the
  matching lines with their file and line, a preview of the selected match, Enter to open it.
  Results appear as they are found; open files are searched with their unsaved changes. Replace
  All changes open files in the editor (one undo step per file) and the others on disk.
- AI agents search and replace across the project with the `search.text.*` commands.

## 0.16.0 - 2026-10-17

### Added

- **Recent Files** (⌘E): the files shown last, with the previous one selected, so
  that ⌘E and Enter go back to it; ⌘E again moves down the list, typing filters it. The Project
  and Terminal tool windows are on the left. The list is kept between sessions.

## 0.15.0 - 2026-10-17

### Added

- **Go to File** (⇧⌘O): type part of a name, letters in order (`edvi` finds
  `editor_view.go`), or a path with `/`; the best match is first, open files rank higher, ↑ and ↓
  choose, Enter opens. The project is indexed in the background, respecting `.gitignore`, and
  follows changes on disk; a hundred thousand files answer at once.
- AI agents find files with the `navigate.file.goTo` command.

## 0.14.1 - 2026-10-17

### Changed

- The project tree's spacing is new: the arrows, icons and names sit in their places, each level
  indents a little more, and the selection lines up with the tool window's title.
  Open folders keep the plain folder icon.

### Fixed

- Large images open without freezing the window: the tab opens at once with a loading indicator,
  and the picture appears when it is decoded. An image that cannot be decoded shows why.

## 0.14.0 - 2026-10-17

### Added

- **Go to Line:Column** (⌘L): type `line` or `line:column`; a click on the caret
  position in the status bar opens it too.
- **Files changed outside GoGarden show up at once**: the project tree lists new, renamed and
  removed files, and open files without unsaved changes reload (⌘Z brings the old text back).
  When a file with unsaved changes changes on disk, a notification offers Load File from Disk or
  Keep Memory Changes.

## 0.13.0 - 2026-10-17

### Added

- **Line actions**, at every caret: ⌘D duplicates the line or the selection, ⌘⌫
  deletes the line, ⌥⇧↑ and ⌥⇧↓ move lines up and down, ⌘/ comments and uncomments lines, ⌥⌘/
  wraps the selection in a block comment.
- Tab indents the selected lines and ⇧Tab unindents them; Go files use tabs.
- **Smart Enter** keeps the indentation, indents after `{`, `(` and `[`, and puts the closing
  bracket on its own line.
- **Paired brackets and quotes**: typing `(`, `[`, `{` or a quote adds the closing one (or wraps
  the selection), typing the closing one steps over it, and ⌫ between an empty pair deletes both.

### Changed

- ⌥⇧↑ and ⌥⇧↓ move lines instead of adding carets (add carets with ⌥⇧-click or ⌃G).

## 0.12.0 - 2026-10-17

### Added

- **Find and Replace in the file**: ⌘F opens the search bar (a selection becomes the
  query), ⌘R adds the replace row. Match Case, Words and Regex toggles; the match count; Enter and
  ⇧Enter (or ⌘G and ⇧⌘G) step through the matches, ⌥Enter selects them all; Replace and Replace
  All, with `$1` groups for regular expressions; Escape closes the bar.
- Matches are highlighted in the text and marked by the scroll bar.
- AI agents search and replace with the `editor.find.*` commands.

## 0.11.0 - 2026-10-17

### Added

- **Several carets**: ⌃G selects the word at the caret, then each next occurrence
  with a caret of its own; ⌃⇧G takes the last one back; ⌃⌘G selects every occurrence. ⌥⇧↑ and
  ⌥⇧↓ add a caret on the line above or below.
- **Column selection** by dragging with ⌥.
- Pasting as many lines as there are carets gives each caret its own line.

### Changed

- A caret is added or removed with ⌥⇧-click (not ⌥-click).

## 0.10.0 - 2026-10-17

### Changed

- **A new editor**, drawn by GoGarden itself instead of a generic text field: only the visible lines
  are laid out, so a 50 000-line file scrolls and pages smoothly; the line numbers stay in place
  while the text scrolls sideways; tabs are four columns wide.
- The caret blinks; the current line is highlighted; selections are drawn per line.
- Keyboard: ⌥←/⌥→ by words, ⌘←/⌘→ to the line start and end (the first press goes to the first
  character), ⌘↑/⌘↓ to the file's start and end, Page Up and Page Down, ⌥⌫ and ⌥⌦ delete words,
  ⌘A selects all; ⇧ with any of them selects.
- Mouse: a double click selects a word, a triple click the line, ⇧-click extends the selection,
  ⌥-click adds a caret; a click on a line number selects the line.
- Copy and Cut without a selection take the whole line. Input methods (Chinese,
  Japanese, accents) compose at the caret.
- Several carets (from ⌥-click or AI agents) are drawn and edit together; Escape keeps one.

## 0.9.3 - 2026-10-17

### Fixed

- In the project menu and on the Welcome screen, a project's path no longer runs below its row:
  rows fit their text, and a path too long for the row ends with "…".

## 0.9.2 - 2026-10-16

### Fixed

- A file closed and opened again after it changed on disk could show its old text until edited.
- An image opened after closing another could show the previous picture.
- Deleting and creating files can no longer reach outside the project through `..` in a path.
- Enter on a name that is taken keeps the cursor in the New File field, to fix the name.

### Changed

- The Delete dialog warns when open files with unsaved changes would be closed.

## 0.9.1 - 2026-10-16

### Added

- When the editor tabs do not fit, their row scrolls: with the mouse wheel or the trackpad over the
  tabs, with a thin scroll bar while it moves. The active tab is always brought into view, and a
  button at the right lists every tab.

## 0.9.0 - 2026-10-16

### Added

- **Select several files** in the Project tool window, as in the Finder: `⌘`-click
  adds or removes a file, `⇧`-click selects everything between, `⌘⇧`-click adds a range, `⇧↑` and
  `⇧↓` grow or shrink the selection, `⌘A` selects every row shown. Selected neighbours join into
  one block.
- **Delete…**, `⌫` and `⌘C` act on every selected file and folder.
- The keyboard in the project tree: `↑` and `↓` move, `→` and `←` unfold and fold (or go to the
  parent folder), `Enter` opens a file or unfolds a folder.
- AI agents read and set the selection with the `get_tree_selection` and `select_in_tree` tools.

### Changed

- A right click on a selected file keeps the selection, so the menu acts on all of it.

## 0.8.3 - 2026-10-16

### Added

- **Delete** files and folders: **Delete…** in the Project tool window's context menu, or `⌫` on the
  selected item. After you confirm, it goes to the Trash, from where it can be restored, and its
  tabs close.
- AI agents delete files with the `file.fs.delete` command.

## 0.8.2 - 2026-10-16

### Changed

- In the image viewer, a trackpad moves the picture in any direction, also diagonally; text still
  scrolls along one axis at a time.

## 0.8.1 - 2026-10-16

### Changed

- In the image viewer, a picture larger than the window shows a hand: open over the picture,
  closed while dragging it.

### Fixed

- Dragging a large picture follows the pointer at once instead of lagging behind it.

## 0.8.0 - 2026-10-16

### Added

- Images open in an image viewer: PNG, JPEG, GIF, WebP, BMP, ICO and SVG, over a chessboard that
  shows transparency, with Zoom In (⌘=), Zoom Out (⌘−), Actual Size (⌘/), Fit Zoom to Window,
  ⌘-scroll and pinch zoom, and the image's size, format, colour depth and file size.
- AI agents zoom and inspect images with the `editor.image.zoom` and `editor.image.info` commands.

### Changed

- The New File, New Directory and Paste Image dialogs are redesigned: an icon and title, a framed
  name field with the name before the extension selected, the target folder, a warning when the name
  is taken, a preview of a pasted image, and Cancel and Create buttons.

## 0.7.4 - 2026-10-16

### Fixed

- Smooth scrolling in large files: highlighting and layout are reused until the text changes
  (a 700 KB YAML file scrolls at the display's full rate instead of half of it).

## 0.7.3 - 2026-10-16

### Fixed

- Enter in the Paste Image, New File and New Directory dialogs creates the file; the dialogs also
  have OK and Cancel buttons.

## 0.7.2 - 2026-10-16

### Changed

- Ignore files (`.gitignore`, `.dockerignore`, …) show a crossed circle, and `.gitlab-ci.yml` the
  GitLab logo.

## 0.7.1 - 2026-10-16

### Changed

- **Wide scroll bars that hide:** they show while you scroll and fade out a moment after; hover a bar to grab it.

## 0.7.0 - 2026-10-16

### Added

- **Popular plugins come built in and on:** Markdown, YAML, JSON, TOML, Docker (Dockerfiles and
  Compose files), GitLab CI, Ignore files, Dotenv and Shell scripts. Switch any of them off in
  Settings → Plugins; newer versions from the Marketplace replace them.

## 0.6.4 - 2026-10-16

### Added

- Symbolic links show a small arrow on their icon in the project tree, as in the Finder; a link to a
  folder opens like a folder.

## 0.6.3 - 2026-10-16

### Added

- Plugins can support files by name as well as by extension (`.env.local`, `Dockerfile`,
  `.gitlab-ci.yml`), and a plugin for a file's name wins over one for its extension.

## 0.6.2 - 2026-10-16

### Added

- **Paste images:** ⌘V in the project tree with a screenshot or a copied picture on the clipboard
  saves it as a PNG file, named in a dialog (`image.png` by default).

## 0.6.1 - 2026-10-16

### Changed

- The Markdown file icon is the familiar blue M↓ mark again, without a frame.

## 0.6.0 - 2026-10-16

### Added

- **Update notifications:** when a new version is out, GoGarden says so and can update and restart
  itself, with the release notes a click away.
- **Paste files copied in the Finder** with ⌘V into a folder of the project tree; copying in the
  tree puts the file on the clipboard for the Finder too.

### Fixed

- After an update through Zen Toolbox, GoGarden restarts into the new version even when its window
  is hidden or in another full-screen space.
- Plugin descriptions no longer run under the Install button.

## 0.5.1 - 2026-10-16

### Fixed

- Scrolling sticks to one direction: once a gesture starts vertically or horizontally, the other
  direction waits until it ends, so files no longer wobble on a trackpad.

## 0.5.0 - 2026-10-16

### Added

- **Language** in Settings → Appearance & Behavior: as in the system, English or Russian, applied
  at once.
- **Plugins** in Settings: browse the Marketplace, install, update and uninstall plugins.
- A new **Ignore files** plugin highlights `.gitignore`, `.dockerignore` and other ignore files.

## 0.4.1 - 2026-10-16

### Changed

- **Hints:** a dark plate beside each button with its name and shortcut
  (`Terminal ⌥F12`); buttons that do not work yet say "In development".

### Fixed

- The close button of an editor tab can be clicked: every tab keeps room for it, and it stays while
  the pointer is over the tab.

## 0.4.0 - 2026-10-16

### Added

- **Comes back as you left it:** open files, folders, the Terminal with its tabs, the project tabs,
  and the window in full screen or at its size and place.
- **go.mod and go.sum are highlighted** out of the box: directives, versions, comments and hashes.
- **Test files** have their own icon (`*_test.go` and the test files of other languages).
- **Plug-ins update themselves** when a newer version is published.

### Changed

- Calmer greens and Markdown colours: pink italic headings, green code.

## 0.3.5 - 2026-10-16

### Added

- **Restarts itself after an update:** when Zen Toolbox installs a new version, GoGarden saves your
  changes and reopens your projects in the new version.

### Fixed

- The Dock shows the current application icon even when macOS still caches an older one.

## 0.3.4 - 2026-10-16

### Changed

- **A new look:** panels are darker than the window around
  them, the main toolbar glows in the project's colour, active tool window buttons are blue, and
  popups float a step lighter.

## 0.3.3 - 2026-10-16

### Changed

- **Projects open as tabs** of the main window, under the toolbar; the window's close button closes
  the active tab. Settings → Appearance & Behavior can open projects in new windows instead.
- New pastel application icons.

## 0.3.2 - 2026-10-16

### Changed

- Calmer file icons: language colours are muted and folders are grey, with a small mark for
  `src`, `tests`, `docs` and similar; build output stays orange.

## 0.3.1 - 2026-10-16

### Added

- **Settings** (⌘, or the gear): Tools → MCP Server turns on access for AI agents, with
  ready-to-copy commands for Claude Code.
- **`gogarden mcp`**: connects agents that start MCP servers as programs, and starts GoGarden when
  needed.

### Changed

- Files open with a double click in the project tree; a click selects.
- The Terminal spans the whole window under the Project tool window and the editor.

## 0.3.0 - 2026-10-16

### Added

- **Welcome window**: search your projects, see their branches, remove them
  from the list. Closing the last project window brings it back.
- **Projects menu** in the toolbar: switch between open projects or open a recent one.
- **Terminal** (⌥F12): your shell in the project folder, in tabs.
- **New files and folders** from the project tree's context menu; **paste** copied paths and
  **drop files** from the Finder onto the tree.
- **File icons** for Go, Rust, Python, web, config and many other file types.
- **AI agents** can drive GoGarden over MCP: start it with `ZEN_AUTOMATION=1` and connect to
  `http://127.0.0.1:63530/mcp`.
- The application icon in the toolbar; buttons that do not work yet say "In development".

### Fixed

- Opening a second project no longer freezes the first window.

## 0.2.0 - 2026-10-16

### Added

- **One window per project.** Open several projects at once; each gets its own window.
- **Projects reopen.** The projects that were open when you quit open again at the next start, and
  the welcome screen lists recent projects.
- **The new window layout:** the main toolbar in the title bar with the project and branch, tool
  window stripes on both sides, rounded panels, outlined tabs and the file path in the status bar.
  Build output such as `target` is shown as excluded.
- **Plug-ins.** When you open a file GoGarden does not support, it offers a plug-in for it. The
  first one is **Markdown**.
- Go constants are highlighted wherever they are used.

## 0.1.1 - 2026-10-16

The new GoGarden icon.

## 0.1.0 - 2026-10-16

The first GoGarden: project tree, tabs, an editor with Go highlighting, save, undo and redo, in English and Russian. No code intelligence yet.
