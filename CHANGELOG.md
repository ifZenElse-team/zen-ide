# Changelog

All notable changes to this repository are documented in this file. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the IDEs are released together under
one calendar version `YY.M.N` (ADR-0146).

Product and plug-in releases, with the notes users see as "What's new", are in
[changelog/](changelog/README.md), one file each.

## [Unreleased]

Changes not released yet are not written here: each pull request adds its notes as a file of
[changelog/unreleased/](changelog/unreleased/README.md) (ADR-0188), and `cargo xtask changelog
gather <YY.M.N>` moves them into the release's section below and into the products' changelogs.

## [26.11.10] - 2026-11-27

### Added

- `cargo xtask changelog preview` prints the notes waiting in `changelog/unreleased/`, by product and
  section, and a workflow writes them into the draft "Next release" on GitHub after every merge into
  `main`, so what the next release brings can be read at any time.
- `cargo xtask release-build <ide|toolbox> <version> --trial [--platform ...]` builds one product's
  release artefact without the release checks (the source check, the quality gate, the version
  check and the release notes); its `build.json` says the gate was skipped, so `release-publish`
  refuses it. The workflow `.github/workflows/release-build.yml`, started by hand, runs it on
  GitHub-hosted runners for macOS (Apple silicon), Linux and Windows with no secret and keeps the
  results as workflow artefacts for three days (docs/releasing.md).
- The workflow `.github/workflows/release.yml` builds the release of a tag `v<YY.M.N>` on
  GitHub-hosted runners and publishes nothing yet (ADR-0300, step 3): a plan, the full gate once on
  macOS, the application bundles in three macOS jobs, one Linux job for every other product and
  platform, and a join that keeps the whole release as one workflow artefact. New steps of
  `cargo xtask release-ci`: `matrix`, `gate`, `part <ide|toolbox|plugins> <platform>` and `join`.
  A part records the gate as `deferred` in `build.json`; `join` takes it only when its files match,
  its commit, tree and compiler are those of the gate's pass and the parts are exactly the release,
  and only then records `passed` (docs/releasing.md, "Releasing from a tag on GitHub").
- The workflow `release.yml` can stage, verify and release a tag from GitHub (ADR-0300, steps 4
  to 6), once the repository variable `ZEN_RELEASE_FROM_GITHUB` is `true` and the environment
  `release` holds the secrets: `stage` and `live` in that environment, the secrets given only to
  the step that signs or sends; `verify`; a draft GitHub release with the notes, the files and the
  checks, published after the release is live. Without the variable a tag's run still ends with
  the built release.
- `cargo xtask release-ci credentials` and the hand-started workflow `release-credentials.yml`
  check the signing key and the upload token against the release server, sending nothing.
- `cargo xtask release-ci notes` writes the text of the GitHub release from a stage.
- `cargo xtask release-publish --run <run id>` publishes a release built by the workflow from a
  maintainer's machine, fetched with `gh run download`; `--pipeline` and `--job` keep fetching
  from GitLab (docs/releasing.md, "Releasing from a tag on GitHub").

### Changed

- The release tooling reads what it needs from CI in one place, `xtask/src/ci_host.rs`, for GitLab
  CI and GitHub Actions: the tag, the run's number for the stage's name, whether the ref is
  protected. The two rules that held only in GitLab now hold on GitHub too: the release secrets are
  refused on a ref that is not protected and in every pull request's run, and `release-ci` releases
  only its run's own tag. A CI that is neither host is refused the secrets and the release.
  Nothing changes in GitLab CI or on a maintainer's machine (ADR-0300).

## [26.11.9] - 2026-11-17

### Added

- ADR-0294: the login shell's environment for every program the IDE starts. A new L0
  crate, `zen-env`, holds it.
- `ide.environment.get`, the setting `environment.loginShell`, and `source` on a plug-in tool
  window's choice (`ChoiceInfo.source`).
- ADR-0292: a plug-in tool window declares an icon of its own as checked, monochrome
  SVG markup and a default side.
- ADR-0296 extends plug-in run targets:
  - `${window.<window>.<choice>}` values from the plug-in's tool window (`choiceNeeded`);
  - `"targetFiles"` in `plugin.json`;
  - folder targets, in `run.targets.list` and the Project view's **Run Targets** menu slot;
  - `"output": "diff"`, read into the diff viewer (`zen_workbench::plugin_diff`).
- `zen-plugin-kubernetes` exports `zen_run_targets` and declares `targetFiles` and `personOnly`.
- ADR-0299:
  - `"windowValues"` lets a tool window use another plug-in's window choice, read-only, dropping
    rows listed under an old value;
  - an index section's `"state"` listings lay a cluster state over its nodes
    (`plugins.toolWindow.rows` answers `cluster`, `zen_workbench::plugin_tools::state`);
  - the index view gets state dots, a details pane and row actions.
- `zen-plugin-gitops` exports `zen_tool_rows` for Flux and Argo CD state and declares `kubectl`
  with its person-only subcommands.
- Project indexes in the editor (ADR-0293): `zen_workbench::project_index` adds an index's problems
  to `problems.file.list` and the editor while the file is the indexed text, answers
  `navigate.declaration.goTo` from its references before the language server (a folder is shown in
  the Project tool window), and `navigate.usages.find` with a Find tab of the references to a node.
  The GitOps plug-in adds each object's name as a reference to itself.
- Plug-in project indexes (ADR-0293):
  - plugin.json's `"index"` and the ABI v1 export `zen_index` (`zen_syntax::ProjectIndex`,
    `IndexDeclaration`);
  - `zen_workbench::project_index`: the background, bounded and cancellable walk of the project's
    matching files (with the project's Git remote URLs from `zen_vcs`), a fresh index 500 ms after
    a change, and the checks of what a plug-in answers;
  - `plugins.index.get` and `plugins.index.refresh`;
  - the manifest field carried by the catalog (`zen-releases --index`) and the bundled manifest.
- `plugins/gitops`, the GitOps plug-in (ADR-0293): a YAML reader of its own, the Flux, Argo CD,
  Kustomize and Helm model, and the `zen_index` export; bundled with DevopsDaruma.
- Index sections in plug-in tool windows (ADR-0293): `"rows": "index"` (`zen_syntax::INDEX_ROWS`),
  answered by `plugins.toolWindow.rows` from the project index with no program and no trust
  needed, and drawn by `ide::plugin_index_view` as a tree and a layered graph. The GitOps plug-in
  declares its window with an icon of its own.
- ADR-0295: typed inputs (`count`, `port`, `forward`) for plug-in tool window actions,
  as whole arguments, entered by the person only.
- ADR-0297 extends plug-in indexes and run targets:
  - `"inGraph": false` on an index node keeps it out of index tool windows;
  - an index's `completions`, offered by `editor.completion.start` after their prefix in the files
    they are for (`zen_syntax::IndexCompletion`);
  - `"output": "document"` with `"documentType"` on a run target: the standard output opens as a
    read-only tab outside the project (`zen_workbench::plugin_document`, at most 16 MiB), captured
    with `zen_run::RunOutput::capture_stdout`.
- `zen-plugin-gitops` reads Helm values and templates and Kustomize patches, answers index
  completions and nodes outside the graph, and exports `zen_run_targets` with `"runs": ["helm",
  "kustomize"]`, `targetFiles` and `"output": "document"` (ADR-0297).
- ADR-0298: a Go project's environment, its two layers, its order against the login shell and run
  configurations, and the trust it needs. `go.env.get`, `go.env.set` (people only) and
  `go.env.settings`; `zen_lsp::ServerTools::project_environment` and `zen_project::trust`.

### Fixed

- `plugins/kubernetes` (0.1.1) and `plugins/shell` (0.2.1) answer `zen_abi` 1: with 0 the host
  never looks up their ABI v1 exports (`zen_tool_rows`, `zen_run_targets`). A test of
  `zen-plugins` now refuses any plug-in that defines an ABI v1 export and answers 0.
- The terminal's selection test gives egui a frame clock of its own instead of the wall clock, so a
  slow machine no longer splits its double-click into two single clicks.
- The test of Enter in the Git popups waits for Update Project and Checkout to end before it looks
  at the branch: both run in the background.
- The zen-db test of a connection lost while a statement runs no longer races a 50 ms pause: it
  holds the test server until the statement has reached it, drops the connection, then lets it go.
- Release uploads (artifacts and the staged website) are tried again up to three times after a
  broken pipe, a reset connection, a timeout or a 502–504 answer, with time limits on every upload,
  and a file the server already has with the same SHA-256 is not sent again.
- Release uploads no longer fail on a slow line: the body's time limit assumes 16 KiB/s with a
  10-minute floor (it assumed 256 KiB/s, which a 73 MB file over a 145 KB/s line shared three ways
  cannot meet), files over 20 MB go one at a time, and `ZEN_RELEASES_UPLOAD_PARALLEL` and
  `ZEN_RELEASES_MIN_RATE` override both. A socket error `EINVAL` after the server drops the
  connection is tried again too.

## [26.11.8] - 2026-11-14

### Changed

- SqlScroll and DevopsDaruma are released with the other IDEs from 26.11.8 (the owner,
  2026-11-14): `release::IDES` has all six apps (`UNRELEASED_IDES` is empty), so `release-build
  ides`, `release-publish` and the release pipeline build and publish them on macOS, Linux and
  Windows; `changelog/sqlscroll.md` and `changelog/devopsdaruma.md` start, and the changelog check
  compares an IDE's releases with the others' from its first one. `cargo xtask release-ci plan`
  names the IDEs it builds.

## [26.11.7] - 2026-11-14

### Added

- `run.process.rerun` and `run.tests.rerunFailed` take `waitMs` and answer the new run's summary, as
  `run.process.start` does; the MCP server's instructions list them among the main commands.
- The Shell Script plug-in exports `zen_run_targets` and declares `"runs": ["sh", "bash", "zsh", "ksh"]`
  (ADR-0286).
- ADR-0284 (accepted by the owner): Java to Kotlin conversion is our own, in the Kotlin plug-in. The optional
  plug-in exports `zen_convert` and `zen_convert_from` (`TextFeatures::convert`,
  `Feature::Conversion`), the commands `file.convert.toLanguage` and `editor.convert.text`, the
  keymap action `file.convert.toLanguage/kotlin`.
- `zen_run::ClasspathAgent` (`Debuggee::agent`): an agent jar found by its name's prefix among the
  entries of the classpath file the run's build wrote, inserted when the debuggee starts; the
  `trackCoroutines` option of `java.application`.
- `zen_workbench::java::coroutines`: the `coroutineDebug` option of `java.application`
  configurations (`default`, `on`, `off`), the `coroutineDebugMode` setting
  (`debugger.kotlinCoroutines.debugMode`), and the flag added after the debugger agent in
  `debug::start` only.
- `debug.coroutines.list`: the program's coroutines from one fixed read-only dump, evaluated in the
  first of the stopped thread's top 8 frames that can run it, never after Pause; `waitMs` 0 answers
  `pending` at once.
- `zen_terminal::host::record`: the host keeps a private record per session
  (`<data>/terminal/sessions/<host>-<id>.json`, `0600` in a `0700` folder, at most 2,000 lines and
  1 MiB of text, written every 2 s when changed, removed when the session ends) with its folder,
  given environment, last shell command (read when Enter is typed while the shell is the
  terminal's foreground process group) and the foreground program's arguments. `OpenRequest.keep`
  and `preface`, the `keep` message, `Session::set_keep`, `Session::lost_host`; the commands
  `terminal.session.rerun` (refused to agents, `userOnly`) and `terminal.session.dismissRerun`;
  the `terminalRestore` setting (`terminal.restore` in the session).
- Agent resume adapters (ADR-0275, point 9): `zen_workbench::terminal::agents` (`AgentAdapter`,
  `terminal.agent.list`, `terminal.agent.report`, the `terminalAgents` setting), the plug-in
  manifest's `terminalAgents` (catalog, installed plug-ins, `zen-releases --terminal-agents`),
  report files in `<data>/terminal/agents/` (private folder, names checked, at most 64 KiB), and
  `ZEN_TERMINAL_ID` / `ZEN_TERMINAL_AGENTS` in every terminal's environment. Adapters are the
  person's: `set_settings` refuses any change of `terminalAgents` to agents, `install_plugin`
  refuses a plug-in that declares `terminalAgents` as it does one with driver programs, so do
  `set_plugin_enabled` and `plugins.plugin.enable` for such a plug-in while it is off, and
  `terminal.agent.report` takes only an id for an adapter that exists.
- ADR-0275: terminal sessions that outlive the IDE (a detached PTY host per data folder, its socket
  and protocol versions, a versioned copy outside the bundle, a screen snapshot on reattach, an MCP
  bridge that reconnects), tabs restored after a reboot, and agent resume adapters contributed by
  plug-ins, with no agent built into the platform.
- `"personOnly"` in a plug-in's `plugin.json` (ADR-0291): `zen_syntax::PersonOnly`,
  carried by the catalog (`zen-releases --person-only`) and the bundled manifest; `Runs::prepare`
  refuses such a run to agents (`userOnly`) and holds a person's start as a question
  (`personConfirmation`) that the window answers with a one-time token; agents' keys and window
  actions are refused while it asks or runs. The Terraform plug-in's `zen_run_targets`.
- `plugins/terraform`: the Terraform plug-in for every IDE (`.tf`, `.tfvars`, `.hcl`,
  `.terraformrc`): highlighting, folding and Structure from its own HCL lexer, and terraform-ls
  declared as its language server with archives for macOS, Linux and Windows. `zen-lsp` finds a
  downloaded server with a plain command (`terraform-ls`) in the server's own folder.
- ADR-0290: a plug-in tool window's header `choices`, `${window.<id>}` arguments, and
  `confirm` questions on change and destroy actions; `plugins.toolWindow.choose` (person only),
  `choiceNeeded`, and the choices in `plugins.toolWindow.rows`.
- `plugins/kubernetes` (`zen-plugin-kubernetes`), with `"runs": ["kubectl"]`.
- ADR-0280: the clean-room process. Observers write a behaviour specification in our own words;
  implementers build only from it. Observation records stay in a private archive outside the
  repository; the default keymap, action names and import features stay compatible; icons, texts,
  colours and layout are our own. It supersedes ADR-0015 and an earlier record, and amends ADR-0002, ADR-0016
  and ADR-0057.
- `zen_terminal::host`: the host copy is private (`0700` folders, `0500` file), named by the
  SHA-256 of the build, checked again before it runs and written atomically; unused copies are
  removed under a start lock. Each protocol has its own socket, lock and `host-<protocol>.json`
  (with the host's `program` and `instance`); `hello` carries the host's instance, so a saved
  session is never attached to another host's session of the same number. `Speaks`,
  `serve_with`, `Session::elsewhere`, `Session::hosted`, `Error::Incompatible`; the saved layout
  keeps `{id, protocol, host}` per pane; `terminal.session.list` gives `elsewhere`.
- ADR-0282 and `cargo xtask notices`: third-party notices generated from `Cargo.lock` and the crates'
  licence files into every release artefact (a `Licenses` folder, `/usr/share/doc/<package>/copyright`
  in a `.deb`, a `zen-licences` section in a plug-in module, `--licences` in a driver program); the
  build refuses an artefact without them, and a test fails when a shipped crate has no licence text
  (`licenses/crates.json`, docs/licences.md).
- `apps/zen-pty-host` and `zen_terminal::host`: the PTY host of ADR-0275, its Unix socket in
  `<data>/terminal/` and protocol 1; `Session::open`, `Session::attach`, `Session::close`; the
  release bundle carries `zen-pty-host` next to the IDE.
- `apps/sql-scroll`: SqlScroll, the IDE for SQL and databases (ADR-0276). The Database workspace is on
  and the `sql` plug-in is bundled; Go and Rust are off. Not released yet: it is in
  `release::UNRELEASED_IDES` until the owner releases it.
- ADR-0286: plug-ins offer run targets through a `zen_run_targets` export, and the
  platform runs them as configurations of one kind, `plugin`; agents are refused by default, per
  plug-in.
- `zen_i18n::Message`: a message kept by its id and arguments and worded when it is read.
- ADR-0289: plug-ins declare tool windows over the programs they declare
  (`"toolWindows"` in `plugin.json`) and read their rows through a `zen_tool_rows` export; the
  platform runs the listings and actions through the program's command line, never a socket, under
  the trust rules and the plug-in's agent setting, and destroying actions ask the person every time.
- `cargo xtask legal`, a new step of `cargo xtask ci`: a change may not add lines that name other
  products or claim parity with them, images outside the folders of our own pictures,
  file names with product names, or what looks like a secret, and every commit needs its
  `Signed-off-by` (ADR-0280, AGENTS.md golden rule 14). Import and compatibility features, the legal
  notices and accepted ADRs are on an allowlist with reasons; the rest of the tree is only reported
  until the cleanup is merged, and `cargo xtask legal --all` lists it.
- `docs/network.md` and the website's privacy page (`/privacy/`): no telemetry, and every network
  call of the IDEs and Zen Toolbox with its code and the setting that turns it off; the footer
  links it. ATTRIBUTION.md lists the data used at run time.
- `apps/devops-daruma`: DevopsDaruma, the IDE for DevOps (ADR-0285): product id and launcher
  `devopsdaruma`, bundle id `space.ifzenelse.devopsdaruma`, MCP port 63535, its tile and Kozō. It
  ships the base set with every plug-in on (Docker, YAML, GitLab CI, shell, `.env` and ignore files,
  TOML, JSON) and, with the Database workspace on, SQL; Go and Rust are off. Not released yet: it is
  in `release::UNRELEASED_IDES` until the DevOps work is done and the owner releases it. The
  guide's DevOps page describes it.

### Changed

- Test fixtures of the colour scheme, UI theme and VS Code theme importers say they are synthetic, the
  docs name the sources of the colour scheme and code style readers, and folding tests use an
  invented copyright holder.
- `java.bytecode.show` takes `.kt` files, finding the class through the Kotlin plug-in's JVM reader;
  the keymap action `java.bytecode.show/kotlin` gives it the name Show Kotlin Bytecode.
- AGENTS.md golden rules 2 and 8 follow the clean-room process of ADR-0280: code comes only from our
  own behaviour specification, and the UI follows that specification instead of another product.
  A new golden rule 14 forbids naming other products as references or comparisons anywhere in the
  repository and on the board, except for import and compatibility features and the legal notices;
  CONTRIBUTING.md and docs/ai-workflow.md say the same.
- DevopsDaruma bundles the Terraform plug-in (`PRODUCT_PLUGINS`), beside the base set and SQL.
- DevopsDaruma bundles the Kubernetes plug-in (`PRODUCT_PLUGINS`), beside the base set, SQL and
  Terraform.
- `docs/git-workspace.md`, `docs/database-workspace.md` and `docs/behaviour/ide/database.md` are
  specifications of our own Git and Database workspaces: the feature maps of other products, their
  links and the observation notes are gone, and the layout is specified in our own words. The
  guide's Database page and the changelogs no longer compare the workspaces with other products;
  importing DBeaver connections stays documented.
- Code, tests and command descriptions describe our behaviour instead of naming another product:
  test names and assert messages no longer say "as in the reference", the spelling module that
  reads project dictionaries is `zen_spell::idea_dictionary`, and three types are renamed
  (`ContextAction`, `FormatCode`, `EverywherePopup`).
- The user guide and the release notes describe what each IDE does on its own: comparisons with
  other products are removed, and product names stay only where a feature imports from or works
  side by side with them.
- `docs/behaviour.md` and the notes under `docs/behaviour/` describe the interface we build as our
  own specification: product comparisons, observation provenance, other vendors' internal names and
  settings-file details, and long quotes are gone; behaviour, metrics, menus and shortcuts stay.
- The README, CONTRIBUTING, ROADMAP and the documentation describe Zen IDE on its own: comparisons
  with other products are removed, and the README says that Zen IDE is not affiliated with any IDE
  vendor. The icon set's design file key is no longer in the repository.
- The README and the website say that Zen IDE is a non-profit project and that commercial use is
  allowed under its licences.

### Removed

- The reference screenshots and the menu dump under `docs/behaviour/reference/` and
  `docs/behaviour/toolbox/reference/`, and the links to them: the repository keeps only our own
  images.

## [26.11.4] - 2026-11-09

### Added

- `editor.tab.move { group?, index?, to }` and `project.tab.move { to }`.

## [26.11.3] - 2026-11-07

### Added

- `refactor.extract.variable`, `refactor.extract.constant`, `refactor.extract.field`, `refactor.extract.parameter`, `refactor.extract.method` and `refactor.inline.apply`; snippet text edits from servers go in as plain text, and the client declares `experimental.snippetTextEdit` (ADR-0219).
- `zen_run::test_support`: the `TestSupport` provider interface for test naming, frameworks and test
  generation, built-in providers for Go and Rust, and the plug-in manifest key `tests` (ADR-0209).
- Commands `navigate.test.goTo`, `navigate.test.options`, `navigate.test.create`, `run.history.list`
  and `run.history.open`.
- `inspect.profile.*`, `inspect.cleanup.run` and `inspect.cleanup.undo`, ADR-0258; a language server
  declares `inspectionSettings` and which inspections are `off` by default.
- `inspect.suppress.list`, `inspect.suppress.apply` and `inspect.results.suppress`, ADR-0269; a
  language server declares `suppression`.
- ADR-0227: JaCoCo for Maven and Gradle runs; command `java.jacoco.download`.
- ADR-0263: keys from `press_keys` count as the agent.
- ADR-0208: one coverage model in `zen-run`, read by the run providers through
  `RunProvider::coverage`; commands `coverage.*`; theme keys `editor.coverage.covered`,
  `editor.coverage.partly` and `editor.coverage.notCovered`.
- ADR-0225: math in the Markdown preview through `latex-rust` (MIT OR Apache-2.0) and its embedded
  STIX Two Math (SIL OFL 1.1), behind `zen_workbench::markdown::math`; `zen_image::render_svg`
  renders an SVG at a given scale.
- `project.file.copyPath` takes `form`.
- Commands `editor.tab.pin`, `editor.tab.next`, `editor.tab.previous`, `file.document.reopenClosed`,
  `file.document.closeAllButPinned`, `.closeLeft`, `.closeRight`, `.closeReadOnly` and
  `editor.split.moveToOpposite`; `EditorGroup.pinned`; the project layout keeps `closedTabs`.
- The before-launch task type `fileWatchers` (`{ watcher, files }`), the field kind `fileWatcher`
  of `run.beforeLaunch.types`, and `Started::Wait` for task types that wait on the session.
- The `javaPullUp`, `javaPushDown`, `javaExtractSuperclass`, `javaExtractInterface`, `javaEncapsulateFields` and `javaIntroduceParameterObject` refactoring providers, through JDT LS's `java/checkExtractInterfaceStatus` and `java/getRefactorEdit` for Extract Interface and planned from a Java member scanner with the server's type hierarchy, references and implementations for the others; table columns have kinds (`text`, `checkbox`, `label`) and tables can be `fixed`; the window actions `view.pullMembersUp.open`, `view.pushMembersDown.open`, `view.encapsulateFields.open`, `view.extract.parameterObject`, `view.extract.interface` and `view.extract.superclass` (ADR-0232).
- `zen_workbench::nodejs`: the kinds `node` and `node.test`, a DAP adapter over the DevTools protocol
  that is a thread of the IDE (`AdapterLine::builtin`) and reporters for Jest and Vitest
  ([ADR-0268](docs/adr/0268-npm-and-nodejs.md)).
- `view.overrideMethods.open` and `view.implementMethods.open`; a generator's `action` in
  `editor.generate.list`; in a server declaration's `generate.prompts`, `match`, `name` and `action`
  per prompt, `when` per member list and `!` for an unset pointer (ADR-0226). The Java plug-in
  0.3.0 declares the Eclipse JDT Language Server's prompt-based generators and turns on their
  extended client capabilities.
- `zen_vfs::replace_unchanged` and `zen_workbench::disk::edit_file`, for code that rewrites project files (ADR-0238).
- Commands `java.bytecode.show`, `java.jshell.open|restart|stop` and `kotlin.repl.open|restart|stop`;
  run kind `kotlin.file`; `VirtualKind::Bytecode` (ADR-0230).
- ADR-0231: a debug adapter declares how it switches between the elements view and the raw fields (`AdapterLine.views`); java-debug's is its `vscode.java.updateDebugSettings` command in Eclipse JDT LS.
- ADR-0233: build triggers around the IDE's builds and keymap actions that carry parameters.
- The `springBoot.properties.list`, `springBoot.applications.list` and `springBoot.endpoints.*`
  commands, `editor.lineMarkers.list` and `editor.lineMarkers.navigate`, the `view.toolWindow.endpoints`
  action, the run kind `springBoot`, the hooks `RunProvider::plugin()`, `RunProvider::replaces()` and
  `Inspector::plugin()`, and `zen_update::fetch::conditional` (ADR-0217).
- ADR-0235: the dependency analyzer; `build.dependencies.analyze`, `build.dependencies.declaration` and `build.dependencies.exclude`.
- ADR-0252: editing the starters of an existing project.
- `plugin.json`'s `liveTemplates` takes `surrounders`, texts per language
  (`{"en": …, "ru": …}`), thrown types and fields in `docComments`, and method declarations in
  `statements`; ADR-0223. The release server keeps them once it runs this `zen-releases`.
- Code style declarations take an Eclipse formatter profile or an `.editorconfig` section as the configuration file (`format`, `section`), hand its path to a server setting (`location`), write one option as several keys with mapped values (`moreConfigKeys`, `configValues`) and send a text option as a list (`separator`); ADR-0228.
- ADR-0246: file encodings, and `.idea/encodings.xml` as the second file the IDE writes in `.idea`.
- ADR-0264: Go templates and embedded files are in the Go module; `zen_modules::file_references`.
- Plug-in ABI v1 exports `zen_inlays` (inlay hints from a plug-in) and `zen_key_references` (string
  literals that name a plug-in's keys, completed and navigated by the platform); the
  `properties.bundle.*` commands and the `view.resourceBundle.*` actions (ADR-0214).
- ADR-0273: one profile model in `zen-run`, a pure Rust pprof parser, and `RunProvider::profiler`.
- `refactor.safeDelete.find`, `refactor.safeDelete.apply` and `refactor.safeDelete.cancel`; `refactor.dialog.list`, `refactor.dialog.form`, `refactor.dialog.preview`, `refactor.dialog.apply` and `refactor.dialog.cancel` with the `RefactoringProvider` trait that platform and plug-in refactorings implement; the window actions `view.safeDelete.open` and `view.refactoring.open` (ADR-0222).
- `set_settings` takes `editorTabs`; `file.document.open` takes `preview`; `editor.tab.keepOpen`;
  `EditorGroup.preview`; `view.settings.open` with the page `editorTabs`.
- A plug-in's `tests` declaration takes `keywords`, words its `members` expression must not take
  for a member, and a member's name in backticks is read without them (ADR-0224).
- `Arranger::before_reformat`, a language's own step before Reformat Code, and `Insertion::end`, a
  generator that replaces text instead of only inserting it.
- The trust store in `zen-net` (ADR-0257).
- `npm.project.get`, `npm.script.run`, `npm.dependencies.install`, `npm.options.set` and
  `view.toolWindow.npm`; `zen_workbench::npm::model::NpmModel`, the project model of `package.json`
  ([ADR-0268](docs/adr/0268-npm-and-nodejs.md)).
- `project.trustedLocations.get` and `project.trustedLocations.set`, the keymap action
  `view.project.trust`, `safeMode` and the popup `trustProject` in `get_window_state`, and
  [ADR-0248](docs/adr/0248-safe-mode.md).
- `set_settings` takes `todo`; `todo.items.list` and `todo.scan.start` take `filter`; commands
  `todo.document.ranges` and `todo.commit.check`; action `view.todo.filter`; theme key `todo`.
- `zen-net`, the crate every HTTP agent of the IDE is built from (ADR-0257).
- A file template's `markers` take `*.go` for any such file, and `kindOf` makes one template a kind in another's New dialog.
- ADR-0247: detached editor windows.
- The `javaMove` and `javaChangeSignature` refactoring providers over JDT LS's `java/getMoveDestinations`, `java/move`, `workspace/willRenameFiles`, `java/getChangeSignatureInfo` and `java/getRefactorEdit`; providers name the window action they answer, and fields can be tables; the window action `view.changeSignature.open` and `view.project.move` beyond scratch files (ADR-0229).
- `plugin.json` takes `liveTemplates` (templates, postfix templates, doc comment stubs, statement
  rules) for a plug-in's languages; ADR-0218.
- Project generators can have a `jdk` field, `advanced` fields and, for a language's plug-in,
  `language: true` to stand above Empty Project (ADR-0242).
- Languages declare their code style options as data (ADR-0220): `ModuleHost::code_style_options`, `"codeStyles"` in `plugin.json`, `zen_syntax::Arranger`; commands `editor.codeStyle.list`, `editor.codeStyle.writeProject`, `editor.code.rearrange`.
- A test with a real Delve holds conditional breakpoints and Go expression evaluation in every
  context the IDE uses; the Go guide points at the debugger.
- The command `go.module.versions`, the extension point `zen_modules::assist::LanguageAssist` and
  [ADR-0272](docs/adr/0272-language-assists.md).
- ADR-0236: when the Gradle model loads, how it reaches a composite build, and tasks around a sync.
- The commands `go.inspections.run`, `go.inspections.status` and `go.staticcheck.install`.
- `RunProvider::single_file` and the command `file.scratch.run`; run kinds `rustc.file` and
  `java.file` (ADR-0212).
- Project generators: fields with `page`, `segmented` and `tooltip`, forms with `server`. File
  templates: `sourceFolder` and `group`. `file.new.list` takes `menu` (ADR-0243).
- The commands `tasks.background.pause`, `tasks.background.resume` and `tasks.panel.toggle`; the
  task state `cancelled`; `docs/automation.md` lists the `tasks.*` and `problems.fix*` commands
  (ADR-0239).
- ADR-0244: clone, existing sources, modules and generators that run a build tool;
  `project.fromVcs.*`, `project.fromSources.detect`, `project.module.*`, `project.newLists.*`;
  `moduleFiles` and the `addModule` step of `zen_project`; `tool` and `formDependsOn` of
  `projectGenerators`.
- `editor.generate.list`, `editor.generate.members`, `editor.generate.apply` and
  `view.generate.open`; a language server declaration's `generate` (`kinds`, `prompts`) and
  `zen_modules::generate::CodeGenerator` for modules (ADR-0216).
- `zen_update::fetch::conditional` and `post_json`; the commands `dependencies.*` and the `DependencyProvider` trait (ADR-0234).
- `inspect.*` commands and ADR-0256: Inspect Code for agents; inspectors and language server
  declarations list their inspections.
- ADR-0215: file operations in language server edits, and `workspace/will…Files` and
  `did…Files` from the Project tool window.
- ADR-0237: the New Project wizard and project generators; `projectGenerators` in `plugin.json`, the
  ABI v1 export `zen_project` and `--project-generators` of `zen-releases plugin`.
- Commands `run.configuration.new`, `run.template.list`, `run.template.update`,
  `run.template.reset`, `run.services.list`, `run.services.add`, `run.services.remove`,
  `run.services.hide` and `run.services.restore`; `run.process.stop` takes `configurationId`
  (ADR-0213).
- The before-launch task type `buildArtifacts` (`{ "artifacts": [names] }`) and the field kind
  `artifacts` of `run.beforeLaunch.types`.

### Changed

- ADR-0240: `zen_project` can answer `archive` and ask for saved answers (`cache`); generators declare
  `settings`; the zip reader moved from `zen-workbench` to `zen-vfs`.
- The warnings removed with the code comments of the Git code (`zen-vcs`, the workbench's `vcs`) are triaged: the rules that mattered are tests named after them, or one `// WHY:` line where no test can hold them.
- The warnings of the workbench outside its Git code are triaged the same way; two programs' secret files now share one helper, `private_file`.
- The warnings of `zen-db` and of `xtask` are triaged the same way. `xtask` gains small functions a test can reach: running the gate's steps, the curl call to GitLab, macOS first when publishing.
- `log` is a direct dependency, with the file sink in `zen_workbench::ide_log` (ADR-0245).
- The CVSS scoring and OSV range parsing moved from the Go module to `zen_project`, shared by Go and the dependency manager.

### Removed

- `preview` of a project generator (ADR-0243).

### Fixed

- Two language tests that failed now and then wait for what they check: the test fixture's `open` waits for the file's own server, not any server, and `FakeServer::wait_for` wakes on each message instead of polling.
- Six more tests that failed now and then check what they mean to: the two forced debugger stepping tests read what was sent before the step, not whatever came last, because the breakpoints come back as soon as the program stops, and the intention tests publish problems for the open file as a server does. So do the tests of exiting and of the hand-off's stale socket.
- Test runs no longer leave unpacked JDK sources (237 MB each) and Local History folders in the system temp folder.

### Security

- In a project you have not trusted, nothing loads the build: Reload, Show Dependencies, Download Sources, Show Effective POM, Analyze Dependencies and the Gradle tasks say so and offer to trust it. A `pom.xml` outside the project links only from a trusted folder.
- The release server checks the token before it reads a body, keeps small limits on documents, and writes artifacts to disk as they arrive.
- The release server refuses to replace an artifact the live manifest lists, and checks every listed artifact by its SHA-256 when staging and releasing.
- A crash between storing a manifest and its signature no longer leaves a release server that will not start.
- `deploy/releases/Caddyfile` refuses `/api/*` over plain HTTP, except the health check, and `install.sh` verifies the manifest's signature. Both take effect at the next deployment.
- `install.sh` on a Mac installs the archive the signed manifest lists, checked by its SHA-256, not the `.dmg`, which no signature covers. It takes effect at the next deployment.

## [26.11.2] - 2026-11-04

### Added

- ADR-0199: Project Structure shows the project model of every language; the model gains module
  folders, output folders and dependency scopes, and the `project.structure.*` commands.
- ADR-0198: documents a language server shows under its own URIs (`virtualDocuments` in a server
  declaration), libraries that are jars, and `ModelProvider::generation`; commands
  `java.libraries.downloadSources` and `java.libraries.document`.
- ADR-0207: file watchers, and the `run.fileWatchers.*` commands for agents.
- `run.configuration.resolve` and `run.environment.system`; `run.process.start` takes `prompt`,
  `filePrompt` and `confirmRerun`, and `run.process.list` answers `instance` and `replaces`.
- A plug-in's `project` support may declare `build` (`files`, `reload`): the build scripts the
  platform watches for Load Changes and the command that reloads them (ADR-0196); `build.changes.get`
  and `build.sync.get` give agents the changes and the Sync view.
- Run providers contribute Run Anything commands (`RunProvider::anything_prefixes`, `anything`);
  commands `run.anything.list` and `run.anything.run`; window actions `view.runAnything.open` and
  `view.runConfigurations.choose`; a keymap keystroke can be a double modifier (`⌃⌃`).
- ADR-0203: breakpoint properties the IDE does itself when the debugger lacks them.
- `run.beforeLaunch.*` commands, the run state `beforeLaunchFailed`, and before-launch task types as
  an extension point of run providers (`RunProvider::task_kinds`, `task_lines`, `adopt_task`) and of
  the workbench (`before_launch::TaskType`), [ADR-0202](docs/adr/0202-before-launch-tasks-are-an-extension-point.md).
- The optional plug-in export `zen_jvm` (ABI v1): what a file runs on the JVM, its mains, tests
  and classes; the Kotlin plug-in 0.2.0 answers it (ADR-0200).
- `zen_dap::SourceMap`: a DAP client may send a file's breakpoints for other files; java-debug gets
  Kotlin breakpoints through a Java file of their class.
- Trusted projects (ADR-0205): `zen_workbench::trust`, the commands `project.trust.state`,
  `project.trust.grant` (refused for agents, also through `ui.notifications.action`) and
  `project.trust.revoke`; trust kept in the IDE data folder's build settings (`projectTrust`).
  Untrusted projects get no `-javaagent`, annotation processing off and Gradle import off in the
  Java language server, no automatic Load Changes, and `notTrusted` for agents' Gradle, Maven and
  Ant reloads and runs; saved and adopted run configurations, and configurations with before-launch
  tasks that run programs, and `build.artifacts.build`, `rebuild` and `clean`, are refused with `notTrusted` until the project
  is trusted.
- ADR-0197: Ant is a plug-in, with its run kind, commands (`ant.*`) and tool window in the
  platform, and its build files and their settings in `.zen/ant.json`.
- ADR-0201: jar artifacts kept in `.zen/artifacts.json`, written by the IDE without a `jar` tool, and
  the `project.structure.artifacts`, `project.artifact.*` and `build.artifacts.*` commands.

### Changed

- The manual `release-build` pipeline passes `PLATFORM` to a plug-in's build too
  (`PRODUCT:plugin:<id>,VERSION:<version>,PLATFORM:all`), so a plug-in's driver programs are built
  for every platform there (ADR-0191); before, a plug-in got only the macOS program.

## [26.11.1] - 2026-11-01

### Added

- More PostgreSQL objects in the Database Navigator: every schema lists Foreign
  Tables, Materialized Views, Indexes and Aggregate Functions, every table Policies and Partitions,
  and the database Extensions, Event Triggers, Tablespaces and Roles, each with its Properties;
  tables and views have Dependencies and Rules pages. The structure snapshot moves to version 5, so
  each connection is read once again.
- The Database Navigator acts on objects: a double click or ↩ on a table opens its
  Properties, with Data the next page of its tab; a double click or ↩ on a disconnected connection
  connects it. Delete… (⌦), Rename… (F2), Create ▸ and Tools ▸ Truncate… work on objects, and
  Duplicate Connection, Rename and Delete on connections; a drop or truncate first names what is lost
  and shows its statement (`db.object.drop`, `db.object.rename`, `db.object.truncate`,
  `db.navigator.create`, `db.dataSource.duplicate`). A table's size shows as text (`72K`) in its own
  column with a tooltip instead of a bar over long names, long names end with "…", rows never draw
  outside the panel, and a selection made elsewhere scrolls the tree to it.
- Splits inside a workspace and scripts that move between a workspace and the editor (ADR-0189): while a workspace is expanded, Split Right, Split Down, Unsplit, Goto Next
  Splitter and resizing act on its area, each part with its own tab strip (`workspace.tab.split`;
  `workspace.host.state` lists `panes` and each tab's `group`; `editor.split.list` reports a
  workspace's groups as a tree, `workspaces: [{ workspace, root, focused }]`), kept across restarts.
  A project's `.sql` script moves between the code editor and the Database workspace by dragging its
  tab (onto the workspace's tab, into the workspace, back onto the editor's strip) or with Move to
  Editor (`editor.split.moveTab { workspace }`, `workspace.tab.move { group }`); consoles and other
  moves are refused with code `notMovable` and the reason. Local History keeps the versions of
  consoles, recording the folder of consoles as a root of its own.

- Migrations in the Database workspace: the project's migration folders are
  recognised from their files in the background — Flyway, Liquibase, golang-migrate, goose, Atlas,
  sqlx, Diesel, Alembic, Django, Prisma, Rails and Knex — and a **Migrations** tab (in a
  connection's navigator menu) shows each migration as applied, pending, failed or missing, read
  from the tool's own table on a connected connection; it never connects by itself. **Up** and
  **Down** run the project's tool as a run configuration in the Run window with the password in its
  environment only, after the production-safety question (Down always asks), and the connection's
  structure is read again when the run ends. On a read-only connection Up and Down are off, with
  the reason in their tooltip; in a narrow tab the columns shrink with "…" and a file shows its
  whole path on hover. Commands `db.migration.list`, `.status` (with `readOnly`),
  `.run` and `.open` (docs/automation.md).
- Database-to-database transfer and Parquet export in the Database workspace (ADR-0181): **Transfer Data…** on a table, a schema or its Tables folder copies tables into another
  connection (or schema) in four steps: target, tables and columns (existing tables by column name,
  new ones created with types mapped into the target's dialect and the source's primary key),
  loading (rows a statement, commit every N, truncate, stop or skip, COPY) and confirm, then
  progress with Cancel and Run in Background. Each table is read page by page in a read-only session
  and written through the CSV import's loader (`zen_db::transfer`, `Engine::transfer`); commands
  `db.transfer.preview`, `.start`, `.state`, `.cancel`, behind the target's safety and both sides'
  agent access. Export writes **Parquet** through arrow-rs (`parquet` 60 without default features:
  Snappy, gzip through flate2's Rust backend, LZ4 or none; no C), typed columns and row groups of a chosen size,
  to files only (`binary`).
- The SQL plug-in grows to ABI v1 (ADR-0182, `zen-plugin-sql` 0.2.0): folding,
  the Structure, keyword completion and syntax problems for any `.sql` file, with or without a
  connection, in the dialect of the connection a console or project file is bound to. A new
  optional export, `zen_dialect`, tells a plug-in the dialect before every call
  (`TextFeatures::in_dialect`); in a bound file the connection's completion stays the only list.
  GoGarden, RustRonin and WebWave, which have database support on, bundle the plug-in.
- ER diagrams in the Database workspace: **View Diagram** on a schema opens its
  tables and views with their columns, key icons and foreign keys in a Diagram tab, and a table's
  **Diagram** page shows it with the tables it references and that reference it. Read-only, from the
  structure the IDE keeps (offline, never connecting), laid out automatically and deterministically
  on a thread of its own (cancellable); drag or scroll to pan, zoom with ⌘-wheel, pinch, −, +, 100%
  and Fit; a click selects a table, a double click opens its object tab. Export as PNG or SVG in the
  theme's colours, or as Mermaid text. Commands `db.diagram.open`, `.state`, `.zoom`, `.view`,
  `.select`, `.export`, `.cancel`; the canvas (`zen_ui::diagram`) is shared for other features.
- Backup and restore in the Database workspace: **Tools → Backup…/Restore…** on a
  PostgreSQL, MySQL or MariaDB connection, schema or table in the navigator runs `pg_dump`,
  `pg_restore` (or `psql` for a plain SQL backup), `mysqldump`/`mariadb-dump` and `mysql`/`mariadb`
  as runs of the Run tool window, cancellable, with no "client home" to set: the tools are found on
  `PATH`, in Homebrew (keg-only `libpq`, `postgresql@N`, `mysql-client` too), in Postgres.app, or
  inside the Docker container that publishes the connection's port (`docker exec`, chosen when its
  tool is newer than the local one). Options per tool (format, content, schemas, tables, clean,
  owner, privileges, compression, jobs, consistency, routines, events, extra arguments, …); the
  password only in a 0600 file the tool is pointed at (`PGPASSFILE`, `--defaults-extra-file`,
  ADR-0193), never its environment or arguments; backups written under a hidden name and moved into
  place; Rerun in the Run tool window repeats a job (a restore asks again); a restore always asks
  first. Commands `db.backup.settings`, `.tools`, `.start`,
  `.restore`, `.state`, `.cancel`; window actions `view.database.backup` and `.restore`.
- Clickable links in the Terminal: `zen_terminal::links` finds URLs and file paths
  with `:line[:column]`, `(line,column)` or Python's `", line N` in each (wrapped) line, bounded to
  4096 characters and 64 links per line, without regular expressions or the file system; paths are
  resolved apart, in the session's folder, then the project (`a/` and `b/` diff prefixes dropped).
  `Screen` now tells which rows wrap. While ⌘ is held the link under the pointer is underlined
  with the hand cursor; ⌘-click opens a centred dialog (product mark, title, the link, "Remember my
  choice", full-width buttons, the default accented, Cancel last; ⏎ and ⎋). New commands
  `terminal.link.list` and `terminal.link.open` (actions `openInBrowser`, `copyLink`,
  `openInEditor`, `revealInProject`, `openInFinder`, `copyPath`, `dryRun`); `openInBrowser` refuses
  agents with the code `userOnly`. The remembered choices are the `terminalLinks` setting, shown in
  Settings → Tools → Terminal, which is no longer "in development".
- The first database driver program of our own (ADR-0175): SQL Server and Azure SQL
  through `drivers/sqlserver` (`zen-db-sqlserver`, `tiberius` built without its TLS features) and
  the plug-in `plugins/sqlserver`, not released yet. TLS is
  rustls with RustCrypto in the program: negotiated in the pre-login exchange with the handshake in
  PRELOGIN packets (TDS 7.x, TLS 1.2; the program sets the encryption option tiberius sends, and
  checks the handshake's signature with SQL Server's own version 1 certificate), or first with ALPN
  `tds/8.0` (TDS 8.0, the `strict` setting); after an attention signal the server's two closing
  messages are joined so the session goes on; logins
  with a password or an Entra ID access token, Azure SQL's redirection; introspection from `sys.*`
  with fingerprints per schema, the tree, results read page by page from the open stream, Cancel and
  timeouts as TDS attention, manual commit through `IMPLICIT_TRANSACTIONS` and `@@TRANCOUNT`, and
  editable results (columns name their table through `sys.dm_exec_describe_first_result_set`).
  `zen-db` gains a T-SQL dialect for generated SQL (`SqlDialect::SqlServer`: brackets, `N'…'`,
  `@P1` placeholders, `0x…`, `CASE` for `NULL` order, `[` escaped in `LIKE`), SQL Server keywords
  and functions for completion, and bracket quoting for the `sqlserver` driver. Driver programs
  share `zen-db-driver-kit` (`drivers/kit`); `cargo xtask layers` knows the new kind "driver"
  (depends only on drivers, never depended on, not built for WebAssembly). Its ignored Docker tests
  and the conformance suite pass against SQL Server 2022, also one that forces encryption; Oracle
  and publishing the driver programs come later.
- MongoDB as a driver program of our own (ADR-0175, decision 5): `drivers/mongodb`
  (`zen-db-mongodb`) on the pure-Rust `bson` crate, with the plug-in `plugins/mongodb`, not released
  until driver programs can be published. It speaks OP_MSG, `hello`, SCRAM-SHA-256 and SCRAM-SHA-1,
  finds a replica set's primary, reads `mongodb://` and `mongodb+srv://` addresses (SRV and TXT
  looked up by a small DNS client of its own) and runs TLS through the kit from the first byte.
  Consoles take a subset of the MongoDB shell (`find` with `sort`/`limit`/`skip`, `aggregate`,
  counts, `distinct`, inserts, updates, deletes, `findOneAnd…`, indexes, `runCommand`, `show`,
  `use`); documents are rows with nested values as JSON, read through server cursors with
  `getMore` and `killCursors`; Cancel closes the connection and kills the operation with `killOp`;
  read-only sessions refuse changes in the program. The navigator lists databases, collections,
  views and indexes. Statements of the `mongo` language are split as a command language that spans
  lines (`zen_db::sql::Dialect::Script`: `;` and blank lines, JavaScript quotes and `//` comments),
  and the engine takes the method of a call chain (`db.users.find(…)`) as a statement's verb for
  the declared `readVerbs`. Its ignored Docker tests pass against `mongo:8.0`, TLS included.
  Documents of a `find` of one collection are edited in the grid by `_id`
  (`zen_db::edit::document`: typed input by BSON type, `insertOne` with an ObjectId made by the IDE,
  `updateOne` by `_id` and old values with `$set`/`$unset`, `deleteOne`), saved without a
  transaction where the driver has none (`SaveReport::kept`); results of a document database also
  show as JSON (`db.result.view` `json`); collections open from the navigator as a console with
  their `find`; the shell reads Extended JSON wrappers as the values they stand for, and an update
  counts the documents it matched. `MONGODB-X509` logs in with the TLS client certificate, and
  manual commit runs statements in a transaction of the program's own logical session
  (`commitTransaction`, `abortTransaction`), both tested live.
- DuckDB as a driver program (ADR-0175, decision 6): `drivers/duckdb`
  (`zen-db-duckdb`) with the plug-in `plugins/duckdb`, not released until driver programs can be
  published. It drives the `duckdb` command-line program the user installs, one process a session,
  never through a shell: statements are checked first (one statement, no dot-command, quotes and
  comments closed), queries are described for typed columns and read in `.mode jsonlines`, errors
  keep their position, transactions follow `BEGIN`/`COMMIT`/`ROLLBACK` and manual commit, Cancel
  and timeouts interrupt and restart the process, and the navigator lists databases, schemas,
  tables, views and columns. External SQL drivers without introspection open a table of their tree
  as a console with its `SELECT`. Its ignored tests and the conformance suite pass against
  `duckdb` 2.0 in Docker.
- Database driver programs are published with their plug-in (ADR-0191):
  `cargo xtask release-build plugin <id> <version> --platform …` builds every program of a driver
  with a crate in `drivers/` for the platforms asked (the tag pipeline: all), signs the macOS one ad
  hoc, and lists each in the catalog entry with its address beside the module, SHA-256 and size;
  publishing checks the two agree and uploads the programs before the catalog. The release server
  accepts or stages a catalog only when it stores every program it lists at a relative address as
  listed (deployed by the owner). An update of such a plug-in replaces its program.
- Oracle Database as a driver program (ADR-0190): `drivers/oracle` (`zen-db-oracle`)
  with the plug-in `plugins/oracle`, not released yet. It drives
  SQLcl, which the user installs with Java 17+, one process a session, started silent, restricted
  and not logged in; the password reaches it only in `CONNECT` on its standard input. SQLcl is found
  from the connection's `program`, `SQLCL_HOME`, `ORACLE_HOME/sqlcl`, `PATH` and Homebrew, Java from
  `javaHome` or SQLcl's own lookup (absolute paths and `PATH` folders only; a project connection's
  `program` and `javaHome` are never passed to a driver, ADR-0193), and a missing one fails with code `sqlclMissing` or
  `javaMissing`. Connections name a service, a SID, a TNS alias or a connect string, with TCPS for
  TLS, wallets (Autonomous Database's `.zip` too) and administrative roles. Results come from
  `SET SQLFORMAT json` with typed columns and exact numbers, errors keep their Oracle code and
  position, only SQL and PL/SQL reach SQLcl, manual commit follows `AUTOCOMMIT`, Cancel interrupts
  SQLcl (or ends it), and introspection reads schemas, tables, views, packages, procedures,
  functions, sequences and types from the `ALL_*` views, so completion works and a query of one
  table is edited. The platform gains `ObjectKind::Package` (folder **Packages**), an Oracle dialect
  for generated SQL (`SqlDialect::Oracle`: quoting, `HEXTORAW`, `DECODE`, `FETCH FIRST`, no `AS`
  for subquery aliases; `SqlDialect::limit`, `SqlDialect::alias`, and `relations::lookup_statement`
  takes the dialect) and Oracle script splitting (`Dialect::Oracle`: `q'[…]'` quotes, PL/SQL ending
  at a `/` line). Tested with a fake SQLcl and the conformance suite; ignored tests against
  `gvenzl/oracle-free` need a real SQLcl.

### Changed

- Release notes not released yet are files of their own in `changelog/unreleased/`, one per pull
  request, and `cargo xtask changelog gather <YY.M.N> [--toolbox <version>]` moves them into the
  release's sections of every IDE's changelog, Zen Toolbox's and `CHANGELOG.md` (ADR-0188); the
  gate checks their format and refuses notes written under `CHANGELOG.md`'s `[Unreleased]`. The
  notes collected there up to the tag `v26.10.4` are the section `[26.10.4] and earlier`, as they
  stood at that tag.
- The ADR index in `docs/README.md` stops at ADR-0179: the records are the index, printed by
  `cargo xtask adr list`; `cargo xtask adr next` names the next number no branch uses, and the gate
  also refuses a record whose heading names another number, one without a status, and a number
  `origin/main` gives another record.
- The data grid: the value panel is open by default, a table's Data page reads its
  rows in the database's order (no `ORDER BY` of the key), headers mark primary and foreign key
  columns and have a ▾ column menu, the context menu is grouped into Advanced Copy, Edit, Filter,
  Order, Navigate, View/Format, Panels and Layout with shortcuts, the bottom bar has first/previous/
  next/last buttons, an editable fetch size and Calculate Total, Go to Row (⌃G,
  `db.result.select`), ⇥ switches between the grid and the record, and result tabs are named after
  their table and run (`orders 3`).
- The Git workspace moved onto the workspace host (ADR-0187): it expands over the
  editor area like the Database workspace, collapses with ⇧⎋, its button or a code file, and comes
  back after a restart; its graph is a pinned inner tab, and a diff opened from it, the interactive
  rebase editor and the conflict editor are inner tabs beside it instead of extra editor tabs. Every
  open workspace (Git, Database) has a tab with its icon and × in the editor tab strip, which stays
  on top while it is expanded: a click expands it, × closes it (`workspace.host.close`, its tabs
  kept); `workspace.host.list` and `.state` report `open`. The `vcs.workspace.*` commands keep their
  parameters and answers.

- The IDE view keeps which non-document tab is in front of the editor (diff, Compare Directories,
  Git workspace, rebase, conflicts) in one value instead of a flag and a document per tab, ready
  for the Git workspace to move onto the workspace host; nothing users see changes.

### Fixed

- An agent can no longer remove or duplicate a database connection closed to agents (ADR-0179):
  `db.dataSource.remove`, `db.dataSource.removal` and `db.dataSource.duplicate` go through the same
  gate as the other database commands; a person still can.
- Consoles of database connections closed to agents (ADR-0179) stay hidden from agents' MCP file
  tools now that consoles in the IDE's data folder are open to agents: the MCP server's hidden-file
  check takes the windows' `AgentHiddenPaths`.
- **F4** (and **⌘↓**) runs Jump to Source in the Commit window's change list and in the diff
  viewer: the file opens in an editor tab with the keyboard; from the diff, at
  the caret's line (the line clicked last, or the current difference), and from the HEAD side at the
  matching line of the file. The binding is the keymap action `view.vcs.jumpToSource`, which can be
  reassigned and is reachable over MCP; the context menu item shows its keys.
- SQLite takes numbered parameters: `$1` and `?1` are placeholders asked in Bind Parameters and
  written as literals, as `?` and `:name` were, so `WHERE id = $1` returns its rows instead of none
  (the text went to SQLite unbound and compared with NULL).
- A SQLite connection added from a URL points at its file: `sqlite:///abs/shop.db` keeps the
  leading slash (it became the relative `abs/shop.db`), and `sqlite:/…`, `sqlite:relative.db`,
  `sqlite:file:…`, `file:///…` and `duckdb:///…` give the file's path too, in the connection
  dialog, `db.dataSource.add` and DBeaver's imported connections.
- Making a connected connection Production took effect only after a reconnect: its consoles kept
  committing on their own. A new connection type, or a type's changed settings, now
  applies to the open sessions at once; a session with uncommitted changes keeps manual commit
  until they end, and nothing is committed by the change. The question before a change says whether
  it is committed at once or stays in the open transaction.
- Database messages people read no longer show command ids such as `db.console.chooseConnection`
  or `db.detect.list`: they name the controls to use, in Russian by their Russian
  titles, while AI agents still get the command to call in the same refusal.

- Agents and database consoles: consoles in the IDE's data folder are open to agents
  (`get_document_text`, completion, running), so a console in front no longer makes unrelated calls
  refuse or warn; `ZEN_AUTOMATION=1` serves MCP on the IDE's default port, not port 1, so
  `<ide> mcp` starts an IDE whose MCP server was off; `run_action` opens the database dialogs by
  their window actions (`view.database.export`, `view.database.import`); `db.script.new` without
  `folder` is no longer refused because the project holds a file hidden from agents (`.env`).
- Connecting no longer waits forever when the Keychain does not answer (a prompt hidden behind a
  locked screen): reading a saved password gives up after 30 seconds with a message to allow access
  in the system's prompt and connect again (`secretTimeout`). While a connection connects, the
  Overview and `db.dataSource.list` say what it waits for (the Keychain, the SSH tunnel or the
  server), and **Cancel** stops it.

- An empty SQLite table opens on its Data page with its columns and no rows instead of
  "Cancelled.", and a SQLite query that returns no rows shows its columns: `sqlite3` names columns
  only above a first row, so the driver reads them from a temporary view of the statement. A Data
  page says "Cancelled." only when its read was cancelled.
- Reading a SQLite table again quickly no longer fails the next read with "sqlite3 ended" or
  "Program interrupted.", nor cancels it: a cancel that reached `sqlite3` after its statement had
  finished left a program that ended at its next input; such a process is now retired at once and
  the next statement runs on a fresh one.
- Removing a database connection closes its object, plan, sessions and key tabs and its consoles'
  result tabs; its consoles stay open without a connection and say which one was removed, instead
  of silently running on another. Unsaved data or structure changes in those tabs are asked about
  first, and `db.dataSource.removal` gives the confirmation naming how many tabs close.
- Result tab names start from 1 again in each run of a console (`orders`, then `orders 2` for a
  second result of the same table, `Result 1` for others) instead of counting up across every run;
  pinned result tabs keep their names.

- The SQL connection chooser of a code file counts every ↑ and ↓: several presses that arrive
  in one frame moved the highlight by one; it starts on the connection in use, and ⏎ binds the
  highlighted row.
- Run hints and the execution log: the "N ms · N rows" hint of SQL run from a code string is drawn
  at the end of its line, after the closing quote and `);`, never inside the string, and a console's
  goes after the statement's `;`; the Execution Log shows no time for a statement that never ran,
  and its time is a translated message; running SQL in a code file no longer scrolls the editor
  sideways while the Database workspace opens.
- The data grid shows its values correctly: floating-point numbers far from 1 in exponent form
  (`1e300`, not 301 digits; the value panel's sums too), SQLite columns with the type badge and
  value panel type their table declares (`NUMERIC`, `DATE`, `TIMESTAMP`, `JSON`, `BOOLEAN`),
  `[DEFAULT]`, `[NULL]` and the filter box's hint end in an ellipsis instead of a hard cut, the
  strike-through of a deleted row ends at the last column, a changed cell keeps a mark when
  selected, the keyboard scrolls the selected cell fully into view, a partly scrolled column keeps
  its header title, and row counts say "1 row" / "N rows" (with Russian plural forms).

- The data grid says why a value cannot be edited: a read-only result shows a **Read-only** lock and
  its reason in the bar under the grid, an attempt to edit turns the reason red instead of doing
  nothing, and the edit items of the context menu stay, disabled, with the reason. F2 and typing a
  character start editing, the cell editor keeps its cell's width, and **Set to NULL** is disabled
  on a `NOT NULL` column. A filter that fails on a table's tab shows its error in place of the rows
  and outlines the filter box in red, instead of keeping the rows read before; an open table's tab
  reads its rows again after a CSV import into it.

- Database workspace keys and toolbar (ADR-0186): inside the workspace ⌥X runs the
  script (without typing "≈"), ⌃4 and ⌃8 commit and roll back, ⌃] opens a new console, ⌃⇧F formats,
  ⌃⇧E explains, ⌃9 and ⌃0 choose the connection and schema, and more; the IDE's ⌘ keys stay as second
  keys and every key keeps its meaning outside the workspace. The connection, schema, lock, commit
  mode, Commit, Rollback and the pending-transaction counter stay on the toolbar whatever tab is in
  front (a table's connection, else the navigator's selection); one commit-mode control with a
  tooltip replaces the two "Auto" labels; every toolbar button has a tooltip with its key; the
  statements that ran flash for a moment.


- Database workspace layout: the Overview's connection rows no longer overlap at any width or in
  Russian (status and buttons in fixed columns that line up, long names and addresses end with "…",
  two lines in a narrow window, the header's buttons wrap); Properties numbers the rows once, ends a
  long last column (an index's definition) with "…" in view, counts every section and hides what the
  driver's databases do not have (SQLite: owner, tablespace, policies, partitions, permissions).
- Database workspace: New Console opens on the connection selected in the navigator, else on the
  tab in front, instead of the placeholder folder; the tab row brings the active tab into view after
  a resize or a slide and never starts with half a tab; only consoles and `.sql` files open as tabs
  of the workspace, any other file in the editor (`workspace.tab.open` refuses it with
  `notHosted`).
- Database dialogs lay out cleanly and default to the safe choice: labels line up
  in one column, dropdowns are framed and checkboxes and radios show in both themes, the connection
  dialog's SSH/SSL page scrolls with a visible scroll bar and new connections connect after saving
  (an option, on by default); Transfer never preselects a read-only target, marks read-only
  connections, says when it reuses the last settings (with Reset) and keeps one size; Export names
  its source, offers COPY only on PostgreSQL, has Browse… and can export only the selected cells;
  CSV import has Browse…, unbroken preview headers and a Before loading row for TRUNCATE; Bind
  Parameters shows no error before typing; Skip shows only for several statements; the unsaved
  changes question on close asks one clear question; "1 tables" reads "one table".

- A CSV import cancelled just as the server answered could still commit: the loader picked
  between the next batch and the cancel at random, and the last `COMMIT` did not look at the cancel.
  `the_csv_import_dialog_walks_its_steps_shows_the_sample_and_the_progress_in_light_and_dark` failed
  so once under load. Cancel now wins over the next batch and no `COMMIT` is sent after it, also for
  transfers; `an_import_cancelled_while_the_server_answers_never_commits` covers it.
- Problems of external drivers reach the user: a query on a connection whose connect failed says
  why (DuckDB without its `duckdb` program says the program is missing, not "not connected"), each
  failed connect leaves a line in the driver's log (`db.driver.log`), and a new connection of a
  driver that asks no password (DuckDB) starts without password authentication.
- A database driver program a plug-in declares at an absolute `https://` address is downloaded from
  that address; the IDE fetched it from the plug-in catalog's folder and got 404.

### Security

- A DuckDB connection's file whose name starts with `-` is opened as a file, never read as an option
  of `duckdb` (`-unsigned`). An agent's `db.dataSource.add` or `db.dataSource.update` that gives
  `settings.program` or `settings.javaHome` a new value is refused with `agentNotAllowed`.

## [26.10.4] and earlier - 2026-10-27

Every change to the repository up to the tag `v26.10.4`, exactly as it stood under `[Unreleased]`
at that tag: until then the repository's changes were not cut into releases. The products'
release notes of 26.10.4 and before are in [changelog/](changelog/README.md).

### Fixed

- A connection type that gives AI agents no access now closes its connections to agents entirely
  (ADR-0179): besides their statements, agents no longer see their table and column names,
  structure, consoles, history, snapshots, tabs or generated code. Every `db.*` command goes through
  one gate that refuses calls naming such a connection (`agentNotAllowed`) and leaves it out of
  lists; only the commands about the connection itself stay open.
- Production safety (ADR-0172) also holds for exports and the query history: an agent exports only
  what it may read (`db.export.start` goes through the same check as a query), and the history of a
  connection whose type lets agents run nothing is not listed or opened for agents, nor are a key's
  value, a query's state or the rows of results a person read there.
- Database commands that read or write a file named by an agent (CSV import, export targets, a
  value saved to a file, schema snapshots, migrations, SQLite files, `.sql` files run) refuse files
  hidden from agents (ADR-0143) with code `redacted`, as the editor's commands do; before, an
  agent could import a hidden `.env` as CSV and read it back.
- The release path after the first tag release (26.10.3): the release server no longer re-reads and
  hashes every published artifact (221, 4.5 GB) on each manifest or catalog upload, only the new
  ones, so staging a release takes minutes, not an hour; `zen-releases upload` sends files three at
  a time and only those the server lacks (`GET /api/v1/artifacts/…` gives a stored file's size and
  SHA-256), so a retried stage sends nothing again; the deploy script makes the data folders
  (`artifacts`, `stages`, `site`) the server's before Docker starts (Docker had made `site` root's
  and staging the website failed), and the server stops at start-up, saying what to do, when it
  cannot write them; the stage and verify jobs have timeouts of their own.
- Database workspace, found while testing it live: the Bind Parameters dialog gives the first value
  the keyboard and moves between the values with ⇥ / ⇧⇥ (⇥ typed into the first field before), and
  an empty value of a type other than NULL blocks Run with a message, as an invalid number does. Two
  console tabs of different connections, both `console.sql`, are titled with the connection's name
  (`console.sql (Shop)`) while both are open; tabs of a workspace with the same file name get their
  folder otherwise. New Console with no connection opens New Connection… (its tooltip says so), and
  `db.console.new` is refused with `noConnection`. In a narrow window the console toolbar no longer
  runs off the edge (the transaction mode was cut to "Aut…" and the counter hidden): Explain Plan,
  Execute on Several Connections… and Export From Query… go into a ⋯ menu, then New Connection and
  New Console show their icon only; the result bar's Fetch Next, Fetch All and Export… and the
  object's structure actions past the edge go into ⋯, the Plan tab's totals no longer cover its
  options (the bar wraps), Find in Database folds its column types into a menu and narrows its
  column field, and the Sessions tab shows icon buttons.
- Database races found by running the tests under load: a query whose statement was cancelled could
  read as `done` with nothing run in the moment before its task ended (`db.query.state`, and a run on
  several connections then said `done` for a cancelled connection); it reads `cancelled`. A
  statement recorded in the query history was not found by `db.history.list` until the history's
  writer thread had taken it; it is found at once now, also before the journal is read at start.
  The key-lookup test waited for the row count but clicked a row drawn a frame later.
- A PostgreSQL statement with `$1` run without values failed as a lost connection (kind
  `Connection`) instead of a statement error; client-side statement errors (parameter count, a
  value that cannot be sent, a column that cannot be read) are `Server` errors now.
- A database connection added without a password for a server that commonly needs none (Redis and
  Valkey, SQLite, ClickHouse's `default` user; `redis://localhost:6379/0`) was kept with
  `auth: password` and then refused to connect with `passwordNeeded`. `db.dataSource.add` and the
  connection dialog now give it `auth: none`; PostgreSQL, MySQL and MariaDB without a password keep
  `password` and ask when connecting; an `auth` given explicitly stays as given.
- The Compare tab of the Database workspace stayed on "Choose a source…" when `db.compare.schemas`
  or `db.compare.data` was started by a command (an AI agent over MCP). It now shows the window's
  latest comparison with its source and target chosen, whoever started it; `db.compare.state`
  answers each comparison's `request`.
- Reformat Code (⌥⌘L), Optimize Imports and Fix Problems with Linters pressed with no file open in
  the editor (the Database workspace collapsed, nothing focused) posted an error notification "No
  file is open."; like Go to Line, they now do nothing.
- Explain Plan (`db.console.explain`) sent `:name`, `$1`, `?` and `${var}` as written, so the
  server refused the statement (MySQL error 1064). It now takes the same parameters step as
  Execute: the Bind Parameters dialog, or `parametersNeeded` with `retry` and the `parameters` and
  `variables` params over MCP; values are bound on the server (PostgreSQL's extended protocol,
  MySQL's and MariaDB's prepared statements) and written as quoted literals only on SQLite, which
  the answer's and the Plan tab's `inlined` say.
- Execute on Several Connections (`db.script.runOnConnections`) ignored parameters and variables.
  One dialog (one `parametersNeeded` refusal) now asks the values once for every connection; each
  connection binds them or, where its driver cannot, writes them as literals, said in its output
  and its target's `inlined`.
- The Bind Parameters dialog started every new placeholder as Text. It now starts with the type its
  place suggests (`inferred` in the refusal and in `db.console.parameters`): Number next to a
  number, in arithmetic, after `LIMIT`/`OFFSET` or against a numeric column of the cached
  structure, Boolean against a boolean, a cast's type; a remembered value keeps its own type.
- The release's Windows build failed in `zen-db`: the SSH askpass channel used Unix sockets only.
  On Windows it is now a loopback TCP port (`tcp:127.0.0.1:<port>`; a request is answered only with
  a live tunnel's one-time token), Unix sockets stay elsewhere; `xtask`'s disk image link and a few
  Unix-only fields are gated, so `cargo check --workspace` passes for `x86_64-pc-windows-gnu` and
  `x86_64-unknown-linux-gnu`. The nightly schedule runs that check before its gates.
- Two tests of adding a detected database read the Keychain before its background write ended
  (`secretOperation`): `a_detected_database_opens_the_dialog_filled_in_and_agents_never_get_its_values`
  failed so under load, in the v26.10.3 tag pipeline too, and
  `adding_an_offer_stores_its_password_in_the_keychain_only_and_a_changed_file_offers_it_again` had
  the same race. Both now wait for the operation with `db.operation.state`.

### Added

- A connection's command-line client in the Terminal and code types from tables in the Database
  workspace: **Open in Terminal** in a connection's menu starts psql, mysql or
  mariadb, sqlite3, redis-cli or clickhouse client in a new Terminal tab with the connection's
  settings and its TLS options, through its open SSH tunnel; the password reaches the client only
  in a file only the user can read (`PGPASSFILE`, `--defaults-extra-file`, `--config-file`), removed
  when the client ends; redis-cli asks for it. Never on the command line or in the environment.
  **Copy Client Command** copies the command without the password, and **Generate Code ▸** copies a
  Go struct, a Rust struct or a TypeScript interface of a table's columns. Commands
  `db.connection.openTerminal`, `db.connection.clientCommand` and `db.object.generateCode`.
- The geometry viewer in the value panel: spatial values (PostGIS `geometry` and
  `geography`, MySQL geometry, GeoPackage blobs, WKB, WKT, EWKT and GeoJSON) offer a Geometry format
  that draws them, and every other spatial value of the selection, as shapes on a canvas without a
  map, with a grid, zoom, pan and fit, in theme colours; no map tiles, nothing goes online. Parsed in
  pure Rust by the new `zen_db::geometry`, off the UI thread. `db.result.value` tells a spatial
  value's kind, SRID, axes, points and bounds and renders it as WKT; the new `db.result.geometry`
  lists the shapes of a selection. The status bar shortens long minimum and maximum values.

- Explain Plan in the Database workspace: ⌥⌘E or the console toolbar explains the
  selection or the statement under the caret into a Plan tab — a tree of nodes (operation, entity,
  cost, rows, time, condition) with hot nodes tinted by their share of time or cost, a details pane
  and the server's text. PostgreSQL takes ANALYZE, VERBOSE, COSTS, BUFFERS and SETTINGS; MySQL and
  MariaDB explain in JSON and analyze; SQLite shows its query plan. ANALYZE is off by default, warns,
  runs in a rolled-back transaction or savepoint and is checked like running the statement.
  Commands `db.console.explain`, `db.plan.state`, `db.plan.select`, `db.plan.view`.
- Database objects in Search Everywhere: a **Database** tab (and group of All) and Go
  to Database Object (⌘O in the Database workspace, `view.gotoDatabaseObject.open`) find tables,
  views, functions, sequences, types, schemas and columns of every connection's snapshot, offline and
  without connecting, with the platform's name matcher on a thread of its own (100 k names in about
  1 ms), then the connections' scripts and the query history; each shows its kind, schema and
  connection, and ↩ opens its tab. `db.object.goTo` gives agents the same search, and a Search
  Everywhere item can now run a command (`SearchTarget::Run`).
- Following foreign keys in the Database workspace: the cells of foreign-key columns
  are links that open the referenced row in its table's tab, filtered by the key in the connection's
  dialect (composite keys and quoting included); **References ▸** opens the rows of other tables that
  point at a row; **Back** and **Forward** under the grid walk the tabs visited this way with their
  filters; ⇧↩ or **Choose Value…** on an editable key cell lists up to 200 rows of the referenced
  table, searched, and stages the chosen key. Reads run in the background in the Main context
  (`zen_db::relations`). Commands `db.result.relations`, `.openReferenced`, `.openReferencing`,
  `db.data.back`, `.forward`, `.navigation`, `db.lookup.start`, `.state`, `.choose`, `.close`.
- SQL formatting: Reformat Code (⌥⌘L) formats SQL consoles and files in the dialect
  of their connection, the selection's statements or the whole file, in the background, as one undo
  step; Settings → Editor → Code Style → SQL sets the indentation, keyword case (as is, upper,
  lower), commas trailing or leading, clause keywords on their own line and one list item per line.
  Only white space and the case of reserved keywords change, checked token by token; statements it
  cannot format safely are kept. `db.sql.format` formats text for agents (`zen_db::format`,
  `zen_workbench::sql_format`).
- Import from CSV in the Database workspace: **Import Data…** on a table, a schema
  or its Tables folder opens a five-step dialog (file; format with delimiter, header, quote, escape,
  NULL text, a date format in Java or strftime style and a sample of 100 rows with the kinds they
  hold; columns mapped onto the table's or a new table's with types; batch size, truncate first,
  commit every N rows, stop or skip on error; a last check). The file is read row by row on a thread
  of its own and loaded in the background in batches inside savepoints: PostgreSQL through
  `COPY … FROM STDIN` (`DriverSession::copy_in`), the other SQL drivers through multi-row `INSERT`s
  in their dialect; a failing batch is retried row by row, so every failing row is noted with its
  row and line number. Read-only connections refuse, Production and `TRUNCATE` ask first (ADR-0172);
  commands `db.csvImport.settings`, `.preview`, `.start`, `.state`, `.cancel`; imports show in the
  query history as **Imports**. `view.database.export` and the new `view.database.import` now open
  their dialogs: the workspace's own `view.database.*` actions no longer catch them first.
- Bind parameters and script variables in the Database workspace: `:name`, `$1`
  (PostgreSQL) and `?` placeholders outside strings, comments, quoted names and dollar quotes are sent
  as real bind parameters (PostgreSQL's prepared statements, MySQL's and MariaDB's with typed values;
  quoted literals on SQLite and ClickHouse, which have none, and the output says so); the Bind
  Parameters dialog asks typed values (Text, Number, Boolean, NULL) remembered per console without
  secrets; `${name}` variables, `@set name = value`, `@include file.sql` and a Variables tab in the
  results panel; `db.console.parameters`, `.variables`, `.setVariable`, `.setParameter`, and
  `parameters`/`variables` on every run command and `db.query.run` (refusal `parametersNeeded`). The
  confirmation before a run lists the values and the query history keeps parameter names (values
  with `db.history.settings { parameterValues }`, secret-looking ones masked).
- Sessions and locks in the Database workspace: a Sessions tab per connection lists
  the server's sessions (PostgreSQL: Pid, User, Database, Application, Query start, State, Query;
  MySQL and MariaDB: Id, User, Host, Database, Command, Time, State, Query), marks waiting and
  blocking sessions and the IDE's own, and shows the locks waited for with the blocking chains;
  Refresh, Show idle and an auto-refresh that runs only while the tab is shown. Cancel Query and
  Terminate (Kill Query, Kill Session) always ask first, touch only the chosen session, are refused
  on read-only connections and recorded in the query history. Commands `db.session.list`, `.state`,
  `.cancelQuery`, `.terminate` and `.open`, sized for agents with queries masked as in the history.
- Structure editing with an SQL preview in the Database workspace: on an object tab's
  Properties page you add, change, rename and drop columns, constraints, foreign keys and indexes,
  rename, comment and delete objects, and from a schema create tables, views, sequences, functions
  and schemas; the changes accumulate, and **Save…** shows the script in the connection's dialect
  (`zen_db::alter`, ADR-0170) with Execute, Copy, Open in Console, Save as Migration (a new
  `migrations/<UTC time stamp>_<name>.sql` in the project) and Cancel. PostgreSQL and SQLite run it
  in one transaction; SQLite rebuilds a table where its `ALTER TABLE` cannot (and refuses when the
  rebuild would lose `CHECK`, `GENERATED`, `COLLATE` and the like); MySQL and MariaDB restate a
  column whole and refuse when it has attributes the IDE does not read; what a server lacks is
  refused when staged. Production safety applies (DROP asks, read-only refuses, agents on
  Production hand the script to a person), and afterwards only the changed objects are read again.
  Commands `db.structure.stage`, `.state`, `.unstage`, `.discard`, `.preview`, `.execute`,
  `.toConsole` and `.saveMigration`; query history source `structure`.
- One script on several connections in the Database workspace: **Execute on Several
  Connections…** in a console's toolbar and above a `.sql` file (`db.script.runOnConnections`,
  `db.script.connectionsState`) runs the script on the ticked connections in turn, each in its own
  dialect; stop at the first error (the rest skipped) or go on; every connection is checked by its
  own type before anything runs, a refusal refuses the whole run and one confirmation lists every
  connection with its type; results are grouped per connection (a Connections tab with each one's
  outcome, result tabs named after their connection); Cancel stops the rest; the Query History
  source is "Several Connections".
- Find in Database: a tab of the Database workspace (the toolbar's magnifier, **Find in
  Database…** on a connection or schema) looks for a text in a schema's tables on the server, in
  each driver's dialect (PostgreSQL, MySQL, MariaDB, SQLite, ClickHouse): text columns by default,
  numbers, dates and other kinds on request, chosen columns, names of tables and columns, views on
  request; in a read-only session of its own, one table at a time with a row limit and a deadline
  each, in the background with progress and Stop. Only counts per column come back, never values;
  each table found opens on its rows filtered to the text. Warnings on Production and large
  schemas. Commands `db.search.targets`, `.open`, `.start`, `.state`, `.cancel`, `.openResult`; the
  query history gains the source `search`.
- Schema and data compare in the Database workspace: the **Compare** tab and
  `db.compare.*` commands compare two schemas (of connections, read again first, or of saved
  snapshot files from `db.compare.saveSnapshot`) as a tree of objects and parts with each object's
  DDL side by side in the diff viewer, and write the sync script in the target's dialect (drops only
  when asked; opened in a console, never run); a table's rows compare by key page by page in
  read-only sessions of their own, with counts, the differing rows and an INSERT/UPDATE/DELETE
  script.
- Database profiles for wire-compatible servers (ADR-0173): 25 servers as data over
  the compiled-in drivers, with their vendors' default ports, databases, users, TLS modes, URI
  schemes and managed-service hosts: CockroachDB, YugabyteDB, TimescaleDB, Neon, Supabase, Amazon
  Redshift, Greenplum, AlloyDB, Citus, Materialize, RisingWave, QuestDB and CrateDB over PostgreSQL;
  TiDB, SingleStore, PlanetScale, Vitess, StarRocks, Apache Doris, Percona Server and Aurora MySQL
  over MySQL; Valkey, KeyDB, Dragonfly and Garnet over Redis. A connection keeps its `profile`;
  servers whose catalogue is not PostgreSQL's are read from `information_schema` (or QuestDB's meta
  functions) without session guards they lack, and folders they do not have are hidden. The New
  Connection dialog groups drivers and profiles by family with search; a URI picks its profile by
  scheme (`cockroachdb://`, `redshift://`, `valkeys://`) or host (`*.neon.tech`); `db.driver.list`
  answers the profiles (`query`, `family`, `limit`, `truncated`). After connecting, the server's
  version and host name the profile it is: Test Connection offers it, `db.connection.status`
  answers `detectedProfile` and a notification offers to use it; stored settings never change by
  themselves. Snapshot files are format 4.
- External database drivers (ADR-0174, `docs/database-drivers.md`): a plug-in can
  declare database drivers served by a program of its own (`"databaseDrivers"` in `plugin.json`, a
  program and SHA-256 per platform), downloaded only when the user installs or updates the plug-in
  and checked before every start. The program speaks protocol v1, JSON-RPC 2.0 one message per line
  on its standard streams (`initialize`, `connect` with the password in memory only, `execute`,
  `query`/`fetch`/`closeCursor` page by page, `cancel`, `apply`/`commit`/`rollback`, `children`,
  `introspect`, `key`, `ping`, `close`, `shutdown`); `zen_db::external::ExternalDriver` runs it
  under the engine's runtime, one program per connection or shared, with timeouts, a restart limit
  and a log without secrets (`db.driver.log`). `db.driver.list` lists such drivers after the
  compiled-in ones with `external`. A reference driver (`zen-db-reference-driver`, in no product)
  and a conformance suite any driver can run (`cargo test -p zen-db --test external_driver`,
  `ZEN_DB_DRIVER=…`) come with it. Connection capabilities, schema objects and constraints are now
  read with defaults for absent fields.
- Production safety in the Database workspace (ADR-0172): `zen_db::safety` judges each
  statement by the dialect's tokens (reads, session, transaction, data, structure; DELETE or UPDATE
  without a WHERE, also always-true ones and inside CTEs, DROP, TRUNCATE, FLUSHALL); read-only
  connections and the new read-only connection types are read-only on the server and refused by the
  engine for every driver (`db.connection.readOnly`, the toolbar's lock); types that confirm SQL
  execution and every risky statement ask first (code `confirmExecute` with a token; Yes, Skip, Yes
  to All, Cancel in the window, Run Anyway after ticking the risk); agents follow the type's new
  `agents` setting (Production: reads only, changes handed to a person); refused, skipped and
  confirmed statements are marked in the query history; consoles of Production carry a frame and a
  type badge.
- Copy, Copy As and spreadsheet paste in result grids: ⌘C now really
  copies from a focused grid (it arrives as a copy event) as tab-separated values quoted where
  needed; **Copy As ▸** CSV, TSV, JSON, Markdown, HTML, XML (the export writers), SQL INSERT,
  UPDATE and `IN (…)` in the connection's dialect, and Go, Rust, TypeScript, Python and Kotlin
  literals (`zen_db::clip`); Copy Column Names, Copy Row as JSON; **Advanced Copy** (⇧⌘C) with
  remembered delimiters, quote, header, row numbers, NULL text and value style; large selections in
  the background with Cancel and a cap of 100 000 rows and 64 MiB. **⌘V** and **Advanced Paste**
  (⌥⇧⌘V) read TSV and CSV from Excel, Numbers and Google Sheets (delimiter, quotes, line breaks,
  header found by themselves), check each value against its column and stage it over the cells or as
  new rows, with a preview of the problems. Commands `db.result.copy` (extended), `db.result.copyAs`,
  `db.result.copyState`, `db.copy.settings`, `db.result.paste`. Clicks on a menu or dialog over a grid
  no longer change its selection.
- Scripts bound to a connection: any `.sql` file of the project can be bound
  to a connection and schema from a bar above it in the code editor or from the Database workspace's
  toolbar; the binding is kept per project in the data folder (`databases/scripts/<project key>.json`),
  never in the file, and comes back after a restart. ⌘⏎, ⌥⌘⏎ and a run icon in the gutter per
  statement run it there, with its results under it in the Database workspace; a file bound to no
  connection never runs and asks for one first. Completion, navigation and splitting follow the bound
  connection's dialect. The navigator's SQL Editor menu lists each connection's scripts and creates a
  new one in a folder of the project. Commands `db.script.bind`, `.state`, `.run`, `.targets`, `.list`
  and `.new`.

- The query history: every statement the engine runs — consoles,
  filters, object tabs, saved edits, Commit and Rollback, exports, `db.query.run`, agents' marked —
  is kept in an append-only JSON-lines journal in the data folder (`zen_db::history`), with passwords in
  statements masked, pruned by age and size, and searched in memory. Commands `db.history.list`,
  `.get`, `.openInConsole`, `.open`, `.clear` and `.settings` (off switch, retention), and a Query
  History tab in the Database workspace with search, filters by connection, type, result and date,
  Open in Console, Run Again and Copy.

- Completion and navigation in SQL consoles (ADR-0164): `zen_db::assist`, our
  own error-tolerant resolver over each dialect's tokens (query blocks, aliases, CTEs, subqueries,
  correlated scopes), offers keywords per dialect, schemas, tables and views, columns of the tables
  in scope, functions and joins from foreign keys from the connection's snapshot, offline;
  `editor.completion.start`, ⌘B, ⌘-click, F4 (`db.sql.navigate`), F1 and the ⌘ peek work in consoles;
  unknown tables and columns are warnings where certain; Redis consoles complete commands and known
  keys. Commands `db.sql.complete`, `db.sql.resolve`, `db.sql.navigate`. Missing columns are read in
  the background only when connected.

- Export in one dialog, streamed: **Export…** under a result, **Export Data…** in the
  navigator (a table, or a schema's tables each to its own file) and **Export From Query** in a
  console write CSV, TSV, JSON, SQL `INSERT`, Markdown, HTML or XML (`zen_db::export`, shared with Copy
  As) to files named by a pattern or to the clipboard. Rows are read again from the server and
  streamed through a bounded queue to a writer thread (a million rows within a few MiB); PostgreSQL
  writes CSV and TSV with `COPY (query) TO STDOUT` (`DriverSession::copy_out`), and SQLite's output
  is now read as `sqlite3` prints it. Progress, cancel on the server, files moved into place only
  when complete, a notification with Open and Reveal in Finder, settings remembered. Commands
  `db.export.settings`, `.start`, `.state`, `.cancel`.
- Releases from a protected tag through GitLab (ADR-0171, the owner, 2026-10-27): the tag `v<YY.M.N>`
  starts a pipeline on the Mac runner (`cargo xtask release-ci build`, `stage`, `verify`, `live`)
  that builds every IDE for every platform (and Zen Toolbox and the catalog's plug-ins with a new
  version), stages them on the release server, checks every staged download (size, SHA-256,
  `codesign`), and waits for the manual `release` job, which makes the stage live in one request and
  creates the GitLab release with the notes and a link to every file. The release server stages
  (`/api/v1/stage/<stage>/…`, `POST …/release`, refused while the live manifest changed since the
  stage), and serves the website a release brings (`site/current`, Caddy). The key and token are the
  protected GitLab variables `ZEN_PUBLISHER_KEY`, `ZEN_RELEASES_TOKEN`, `ZEN_RELEASES_SERVER` of the
  `release` environment; `zen-releases` reads the key from `ZEN_PUBLISHER_KEY`, `check-credentials`
  checks them without publishing, and `cargo xtask` refuses them in a job of an unprotected ref.
  `release-publish --stage` stages from a maintainer's machine. Notes beginning with `-` no longer break
  `zen-releases publish` (`--notes=`).

### Fixed

- Saving data edits with the "all columns" virtual key (a table without a primary or unique key)
  could delete or update several equal rows at once; such a statement now rolls the whole save back
  (`severalRows`), as it already did for real keys, and the No Unique Key dialog says so.
- Filtering or sorting a table's Data page showed nothing while the rows were read again: the
  filter bar now says **Filtering…** with **Cancel**, as for a console result, and
  `db.result.filterState` answers the read as its `query`. A read replaced by a newer one (filter,
  then sort) is cancelled and its rows freed, instead of running on and holding the connection.
  The test `a_data_view_filters_with_completion_stages_edits_and_previews_them_in_light_and_dark`
  failed so under load, editing the old rows before the filtered ones came; the console test
  `a_console_runs_its_script_and_shows_results_output_and_log` now waits for the script's end, as
  its last result can show a moment before.
- A database console could open an empty extra result tab, and stay on it instead of the log when
  the script failed: a running statement's result was listed before the statement returned columns,
  so one that returns none (`SET`, an update) showed the result it drops when it ends. A query's
  state now lists a running statement's result only once it has columns (`zen_db` engine); the
  tests `a_console_runs_its_script_and_shows_results_output_and_log` and
  `a_failing_statement_says_where_and_cancel_stops_a_long_one` failed so under load.

### Changed

- Decision records (docs/adr): the record on missing language servers, symbols and run options is
  ADR-0169 now, as ADR-0121 was taken by file and folder bookmarks; `cargo xtask changelog` and the
  gate's changelog step fail when two records share a number.

### Fixed

- The file index kept no change refreshed while its first walk ran: a file written and refreshed
  just after a project opened could be left out of Go to File and Find in Files until it changed
  again, when the walk had listed its folder before. Such refreshes are now applied once the walk is
  taken in (`zen_index::FileIndex`; Search Everywhere's text test failed so under load). The test
  `an_empty_answer_says_why` holds the server's answer until its first, short question is answered,
  so that a fast answer within the 1 ms no longer reads as final.

### Added

- MySQL and MariaDB (ADR-0164): two built-in drivers, `mysql` and `mariadb`
  (`zen_db::mysql`), on `mysql_async` 0.37 without default features (rustls with the RustCrypto
  provider, flate2's Rust backend; no C), TCP only, keepalive, `caching_sha2_password` and
  `mysql_native_password`, the TLS modes, `mysql://` and `mariadb://` URIs with `ssl-mode`. Values on
  the text protocol as the server writes them (exact decimals, zero dates, bits, JSON, enums, sets,
  binary), warnings as messages, syntax error positions, row caps through `sql_select_limit`,
  `max_execution_time`/`max_statement_time` and lock waits as guards, transactions read from the
  server's status, cancel with `KILL QUERY` on a second connection. Introspection reads the
  server's databases as schemas of the catalogue `def` (the navigator skips that single catalogue
  when a driver has no catalogues, so databases show under the connection) with `CRC32` fingerprints from
  `information_schema` and `SHOW CREATE` DDL; a new object kind `event` (folder **Events**). Names in
  generated SQL are quoted for the connection (`zen_db::schema::quote_for`; `db.object.get` answers
  `driver`). Ignored integration tests run against MySQL 8.4 and MariaDB 11.4 in Docker.

- The workspace host (ADR-0163): a workspace owns an editor
  group outside the split tree (`editor.split.list` lists it under `workspaces`) and is expanded
  exactly when that group has the keyboard; `file.document.open` takes a `group`, and moving a tab
  between a workspace and the main editor is refused with the reason. `zen_workbench::workspaces`
  keeps the registered workspaces and their inner tabs, driven by `workspace.host.list`, `.state`,
  `.expand`, `.collapse`, `.toggle` and `workspace.tab.open`, `.select`, `.close`, `.move`; closing
  an empty console in `<data folder>/consoles` deletes it. The window draws an expanded workspace in
  place of the split editors (toolbar, left panel, inner tabs, sliding in from the right) and gives
  them back untouched. The editor tab row is one widget shared with it, and a middle click on a tab
  now closes it.
- Workspace buttons, keys and restarts (ADR-0163): each
  registered workspace has a button at the top of the right stripe under the Git workspace's (lit
  and "pressed" while expanded; a click runs `workspace.host.toggle`) and a "*Title* Workspace"
  entry in Find Action (`workspace.host.toggle/<id>`), reached in every mode. While a workspace is
  expanded, ⌘W (`file.document.close`) closes its active inner tab, never hidden code; ⇧⎋ collapses
  it unless a tool window has the keyboard; ⎋ never does; the Switcher and Recent Files list its
  tabs and expand it on the one picked; `editor.*` commands and the window actions that need a text
  file are refused on a tab that is not a file with a reason naming it (the conflict editor in front
  too); `get_window_state` reports `workspace: { id, expanded, activeTab }`, `editor: "workspace"`
  and the view's popups. `workspace.json` keeps each workspace's tabs, active tab, expanded state,
  consoles' carets and first lines and panel sizes, read leniently and restored without connecting;
  consoles are saved after a second of quiet even with autosave off; switching a window to another
  project keeps the workspace tabs with the project they belong to and brings back the new one's,
  collapsed.
- Spelling quick-fixes and dictionaries (ADR-0161, amends ADR-0051): typo problems
  carry Save '<word>' to dictionary (`spelling.word.save`, or the window action
  `view.spelling.saveTo`, the Select Dictionary popup, while Use single dictionary is off), and
  `problems.fixes.list` puts the suggestions before it on the word only: a greyed Fix typo (the new
  `FixKind::Heading` of `zen-project`) over the words (`spelling.typo.change`), or Typo: Rename to
  '<name>' (`refactor.rename.apply`) on a declared name.
  `zen-spell` gains `suggestions` (edit distance), `typos_with`, `word_list`, `hunspell::words` (a
  clean-room subset: `SET`, `FLAG`, `AF`, `PFX`/`SFX` with conditions and cross products,
  `NEEDAFFIX`, `FORBIDDENWORD`) and `jetbrains` (`.idea/dictionaries/*.xml`, edited in place) and
  depends on `roxmltree`. `zen_workbench::spelling` keeps the application-level dictionary
  (`<data>/dictionaries/application.dic`, `Spelling::set_application_dictionary`), the project's
  `.idea/dictionaries/` (written to `project.xml` only on request; symbolic links refused) and the
  custom dictionaries, read again when their files change; commands `spelling.typo.get`,
  `spelling.typo.change`, `spelling.suggestions.list`, `spelling.word.save`, `spelling.word.remove`
  and `spelling.dictionaries.get`; `editor.edit.undo` takes out a word just saved from the file.
  Settings → Editor → Natural Languages → Spelling, `IdeSettings::spelling` `customDictionaries`,
  `useSingleDictionary` (off by default), `savingDictionary` (`project` by default) (`set_settings`).
- Quick Switch Scheme (ADR-0154): `view.quickSwitchScheme.open` (⌃`, View → Quick
  Switch Scheme…, Find Action; `QuickChangeScheme` in imported JetBrains keymaps) opens the Switch
  popup (`ide/quick_switch.rs`), whose lists replace it in place and run `ui.theme.select`,
  `ui.keymap.select`, `view.settings.open` (Configure Keymap…), the View Mode actions, the
  `ideZoom` setting and the new `codeStyleScheme` setting (Settings → Editor → Code Style → Scheme, now a
  choice instead of a fixed label; `editor.projectCodeStyle` in the session, read by
  `project_settings::code_style_override`); `run_action` answers its rows and lists, `get_window_state`
  names it `quickSwitchScheme`. Keystrokes read and write the backtick key as `` ` `` (`` ⌃` ``),
  `Backtick` still read.
- Tool window view modes (ADR-0156): `ToolWindowState` has `mode` (`dockPinned`,
  `dockUnpinned`, `undock`, `float`, `window`) and `bounds`, kept in `workspace.json`; unpinned tool
  windows hide on a click elsewhere, undocked ones are drawn over the editor, floating and windowed
  ones in deferred viewports of their own; layouts saved per IDE in `window-layouts.json`
  (`SavedLayouts`), listed in Window → Layouts, each saved one a submenu (Restore, Save Changes,
  Rename, Delete…); commands `view.toolWindow.viewMode`, `view.toolWindow.viewMode/<mode>`,
  `view.layout.saveAsNew`, `view.layout.apply`, `view.layout.restore` (⇧F12, the current layout;
  `view.layout.restoreDefault` loses the shortcut), `view.layout.saveChanges`, `view.layout.rename`
  and `view.layout.delete`; `get_window_state` gives each tool window's `mode` and `bounds` and
  `toolWindows.layouts`. Stretch to Left/Right/Top/Bottom move to ⌃⌥←→↑↓, the stripe button's
  menu keeps Hide and Move to only.
- The New list and the File and Code Templates page (ADR-0155): the window action
  `view.newElement.open` (⌘N in the Project tool window, `NewElement` in the keymap import) opens the
  New popup `newElement` (`ui::ide::new_popup`), which the tree's New › and the native File → New
  (`main_menu::Entry::NewItems`) share; `file.new.list` lists File, Directory, the templates that
  fit the folder (built-in `markers`) and Edit File Templates…, and `new_popup::PLUGGED` adds Scratch
  File and EditorConfig File in their places (Scratch File before Directory, EditorConfig
  File after Edit File Templates…; the separate File → New submenu and the tree's EditorConfig
  button are gone). Child templates (`FileTemplate::children`, stored as
  `<name>.<ext>.child.<n>.<ext>` without `.ft`), `file.templates.list { scheme }` and
  `file.templates.useScheme { scheme }`, which keeps the project's choice in
  `file-template-schemes.json` in the data folder and deletes nothing (`FileTemplates::scheme_of`
  is a method now). Settings → Editor → File and Code Templates (`settings::file_templates`, page
  `fileTemplates`).
- IDE settings export, import and restore (ADR-0157): `zen_workbench::ide_settings`
  with the commands `settings.ide.components`, `.export`, `.inspect`, `.import` and
  `.restoreDefaults` over five components (settings, keymaps, themes, templates, plugins); our ZIP
  starts with `zen-settings.json`; a JetBrains Export Settings ZIP gives its `keymaps/*.xml` and
  `colors/*.icls`; the backup goes to `settings-backups/<date-time>` in the data folder;
  `zen_workbench::ide_settings::zip`, a small ZIP reader and writer on flate2 with hard limits (64 MB,
  4096 entries, 8 MB an entry, safe names only); `KeymapStore::import_text` and
  `ThemeStore::import_text`; File → Manage IDE Settings with the dialogs (`view.ideSettings.*`).
- External Tools (ADR-0160): programs set up once in Settings → Tools → External
  Tools (`externalTools.json` in the IDE's data folder) and run from a submenu named after their
  group in the Project view's and the editor's context menus and a shortcut
  (`run.externalTools.run/<id>`, the keymap's External Tools group); the Tools
  menu has no External Tools entry. `zen_run::macros` expands eighteen macros without a shell;
  `zen_run::external_tool` runs the `externalTool` kind with output filters as links; commands
  `run.externalTools.{list,save,replace,remove,move,copy,macros,preview,run}` and the window action
  `view.externalTools.run`, which asks for `$Prompt$` and `$FilePrompt$`.
- Power Save Mode, status bar widgets, the Memory Indicator and the Activity Monitor
  (`zen_workbench::power_save`, `zen_workbench::activity_monitor`): the window actions
  `view.powerSaveMode.toggle`, `view.notifications.balloonsOff { group }` (the notification group
  `powerSave`), `view.statusBarWidgets.open` (one `view.statusBarWidgets.open/<widget>` per
  item of View → Appearance → Status Bar Widgets) and `view.activityMonitor.open`, the
  commands `powerSave.status.get`, `diagnostics.memory.get` and `diagnostics.activity.get`, the
  settings `powerSaveMode` and `statusBarWidgets`; a server's `powerSave` settings in
  `language-servers.json` (gopls: diagnostics on save); the index defers reading changed files
  while paused (`FileIndex::set_paused`) and reports its busy time and memory.
- Java hot code replace (ADR-0167): `debug.classes.reload` (Run → Reload Changed
  Classes) and a reload by itself after java-debug's `BUILD_COMPLETE`, through its `redefineClasses`
  request (`ServerAdapter::hot_code_replace`); the outcome in the console, a Debugger notification,
  the command's answer and `reloaded` in `debug.session.get`. Every save reaches the language server
  (`Document::save_count`), also an edit saved between two syncs.
- Redis, Valkey and ClickHouse in the Database workspace (ADR-0164):
  `zen_db::redis` (`redis` 1.8 on tokio with rustls and the process's RustCrypto provider;
  standalone, `rediss://`, Sentinel and Cluster; logical databases from `INFO keyspace`, keys grouped
  by the `separator` setting and read with `SCAN`, never `KEYS`; a console of one command per line,
  replies as values; cancel through `CLIENT UNBLOCK`; a read-only connection refuses writing commands
  by `COMMAND INFO`) and `zen_db::clickhouse` (its HTTP interface through `ureq` and rustls with
  RustCrypto, results streamed as `JSONCompactEachRowWithNamesAndTypes`, cancel by `KILL QUERY` on
  the query id, an HTTP session per context, `readonly=2` and `max_execution_time` from the
  connection type, its tree of databases, tables, views, dictionaries and columns). A key opens a
  value tab (`db.key.open`, `db.key.get`, `db.key.expire`, `db.key.state`) with its type, TTL,
  size and encoding over its items; `DriverSession::key`, `Driver::open_text`, the snapshot format 3
  (cached nodes keep system flags, sizes, rows and comments). MongoDB and SQL Server wait for
  upstream TLS without a C provider.
- The Database workspace (ADR-0164): the built-in module `database`
  (`zen_workbench::database::MODULE`, on by default in GoGarden and RustRonin for now, off in WebWave
  and JvmJizo) registers the workspace `database` with its Database button and a pinned, non-closable
  Overview tab ("No connections yet" with New Console; `Workspaces::pin`). `db.console.new` creates
  `console.sql`, `console_1.sql`, … under `<data folder>/consoles/<project key>/<connection>/` (the
  placeholder connection `local` until connections exist) and opens it as a real SQL editor in the
  workspace; `db.console.list` lists the project's consoles with `limit` and `truncated`. Nothing
  connects. Feature map in `docs/database-workspace.md`, guide page "Database workspace".
- The database engine `zen-db` (ADR-0164, ADR-0165): the connection registry
  (global connections in `<data folder>/databases/data-sources.json`, project connections in
  `.zen/data-sources.json` without secrets, personal overrides per project, folders, unknown fields
  kept), connection types Development, Test and Production with their defaults
  (`connection-types.json`, colours from the theme, an unknown type treated as Production), the
  driver interface by data model and capabilities with one value type for every model (shown with an
  in-memory key-value test driver), and the PostgreSQL driver for 12–18: tokio-postgres over rustls
  with the RustCrypto provider (disable, prefer, require, verify-ca, verify-full, a CA file, a client
  certificate, PostgreSQL 17 direct TLS with ALPN), SCRAM and md5, `application_name = "Zen <IDE>"`,
  NOTICE messages, cancel through CancelRequest, TCP keepalive, exact decimals and dates as the
  server's ISO style writes them, intervals, UUIDs, JSON, arrays, NULL, and a navigator tree.
  Commands `db.dataSource.list`, `.get`, `.add`, `.update`, `.remove`, `.test` and
  `db.connectionType.list`, `.create`, `.update`, `.delete`; no answer holds a secret. `deny.toml`
  bans `ring`, `aws-lc-rs`, `aws-lc-sys`, `openssl-sys`, `native-tls` and `libsqlite3-sys`.
  Ignored integration tests run against PostgreSQL in Docker (`docs/development.md`).
- The connection lifecycle (ADR-0164): one tokio runtime on the `zen-db` thread,
  started by the first operation; commands start operations and answer at once, a waker redraws the
  window. Connect opens Main, then Metadata; each console gets its own session, an embedded driver
  shares one; a connect while one runs answers that one. Guards: `statement_timeout` from the type
  (Production 300 s) or the connection, `lock_timeout` and `idle_in_transaction_session_timeout` on
  the IDE's own Metadata context. A session idle for 30 s is pinged before use; a lost one reopens
  with backoff, restoring only tracked state (schema, isolation, auto-commit, read-only); a plain
  read in auto-commit is retried once, a write never (`notReplayed`). Cancel asks the server and
  ends within a second or says why (`cancelUnconfirmed`); deadlines cancel the same way. Idle
  connections close after the type's time and idle transactions roll back, with a notification;
  after the computer sleeps, connections close and reopen on next use. A pooler option (pgbouncer)
  uses unnamed statements and never holds a transaction. Commands `db.connection.connect`,
  `.disconnect` (open transactions refused unless `commit` or `rollback`), `.disconnectAll`,
  `.invalidate`, `.status`, `.cancel` and `db.operation.state`. The Overview lists the connections
  with their state and Connect/Disconnect.
- Secrets in the macOS Keychain (ADR-0165): the new crate `zen-secrets` (L0) with
  `SecretStore`, a `Secret` that never prints and is zeroed when dropped, the Keychain store
  (`keyring-core` and `apple-native-keyring-store`, items of the service
  `space.ifzenelse.zen.database`, accounts `<connection id>/<kind>`) behind a per-process cache
  that reads each item once per run and does not ask again after a denial, an unsupported store on
  other systems, a memory store for tests, and `database::delete_all` for deleting a connection.
  `db.connection.setSecret { connection, kind, value, scope? }` stores a secret of an existing
  connection and never echoes it; no command reads one. The engine uses it: `zen-db`'s own secret
  module is gone, a saved password is read through the cached store on `zen-db`'s runtime when
  connecting or testing (never on the UI thread), `hasPassword` comes from the registry
  (`passwordSaved`) without reading the Keychain, removing a connection deletes its secrets, and
  turning Save password off deletes the saved one.
- The connection dialog (ADR-0165): one dialog with the drivers and a
  search on the left and the driver's fields beside, on the pages Main (by host or by URL: a pasted
  URI fills every field, also pasted into Host; show all databases; authentication, user, password,
  Save password), SSH/SSL (SSH host, user and port kept; SSL mode, CA file, client certificate and
  key, direct TLS), Advanced (the driver's declared settings and a driver properties table) and
  General (name, connection type, folder, global or project, read-only, navigator view, system
  objects, schema filters, auto-commit, isolation level, default schema, keep-alive, bootstrap
  queries). Fields come from the driver's capabilities, so the key-value test driver gets its own.
  Test Connection runs in the background, can be cancelled and saves nothing; it shows the server's
  version and round-trip time, or the error with its likely cause (DNS, refused, TLS,
  authentication, timeout). Opened by **New Connection…** and **Edit** on the Overview, the
  workspace toolbar, Find Action ("New Database Connection…") and `view.dataSource.open` with fields
  to fill in. New: `db.driver.list`; `db.dataSource.test` takes unsaved fields and answers `test {
  serverVersion, connectMs, roundTripMs }` and `cause`; URIs `rediss://`, `mongodb+srv://` and
  several hosts; `bootstrap.isolation`; PostgreSQL's `keepAlive` setting.
- Running SQL in consoles (ADR-0164, ADR-0168): our
  own line lexers split statements per dialect (`zen_db::sql`: quotes, `E''`, comments, `$$`/`$tag$`,
  `BEGIN … END` blocks, `DELIMITER`, `GO`, psql meta-commands; blank lines Always, Smart or Never)
  and follow edits incrementally. ⌘⏎ runs the selection or the statement under the caret, ⌥⌘⏎ the
  script, ⌃⌘⏎ into a new tab, ⌘F2 cancels (keymap context Database, group "Database"); a console's
  connection and schema are chosen in the workspace's toolbar and kept with its tab. Queries run
  statement after statement in the console's session, stop and roll back at an error, keep NOTICE
  output and times; the editor shows each statement's time and rows after it and underlines a
  failure where the server points. The results panel has a header line, result tabs (pin, rename,
  close), Output and Execution Log; results are typed columns within a 256 MiB budget, read through a
  PostgreSQL portal to a row cap and committed outside your transaction, kept open inside it, read
  again past the cap; `timestamptz` in the session's zone. A virtualised grid (u64 row offset,
  cached cells, resize, fit, selection, copy, NULL, numbers, booleans), Text and Record views.
  Commands `db.script.split`, `db.console.statement`, `.nextStatement`, `.previousStatement`,
  `.execute`, `.executeInNewTab`, `.executeScript`, `.executeFromCaret`, `.chooseConnection`,
  `.chooseSchema`, `.schemas`, `.state`, `.output`, `.toggleResults`, `.maximizeResults`,
  `.showPanelTab`, `db.operation.cancel`, `db.query.run`, `.state`, `.cancel`, `db.results.list`,
  `db.result.page`, `.fetchNext`, `.fetchAll`, `.count`, `.refresh`, `.pin`, `.rename`, `.close`,
  `.view`, `.copy` and `db.settings.get`, `.update`.
- Database introspection with an on-disk snapshot: `zen_db::schema` holds the
  structure model (catalogues, schemas with a level `notRead`/`names`/`details`, tables, foreign
  tables, views, materialised views, functions, procedures, sequences, types; columns with type,
  nullability, default, identity, generated expression, collation and comment; constraints, foreign
  keys, indexes, triggers, policies, partitioning, sizes and row estimates; system and extension
  objects flagged). PostgreSQL reads it in bulk on the Metadata context: one query per object kind
  for every changed schema, sent together (pipelined); a schema with more than 2000 objects is read
  by names and its objects' details on demand. Per-schema fingerprints (row counts and summed `xmin`
  over `pg_class`, `pg_attribute`, `pg_attrdef`, `pg_rewrite`, `pg_proc`, `pg_constraint`,
  `pg_index`, `pg_trigger`, `pg_policy`, `pg_type`, `pg_enum`, `pg_sequence`, `pg_inherits`,
  `pg_description`) decide what to read again; nothing is installed on the server. The snapshot is a
  postcard file with a magic, version, length and CRC-32 header, written through a synced temporary
  file and renamed, at `<data folder>/caches/databases/<global|project key>/<id>.snapshot`; it is
  read before connecting on a thread of its own, refreshed in the background on connect and after
  DDL run anywhere (`Event::StructureChanged`, `Driver::changes_structure`). Drivers whose tree is
  their own (key-value, document) keep their `children` answers in the same snapshot.
  `Driver::ddl` generates DDL offline for tables, views, functions, procedures, sequences, types,
  indexes, constraints and triggers, with the toggles nested objects, permissions, separate foreign
  keys, separate constraint indexes, comments and partitions (all off). New commands:
  `db.schema.children`, `db.schema.refresh`, `db.object.get`, `db.object.ddl`, `db.object.open`,
  `db.object.copyName`. New dependencies: `postcard` (without heapless) and `crc32fast`.
- SSH tunnels and TLS for databases (ADR-0165): a connection with an SSH
  host connects through the system's OpenSSH (`ssh -N -L` to a free loopback port, `~/.ssh/config`
  aliases, keys, agent and ProxyJump; jump hosts in the dialog; one ssh process per tunnel, no
  multiplexing), opened before the first session and for Test
  Connection, closed with the connection, reopened when ssh dies; sessions keep the database's host
  name for TLS (`hostaddr`). ssh's questions come through the IDE as its askpass helper
  (`SSH_ASKPASS_REQUIRE=force`, a private Unix socket, nothing written to files or arguments): a
  saved SSH secret answers once, otherwise a dialog asks (Save in Keychain; unknown host keys only
  on Yes). Failures say what ssh said (error code `ssh`). Every TLS mode is tested against the
  Docker image's certificate. New commands: `db.ssh.prompts`, `db.ssh.answer`; `ssh.jump`; tokio's
  `process`, `io-util` and `net` features in `zen-db`.
- Transactions you can see: every console (and the Main context) has a
  commit mode — **Auto**, **Manual** or **Smart** (auto until the first statement that does not
  start with SELECT, SHOW, USE, SET or EXPLAIN, back to auto when it fails, and after Commit or
  Rollback when the type says "return to auto-commit"; off by default) — and an isolation level, from
  its connection type (Production: manual) and kept per console across reconnects. The toolbar shows
  the mode (with isolation and Pending Transactions… in its menu), Commit (⌃⌘K) and Rollback (⌃⌘Z),
  disabled in auto-commit, and a box with **Auto**, **None** or the statements of the open
  transaction, green, then orange from 5, red from 20, with the statements that changed data and the
  age in its tooltip; a click opens the transaction log. A console tab with an open transaction
  carries a dot. Closing a console, disconnecting, Disconnect All, Reconnect, switching a console's
  connection and leaving manual commit with changes pending ask **Commit / Rollback / Cancel**
  (refusals with code `openTransactions` carry the pending transactions and the command to run
  again; a transaction of reads only is rolled back without asking). An idle transaction is
  announced with a 10-second countdown (Keep Open, Roll Back Now) before it is rolled back, with a
  notification. **Pending Transactions** lists every open transaction with Commit and Rollback. New
  commands: `db.transaction.commit`, `.rollback`, `.mode`, `.status`, `.pending`, `.postpone`,
  `db.console.close`; `db.connection.invalidate` and `db.console.chooseConnection` take
  `transactions`; `db.console.state` answers `transaction`. Workspaces can guard closing their tabs
  (`Workspaces::guard_close`).
- The Database Navigator: the Database workspace's left panel with its tree:
  folders of connections, connections (driver icon, state dot, error badge, lock when read-only,
  host:port, a tint of the connection type's theme colour), databases, schemas, folders per object
  kind, objects with a size bar and their Columns, Constraints, Foreign Keys, Indexes, Triggers,
  References, Policies and Partitions; children from the snapshot at once, else read in the
  background with a "Loading…" row; an error collapses its node and badges it. Toolbar (New
  Connection ▾, Connect, Reconnect, Disconnect, Refresh, Collapse All, Link with Editor, Options with
  the Simple/Advanced/Custom view, Show System Objects, Show Host, Show Descriptions, Show
  Statistics, Show Connected Only, Hide), a filter box (typing in the tree starts it; matches among
  what was read show with their ancestors expanded), keys (arrows, Home, End, ↩/F4/⌘↓ open, F4 on a
  connection edits it, ⌘N adds a connection, F5 and ⌘R refresh, ⎋ clears the filter), context menus (SQL Editor ▸ Open
  Console, Edit Connection…, Connect, Reconnect, Disconnect, View Data, Generate SQL ▸ DDL, SELECT,
  INSERT, Copy Name, Copy Qualified Name, Refresh), resizable and collapsible, AccessKit tree items.
  Expanding a disconnected connection connects it. Expansion, selection and the view options
  are kept per project in `<data folder>/databases/<project key>/navigator.json`. New commands:
  `db.navigator.state`, `.expand`, `.collapse`, `.select`, `.refresh`, `.filter`, `.options`.
- Databases the project names, and imports (ADR-0165): `.env` files (`DATABASE_URL`,
  `*_DATABASE_URL`, `REDIS_URL`, `MONGO_URL`… and the `PG*`, `POSTGRES_*`, `MYSQL_*` groups) and
  Compose files (PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, ClickHouse, Elasticsearch,
  OpenSearch, Cassandra and ScyllaDB services with their published ports and credentials) are
  scanned in the background, bounded, without excluded or ignored folders, and again when they
  change. The Overview lists them under "Detected in this project" with **Add…** (the connection
  dialog filled in; a password found goes to the Keychain only on OK) and **Dismiss** (kept per
  project until the file names another database); a changed file offers **Update…**; the dialog
  shows them as links. Nothing connects. **Import Connections…** reads DBeaver's `data-sources.json`
  and a JetBrains project's `.idea/dataSources.xml` (with the user names of `dataSources.local.xml`),
  lists them, and imports the ticked ones, never their passwords and never changing their files.
  Connections keep their `origin`. Agents never get a password, and offers from files hidden from
  them come without values. New commands: `db.detect.list`, `.accept`, `.dismiss`,
  `db.import.dbeaver`, `db.import.jetbrains`; the window action `view.dataSource.import`;
  `view.dataSource.open` takes `detection`.
- SQLite (ADR-0164): the driver `sqlite` runs the system `sqlite3` program, one
  long-lived process per connection shared by every console, fed one checked statement at a time
  (never a dot-command) in quote mode with markers on both output streams; values by storage class
  (integer, real, text, blob as bytes, NULL), `changes()` as rows affected, SQLite's errors with
  their positions, row caps, manual commit with `BEGIN DEFERRED`/`IMMEDIATE`/`EXCLUSIVE`, read-only
  files and `PRAGMA query_only`, cancel and statement timeouts by SIGINT (the session then opens
  again). The navigator shows `main`, `temp` and attached databases with tables, views, columns,
  keys, foreign keys, indexes and triggers, re-read when `schema_version` changes; DDL is SQLite's
  own. The connection dialog's Main page is **Path** with **Open…** and **Create…** (never replacing
  a file), and the `sqlite3` program's path is a setting with **Browse…**; a project connection's
  relative path starts at the project's folder; connecting never creates a file. Opening a `.db`,
  `.sqlite`, `.sqlite3`, `.db3` or `.s3db` SQLite file from the Project tool window selects its
  connection in the Database workspace or offers a new one. New commands `db.databaseFile.create`
  and `db.databaseFile.open`.
- Object editors: `db.object.open` now opens a table, view, materialised
  view, foreign table, function, procedure, sequence, type or schema as an object tab of the Database
  workspace (kind `object`, coloured by the connection type), with the pages Data · Properties · DDL
  · Diagram (tables open on Data; the rest on Properties). Data reads `SELECT *` ordered by the
  primary key (or a unique non-null key) in the Main context into the result store and shows it in
  the results grid, only by itself when connected (`db.object.readData` connects); Properties shows
  the header (name, comment, owner, tablespace) and the sections from the snapshot (columns,
  constraints, foreign keys, indexes, triggers, references, policies, partitions, statistics,
  permissions; a view's or routine's source, a sequence's or type's settings, a schema's objects);
  DDL the offline generator's text with its toggles, Copy and Open in Console; Diagram is a
  placeholder. The tab keeps its page, section and DDL toggles in `workspace.json`, never rows, and
  frees its rows when closed. The navigator opens objects on ↩, F4 and a double click (F4 a schema
  too) and has Open Object. New commands: `db.object.state`, `.showPage`, `.showSection`,
  `.properties`, `.ddlOptions`, `.readData`; `db.result.refresh` reads an object tab again.
  Introspection reads tables' tablespaces (snapshot format 3), and the DDL generator writes a
  table's `TABLESPACE` and a schema's `CREATE SCHEMA`.
- The value panel and Calc: beside every result grid (Value Panel, ⇧⌘P,
  `db.result.toggleValuePanel`), the selected cell as Text (arrays as SQL literals), JSON (pretty,
  folding), XML (indented, folding), Hex or Image (pictures decoded off the UI thread), with Copy and
  Save…, for every driver's values; Calc with count, distinct, sum, average, minimum and maximum of
  the selection, also in the status bar. The record view shows whole values and selects the field
  clicked. `zen_db::present` renders values and sums selections. New commands: `db.result.value`,
  `.saveValue`, `.calc`; the settings `valuePanel` and `valueFormat`.
- Filtering and sorting data views: a WHERE filter box above every result grid
  with completion of columns and keywords, back and forward, history per table and saved filters
  (`<data folder>/databases/filters.json`), the statement applied shown beside it; conditions are
  checked to be one balanced expression (`zen_db::filter`) and go into a table's generated SELECT,
  merged into a plain SELECT or around any other read-only query as a subselect, read again in the
  background with the old rows kept until the new ones come; sort marks in the headers (⇧-click adds
  a column; in memory when every row is loaded); quick filters from a cell; find in loaded rows.
  New commands: `db.result.filterState`, `.filter`, `.sort`, `.filterByValue`, `.resetFilter`,
  `.filterBack`, `.filterForward`, `.completeFilter`, `.distinctValues`, `.find`, `db.filter.save`,
  `.delete`; ⌥⌘F focuses the box.
- Editing data with a change preview: staged cell edits (typed input per column type,
  NULL, default), new, duplicated and deleted rows, copy from the row above, marked in the grid and
  on the tab; editable only for one table with a key on a writable connection (primary key, unique
  non-null key, `ctid`, a virtual key kept in `.zen/databases/virtual-keys.json`, or all columns),
  read-only with the reason otherwise; Preview SQL (forced on connection types that confirm data
  changes); Save (⌘S) runs deletes, inserts and updates with real bind parameters in one transaction
  (a savepoint inside an open transaction), checks the changed cells' old values, reports conflicts
  and rolls back on the first error, and reads saved rows back; Cancel (⌥⌘Z); Generate SQL. Result
  columns carry their table column (`Column.source`), `DriverSession::execute_bound` binds text
  parameters, `zen_db::edit` plans the statements, `Engine::save_edits` runs them. New commands:
  `db.result.editState`, `.setCell`, `.revertCell`, `.addRow`, `.duplicateRow`, `.deleteRows`,
  `.copyFromAbove`, `.previewChanges`, `.save`, `.revert`, `.setRowKey`, `.generateSql`.

### Fixed

- Storing and deleting database secrets (`db.connection.setSecret`, `db.dataSource.add`/`update`
  with a password or with Save password off, `db.dataSource.remove`) runs on zen-db's runtime like
  reading, never on the window's thread, as operations (`storeSecret`, `deleteSecrets`) the
  commands answer at once (`operation`, `secretOperation`), in the order asked; a failure has error
  kind `secret`, is posted as a notification, and a failed store takes back the saved-password mark.
  Removing a connection no longer fails when the Keychain refuses.
- A second start of an IDE (`<ide> <file>`, a double-click) could open a window of its own although
  the running IDE had taken its files: when the running IDE answered and closed the connection before
  the second start read the answer, macOS refused the second start's read timeout (`EINVAL`) and the
  answer was dropped. Timeouts are now set while both ends are open, and a refused one no longer
  drops an answer (`zen_workbench::handoff`; the hand-off test's flake, 3 in 160 runs under load).
- The hierarchy test `answers_taken_in_by_any_reader_change_the_revision` holds the server's answer
  of the callers until it has read the revision; a fast server answered within the call and left the
  reader nothing to take in (2 in 120 runs under load).
- `IdeView::appearance_title` named the View → Appearance items only while a mode was on; with
  none on, the macOS menu showed the keymap's Toggle names instead of Enter.
- Review of the Java plug-in, debugging and tests, with Eclipse JDT LS installed for real by
  `lsp.server.install` (archive and java-debug downloaded and checked): a language server the IDE
  installed in its data folder (`language-servers`) was missing from `lsp.servers.list` over MCP,
  as the data folder is hidden from agents (ADR-0143) and the list's entry named a path in it; that
  folder is now let through like the scratches. The Java plug-in 0.1.1 starts JDT LS with
  `-Djava.import.generatesMetadataFilesAtProjectRoot=false`, so `.project`, `.classpath` and
  `.settings` stay in the IDE's cache instead of the project.
- Publishing checks the release server first: `zen-releases` (and so `release-publish`, `release
  plugin`, the Linux and Windows publishes) asks the server to read the new signed manifest or
  plug-in catalog (`PUT /api/v1/check/manifest|plugins`, new) before uploading any artifact or
  module, and a server too old for the format refuses with "deploy the release server first"
  (the Java and Kotlin plug-ins met HTTP 422 after their modules were uploaded, 2026-10-26).
- The hand-off test every IDE runs is one function, `zen_testkit::handoff`, called by each app's
  tests (the JvmJizo copy had drifted); it keeps reading the IDE's output, ceilings of a minute,
  and a failure shows the IDE's log and the second start's error. `xtask` checks every IDE's app
  calls it.
- Reload All Maven Projects and Reload All Gradle Projects now ask the Java language server to
  import the build again: they tell every running server that watches the build's files
  (`pom.xml`, `build.gradle(.kts)`, `settings.gradle(.kts)`) that they changed
  (`Languages::build_changed`). The `BuildEnv::set_reimport` hook of ADR-0149 was never set, so
  Reload always answered `reimported: false` and left the server's import as it was.

### Fixed

- More tests wait for what they check rather than a short clock, for the CI Mac running two jobs at
  once: the hierarchy tests wait on with `navigate.hierarchy.get` until
  a tab is no longer pending (`settled`), the language test fixture gives the fake server a minute
  to have a file, and 70 waiting loops in tests have a ceiling of a minute instead of 2 to 10 s.

### Fixed

- Tests no longer fail on a busy machine: wall-clock budgets of speed
  are hand-run checks (`ZEN_TIMING_CHECKS=1`, or ignored timing tests in zen-vcs); tests that meant
  "does not block" or "does not hang" check it without the clock (a server that reads nothing until
  told, a run that waits for a gate the test opens, a grace of ten minutes for a second stop,
  `Process::stop_within`) or with ceilings of a minute; polling waits allow a minute. A second start
  waits up to 20 s for the running IDE's answer once connected (the IDE still gives a stalled client
  5 s), and the IDE takes the handed-over paths under a lock of their own, never behind a busy
  frame's.
### Changed

- JvmJizo is released with the other IDEs from 26.10.2, a preview not announced yet (ADR-0147): `release::IDES` has it (`UNRELEASED_IDES` is empty), so `release-build ides` builds it
  and `release-publish` takes it; the website lists it as available; the guide's Java page and the
  READMEs no longer say it is not released.

### Added

- Kotlin (ADR-0151):
  - **The plug-in:** `plugins/kotlin` takes `kt` and `kts` (Gradle's Kotlin DSL too) with our own
    lexer: highlighting, folding, Structure, the DSL's blocks as modules. It declares two servers,
    the Kotlin team's kotlin-lsp (the default, with a notice of whose software it is) and fwcd's Kotlin
    Language Server, plus the Kotlin file templates.
  - **Platform:** ADR-0148's `archive` install kind gains per-platform archives (`platforms`), zip
    archives and versions read from the URL's folders.
  - **Choosing among servers:** a language several servers serve gets a choice
    (`LanguageServers::alternatives`, `choose`, `lsp.server.choose`, "Server for …" in Settings →
    Language Servers; the banner offers only the chosen one).
  - **Notices:** a server may carry a `notice`, shown where it is offered and installed.
  - **Icons:** a Kotlin Script file icon.
  - **Completion:** a server's incomplete list (`isIncomplete`) is asked for again, with trigger kind
    3, once the typed word changes; the new answer replaces the server's earlier items (fwcd's
    server caps an unfiltered list).
- Tool windows in a headless IDE: each project keeps a virtual window, so
  `run_action`, the `view.*` actions, `get_window_state` and the tree's selection work over
  `mcp --headless` (only `press_keys` needs a drawn frame). The Maven and Gradle tool windows read
  their build while they show, drawn or not, `get_window_state` lists a shown one's rows
  (`buildTools.<tool>.rows`), and `view.buildTool.select|toggle|run { tool, row }` select, open and
  run a row as a click and a double-click do.

- Linux and Windows release builds on the Mac runner without Docker (ADR-0150):
  `release-build --platform macos|linux|windows|all` (CI: `PLATFORM`) cross-builds Linux x86_64 with
  `cargo zigbuild` (glibc 2.35) and Windows x86_64 with MinGW-w64, for the IDEs (all or none) and
  Zen Toolbox, each platform a folder with `build.json`; the `.deb` is built in xtask (`deb.rs`, ar
  and tar, owned by root); `release-publish` publishes each platform and uploads the `.deb`/`.zip`,
  macOS first, every IDE on each platform, and a Linux or Windows build only of a version published
  for macOS or with it. `release-linux` and `release-windows` (Docker) stay as the fallback.
- Maven and Gradle (ADR-0149):
  - **Run kinds and commands, in every IDE:** run kinds `maven` and `gradle` (`zen_workbench::maven`,
    `zen_workbench::gradle`), the project's wrapper first, adopting shared Maven and Gradle
    run configurations.
  - **Commands:** `maven.project.get|goal.run|options.set|project.reload|dependencies.tree|dependency.open|distribution.download`
    and `gradle.project.get|tasks.list|task.run|project.reload`.
  - **Reading the build:** POMs read with `roxmltree`; `dependency:tree` and `gradle tasks --all`
    output parsed.
  - **Shared environment:** `zen_workbench::build_tools::BuildEnv`, with the JDK and re-import hooks
    for ADR-0147/0148.
  - **Tool windows:** the Maven and Gradle tool windows (`view.toolWindow.maven|gradle`), listed only
    while their plug-in supports the project.
  - **Plug-ins:** `plugins/maven` (`pom.xml` by name: XML plus POM completion) and `plugins/gradle`
    (a Groovy lexer of our own for `*.gradle`).
- Java debugging and tests (ADR-0153): debug adapters a language server
  runs (`zen_run::AdapterLine::server`, `ServerAdapter`, `LaunchResolve`; java-debug 0.53.1 from
  Maven Central installed as an archive server's `files` and loaded as a JDT LS bundle), programs
  debugged by attaching (`DebugPlan::debuggee`), `attach` requests, exception breakpoints
  (`debug.exceptionBreakpoint.set|list`); `zen_workbench::java`: the tests of a file for the gutter,
  the `junit` and `testng` run kinds run by Maven (Surefire's plain report) or Gradle (an init script
  that prints `##zen-test` lines), with Rerun Failed Tests and debugging (Maven in its own JVM,
  Gradle's test JVM), Application and JAR Application (ADR-0152) debugged by attaching to their own
  `java` line (`attach_plan`), `application_plan` for a `main` class java-debug launches; a JVM failure's message starts at
  its exception; `javaNeeded` in `lsp.servers.list` and a **Download JDK** banner
  (`toolchain.jdk.download`); `java.configuration.updateBuildConfiguration: automatic`; real-tool
  tests in `tests/java_debug_and_tests.rs`.
- Java run configurations in every IDE (ADR-0152): `zen_workbench::java_run`, the run
  kinds `java.application` (main class, module, JRE, build before run; VM options are the tool
  arguments) and `java.jar` (`java -jar`). The classpath comes from the build tool: Maven
  (`compile dependency:build-classpath` plus `target/classes`, joined by a small Java helper run
  with the source launcher), Gradle (an init script's `zenClasspath` task writing the runtime
  classpath), or `javac` into the run's folder at the project's language level; it reaches `java`
  as `-cp @file` with quoting, so long classpaths and spaces work. Gutter icons on classes with a
  `main` (static, instance and implicit-class mains of Java 21+, parsed in Rust), `run.context.get`,
  stack-trace links to `pkg/File.java` in the source folders, adoption of shared
  Application and JarApplication configurations. zen-ui's macOS-only menu code no longer warns on
  other platforms.

- The Java plug-in and Eclipse JDT LS (ADR-0148): `plugins/java`
  (ABI v1) with its own Java lexer and parser in Rust (highlighting with contextual keywords,
  folding, the Structure, completion and snippets), seven file templates and the `${PACKAGE_NAME}`
  template variable; language servers installed from an archive (`install.kind: "archive"`, `url`,
  `sha256`; `zen_workbench::language::servers::archive`; `zen_update::fetch::download`) into
  `<data>/language-servers/<id>`, and launch placeholders (`${serverFolder}`, `${glob:…}`, `${os}`,
  `${archSuffix}`, `${workspaceData}` in `<data>/caches/language-servers/<id>`, `${javaHome}` with
  the declaration's `java` version; `LanguageServers::set_java_finder`); JDT LS 1.63.0 runs on Java
  25; a real-server test of a Maven and a Gradle project (`tests/java_language_server.rs`).
- JvmJizo, the IDE for Java and the JVM (ADR-0147): `apps/jvm-jizo`
  (product id and launcher `jvmjizo`, bundle id `space.ifzenelse.jvmjizo`, MCP port 63533, its tile
  and Kozō), `PRODUCT_PLUGINS` `java`, `maven`, `gradle` with `PENDING_PLUGINS` until they land, in
  `release::UNRELEASED_IDES` until its release moves it to `IDES` (built and tested,
  never in `release-build ides`; `changelog/jvmjizo.md`; the released IDEs' changelogs compared
  from the newest IDE's first release), its Kozō on the website, the guide's Java page. JDKs in every IDE (`zen_workbench::jdk`): detection (`JAVA_HOME`, `java_home -V`, the
  JVM folders, Homebrew, SDKMAN!, asdf, mise, Gradle and Maven toolchains; `release` files), the
  project's JDK and language level kept in `jdks.json`, JDKs added by hand, Eclipse Temurin downloaded
  from the Adoptium API on the user's word (`zen_update::fetch::checked_with_progress`), Settings →
  Languages & Frameworks → JDKs, `toolchain.jdk.*`; language servers of JVM languages and run
  configurations of Maven and Gradle projects start with `JAVA_HOME` and `PATH`, and
  `${jdkHome}`, `${javaVersion}`, `${javaLanguageLevel}` expand in server arguments and options.
- Settings → Project Structure and resource roots (ADR-0159): the first top-level
  Settings page (`projectStructure`, ⌘; = `view.projectStructure.open`, File → Project Structure…,
  JetBrains `ShowProjectStructureSettings`): the content roots as a
  tree of folders, Mark as: Excluded / Resource Root toggles, Exclude files under the tree, and on
  the right the content root with Excluded Folders and Resource Folders, × per row; Apply / OK run
  the commands for what changed. Mark Directory As → Resource Root / Unmark as
  Resource Root in the Project tool window, resource roots drawn with a purple
  `FolderKind::ResourceRoot`. `<sourceFolder>` and `<content url>`
  are read (`ProjectSettings::source_folders`, `content_roots`); `IdeaProject::set_resource_root`
  and `set_exclude_patterns` write the module file byte for byte, one mark per folder. Commands
  `project.directory.markResourceRoot` / `unmarkResourceRoot`, `project.structure.get` and
  `project.structure.setExcludePatterns`; the folder commands answer `resourceRoots` and
  `excludePatterns` too. `⌘;` parses as `Semicolon`.

### Security

- HTTP environment files and Local History's count (review of dc0cf303, ADR-0143 item 6):
  `http-client.private.env.json` joins the default hidden patterns; an agent's
  `http.request.run` uses no value of a hidden environment file and is refused when it needs one
  (`without_hidden_environments`); `localHistory.versions.list`'s `hiddenFiles` is the project's
  count (`hidden_files::count`), not the folder's.
- Hidden files out of HTTP requests, copies and Local History (independent review of 7b97fb38,
  ADR-0143 item 6): `zen_http::Prepared::files` and `Exchange::body_files` record the files a body
  reads; an agent's `http.request.run` reading a hidden one is refused, and `http.responses.list`
  / `http.response.get` blank both bodies of such an exchange for agents.
  `hidden_files::guard_copy` makes `file.fs.copyInto`, `project.file.copy`/`cut` and
  `project.file.paste` refuse a hidden source, a folder holding one, or a copy landing in a hidden
  place; these and `localHistory.*` now take paths relative to the project.
  `localHistory.versions.list` leaves hidden files' versions out (`hiddenFiles`), the folder diff
  their names, and a folder revert that would change one is refused. Terminal words
  `<revision>:<file>` (`git show HEAD:.env`) are checked as files. An agent's
  `vcs.changes.discard {all}` reverts tracked files only and deletes no untracked file or link
  (`IndexResult::untracked_kept`, the owner 2026-10-25).
- Agents neither read nor change hidden files (review of 0.2.25, the owner 2026-10-25, ADR-0143
  item 6): `vcs.patch.create` leaves them out of the patch, `files`, clipboard and output
  (`hiddenFiles`); the answer redactor cuts their `diff --git` sections out of any text and hides
  the IDE's data folder (`Redactor::with_store`, but `scratches` and `buffers`);
  `hidden_files::{guard_change, guard_revision, guard_commit, without_hidden, hidden_changes,
  hidden_inside}` refuse an agent's change of a hidden file or a folder holding one (delete,
  rename, paste-move, scratch move, new file, rollback, discard, stage, shelve, unshelve, stash
  apply/pop, patch apply, checkout, merge, rebase, fast-forward, cherry-pick, revert, hard reset,
  worktree removal, renames and intentions) and leave them out of sets (stash via
  `Repository::stash_push_except`, a changelist's commit and shelve, Discard/Stage all, Replace
  in Files, line separators of a folder). Headless tests reproduce each case.

### Changed

- Trademarks checked for JvmJizo's public release: Oracle's and the Kotlin
  Foundation's guidelines read on 2026-10-26 and summed up in docs/naming.md with what our texts
  say; their credit lines in About (`menu-app-about-trademarks`, en and ru), the bundle's copyright,
  the website, README.md and TRADEMARKS.md; JvmJizo's tag line "for Java and the JVM"; open for the
  owner: Oracle's claim on "JVM" (the name JvmJizo) and the terms of the kotlin-lsp binary (a
  question still open).
- One calendar version for every IDE (ADR-0146): the IDEs are released together as
  `YY.M.N` (`26.10.1`), shown alone; release channels are deferred to a later ADR (the owner,
  2026-10-25), so IDE releases stay on `stable` and Zen Toolbox's `toolbox.channel` is unchanged.
  `cargo xtask release-version` gives this month's next version (local time) and `release ides` /
  `release-build ides` refuse any other (a typo in the year or month, a skipped number) unless
  `--allow-version` (CI: `ALLOW_VERSION=1`); `release-publish` publishes the IDEs only as the whole
  set at one calendar version and commit, verifying all first, and says what a rerun finishes if it
  stops midway. Changelog headings `## 26.10.1 - <date>` in every IDE's file, checked to agree. CI's
  `release-build` builds every IDE in one job with `VERSION` (PRODUCT `ides` by default).

### Changed

- JvmJizo, the JVM IDE, Java first (ADR-0145): the product id `jvmjizo` in Zen Toolbox's
  catalog ("Java and the JVM" / "Java и JVM"), the tile colours and `assets/icons/products/jvmjizo.svg`,
  `zen_mascot::Variant::JvmJizo` (a stone Jizō with a red bib, a maple leaf drifting down), the
  site's family list, docs/naming.md, README, NOTICE and TRADEMARKS. `cargo xtask mascot --sheet
  <variant> <folder>` writes a contact sheet of a Kozō's frames for review.

### Security

- Hidden files decided on every path a command takes (review of 0.2.24, ADR-0143): `Redactor::named_in`
  checks every string of a call that names an existing file or folder, not only the known path keys
  (`file.compare.files` `left`/`right`, `file.compare.three` `middle`); the answers' content keys
  include `before`, `after`, `texts`, `hunks`, `diff` and the like;
  `no_command_hands_out_a_hidden_file_through_any_string_parameter` walks the catalogue.

- Hidden files decided on the real path (ADR-0143
  item 4): `zen_workbench::hidden_files::{real_path, is_hidden}` (links, `..`, the disk's case;
  patterns without case on macOS and Windows) for reads, writes, opens, search scopes and counts,
  answers' lists, the document in front and terminal words; `redact::acts_on_active_document` checks
  the document in front for every command of its groups unless the command's schema requires a file.

### Changed

- The contact for security reports and Code of Conduct enforcement is support@ifzenelse.space
  (SECURITY.md, CODE_OF_CONDUCT.md). Tests and examples use neutral project names instead of
  real ones.

### Added

- Settings search: a query in an action's name (as shown or in English), or with
  a word starting a word of one, keeps Keymap with its own search set to the query; the current
  page stays while it matches, else the first match titled by the query, else the first match
  (`search::titled`, `search::page_for`); one naming an installed plug-in (its name, id or a
  file type) keeps Plugins on Installed filtered by it (`settings::search::SearchIndex`, read on the
  first keystroke, the plug-ins again after a second); `view.settings.open { query }` answers what was found, searched
  in the interface's language (`zen_i18n::interface_language`); the Keymap page's own search also
  matches the English names.
- Search Everywhere reads a typed query in the other keyboard layout: the
  ЙЦУКЕН ↔ QWERTY table moves from the Projects popup to `zen_index::layout`; `SearchRequest.typed`
  (the popup sets it while the query is typed rather than pasted or taken from the history;
  `typed` of `search.everywhere.query` for agents) makes every contributor but the fallback Text
  also search the query's other forms, their items after the typed text's, each listed once.

### Security

- An agent can no longer read a hidden file through the active tab (ADR-0143):
  `redact::acts_on_active_document` (commands on the document in front given no path, `run_action`,
  `press_keys`) is refused with `redacted` when the window's active document is hidden.
- Files hidden from agents out of agents' searches (ADR-0143):
  `zen_workbench::hidden_files` (`HiddenFromAgents`, set on every window before an agent's call;
  `search_scopes::resolve_in` leaves the files out of an agent's scope; `SearchResult::hiddenFiles`);
  `zen_core::macros::by_agent`; answers' lists drop the entries of hidden files instead of
  `[redacted]`. `set_settings` lists a short schema (`describe_command {"id": "set_settings"}` gives
  the full one) and command tools lean schemas (`automation::lean_schema`): `tools/list` 65 968 →
  42 148 bytes. `.mcp.json` starts RustRonin with `--no-restore`.

### Fixed

- Tests: the big hierarchy test counts the rows laid out and painted instead of
  timing a frame against a 50 ms budget, which failed on a loaded machine.
- Markdown Preview Only: the preview no longer takes the hidden editor's unchanged top
  line for a move of the editor in every frame (`markdown_view::follows_editor`: only a change of the
  editor's top that the preview did not ask for moves the preview); the line the preview scrolled to
  is given to the editor when it is next drawn (`MarkdownViews::take_editor_line`).

### Added

- CI is the gate (ADR-0140 point 5): pull requests merge only with a green pipeline, and before
  pushing only the quick checks run locally (`cargo xtask ci --skip test` and the changed crates'
  tests); AGENTS.md, CONTRIBUTING, docs/development.md and ai-workflow updated.
- Releases in two steps (ADR-0142): `cargo xtask release-build` builds a
  release without secrets into a folder with `build.json` (files, sizes, SHA-256, commit, tree,
  rustc, gate); CI's `release-build` job builds it on the team's Mac runner from a pipeline run by
  hand on `main` with `PRODUCT` and `VERSION`; `cargo xtask release-publish --pipeline|--job|--from
  [--dry-run]` verifies, signs and publishes it on a maintainer's machine. `cargo xtask release` and
  `release-toolbox` still do both. The release code uses Cargo's target directory
  (`CARGO_TARGET_DIR`) instead of `./target`.
- CI on pull requests again, on the team's Mac runner (ADR-0140): `.gitlab-ci.yml` runs
  `cargo xtask ci` on pull request pipelines only (the fast checks, then the tests), interruptible, with a fetched
  checkout; GitLab's shared runners are off for the project. A nightly schedule runs the whole gate
  on `main` `NIGHTLY_RUNS` times (5) with `--force`.
- A faster gate and lighter UI tests (ADR-0141): `cargo xtask ci` remembers the pass
  of a clean tree by its tree hash and `rustc -V` (`<target>/gate-passed/`, `xtask/src/gate_memo.rs`)
  and skips it on that tree, `--force` running it anyway; the release commands use the same record.
  `zen_ui::offscreen::Offscreen::frame` and `render` return a `Frame` rasterised only when its picture
  is read (textures shared copy-on-write), and the snapshot helpers of the UI tests take frames, so
  only tests that compare or save pictures paint them: the zen-ui tests' CPU time fell from 1 155 s
  to 553 s. docs/ai-workflow.md: running several agents on one Mac.
- Lean MCP answers, hidden files and call costs:
  `compact` rows for `navigate.symbol.goTo`, `navigate.class.goTo`, `editor.structure.get` and
  `navigate.usages.find` (`zen_workbench::language::compact`); `ProblemFilter` (`severity`,
  `sources`, `limit`, `compact`) on `problems.file.list` and `problems.project.list` (with `paths`,
  `exclude`); `navigate.usages.find` by `symbol` or `line`+`name`, with `includeTextMatches`;
  `checkProblems`/`checkMs` → `newProblems`/`unchecked` on `editor.text.replace|write|batch`
  (`language::wait_checked`); `mcpRedactFiles` and `ide/redact.rs` (refused calls with code
  `redacted`, `[redacted]` lines, `sensitive` tabs, a notification); `--no-restore` for
  `--headless` and `mcp --headless` (`IdeApp::build_restoring`); `_meta.elapsedMs` on every tool
  result and `mcp.session.stats` (`zen_automation::stats`, `zen_workbench::mcp_stats`).
- A full Markdown preview (ADR-0138): `zen_workbench::markdown` reads GFM with
  `pulldown-cmark` (tables, footnotes, strikethrough, tasks, alerts, math; bare-address autolinks;
  GitHub anchors; a safe HTML subset) into blocks with source lines and nesting; the preview kind
  `markdown` (ADR-0137's `Content::Markdown`); the commands `markdown.preview.render`
  (text, html, blocks), `markdown.task.toggle`, `markdown.images.allow`, `markdown.images.list`
  (remote images only from allowed hosts, kept per project). `zen_ui::ide::markdown_view` draws it
  natively: typography, highlighted code with Copy, tables, task boxes, alerts, footnotes, images,
  scroll sync both ways (`EditorView::scroll_to_line`), click-to-source, links with their own
  accessibility nodes. The Markdown plug-in 0.3.0 (ABI v1) gives the headings to the Structure view
  and folds sections and fenced code. Mermaid and math shown as source (follow-ups in the ADR).
- Text with a preview (ADR-0137): `zen_workbench::preview` (`PreviewKind`, `svg`
  for `*.svg`; `PreviewMode`, kept per kind in `previews.json`; `Previews::poll` renders the
  document's current text on a worker thread, one at a time, newest text next) and the commands
  `editor.preview.getMode`, `setMode`, `editorOnly`, `editorAndPreview`, `previewOnly`;
  `get_window_state.preview`; `editor.image.zoom` zooms the preview. `zen_image::opens_as_picture`
  and `decode_svg`: an SVG opens as a text document, other images in the viewer. The window draws
  the editor, the preview or both with a divider, and the three-mode switch at the top-right
  (`zen_ui::ide::preview_pane`); the image viewer's canvas and zoom toolbar are shared
  (`picture_canvas`, `zoom_buttons`). Tests: the workbench's commands, unsaved edits, invalid SVG,
  modes kept, zoom; the window's modes, state, Find Action and snapshots (`ZEN_UI_SNAPSHOTS`:
  `preview-<mode>-<language>.png`).
- Linters and formatters (ADR-0136): servers that serve files alongside their
  language's server (`alongside`, `formats`, `fixOnSave`, `settings` answered on
  `workspace/configuration` with `${projectRoot|Uri|Name}`, `fromProject`, `when` in
  `ServerDeclaration`): diagnostics kept per server and merged, context actions listed and applied
  through the server that gave them, formatting and fixes on save through them; project
  formatters (`ProjectSupport::formatters`) run by Reformat Code and on save
  (`zen_workbench::code_quality`); the Code Quality Tools page (`codeQuality.settings.form`,
  `codeQuality.linter.set`, `codeQuality.tools.list`) and `editor.code.fixAll`; `ServerSource::Project`.
  Plug-ins `eslint`, `prettier` and `biome` 0.1.0. Verified against the real tools by
  `crates/platform/workbench/tests/code_quality.rs` (ignored).
- XML and file types by content (ADR-0135): `zen_vfs::file_types::ContentMarker`
  (`starts`, `unless`, `firstTag`), consulted after name patterns and hashbangs; callers give the
  text's `head` (4 KiB) instead of its first line, and `known_name_at`/`type_at` read the head of a
  file no pattern names from disk (`disk_head`: regular files only, never past a NUL byte, cached
  by size and modification time); `editor.fileTypes.typeOf` answers `byContent`,
  `editor.fileTypes.list` gives `contents`, Settings → File Types shows them read-only;
  `plugin.json` `contents` through the catalog, `zen-releases --contents` and xtask. ABI v1's
  optional `zen_problems` (`TextFeatures::problems`, `Feature::Problems`, `decode_problems`), whose
  problems join a file's problems. `plugins/xml` (ABI v1) in `BASE_PLUGINS`: the XML type by 22
  extensions and three content markers, highlighting, folding, Structure, end tags as you type and
  tag problems, on the HTML tokenizer's new XML mode (`lex_xml`); XML icons for the new extensions.
  Tests: markers, BOM, white space, HTML not claimed, binary and huge files, user patterns and
  overrides (`zen-vfs`); `typeOf`/`list` (`zen-workbench`); problem decoding (`zen-plugins`); the
  plug-in's own; Settings; a host test with `timeweb-cloud.cyberduckprofile`.
- Server plug-ins (ADR-0134): `"serverPlugins"` in `plugin.json`
  (`zen_lsp::ServerPlugin`, carried by the catalog, the installed manifest and
  `zen-releases publish-plugin --server-plugins`); `LanguageServers::contribute`/`withdraw`,
  `server_plugins`, `missing_plugins`, `install_plugins` and a per-server stamp that restarts a
  running server when its plug-ins change; installed plug-ins reach `initializationOptions.plugins`
  as `{name, location, languages}`; installing a server installs its contributed plug-ins, a
  contributing plug-in's install installs them into the server's folder, and the banner offers the
  missing ones in the projects their `when` markers match (`missingPlugins` in `lsp.servers.list`,
  `LanguageServers::missing_plugins_for`). Svelte 0.1.1 contributes
  `typescript-svelte-plugin@0.3.52`, Vue 0.1.1 `@vue/typescript-plugin@2.2.12`; verified against
  the real servers by `crates/platform/workbench/tests/server_plugins.rs` (ignored).
- Svelte (ADR-0132): `plugins/svelte` 0.1.0 (id `svelte`), offered for `.svelte`
  and never bundled: the markup read with its `{…}` hidden from the shared HTML tokenizer
  (`svelte/src/markup.rs`), blocks, tags, directives, runes and expressions highlighted, one tree of
  elements and logic blocks for folding, Structure, `{/…}` completion and typing (`>/}` add end tags
  and `{/block}`), completion of blocks, directives and runes, colours, the **Svelte Component**
  template; it includes `vue/src/sfc.rs` for `<script>` and `<style>`. It declares
  `svelte-language-server@0.18.5` with `typescript@5` for `.svelte` only. `.svelte` files get an
  icon. Tests: the plug-in's, the web plug-ins' host test, and
  `crates/platform/workbench/tests/svelte_language_server.rs` against the real server (ignored;
  `ZEN_SVELTE_LANGUAGE_SERVER_PREFIX`, `ZEN_SVELTE_PROJECT`).
- SvelteKit (ADR-0133): `plugins/sveltekit` (ABI v0, no file types, no language
  server, bundled with no product): marker `@sveltejs/kit`, `requires: ["svelte"]`, the run
  configurations `vite dev`, `vite build`, `vite preview`, the setup step `svelte-kit sync`
  (creates `.svelte-kit`) and nine route and hooks templates with fixed names, written as Velocity
  raw blocks for SvelteKit's `$` names; `svelte.config.*` get a file icon. Tests: the plug-in's
  manifest and module, the offer with Svelte and the install order (`zen-ui`), the project's
  configurations and `svelte-kit sync` with fake package managers and the templates
  (`zen-workbench`).
- React and Next.js (ADR-0131): `plugins/react` and `plugins/next` (ABI
  v0, no file types, no language server, bundled with no product). React: marker `react`, the
  run configurations `vite`, `vite build`, `vite preview` only in Vite projects, four file
  templates. Next.js: `requires: ["react"]`, markers `next.config.js|mjs|ts` and `next`, the run
  configurations `next dev`, `build`, `start`, `lint`, eight App and Pages Router templates (the
  App Router's with fixed file names), a Next.js file icon. Platform: a declared run configuration
  may carry `when` markers (`zen_project::support::DeclaredConfiguration`); a matching plug-in that
  another matching one requires is folded into its offer; the JavaScript and TypeScript plug-ins
  0.3.0 add JSX end tags as you type (`script::jsx_on_type`, `zen_on_type` with `>`). Next's
  TypeScript plug-in needs no setting: it loads with the project's own TypeScript, checked by a new
  ignored real-server test with a TypeScript plug-in of the test project
  (`a_typescript_plugin_named_in_tsconfig_loads_with_the_projects_typescript`).
- TypeScript and JavaScript (ADR-0130): the `typescript` and `javascript` plug-ins
  (0.2.0) declare `typescript-language-server@5.3.0` with `typescript@5` for all eight script
  extensions, word for word (the plug-ins' declaration test now requires shared ids to be
  identical); `ServerDeclaration::language_ids` (`languageIds`, language ids by extension) and
  `optimize_imports` (`optimizeImports`, the code action kinds of Optimize Imports in order, chained
  in `language::format`); `tsserver.fallbackPath` through `${nodeModules:…}`; the update at start
  installs no servers (`Job::servers`). Verified against the real server by
  `crates/platform/workbench/tests/typescript_language_server.rs` (ignored;
  `ZEN_TYPESCRIPT_LANGUAGE_SERVER_PREFIX`).
- Plug-ins (ADR-0128): installing or updating a plug-in installs the language
  servers it declares that are not found yet (`zen_ui::plugins::Plugins::set_language_servers`,
  `install_servers`), in the background through `LanguageServers::install` (the IDE's own folder,
  ADR-0127), with a notification when each ends; without the installer's tool one notification
  names it with its download page. A new plug-in's servers are declared at once (no restart);
  uninstalling removes the IDE's copies of its servers. `LanguageServers::source` and
  `lsp.servers.list`'s `source` (`local`, `path`, `custom`, `missing`), shown in the settings
  table's **Installed** column and the status bar list. Tests with a fake npm cover the MCP and
  offer-banner installs, a server on `PATH`, no npm, update and uninstall.
- Nuxt (ADR-0129): `plugins/nuxt`, the Nuxt plug-in (ABI v0, no file types, no
  language server, bundled with no product): it declares `"requires": ["vue"]`, the project markers
  `nuxt.config.ts|js|mjs` and the package `nuxt`, the run configurations `nuxi dev`, `build`,
  `generate` and `preview` (kind `npm`), the setup step `nuxi prepare` (creates `.nuxt`) and five
  file templates. The platform, generally: `plugin.json` may declare `requires` and `project`
  (`zen_project::support::ProjectSupport`: markers, run configurations, setup), carried by the
  catalog, `zen-releases publish-plugin --requires --project` and `cargo xtask release plugin`;
  installing a plug-in installs the ones it requires first (`Catalog::with_requirements`); the
  window offers the catalog's plug-ins whose markers match the open project (a notification,
  `view.plugin.ignoreForProjects`, `plugins/ignored-project-plugins.json`) and applies the installed
  ones through the new commands `plugins.project.list`, `plugins.project.apply` (configurations
  added once per project, setups offered once, kept in `plugin-projects.json`) and
  `plugins.project.setup`; every IDE has the `npm` run provider (`zen_workbench::npm::NpmRun`:
  `run`, `exec`, `install` with npm, pnpm, Yarn or Bun, the project's by `packageManager` or lock
  file; run icons for the scripts of `package.json`); `nuxt.config.*` get a Nuxt file icon.
  Tests: markers and declarations (`zen-project`), the catalog's requirements and projects
  (`zen-plugins`), the offer and installing with requirements (`zen-ui`), the npm provider and the
  `plugins.project.*` commands with fake package managers (`zen-workbench`), the plug-in's module
  and manifest.
- Kozō, the family mascot (ADR-0125): the new L0 crate `zen-mascot` draws him as
  40 × 40 ink grids (idle, work, success, error and sleep, 8 frames of 160 ms; every product's robe
  and prop), pinned to the approved design by a test; `zen_ui::mascot` paints him crisp at
  whole-pixel scale on the splash, the welcome screen and Zen Toolbox's Projects tab, still with
  Reduce motion (now `zen_ui::accessibility::reduce_motion`, shared with Caffeine);
  `cargo xtask mascot` writes the website's animated PNGs into `deploy/releases/site/img/kozo/`,
  shown in the new "Meet Kozō" section of the landing page.
- Vue: `plugins/vue`, the Vue plug-in (ABI v1) for `*.vue` single-file
  components, built from the HTML tokenizer, the JavaScript analyser and the CSS modules
  (`#[path]`, CSS and SCSS chosen per `<style>` block through the CSS modules' new crate-root
  `dialect()`): highlighting of the blocks, directives, interpolations, script and style by `lang`,
  folding, Structure, completion of directives and blocks, colours, and the **Vue Single File
  Component** file template; it declares `@vue/language-server@2.2.12` in full mode
  (`hybridMode: false`), installed with `typescript@5`. No IDE bundles it: opening a `.vue` file
  offers it, in WebWave too (the owner, 2026-10-22).
  The platform resolves `${nodeModules:<path>}` and `${nodeModulesFolder:<path>}` in a server's
  `initializationOptions` (`zen_lsp::servers::expand_options`) and installs several packages of
  one recipe; ADR-0126. `.vue` files get their own icon. Tests: the plug-in's, the web plug-ins'
  host test, and `crates/platform/workbench/tests/vue_language_server.rs` against the real server
  (ignored; `ZEN_VUE_LANGUAGE_SERVER=<path>`).
- WebWave: `apps/web-wave`, the web IDE on the shared IDE window (product id
  `webwave`, bundle id `space.ifzenelse.webwave`, binary and launcher `webwave`, MCP port 63532),
  with Go and Rust off and `scss` as its bundle beyond the base set (`PRODUCT_PLUGINS`); releasable
  with `cargo xtask release webwave`; its guide page (`docs/guide/06-web.md`) and
  `changelog/webwave.md`.
- Navigate: Select In (⌥F1, `view.selectIn.open`, `view.selectIn.target`) with
  Project View (library files too), Structure, Commit (changed files only), Finder and Terminal;
  Open In → Finder / Terminal in the editor tab's menu, which is now one menu with Local History.
- Navigate: line bookmarks, anonymous and mnemonic (0–9, A–Z):
  Edit → Bookmarks (Toggle Bookmark F3, Add Mnemonic Bookmark… ⌥F3, Previous/Next Line Bookmark, Go
  to Mnemonic… ⌥⌘F3), ⌃⇧digit and ⌃digit, the gutter icon with its description, the gutter's
  Bookmarks menu and ⌥-click to rename; `navigate.bookmark.*` commands and `view.bookmark.*` window
  actions; bookmarks follow edits and are kept per project in `workspace.json`. Shifted digits keep
  their digit in keymap lookups and in Keymap recording, and a shifted key still matches what it
  types (`keymap::presses`). Bookmarks follow files renamed or moved in the IDE and changes made by
  other programs, and go with deleted files.
- UI: the Bookmarks tool window (⌘2, left bottom; `bookmarks_window.rs`) with the
  bookmark lists in order, the default first with its tag, the breakpoints as a list of their own,
  context menus, drag between lists, Move Up/Down, ⌃⌥↓/⌃⌥↑ (and ⌥⌘↓/⌥⌘↑) and Delete, ⋮ with Group
  Line Bookmarks by File and Ask Before Rewriting Mnemonic; Show Line Bookmarks… (⌘F3,
  `view.bookmark.show`) with mnemonic jumps and Advanced Settings → Bookmarks → Show only line
  bookmarks in popup (`bookmarksPopupLineOnly`). Ordered lists with ranks in `zen_workbench::bookmarks`
  (ADR-0109): `navigate.bookmarkList.create/rename/setDefault/delete`, `navigate.bookmark.moveUp`,
  `moveDown`, `open`, `moveToList` with `index`, `groups` in `navigate.bookmark.list`, both options
  in `navigate.bookmark.options`; `workspace.json` keeps `bookmarks.lists` and `groupByFile`.
- CamelHumps words: `Movement::HumpLeft`/`HumpRight`, `EditOp::DeleteHumpBackward`/
  `DeleteHumpForward` and `zen_editor::hump_at`/`double_click_word`; the settings `camelHumps` and
  `camelHumpsOnDoubleClick` (session keys `editor.smartKeys.*`) switch `editor.caret.move` and the
  word deletions, which also take `camelHumps`; the new command `editor.selection.word` selects a
  word as a double click does.

### Fixed

- The Go toolchain's version probes (`dlv`, `gopls`, `go version`) wait up to 15 s for an answer
  (`PROBE_WAIT`, was 3–5 s), and start a program again for a moment while the system reports it
  busy ("Text file busy"): macOS checks a program's first start, which on a busy machine and
  outside a terminal (the CI runner) took longer than 3 s, so a fresh Delve was taken for
  something else and the Delve tests failed now and then.
- Tests: the interface language no longer leaks between tests running at the same
  time. `zen-i18n`'s `thread-languages` feature, on in the `[dev-dependencies]` of every crate
  whose tests show text, makes the chosen language the thread's, English until it chooses, so the
  tests pass on a machine in Russian; the UI tests' language lock and the English-only
  `apply_interface_language` under `cfg(test)` are gone, and the idle autosave test polls for the
  save with a deadline.
- Tests: two tests that failed now and then under `cargo xtask ci` wait for what they check —
  Welcome's language switch no longer reads the process-wide language other tests change at the
  same time, and Exit waits for its runs to be marked ended after their programs are killed.
- MCP for agents (ADR-0169): a symbol search names a missing language server only
  when it found nothing and the server could have answered (`workspaceSymbols` of a server
  declaration, false for the Dockerfile, YAML, TOML and JSON servers), never beside results; the
  once-per-session note of ADR-0092 is gone. rust-analyzer starts with
  `workspace.symbol.search.limit: 2000`, so `route` finds `IdeApp::route` (128 subsequence matches
  hid it). `run.process.start` refuses a configuration of an unknown kind or with options its kind
  does not have, naming the right ones, instead of running every test. `vcs.commit.create` takes
  `signOff`.
- Rust: rust-analyzer starts with `workspace.symbol.search.kind: all_symbols`, so Go to Symbol finds
  functions and methods, not only types (found while working on Zen IDE through RustRonin).
- Editor visual aids: `zen_workbench::visual_aids` with the settings of Settings →
  Editor → Appearance (`editorAppearance`: hard wrap and visual guides, indent guides, whitespaces
  with their kinds, the tab rendering of Advanced Settings), per-file toggles on `Document`, the
  commands `editor.whitespaces.toggle`, `editor.indentGuides.toggle` and `editor.appearance.get`;
  the `.idea` code style's `SOFT_MARGINS` read as `CodeStyleOverride::visual_guides`; the editor
  view draws the guides, the margin and the whitespace marks for the visible lines only
  (`ui/src/ide/visual_aids.rs`); theme keys `editor.whitespace`, `editor.indentGuide`,
  `editor.indentGuide.selected`, `editor.rightMargin`, mapped from `.icls` and VS Code themes.
- Editor font size on the fly: `zen_workbench::font_size` with the per-file size
  (`Document::font_size`, dropped on close) and the commands `editor.fontSize.increase`,
  `.decrease`, `.reset` and `.get`; the window actions `view.editorFontSize.increase` (⌃⇧.),
  `.decrease` (⌃⇧,) and `.reset`, which write `editorFont.size`; the settings `wheelFontSize`,
  `wheelFontSizeIn` and `zoomIndicator`, ⌘ with the mouse wheel over the editor and the zoom
  indicator; View and View → Active Editor menu items; the JetBrains keymap import maps
  `EditorIncreaseFontSize`, `EditorDecreaseFontSize`, `EditorResetFontSize`,
  `EditorIncreaseFontSizeGlobal` and `EditorDecreaseFontSizeGlobal`.
  In Presentation Mode the window actions change `presentationFontSize`, and the active editor's
  size starts from Presentation Mode's size (the session's `editor.font.size` is the size shown).

### Changed

- Text with a preview (ADR-0137): the mode is kept per file (the last
  `preview::MAX_FILES` = 500, `Previews::set_file_mode`) over a mode per kind; the switch and the
  `editor.preview.*` actions set the file's own, `editor.preview.setMode` takes `scope: file | type`,
  and `getMode` and `get_window_state.preview` say which applies (`scope`: `file`, `type`,
  `default`). The first `previews.json` (kinds only) is still read.
- Next.js (ADR-0131): `next-env.d.ts` gets the Next.js icon
  (`zen_ui::file_icons`), and `show_usages::generated` takes a file whose first comments say
  `This file should not be edited` (Next's note in it) for generated code.
- Language servers (ADR-0127): npm servers are installed with
  `npm install --prefix <data>/language-servers/<id>` (`zen_lsp::servers::prefix_install_command`,
  `ServerTools::install_command_into`) instead of `npm install -g`, and found in that folder's
  `node_modules/.bin` after a user-set path and before `PATH`; the Vue server's TypeScript comes
  from the project (5/6) or from that folder. The real-server Vue test now takes an IDE-style
  prefix (`ZEN_VUE_LANGUAGE_SERVER_PREFIX`).
- One IDE per data folder (ADR-0111): a start that would open windows hands its
  folders and files (`file:line[:column]`) to the IDE running on the same data folder over a Unix
  socket, `<data>/ipc/open.sock` (folder `0700`, socket `0600`; a private folder in the temporary
  folder when the path is too long), and ends with 0 (`zen_workbench::handoff`,
  `zen_ui::ide::read_command_line`, `IdeApp::accept_launchers`); a windowed IDE listens, a headless
  one only with `--accept-open`; `ZEN_HANDOFF_ONLY=1` makes a start nobody takes end with 1 instead
  of opening a window. A file inside an open project opens in that project. Each client is served on
  a thread of its own with 5 s in all, so a stalled one holds nothing up; on Linux both ends check
  the peer's user (`SO_PEERCRED`); a `ZEN_UI_SCREENSHOT` start never hands over. Agents, tests and
  scripts start IDEs with paths only on a throwaway `ZEN_DATA_DIR` (AGENTS.md).
- MCP for agents (ADR-0090): `run.tests.get` takes `summary`,
  `failedOnly` (failed tests with `file`, one-based `line`/`column` and `message`) and `test` (one
  test's output), the tree staying the default; `run.process.start` takes `waitMs` (at most ten
  minutes) and answers `{ runId, summary }` when the run ends, which the IDE answers from its loop
  (`IdeApp::waiting_runs`) and the MCP server waits for (`zen_automation::call_timeout`);
  `SharedOutput::wait_end`. `search.text.*` count lines and columns from 1 and give one `path` per
  match (relative to the project, absolute outside it; `relative` is gone; `error` only when set);
  `search.text.preview` takes and answers one-based lines. `get_document_text`'s
  `withLineNumbers` lines are `N|text`. The server's instructions say so.

### Fixed

- UI: popups at the caret are placed while drawing, after the editor has scrolled
  to the caret, through one helper (`caret_popup::PopupAnchor`, `EditorView::caret_place`) instead
  of a position taken when they open; context actions, the quick-fix list, completion, parameter
  info, quick documentation, the targets chooser and Show Usages use it
  (docs/architecture.md, 4.9). They flip above the caret when there is no room below, wait in the
  middle of the text while the caret is off the screen, and the wait for the caret is bounded; the
  first frame of an editor scrolls to a caret set before it was drawn.
- Unselect Occurrence (⌃⇧G) drops the occurrence Add Selection for Next Occurrence added last,
  following the order they were added in, also across a wrap-around.
- LSP: `Connection::cancel` leaves alone a request sent through another connection, so cancelling a
  request sent before its server restarted no longer cancels, or takes out of the queue, a request
  of the new server with the same id.
- Project tool window: Always Select Opened File, now under ⋮ → Behavior, is kept
  per project in `workspace.json` (`projectView.alwaysSelectOpened`) and restored when the project
  opens; the header hides Select Opened File while it is on; window actions
  `view.project.selectOpened` and `view.project.alwaysSelectOpened { on? }`, and
  `get_window_state` reports `projectView`.
- Autosave (ADR-0110): `file.document.autosave` (`zen_workbench::autosave`) saves
  every modified file through Save All's path (`closing::save_documents`: Actions on Save, Local
  History) and skips read-only files, files changed on disk (read just before the write; the
  document becomes a conflict, and Keep Memory Changes lets the next autosave write) and files not
  on disk, answering `{ saved, skipped, failed }`. The IDE runs it after `autosaveIdleSeconds`
  (15) without input or a change to the modified files, and when a window loses the keyboard
  (`WindowFocused(false)`) or a terminal session gets it; settings `autosaveOnIdle`,
  `autosaveIdleSeconds` (1–3600), `autosaveOnFrameDeactivation` in Settings → Appearance &
  Behavior → System Settings → Files and over `set_settings`; Tools → Actions on Save links to
  them (Configure autosave options…). An autosave reports a file's formatter or write failure once
  (until the file saves cleanly), tries files it could not save again after new input, compares
  the disk through the document's own reading (`Document::read_disk`), and wakes the root window
  for input in a project's own window. After Keep Memory Changes, a late disk check of the same
  change on disk raises no new conflict (`Document::sync_with_disk`).

### Fixed

- `zen_vfs::write_atomic` writes through a symbolic link to the file it points to instead of
  replacing the link with a file; a dangling link is an error.
- Closing editor tabs keeps unsaved edits: `file.document.close` saves a modified
  file first through the Save All path (`zen_workbench::closing`: on-save actions, Local History)
  and keeps the tab when it cannot be saved; new `file.document.closeOthers`, `closeAll` and
  `closeUnmodified` (tab menu, Window → Editor Tabs, keymap, JetBrains `CloseAllEditorsButActive`,
  `CloseAllEditors`, `CloseAllUnmodifiedEditors`); closing a project window (`close_project`),
  exiting (`exit_ide`, ⌘Q) and restarting save every window first and stay when a file cannot be
  saved. The tab's two context menus (split items, Local History) are one menu, so it opens.
  Save All saves every file it can and names the ones it cannot, instead of stopping at the first;
  a headless IDE ending with its parent or its last client still exits when a file cannot be saved.

- Caffeine: a timed start while Caffeine is already on with the same `what` starts
  `caffeinate` again with the new `-t` (any change of the deadline does), and a timer thread of
  `zen_workbench::caffeine::Caffeine` ends the program at the deadline without a frame;
  `Caffeine::next_check` wakes the IDE at the deadline, and `caffeine.status.get` after it answers
  off. The timer wakes the IDE through `Caffeine::set_waker`, and `IdeApp` draws every window
  again when Caffeine turns on or off, so that the stripe cup of a window drawing nothing shows it.
  Deadlines stay on `Instant`, which stands still while the Mac sleeps, as `caffeinate -t` does.
  The test of an unavailable Caffeine compares with `t!` instead of the bare Fluent key.
- Project: bookmarks of whole files and folders in the same lists (ADR-0121): the
  Project tool window's F3 / ⌥F3 on the selection and its Bookmarks › menu (Toggle Bookmark, Toggle
  Bookmark Mnemonic…, Delete) for one or several rows, the bookmark marker after a row's name, the
  editor tab menu's Bookmarks › (the same for the tab's file, and Bookmark Open Tabs…,
  `view.bookmark.openTabs`), rows of their own in the Bookmarks tool window (a folder opens as a
  selection in the Project tool window). Commands `navigate.bookmark.toggleFiles`,
  `navigate.bookmark.removeFiles`, `navigate.bookmarkList.fromOpenTabs`; `whole` in the place
  parameters; `line` optional and `folder` in `BookmarkInfo`; the error code `folderBookmark`;
  `workspace.json` keeps them in `bookmarks.files` with their place in the list, so older builds
  still read `bookmarks.lines`.

### Fixed

- UI: an editor tab's Local History › submenu could not be opened: it was a second context menu on
  the same tab (one popup id), so the submenu never stayed open. It is now part of the tab's one
  menu, after Bookmarks ›.
- Project: the tree read the selection in shown order for its context menu on every selected row in
  every frame, though only the row whose menu is open needs it; with 500 of 3,000 rows selected a
  frame took 32 s in a release build, now 44 ms (`mod.rs`, `tree_row`).
- Rust: the Cargo model's cache also watches the files its problems point at, so fixing a broken
  member manifest is picked up.
- Project model: the problems of `ModelProvider`s (a `Cargo.toml`, `go.mod` or
  `go.work` that cannot be read) join the language servers' and the inspections' problems in
  `problems.file.list`, `problems.project.list`, F2 and the editor's marks, on the file's line, until
  the file is fixed; one on a line that already has an error is left out. `ModelProblem.warning`
  marks a Cargo model read all the same (only the dependencies unresolved offline). External
  Libraries shows a module name after the dependencies node only for the language's own node (Go
  Modules), not after Cargo's Crates.
- Project model review: a model problem is `source: "project model"`
  (`project_model::MODEL_SOURCE`) and follows unsaved edits (mapped through a line diff from the
  text the model read). Cargo's offline warning names the dependency's line in its member manifest,
  says how to fetch, and is read again by Reload Project Model once Cargo's download folders change.
  A deleted member manifest is reported on the workspace's line naming the member and read again
  once it is back (the Cargo model's cache watches the manifests a problem names); a manifest
  outside the project is named without `..`.
  Files changed on disk (`follow_disk`) are told to each running server that serves them or
  `watches` them (a new optional server declaration field; gopls watches `go.mod`, `go.sum`,
  `go.work`, `go.work.sum`) with `workspace/didChangeWatchedFiles` (`Languages::files_changed`),
  so gopls no longer keeps the problems of a `go.work` or `go.mod` that was fixed.

### Added

- Macros (ADR-0112): `zen_core::macros` (`Recorder`, `MacroStep`, `is_recorded`);
  `Registry::dispatch` records the editor commands while recording and runs a queued playback after
  the command, `Registry::dispatch_unrecorded` for the window's own dispatches and mouse clicks;
  `zen_workbench::macros` (`macros.json`, the `editor.macro.*` commands, one undo step per playback,
  shortcuts moved on rename); `Keymap::with_macros`, `KeymapStore::set_macros`/`move_action`, the
  keymap group Macros; Edit → Macros, Enter Macro Name, Edit Macros, Play Saved Macros… and the
  status bar's recording mark; `Buffer::undo_depth`, `Document::undo_depth`. A recording keeps
  only what its starter dispatches (`zen_core::macros::Source`, `as_agent` around MCP
  `dispatch_command`); a playback refuses steps a macro cannot hold and closes the typing group
  around it (`Document::close_undo_group`).
- Find in Files: a non-modal popup with Pin Window, the count with
  "100+" at its limit, the File mask check box with its history, the query's history, clear, New
  Line and ⌃⌥C / ⌃⌥W / ⌃⌥X, mnemonics on the scope row, a search 150 ms after typing stops, one row
  per line with every match highlighted, folders for ambiguous names, tinted test rows, an editable
  highlighted preview (`search.text.edit`), and Open in Find Window with Open results in new tab and
  Too Many Results (`search.text.occurrences`, `view.findInFiles.openInFindWindow`,
  `view.findInFiles.pin`); the settings `findInFilesLimit` (100) and `findInFilesTooMany` (1000);
  the find history `search.findHistory.*` in `find-history.json` with the file masks
  (`zen_workbench::find_history`).
- The search bar: In Selection (`editor.find.searchInSelection`,
  ⌃⌥E; a multi-line selection turns it on), Scroll to Results During Typing
  (`editor.find.scrollToResults`), Open Results in Find Window (`editor.find.openInFindWindow`, ⌥F7
  in the bar, `occurrences` in the usages state), Next/Previous Occurrence of the Word at Caret
  (`editor.find.nextWordAtCaret`, `.previousWordAtCaret`), a multi-line query (New Line), and the
  query history shared with Find in Files (`search.findHistory.list`, `.add`, `.clear`, kept in
  `find-history.json`); the selected text taken as the query is escaped while Regex is on
  (`zen_text::search::escape`).
- Navigation: Last Edit Location ⇧⌘⌫ (`navigate.edit.last`) and Next Edit Location
  (`navigate.edit.next`, no shortcut) in the Navigate menu, over the edit places of
  `zen_workbench::history::History` (`edits`, `edit_at`, `record_edit`; one per line for a run of
  edits, moved with the text, in the order of the edits across files, 100 kept); `navigate.edit.list`;
  `navigate.history.clear` and `navigate.recent.removeLocation` forget them too; the history's places
  follow a file renamed or moved (`History::relocate`); JetBrains keymaps' `JumpToLastChange` and
  `JumpToNextChange`.
- Navigation: Go to Type Declaration (⇧⌘B, ⌃⇧B, ⇧⌘-click,
  `navigate.typeDeclaration.goTo`, `textDocument/typeDefinition`) and Go to Super Method (⌘U,
  `navigate.superMethod.goTo`: the concrete method around the caret from
  `textDocument/documentSymbol`, then `textDocument/implementation` without the method itself, else
  `textDocument/declaration` for a trait implementation's method) in the Navigate menu, with the
  targets chooser, the hints when there is nothing to go to, Back and Forward, and the
  `GotoTypeDeclaration` and `GotoSuperMethod` actions of imported JetBrains keymaps.
- TODO tool window: `zen_workbench::todo` lists the TODO and FIXME comments
  (`\btodo\b.*`, `\bfixme\b.*`, case-insensitive) in the comments of each file's highlighter over
  the file index (`todo.items.list`, `todo.tabs.list`, `todo.scan.start`, `todo.scan.results`), by
  tab: the project, a file, a search scope or the active changelist. The window (`todo` in the tool
  window layout, no shortcut) rescans in the background 0.3 s after edits, disk changes and Git
  status changes, reading only the files changed since the last scan (`zen_index::SearchMemory`,
  `FileIndex::search_text_remembering`), and draws only the rows in sight; `view.occurrence.next`
  / `.previous` (⌥⌘↓ / ⌥⌘↑) walk its items, and `view.todo.*` switch the tab, Expand/Collapse All,
  Preview Source and Navigate with Single Click; `get_window_state` reports `todo`, and
  `workspace.json` keeps its tab, scope and toggles.
- Search Everywhere's tabs and toolbar (ADR-0095): the type tab named by
- Search Everywhere's Text tab (ADR-0105): `view.gotoText.open`
  (⌥⇧⌘E, Navigate → Text…); `zen_modules::search::TextOptions` in `SearchQuery` (Cc, W, .*, file
  mask), kept in the popup's options (`textMatchCase`, `textWholeWords`, `textRegex`, `textMask`)
  and taken by `search.everywhere.query` (`matchCase`, `wholeWords`, `regex`, `mask`);
  `SearchContributor::fallback`: Text is searched on All only once the other groups are done with
  fewer than 6 items and listed last, `search.everywhere.textOnAll` (`searchTextOnAll`, Advanced
  Settings) turns it off, and Text is in All's Filter; rows say `file:line`; the file mask history
  shared with Find in Files through `zen_workbench::find_history` (word for
  word: `search.findHistory.list`, `.add`, `.clear`, `find-history.json`); the popup widens to fit
  its tabs and toolbar (the Russian "Include non-project items" showed only its box).
- Search Everywhere's `/` commands, calculator and shortcuts (ADR-0102): `CommandsContributor` lists the groups of Settings for `/` (target `settings`) and the
  plug-ins with a switch each for `/plugins` (target `switch`; a switch is toggled only by Enter on
  the row shown or a click, never by an Enter that waits for results);
  `zen_workbench::calculator` gives an arithmetic expression's value (target `value`, at most 64
  levels deep, a decimal comma where the language writes one); ⌥⏎ on an action records a shortcut
  over the popup, and `Keymap::bindings` turns keystrokes the user gave a command without a binding
  into a global binding (`resolved` drops one whose command is gone); abbreviations
  (`abbreviations.json`, `ui.keymap.abbreviations`, `.addAbbreviation`, `.removeAbbreviation`) and
  synonyms (`synonyms.ftl`) in Find Action; `ui.keymap.setShortcut`.
- Search Everywhere's results: a group with more items ends with
  "more…" (`SearchGroup::more`), which lists 50 more of it in place (`SearchRequest::more`,
  `more` of `search.everywhere.query`); ⌘↓ and ⌘↑ jump to the end and the start of the list; tests
  for the matched letters, `file:line[:column]` and Enter on a folder. more… searches again only its
  own group (`SearchRun`), and Enter pressed meanwhile opens the first item it loads; an action
  found by its English name only shows that name as its detail, with `detailRanges`.
- Search Everywhere's tabs and toolbar (ADR-0089): the type tab named by
- Search Everywhere's tabs and toolbar (ADR-0095): the type tab named by
  the language (Types in Go, `zen_modules::search::TypeNoun`, Navigate → Type…); a Text tab with
  the search scope (its shortcut and Text on All come later) and Recent Files on All with an empty
  query; folders in Files; the
  toolbar per tab (Include non-project items or the scope, Preview with a resizable read-only
  editor, Filter of contributors, file types or languages, Open in Find Tool Window, Include
  disabled actions); the footer with the path and Open In Right Split (⇧⏎); the placeholder and the
  query history drop-down; commands `search.everywhere.options`, `.setOptions`, `.filters` and
  `.openInFind`, the options kept in `search-everywhere.json`.
- Hierarchy tool window (ADR-0104): `zen_ui` draws the call and type hierarchies at
  the right top (⌃⌥H, ⌃H, View → Tool Windows → Hierarchy without a shortcut) with tabs, a toolbar
  and a lazily expanding tree; `zen_workbench::language::hierarchy` keeps one live hierarchy per
  tab (`Hierarchies`), marks recursive nodes, sorts, collapses and expands all; new commands
  `navigate.hierarchy.pin`, `.select`, `.collapse`, `.expandAll`, and a `tab` parameter on the
  others; scope, sorting and single-click navigation are saved with the project's layout. Expand All
  asks the server at most 256 times; the tree draws only the rows in view, and
  `places::TextCache::place` finds many places in one file without reading it from the start each time.
- Call and type hierarchy: the LSP client declares `callHierarchy` and
  `typeHierarchy` and tells which capabilities a server offers (`Client::provides`, also those
  `lsp-types` does not model); `zen_workbench::language::hierarchy` wraps `prepareCallHierarchy`,
  `incomingCalls`, `outgoingCalls`, `prepareTypeHierarchy`, `supertypes` and `subtypes` as
  cancellable requests, children asked for per node, with a Project / Test / All scope; commands
  `navigate.hierarchy.call`, `.type`, `.expand`, `.get`, `.refresh`, `.setOptions`, `.cancel`,
  `.close`.
- The Switcher: `view.switcher.open` on ⌃⇥ and ⌃⇧⇥, held with ⌃ (a quick tap
  switches to the previous file without the popup), sharing `ide/recent_files.rs` with Recent
  Files; both list every tool window on the left. `navigate.recent.remove` deletes a file from the
  recent files and closes its tabs, saving its unsaved changes first (⌦, ⌫, ⌘⌫ in the popup).
- Extend Selection and Shrink Selection (ADR-0098): `zen_editor::Structure` cuts a
  text into the units the selection steps through (words, strings, comments by sentence and
  paragraph, expressions, arguments, statements, bracket contents and groups, declarations with
  their doc comments), joined by the language server's `textDocument/selectionRange`; commands
  `editor.selection.extend` (⌥↑) and `editor.selection.shrink` (⌥↓) with a per-editor series;
  Edit menu items; the keymap import maps `EditorSelectWord` and `EditorUnSelectWord`; Advanced
  Settings → Editor → Select whitespace characters with 'Extend Selection'
  (`editor.extendSelection.whitespace`, `extendSelectionWhitespace`).
- TODO completion: in a comment, by the file's highlighter, the completion offers
  the words of the TODO patterns (`TODO`, `FIXME`) when what was typed starts one,
  case-insensitively; accepting puts the word in at every caret whose typed letters start it.
  `zen_workbench::todo_patterns` holds the default patterns once, for the TODO tool window and its
  highlighting too; command `todo.patterns.list`.
- Cyclic Expand Word: `zen_editor::expand_word` lists the distinct words of the open
  texts that complete the word part left of the caret (before it nearest first, then after it, then
  the other open files); the command `editor.completion.expandWord` (`forward` for ⌥⇧/) replaces it
  and cycles on repeated runs, ending the cycle when the document or its carets change; ⌥/ and ⌥⇧/
  in the keymap, Code → Code Completion in the main menu, `HippieCompletion` and
  `HippieBackwardCompletion` in the JetBrains keymap import; the editor no longer types the
  character that a letter, digit, punctuation or space key gave with ⌥ when that key ran a binding
  (only that key's own character, in the same frame).
- Navigation bar: `zen_workbench::language::navigation_bar` with the commands
  `navigate.navigationBar.get` (the segments of the active file or a path, with the declarations at
  the caret under Show Members, session setting `navigate.navigationBar.members`) and
  `navigate.navigationBar.children` (a folder's entries, a file's or a declaration's declarations);
  the interactive bar in the status bar, under the toolbar or floating (`ide/navigation_bar.rs`),
  the window actions `view.navigationBar.jump` (⌘↑, ⌥Home), `.top`, `.statusBar`, `.hide`,
  `.members.toggle`, View → Appearance → Navigation Bar, and the settings `navigationBar` and
  `navigationBarMembers`. Move Caret to Text Start moved from ⌘↑ to ⌘Home.
- Unwrap/Remove (ADR-0099): `zen_editor::unwrap_options` finds the constructs around a
  caret without a parser (`if`/`else` chains, loops, `switch`/`select` cases, function literals,
  bare braces, calls with one argument; strings and comments skipped) and the edit of each, the body
  re-indented; commands `editor.unwrap.list` and `editor.unwrap.apply`; the window action
  `view.unwrap.open` (⇧⌘⌦, Code menu, JetBrains action `Unwrap`) with its popup (`unwrap` in
  `get_window_state`).
- Surround With: `zen_workbench::templates::surround` with the commands
  `editor.surround.list` and `editor.surround.apply`; language modules declare surrounders next to
  their templates (`TemplateSet.surrounders`, Go's `surrounders.json`), and every file with comments
  gets the region and editor-fold comments. Live templates take `$SELECTION$`
  (`templates::expand_around`); what is wrapped is found without a parser
  (`templates::surround::wrap`, ADR-0119), checked at every caret and selection of Go, Rust and
  JavaScript snippets (`zen-workbench`'s `tests/surround_properties.rs`). The popup (`view.surround.open`, ⌥⌘T, Code menu;
  `view.surroundWithTemplate.open`, ⌥⌘J, the new keymap group Other), and the JetBrains keymap
  import maps `SurroundWith` and `SurroundWithLiveTemplate`.
- Editor (ADR-0094): `zen_workbench::indent_detection` finds the indentation a file's
  lines use (tabs or 2, 3, 4 or 8 spaces; comments, multi-line strings and continuation lines
  skipped; at least three indented lines; at most 5000 lines and 512 KB; never tabs in YAML nor
  spaces in a Makefile) when it is opened, reloaded or reformatted; `zen_workbench::file_indent`
  layers it over the configured code style for Tab, Enter, ⇧Tab and the status bar, while Reformat
  Code keeps the configured style; commands `editor.indent.get`, `editor.indent.useDetected`,
  `editor.indent.useConfigured`; the setting `detectIndents` (`editor.indents.detect`) on Settings →
  Editor → Code Style; the status bar's indent menu (`view.indent.menu`). `code_style::for_text`
  (tabs when any of the first 2000 lines starts with one) is gone.
- Zoom IDE ([ADR-0120](docs/adr/0120-ide-zoom.md)): `IdeSettings.ideZoom` (percent,
  100 by default, 50–300) multiplies egui's zoom factor on top of the UI font size and scales the
  editor too; View → Appearance → Zoom IDE (Current: N%)… opens a dialog (popup `zoomIde`); the
  window actions `view.ideZoom.open`, `.in`, `.out` and `.reset` have no default shortcut; Settings →
  Appearance → Accessibility → Zoom with presets 70–200 %, a custom value and Reset to default;
  the JetBrains keymap import maps `ZoomInIdeAction`, `ZoomOutIdeAction` and `ResetIdeScaleAction`;
  `get_window_state` reports `ideZoom`.
- Read-only toggle: `zen_vfs::is_writable`, `make_read_only` (returns the
  `WriteAccess` it took away), `make_writable` (gives it back, the owner's alone when unknown) and
  `set_writable`; `zen_vfs::write_atomic` no longer replaces a file whose write permission is off; a
  document of such a file cannot be edited, undone or saved (`Document::is_locked`, refreshed by
  `file.document.syncWithDisk`), and Replace in Files and refactorings refuse it, open or not; the
  commands `file.document.toggleReadOnly` and `file.document.readOnlyState`;
  `zen_workbench::read_only::offer_writable` posts a Make Writable notification when something tried
  to edit such a file, settled once the file is writable again; the JetBrains keymap import maps
  `ToggleReadOnlyAttribute`; the status bar's lock widget replaces the Read-only text; File → File
  Properties → Make file read-only / Make file writable.
- Line numbers modes and method separators: `zen_workbench::line_numbers`
  (`editor.lineNumbers.toggle`, `editor.lineNumbers.get`, the settings `editor.lineNumbers.show`,
  `editor.lineNumbers.mode`, `editor.methodSeparators.show`), `editor.methodSeparators.list` from the
  file's structure, and the theme key `editor.methodSeparator` (`METHOD_SEPARATORS_COLOR` in
  JetBrains schemes).
- EditorConfig (ADR-0107): `zen_workbench::charsets::Charset` (Latin-1, UTF-8,
  UTF-8 with BOM, UTF-16 BE/LE, decoded and encoded by the IDE, `Unmappable` for a character that
  does not fit); documents open and save in an `.editorconfig` `charset` and refuse to save what it
  cannot hold; `editor_config::SaveProperties` and `save_properties` (cached) for On Save;
  `editor_config::before_write` writes `end_of_line` on save; `editor_config::EditorConfigSyntax`
  highlights and completes `.editorconfig` (`zen_project::editorconfig::KNOWN`, `known_names`,
  `known_values`); commands `project.editorConfig.open`, `.list`, `.create`, `.export`; keymap
  actions `view.editorConfig.new`, `view.editorConfig.showFiles`, `view.editorConfig.disable`; the
  status bar's EditorConfig mark and menu, File → New, the Project tool window's New menus and Code
  Style → Show Scheme Actions → Export.
- Project (ADR-0100): `zen_project::editorconfig` reads whole `.editorconfig` files
  (`File`, with problems by line) and maps their properties onto the code style (`Style`:
  `indent_style`, `indent_size`, `tab_width`, `max_line_length`, `ij_[<language>_|any_]continuation_indent_size`
  and `visual_guides`); `zen_project::settings::FileCodeStyle` is a file's whole code style;
  `zen_workbench::editor_config` caches the parsed files by folder and the result by file (forgotten
  when the file watcher reports an `.editorconfig`, checked on disk at most once a second otherwise),
  `editor_config::margins_over` puts `max_line_length` and `ij_visual_guides` over the drawn margins,
  and `project_settings::StyleLayers` applies the project's and then EditorConfig's
  layer for Tab, Enter, ⇧Tab, the status bar, tab width and Reformat Code; command
  `project.editorConfig.get`; parse errors in `project.settings.get`; the setting `editorConfig`
  (`editor.editorConfig.enabled`) on Settings → Editor → Code Style → General.
- Line separators (ADR-0097): `zen_text::line_separator` (detect, convert, map
  offsets, `repairs`: the line breaks a batch of edits inserted, split or joined, written with the
  file's separator, with property tests); documents keep their text as on disk, know their
  separator and write the line breaks edits insert with it (typing, paste, multi-caret, Enter,
  completion and its caret, live templates and their variables, emmet, formatting, rename, Replace
  in Files on closed files, `editor.text.replace`, where `\n` also matches a CRLF or CR file);
  `file.lineSeparator.get` / `file.lineSeparator.set` (files, folders recursively without links,
  read-only files, files over 16 MB and the project's excluded folders; one undo step, the file
  written before the document changes so a failed write loses nothing, Local History action
  `lineSeparators`); the status bar
  widget and its menu, File → File Properties → Line Separators (keymap actions
  `file.lineSeparator.set/crlf`, `/lf`, `/cr`); Code Style → General → Line separator
  (`editor.lineSeparator.newFiles`, settings `lineSeparator`); `zen_project::editorconfig` reads
  `.editorconfig` files, `end_of_line` gives new files their separator.
- Code Folding settings (ADR-0114): Settings → Editor → General → Code Folding with
  Show code folding arrows (Always / On mouse hover), Show bottom arrows and the "Fold by default"
  options. `zen_editor::Fold` and `zen_syntax::FoldRegion` carry `placeholder` and `option`;
  `zen_modules::folding` names the General settings and gives `FoldingOptions`,
  `ModuleHost::folding_options` and the session state `FoldingGroups`; the Go module tags its folds
  (`zen_lang_go::module::FOLDING`, `incremental::fold_options`) and adds `case` clause and format
  string folds; the workbench folds a first-opened file by the options (`folding::default_option`,
  which takes the file name and guesses the General options only in files with C-like comments,
  `GENERAL`) at once (`Document::fold_all`) and answers `editor.folding.options`; `editor.fold.list`
  gives each fold its `option`.
  `IdeSettings` gains `showFoldArrows`, `foldArrows`, `foldBottomArrows`, `foldFileHeader`,
  `foldDocComments`, `foldMethodBodies` and `foldCustomRegions`. `zen_editor::folds` folds runs of
  `use` declarations as imports and no longer takes a Rust lifetime for a quote.

- Folding (ADR-0101): custom folding regions read from comments in the file's comment
  syntax (`zen_editor::custom_regions`, `FoldKind::Region`, `placeholder` in `editor.fold.list`,
  `defaultstate="collapsed"`) added to the folds of every source; manual regions kept by
  `Document` and moved with the edits (`FoldKind::Manual`); `editor.fold.selection` (⌘.),
  `editor.fold.regions` and the window action `view.customFolding.open` (⌥⌘., a popup); folds that
  start and end on one line drawn with the rest of the line after the mark; `FileFolds.manual`
  remembers manual regions by lines and columns with a `textHash` of their text (dropped when it
  changed); region comments in strings and block comments do not count (the highlighter's tokens,
  else `zen_editor::custom_regions`'s scan); manual regions keep what they hold through edits
  (`CharEdit::map_region`); the JetBrains keymap import maps `CollapseSelection` and
  `GotoCustomRegion`.
- Folding: `editor.fold.expandRecursively`, `.collapseRecursively`,
  `.expandToLevel` and `.expandAllToLevel` (`level`), `.expandDocComments`, `.collapseDocComments`,
  `.toggleAtCaret` and `.collapseBlock`; `editor.fold.collapseAll` and `.expandAll` keep to the
  selection; Code → Folding in its order. Keymaps get shortcuts of two keystrokes
  (`Keystroke::then`, `⌘NumPad*, 1`) and the keypad's `*`, recorded from the window events by
  `zen_ui::keypad::run_native`, which the IDE apps now start with (ADR-0096); the JetBrains keymap
  importer carries second keystrokes and the keypad's `*`, `+` and `−` over. The editor waits
  two seconds for a second keystroke and lets any other key through; the window leaves it the key
  that completes one; only editor shortcuts start a wait; `zen_ui::keypad::run_native` returns an
  error when the app never started, as `eframe::run_native` does; `press_keys` presses `NumPad*` and
  two keystrokes.
- On Save options (ADR-0106): `zen_workbench::on_save` removes trailing spaces (of
  the lines changed since the last save, or of all lines; caret lines, Markdown hard breaks and
  multi-line string literals kept), trailing blank lines and adds a final line break before a save
  writes the file, as one undo step, after the Actions on Save (`format::before_save`, and
  `before_save_all` for Save All); commits and inspection fixes save through it too. Settings
  `removeTrailingSpaces`, `removeTrailingSpacesOn`, `keepTrailingSpacesOnCaretLine`,
  `removeTrailingBlankLines`, `ensureLineBreakAtEnd` (Settings → Editor → On Save);
  `zen_project::editorconfig` (as in ADR-0097's branch) gives `trim_trailing_whitespace` and
  `insert_final_newline`, which override them for the project's files; `editor.onSave.options` for
  agents. Modified lines come from `zen_vcs::diff::changed_lines` (Myers) over lines split at
  LF, CRLF and CR; `Document::replace_ranges` updates the text and moves the places in one pass
  for many ranges (a 100 000-line save in tens of milliseconds, docs/performance.md).
- Mark Directory as → Excluded and Cancel Exclusion (ADR-0123): the Project tool
  window's folder menu and the commands `project.directory.markExcluded` and
  `project.directory.cancelExclusion` add or remove `<excludeFolder>` in the module file holding the
  folder (creating `.idea/modules.xml` and `.idea/<name>.iml` when the project has none), the rest of
  the file byte for byte, through `SettingsProvider::set_excluded` (`zen_jb_import::IdeaProject`);
  the settings are read again and the index rebuilt at once, and Search Everywhere's runs start
  again when the settings change. Project ⋮ → Excluded Files (`view.project.excludedFiles`) hides
  excluded folders, kept per project in `workspace.json` (`hideExcludedFiles`). `zen_vfs::EXCLUDED`
  is gone: `vendor`, `build` and `dist` are no longer excluded by name, and a kind of project
  declares the folders it excludes beside its file (`zen_vfs::excluded_folders`: `node_modules`
  beside `package.json` for every IDE, `target` beside `Cargo.toml` from the Rust module through
  `ModuleHost::excluded_folder`). `$MODULE_DIR$` of a module file in `.idea/` is the project's root.
  `FileIndex::refresh` leaves out what Git ignores, as the first walk does (`FileIndex::git_ignores`),
  and takes many changes at once in one pass; Local History skips external changes to Git-ignored
  files and the window skips Git's status for changes inside ignored folders, so a build writing an
  ignored `target` costs nearly nothing. A read-only module file is not replaced, and an opened
  `<content>` keeps the file's indentation step.
- File types (ADR-0108): `zen_vfs::file_types`, one registry of file types with name
  patterns and hashbangs that built-in modules (`ModuleHost::file_type`), plug-ins (`plugin.json`:
  `extensions`, `names` and the new `hashbangs`) and the platform (Text) declare; highlighting, code
  style (`code_style::language_of`), language servers, inspectors, the Structure and icons ask it
  through `known_name`. The user's changes and the ignored names are kept per IDE in
  `filetypes.json` (`zen_workbench::file_types`); commands `editor.fileTypes.list`, `.typeOf`,
  `.addPattern`, `.removePattern`, `.addHashbang`, `.removeHashbang`, `.setIgnored`, `.reset`;
  Settings → Editor → File Types. `zen_vfs::is_ignored` decides what the project tree lists; the
  tree, the index (`Exclusions::ignored`), library listings, Local History and the file watcher skip
  the ignored names, and a change indexes the open projects again (`file_types::follow`).
  `zen_vfs::HIDDEN` (`.git`, `.hg`, `.svn`, `.DS_Store`) stays: those are never indexed, watched or
  recorded, even when the user stops ignoring them.
- File templates (ADR-0115): `zen_workbench::file_templates` with a Velocity
  subset parser (`velocity`: `${VAR}`, `$VAR`, `$!VAR`, `#set`, `#if`/`#elseif`/`#else`/`#end`,
  `#parse`, comments, `#[[raw]]#`, escapes; the rest reported as `Problem`s with line and column,
  property-tested), the predefined variables (`variables`), storage per IDE and in
  `.idea/fileTemplates` in the shared layout (`storage`), built-in templates and variables from
  modules (`zen_modules::FileTemplateSet`, `ModuleHost::file_templates`) and plug-ins
  (`"fileTemplates"` in `plugin.json`, carried by the catalog, `zen-releases publish-plugin
  --file-templates` and `cargo xtask release plugin`); commands `file.templates.list`, `.variables`,
  `.render`, `.create`, `.set`, `.remove`, `.reset`, `.saveFileAs`; window actions
  `view.fileTemplates.new` and `view.fileTemplates.saveAs` (File → Save File as Template…) with their
  dialogs `newFromTemplate` and `saveFileAsTemplate`. Templates from a project are untrusted:
  rendering is limited (`#if` 64 deep, 64 terms per condition, 64 includes, 1 MiB of text, 100
  problems), a file name pattern never leaves the chosen folder, symbolic links and files over 1 MiB
  in `fileTemplates` are not read, and writing `file.template.settings.xml` keeps the rest of the
  file byte for byte.
- Spelling (ADR-0113): the platform crate `zen-spell` (L0) with the bundled English
  dictionary (Webster's Second, public domain, front-coded and deflated by
  `cargo xtask spelling-dictionary`, plus `data/extra.txt`), lookup rules for inflections,
  prefixes, compounds and British spellings, and the words of names, literals and comments;
  `zen_workbench::spelling` checks open files on a thread of its own (names at their declarations,
  from semantic tokens or declaring keywords) and adds `Severity::Typo` problems to
  `problems.file.list` (not `problems.project.list`); commands `spelling.typos.list` and
  `spelling.words.check`; the theme key `editor.diagnostic.typo` (JetBrains `TYPO`); Settings →
  Editor → Inspections and `IdeSettings::spelling`. Highlighters that are not incremental (plug-ins)
  are never called on the spelling thread: the check takes the window's kept spans. Tuned on this
  repository's Rust and Markdown (`spelling_of_this_repository`, ignored): 4 045 marks down to 805.

- Patches (ADR-0117): `zen_vcs::patch` reads (git, `format-patch` mail headers,
  plain unified diffs), changes (strip, base folder, reverse), writes in git's format, applies hunk
  by hunk with offset and fuzz and builds the three-way merge of hunks that do not apply, in pure
  Rust, never panicking (property tests); `Repository::changes_patch`, `commit_patch`,
  `author_ident`, `blob_named`, `apply_git_patch`, `leave_conflicted`, `commit_as` and
  `commit_staged_as`; `Applying::Patch`; `encoding_rs` as a direct dependency. Applying tries
  `git apply`'s placement first (`HunkResult::exact`, `approximate` hunks reported), finds hunks
  through a line index, refuses paths through symbolic links, `.git` in every spelling
  (`patch::is_git_folder`) and symbolic-link or submodule entries (`FilePatch::is_special`),
  reports executable bits (`executable_change`), and records Local History ("Apply Patch").
  Commands
  `vcs.patch.create`, `vcs.patch.read`, `vcs.patch.diff`, `vcs.patch.apply`, `vcs.shelf.import`,
  `vcs.commit.draft`, `vcs.commit.create` with `author`, `createPatch` in
  `vcs.workspace.commitAction`; actions `view.vcs.createPatch`, `view.vcs.copyPatch`,
  `view.vcs.applyPatch`, `view.vcs.applyPatchFromClipboard`; the dialogs `createPatch` and
  `applyPatch`; `SystemClipboard::text`, `file_clipboard::clipboard_text` and `set_clipboard_text`.
- Associate with File Type and Override File Type (ADR-0124): the platform types
  `detected` and `native`; `file.document.open` fails with the code `unknownFileType` for a file of no
  type whose content is not text (the window shows the Associate with File Type dialog), opens `native`
  files with `zen_workbench::file_types::SystemOpener` (`ZEN_SYSTEM_OPENER` replaces the system's
  program) and files of a chosen type that are not text read-only. Overrides by path in the registry
  (`Registry::set_override`, `set_overrides`, `known_name_at`, `type_at`), kept per IDE in
  `overridefiletypes.json`; consumers that know the path ask `known_name_at`. Commands
  `editor.fileTypes.associate`, `.override`, `.revertOverride`, `.overrides`; `typeOf` answers
  `overridden`, `highlighted` and `languageServer`; the action `view.associateFileType.open` (File →
  File Properties) and the Project tool window's Associate with File Type…, Override File Type › and
  Revert File Type Override. A language server gets `didClose` for a file overridden away from it
  and `didOpen` when reverted; its problems of that file are dropped. Review fixes: only
  `file.document.open` and `associate` start the application (`editorOnly` for restored tabs, the
  crate's own opens refuse), argv only (`explorer` on Windows); UTF-16 is not binary and opens
  decoded read-only (`utf16_of`); pipes, devices and folders are refused before reading; overrides
  follow `file.fs.rename` and cut/paste and go with `file.fs.delete`; plug-ins cannot declare `text`,
  `detected` or `native`; every path-aware list's icon and the diff, conflict, Local History and
  Recent Locations highlighting honour overrides.
- Comparing: `file.compare.three`, `file.compare.clipboard`, `file.compare.texts`
  and `file.compare.writeSide` (`zen_workbench::compare`, with the `Whitespace` modes; `whitespace` on
  `file.compare.files` too); the diff tab's three-side viewer, editable panes, whitespace chooser and
  Swap Sides (`diff_view.rs`, `ide/compare.rs`) with the window actions `view.compare.clipboard`,
  `view.diff.blank`, `view.diff.swapSides`, `view.diff.whitespace`, `view.diff.viewer`,
  `view.diff.next`, `view.diff.previous`, `view.diff.setText`, `get_window_state`'s `diff`, and
  `view.project.compareWith` with `paths`; ⌘D in the Project tool window compares two or three
  selected files; `SystemClipboard::text`; View → Compare With… and Compare with Clipboard. Typing
  in an editable pane writes only what changed (`file.compare.writeSide`, line separators kept),
  compares again after a pause, and the file's side follows its document's other changes.
- Scratch files and buffers (ADR-0122): `zen_workbench::scratch` keeps scratch
  files in `<data folder>/scratches` (`scratch.<ext>`, `scratch_1.<ext>`...) and buffers in
  `<data folder>/buffers` (`buffer1.txt`...`buffer5.txt`, reused in turn), with the commands
  `file.scratch.languages`, `.new`, `.fromSelection`, `.newBuffer`, `.list`, `.changeLanguage` and
  `.move`; `file.fs.rename` and `file.fs.delete` take scratch files; closing an empty scratch
  deletes it; `editor.intentions.list` ends with `scratch.fromSelection` on a selection, also
  without a server, and context actions may edit scratch files; a server that serves a scratch file
  gets its folder as a workspace folder (`zen_lsp::Client::add_workspace_folder`), so gopls loads it
  as an ad-hoc package. The window: File → New → Scratch
  File (`view.scratch.new`, ⇧⌘N), `view.scratch.newBuffer`, `view.scratch.show`, the Project tool
  window's Scratches and Consoles with Change Language (`view.project.changeLanguage`) and Move
  (`view.project.move`, F6), the advanced setting `projectViewScratches`; JetBrains keymaps import
  `NewScratchFile`, `NewScratchBuffer` and `Scratch.ShowFilesPopup`.
- Compare Directories (ADR-0162): `file.compare.folders` and
  `file.compare.synchronize` (`zen_workbench::compare_folders`: size and time before contents,
  `is_skipped` for `zen_vfs::HIDDEN` and the ignored names, paths kept inside both folders, links
  never followed, `confirm` before overwriting or deleting; for agents, entries hidden from them
  left out with `hiddenFiles` and their synchronisation refused); the folder diff tab (`ide/folder_diff.rs`) compared on a thread of
  its own, with the state toggles, the `!`/`&`/`|` filter, the row actions, Synchronize Selected /
  All and their confirmation dialog, ⏎ and double-click into the file diff; window actions
  `view.folderDiff.*` and `get_window_state`'s `folderDiff`; ⌘D and Compare Directories in the
  Project tool window with two folders, a folder dialog with one; `folderDiff` and the actions'
  answers leave entries hidden from agents out (`FolderDiffTab::agent_state`).
- Caffeine: `zen_workbench::caffeine` keeps the computer awake by running
  `/usr/bin/caffeinate -w <pid>` (`systemd-inhibit` on Linux) as a child of the IDE, until turned
  off, for a time, or while runs, debug sessions, background tasks, indexing or MCP agents are at
  work, with battery rules from `pmset -g batt`; commands `caffeine.status.get`,
  `caffeine.awake.start`, `.stop`, `.toggle`; the settings `caffeine`; the status bar widget with
  its animated coffee drinker and menu (`view.caffeine.menu`); Settings → Tools → Caffeine.
- Rust: the Cargo project model from `cargo metadata` (offline): the workspace's
  members as modules, the standard library and the crates as External Libraries (`Library.group`
  names their node), kept until a manifest, `Cargo.lock`, `rust-toolchain.toml` or the toolchain
  changes; the Rust toolchain (`toolchain.rust.info`, `toolchain.rust.refresh`). The Project tool
  window draws module folders in bold with their names; `select_in_tree` takes library files.
- Rust: rust-analyzer (found through rustup or on `PATH`, installed with `rustup
  component add`, `InstallKind::Rustup`), Settings → Languages & Frameworks → Rust and its
  commands. Platform (ADR-0088): every module's toolchain side by side (`zen_modules::Toolchains`),
  pulled diagnostics (`textDocument/diagnostic`) beside the published ones, open files reopened
  on `workspace/diagnostic/refresh` for servers that only publish, and `$/progress` and
  `experimental/serverStatus` as `busy`, `progress` and `readyAfterMs` in `lsp.servers.list` and
  the status bar.
- Rust: `cargo.run`, `cargo.test` and `cargo.bench` configurations (package,
  target, features, profile), gutter targets on `fn main`, `#[test]`, `#[bench]` and
  `#[cfg(test)]` modules, the test tree from libtest's output (binaries' names from the standard
  error paired with their tests from the standard output in any order) and failed tests rerun by
  exact names. Code Vision counts usages again when a language server settles
  (`Languages::settled`).

### Fixed

- UI: the filtering popups also take ⌫ and ⌦ (any modifier) out of the frame, so Recent Files and
  Show Usages no longer delete from the file under them.
- Find in Files keeps its results where the scroll bar or the wheel leaves them instead of jumping
  back to the selected one at the top; ↑ and ↓ still bring the selected result into view.
- Find in Files with many matches no longer slows every frame: the window keeps the results it read,
  lays out only the rows in view and asks for the preview once per selected match (docs/performance.md).
- Find in Files is resized by dragging its edges and corners, and keeps that size until the IDE
  quits.
- Search Everywhere (and Go to Class, File, Symbol and Find Action) is resized by dragging its edges
  and corners, and keeps that size until the IDE quits.

### Security

- Dependencies: rustls-webpki 0.103.16 and webpki-roots 1.0.10 (RUSTSEC-2026-0381, RUSTSEC-2026-0382:
  distrusted emSign intermediates). The provider's rustls-webpki 0.102, used for algorithm
  identifiers only, is listed in `deny.toml` with its reason.

### Fixed

- Go: `missing_from_go_sum` requires a requirement's `go.mod` hash, and for a direct one also its
  files' hash, instead of either line; a failed `go.mod` fix posts an error notification with a
  title (Errors group) instead of a plain workbench message.
- Automation: command parameter descriptions are one line, without line breaks or backslashes.
- Git: Update Project stopped on conflicts names the merge "Merging 'origin/main'
  into 'main'" instead of "'main' into 'main'", and `vcs.blame.get` and `vcs.blame.annotate` take a
  path relative to the project.
- Git workspace in a narrow window: column titles too long for their column end in
  "…" with the whole title in a hint, and the floating details panel, however wide it was dragged,
  stays within the graph's area, off the left panel and its counts.
- Editor: typing in a large file no longer lists the folds or matches, copies and lays out the
  completion items again every frame: the folds listed move with the edits until
  the text settles, `editor.completion.list` takes `since` and answers `unchanged`, and the popup
  lays out only the rows in view (docs/performance.md).
- ⇧⇧ opens Search Everywhere again: the ⇧ key event itself no longer counts as typing.
- Search Everywhere: Enter pressed while the results are held back waits for the query typed (five
  seconds at most) and opens its first result, not a row of the earlier query; Go to Symbol and Go to Class count a
  file reached through a link (macOS `/var` and `/private/var`) as the project's.
- Navigation: the File Structure (⌘F12) and Recent Locations (⇧⌘E) popups consume Enter and
  Escape instead of only reading them, so the editor drawn after them in the same frame no longer
  inserts a line break at the symbol or place gone to.
- Enter in Search Everywhere (every tab), Go to Line, Find in Files, Rename, New File and Rename File,
  Put Label and the find bar no longer reaches the editor: the file gets no line break, and ⏎ (also
  ⇧⏎ and ⌥⏎ in the find bar) from `press_keys` now works in those dialogs and in the debugger's
  watches.
- Find in Files: the preview of a match in an open file with unsaved changes shows the editor's
  text, so the highlight matches the row; agents read it with `search.text.preview`.
- Tests: restarting the IDE under test no longer starts the test binary again (an endless chain of
  test processes), and `cargo xtask ci` fails when a test binary ends without reporting its result.
- Release notes: GoGarden 0.38.13's What's new announces the features that so far were only in this
  file, from Paste from History to live templates.

### Added

- One name matcher for every search by name: `zen_index::Pattern` with camel humps
  (`NPE`, `nullpoex`), `*`, word-boundary and trailing spaces, folders and owners before `/` and `.`,
  a `:line[:column]` suffix and the matched ranges, marked in Search Everywhere and Recent Files;
  one order (`zen_index::Rank`); `navigate.names.match`, `ranges` in the Go to commands and
  `find_action`, and `navigate.recent.files { query }`. Go to Class and Go to Symbol send the
  language servers only the query's letters and leave `*`, spaces and `:line` to the matcher; within
  a kind of match (exact, prefix, humps, middle) the project's names come before the SDK's and the
  libraries' (`Rank::in_project`, `navigate.names.match { libraries }`).
- Search Everywhere: contributors (`zen_modules::search`, ADR-0081) for every tab, built-in modules
  can add their own; results held back until the primary ones are ready (2000 ms at most, or wait for
  all in Advanced Settings); Include non-project items (also ⇧⇧ in the open popup); recent actions
  and query history kept across restarts (⌥↓); `search.everywhere.query`,
  `search.everywhere.contributors`, `search.history.list` and `clear`. Its items
  carry the name matcher's `ranges` and `detailRanges`, marked in the popup, and a file's target the
  query's `:line[:column]`.
- Terminal tabs and split panes: `zen_workbench::terminal`
  keeps the window's sessions (`Terminals`: tabs of split panes, one PTY each, running while the
  tool window is hidden, restored by name, split and folder from `workspace.json`) with the commands
  `terminal.session.new`, `.split`, `.list`, `.send`, `.read`, `.close`, `.focus`, `.focusNext`,
  `.focusPrevious` and `terminal.tab.select`, `.close`, `.rename`; the keymap's `terminal` context
  (⌘T, ⌘W, ⌥⇥, ⌥⇧⇥; only ⌘ chords and ⌥F12 of the window apply there, so ⌃C/⌃D/⌃R reach the
  shell); `zen_ui::terminal::PaneView` draws one session in a rectangle. `send` takes at most
  64 KiB and `read` returns at most 64 KiB (`truncated`).

- Search: scopes for Find in Files and Search Everywhere — the scope row In Project, Module,
  Directory and Scope (⌃⌥P/M/D/S), Directory with browse, recent folders and recursive ⌃⌥Y, the
  predefined scopes, one per module and the named ones of `.idea/scopes` and `.zen/scopes.json`, the
  pattern language (`file:` with `*` and `//*`, `&&`, `||`, `!`, parentheses, `$Name`), the Scopes
  window, the choice remembered per project; `search.text.start` takes `scope`; `search.scope.list`,
  `.select`, `.files`, `.filter`, `.save`, `.delete` (ADR-0083). In Search Everywhere the scope runs in the
  contributors' engine (`SearchRequest::scope`, `search.everywhere.query { scope }`), and Include
  non-project items on All is the tabs' All Places.
- Navigation: the Show Usages popup (⌥⌘F7, also from the Code Vision usages) with read/write
  icons, toggles, scope widening and 100 rows at a time; `navigate.usages.show`,
  `navigate.usages.settings` and `navigate.showUsages.*` (Oz2pC22X).
- Navigation: Recent Locations ⇧⌘E (`view.recentLocations.open`, `navigate.recent.locations
  { editedOnly, query }`, `navigate.recent.removeLocation`), the places visited and edited kept by
  `zen_workbench::history::History` (`Place`, `PLACES_LIMIT`, edits noted by the documents and moved
  with the text); Recent Files' Show edited only (`navigate.recent.files { editedOnly }`, ⌘E again),
  the Recent Locations link, Open in Right Split (`editor.split.openRight`, also the Project tool
  window's) and the path footer; the File Structure popup ⌘F12 (`view.fileStructure.open`) over the
  Structure tool window's data, with `editor.structure.get { query }` and
  `zen_workbench::language::structure::matching`; `zen_index::match_positions`, the letters a name
  match marks; the popups' shared search field (`search_everywhere::search_field`); JetBrains
  keymaps' `RecentLocations` and `FileStructurePopup`.
- Docs: docs/architecture.md and docs/behaviour.md describe the index and Search Everywhere as
  they are.
- Find in Files reads only candidate files: an in-memory trigram index of the project's text, built
  in the background with its progress in the status bar, kept current from the file watcher, and
  `search.index.status` for agents (ADR-0082).
- Search: the Filter of the search bar and of Find in Files (Anywhere, In Comments, In String
  Literals, Except Comments, Except String Literals, Except Comments and String Literals), judged by
  the syntax tokens a match starts in; `context` in the query of `editor.find.set` and
  `search.text.*`.
- Find and replace: Preserve Case, `\U…\E`, `\L…\E`, `\u` and `\l` in regex replacements, Exclude
  (`editor.find.exclude`) and the replacement shown next to each match with Regex on
  (`editor.find.preview`), in the search bar and in Replace in Files.
- Editor: usages of the symbol at the caret from the language server's
  `textDocument/documentHighlight` (declared by `zen_lsp::Client`, `Client::document_highlights`),
  reads and writes told apart (new theme key `editor.symbolHighlight.write`, imported from
  JetBrains' `WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES` and VS Code's
  `editor.wordHighlightStrongBackground`), the text match kept for files without a server;
  `editor.highlights.get`; the selected text's occurrences (`zen_editor::occurrences::selection_occurrences`);
  Highlight Usages in File ⇧⌘F7 (`editor.highlightUsages.toggle`), Find Usages in File ⌘F7
  (`navigate.usages.findInFile`), Next/Previous Highlighted Usage ⌃⌥↓/⌃⌥↑
  (`editor.highlightUsages.next`, `.previous`), `editor.highlightUsages.list` and `.clear` (and
  Escape); the setting `highlightUsagesAtCaret`.
- Themes: the built-in Zen Light theme (`zen-light`, every key set, pinned key by key, written out
  in docs/themes/zen-light.json), light themes building on it, Sync with OS (`ui.theme.followSystem`, in Find Action,
  MCP `follow_system_theme`), readability pairs with WCAG contrast checks (`zen_theme::PAIRS`,
  `unreadable`, `make_readable`) applied to imported themes, and the VS Code importer's derived diff
  colours and `diffEditor.diagonalFill`.
- TestTotami, the QA IDE, joins the family (ADR-0067), with its own icon: a bundle
  of four separate QA plug-ins (E2E and UI tests, API test collections, load testing with k6, test
  management), each switched on and off on its own; listed in Zen Toolbox, on the website, in
  README, naming, trademarks, NOTICE and the board's lists.
- Screen reader support: `zen_ui::accessibility` gives every control the IDE and Zen
  Toolbox draw an AccessKit role and name, tool window titles, the status bar and notifications are
  read, and the editor is a multi-line text field with its lines, caret and selection; built only
  while a screen reader asks; a test fails on a control without a name; guide page "Accessibility".
- Web extras as plug-ins for every IDE (ADR-0063): ABI v1 gains the optional exports
  `zen_colors`, `zen_on_type_triggers` and `zen_on_type`; colour swatches in the gutter with a
  colour picker (`editor.colors.list`, `editor.colors.set`); HTML 0.2.0 closes tags as you type (Settings →
  Editor → Smart Keys: HTML/CSS turns it off) and previews the colours of its styles; CSS 0.2.0 previews colours; the SCSS plug-in 0.1.0
  (`plugins/scss`, from the catalog) shares the CSS plug-in's code.
- Tool window layout: drag a stripe button to another stripe or side to move its
  tool window (left top and bottom, bottom left and right, right top and bottom), resize edges and
  the gap between two tool windows of an edge, Hide (⇧⎋), Hide All (⇧⌘F12), Stretch (⌃⇧ arrows), Move To and Resize
  in the stripe button's menu and ⋮, Window → Layouts → Restore Default Layout (⇧F12); side, order,
  size and visibility kept per project in `workspace.json`; `view.toolWindow.move`,
  `view.toolWindow.hide`, `view.toolWindow.hideAll`, `view.toolWindow.resize`, `view.layout.restoreDefault`, and the layout in
  `get_window_state`.
- View → Appearance: Presentation Mode, Distraction Free Mode, Zen Mode and Full
  Screen (⌃⌘F) as the window actions `view.presentationMode.toggle`, `view.distractionFreeMode.toggle`,
  `view.zenMode.toggle` and `view.fullScreen.toggle`; Escape leaves a mode when nothing else takes
  it; the setting `presentationFontSize` (24 by default); `get_window_state` reports `appearance` and
  `fullScreen` (docs/guide/21-appearance.md).
- Settings → Plugins: Marketplace with a search, sorted by
  name, with Install and Update; Installed with Enabled, Check for Updates, Update and Update All;
  the selected plug-in's details with the release notes of its newest and installed versions, and
  a restart notice for plug-ins with language servers. New commands `plugins.catalog.list`,
  `plugins.installed.list`, `plugins.plugin.details`, `plugins.plugin.install`,
  `plugins.plugin.update`, `plugins.updates.check`, `plugins.updates.all`, `plugins.plugin.enable`,
  `plugins.plugin.disable` and `plugins.plugin.uninstall` (`zen_workbench::plugin_manager`); the
  catalog's versions carry `notes` (`zen-releases publish-plugin --notes`, filled by
  `cargo xtask release plugin`), and installed and bundled `plugin.json` keep the description and notes.
- Go to Symbol (⌥⌘O, Navigate → Symbol…): Search Everywhere has a Symbols tab, and
  All lists symbols, from the new command `navigate.symbol.goTo`; it shares one `workspace/symbol`
  search with `navigate.class.goTo` (debounced, cancelled on a new query, capped), and both now
  match by camel humps and dotted names (`Store.get`).
- The HTTP Client (ADR-0061): the new L2 crate `zen-http` reads `.http` and `.rest`
  files, their `http-client.env.json` / `http-client.private.env.json` environments and variables,
  sends one request over the TLS stack of ADR-0040 and keeps each project's history in the data
  folder (`http-client/…/history.json`, never in the project); commands `http.requests.list`,
  `http.environment.select`, `http.request.run`, `http.responses.list` and `http.response.get`; a
  run icon on each request line and the Services tool window (⌘8, `view.toolWindow.services`); the
  `http` plug-in (highlighting, folding, structure) joins the base set.
- Notifications: one store per project window (`zen_workbench::notifications`,
  read/unread, at most 500, actions that run commands), balloons that fade (errors stay) and the
  Notifications tool window on the right stripe; Settings → Appearance & Behavior → Notifications;
  commands `ui.notifications.list`, `.markRead`, `.clear`, `.action`, `.post` (no actions from
  agents) and the action
  `view.toolWindow.notifications`. The window's notices, update and plug-in offers, language server
  and debugger notices, Git outcomes and failed commands all go to the store.
- Local History (ADR-0060): every save, external change, deletion, Replace in
  Files, Rename, Git Rollback and Revert keeps a version of the file in the data folder's
  `local-history` (content-addressed, deflated, 5 days and 512 MiB); Show History of a file or folder
  with the diff viewer and Revert, Put Label…, and the `localHistory.*` commands.
- The Git workspace's own look (docs/git-workspace.md "The
  look"): a toolbar of captions over icons with the repository and branch pickers, rows tinted in
  their lane's colour, lanes with rounded elbows (`git_workspace/look.rs`), labels filled in the
  lane's colour with a laptop or a cloud, small-caps titles, branches in folders, and a right panel
  with a changes banner, Path/Tree and View all files. New commands `vcs.workspace.view` (column
  widths, how files are listed), `vcs.commit.files` (`zen_vcs::Repository::commit_files`: every
  file of a commit) and `vcs.workspace.pullMenu` (Pull's ▾); `vcs.workspace.state` reports `view`
  and `pullMenu`. New glyphs: cloud, undo, redo, pull, push, pencil, list, tree, check.
- Search and filter in the Git workspace (stage 5 of ADR-0057): the Log's text
  matching is `zen_vcs::filter::TextMatcher` (plain, Match Case, regex; `find` for the range to
  highlight), shared by `vcs.log.filter` and the new `vcs.graph.search` (message, id start, author
  and e-mail; `step` next/previous, `close`; the graph folds its texts once, 12 000 commits are
  searched in milliseconds). Hide and Solo of branches, remote branches and whole remotes:
  `vcs.graph.hide`, `vcs.graph.solo` and `vcs.graph.focus` lay the graph out again from the history
  already read (`vcs.graph.page` counts the rows left out in `hidden`); the window keeps them per
  repository (`repositories` in `workspace.json`). `vcs.workspace.filter` filters the left panel;
  `vcs.workspace.state` reports `search`, `filter` and `focus`.
- Stashes in the Git workspace (stage 4 of ADR-0057): `zen_vcs::Repository` gains
  `stash_push` (all changes with untracked files, or the staged ones only), `stash_apply` (apply or
  pop; conflicts fail with `VcsError::Conflicts`, `Applying::StashApply`/`StashPop`), `stash_drop`
  and `stash_rename` (the stashes down to it stored again in order, each with its time);
  `zen_vcs::graph::attach` adds the stashes' rows to a laid-out graph, right above the commits they
  were made on; `git::run_with_environment`. New commands `vcs.stash.create`, `vcs.stash.apply`,
  `vcs.stash.pop`, `vcs.stash.drop`, `vcs.stash.rename` (in `zen_workbench::vcs::stash`, with
  `vcs.stash.list`), and the stash menu's `vcs.workspace.stashMenu` and `vcs.workspace.stashAction`;
  `vcs.graph.page` rows carry `stash`, and `vcs.graph.find` finds a stash's row.
- Live templates (`zen_workbench::templates`): language modules declare templates
  (`zen_lang_go::TEMPLATES`, 19 for Go; `IdeProduct::templates`) with `$NAME$` variables, defaults,
  linked occurrences, `$END$` and `imports` (`zen_lang_go::add_import`); Tab expands them in code
  after Emmet, completion and ⌘J (`editor.templates.insert`) offer them, a session per document
  walks the variables (Tab and Enter, ⇧Tab, Escape ends), Settings → Editor → Live Templates edits them into `templates.json`, and the
  `editor.templates.*` commands list, expand, step, set, remove and reset them.
- Code Vision: a row above declarations with their usages, from the language
  server's usage code lenses or `textDocument/references` for the declarations shown, and their
  last author from Git blame; clicks run Find Usages and open the author's commit in the Git Log;
  Settings → Editor → Inlay Hints → Code Vision; the command `editor.codeVision.list`.

- Split editors: `zen_workbench::groups` holds the editor groups in a split tree,
  each with its tabs, carets and folds over one shared `Document` per file
  (`Document::show_in`, `Workbench::present`); commands `editor.split.right`, `.down` (with
  `move`), `.unsplit`, `.unsplitAll`, `.next`, `.previous` (no default shortcut yet), `.focus`, `.moveTab`,
  `.resize` and `.list`; `file.document.close` and `editor.tab.select` take a `group`; the window
  draws each group with its own editor view, the tab menu and Window → Editor Tabs have the split
  actions, tabs drag between groups, and `workspace.json` and `get_window_state` carry `splits`.
- Branch and tag actions in the Git workspace (stage 3 of ADR-0057):
  `zen_vcs::Repository` gains `rebase`, `fast_forward`, `rename_branch`, `set_upstream`,
  `delete_tag`, `push_tag`, `delete_remote_branch`, `outgoing_of` (any local branch), `remotes` and
  `default_remote`; `merge` and `rebase` that stop on conflicts fail with `VcsError::Conflicts`
  (`Applying::Merge`, `Applying::Rebase`), and a refused fast-forward with
  `VcsError::NotFastForward`. New commands `vcs.branch.rebase`, `vcs.branch.fastForward`,
  `vcs.branch.rename`, `vcs.branch.setUpstream`, `vcs.branch.deleteRemote`, `vcs.tag.delete`,
  `vcs.tag.push`, and the branch and tag menu's `vcs.workspace.refMenu` and
  `vcs.workspace.refAction`; `vcs.remote.push` and `vcs.push.outgoing` take `branch`;
  `vcs.branch.merge` fails with the code `conflicts`. The window explains every failure with that
  code in the conflicts dialog instead of a notice.
- Paste from History (⇧⌘V): the texts copied and cut in the IDE during the run, kept in
  memory and shared by its windows (`zen_workbench::clipboard_history`, 20 entries by default,
  `clipboardLimit` in Settings → Editor → Limits), in a popup that filters, pastes one or several
  entries at every caret and removes them; agents use `editor.clipboardHistory.list` (previews only),
  `.paste`, `.add`, `.remove` and `.clear` (docs/automation.md#clipboard-history).

- Soft wraps in the editor: View → Active Editor → Soft-Wrap (`editor.softWrap.toggle`,
  `editor.softWrap.get`) and Settings → Editor → Soft Wraps (`*.md; *.txt; *.rst; *.adoc` wrapped by
  default); ↑ ↓ Home End Page Up Page Down by visual rows, also for agents (`editor.caret.move` with
  `visual`, through `zen_workbench::soft_wrap::VisualRows`); only the lines shown are laid out
  (`zen_editor::LineWrap`), the line at the top stays put on resize; `Settings::value`.
- Inlay hints: the language server's `textDocument/inlayHint` for the shown lines
  and a margin, asked again once the text settles and moved with the edits meanwhile, drawn as grey
  rounded labels the caret and clicks step over; Settings → Editor → Inlay Hints per kind (gopls is
  told through its `hints` settings, `inlayHints` in a server declaration); `editor.inlayHints.list`
  and `.resolve`; theme keys `editor.inlayHint.background` and `.foreground`, imported from
  JetBrains (`INLAY_DEFAULT`, `INLINE_PARAMETER_HINT`) and VS Code (`editorInlayHint.*`) themes.
- Context actions (⌥Enter, ADR-0058): the language server's quick fixes, refactorings and source
  actions at the caret in a popup, a bulb on the caret line's gutter when there are some, applied
  in every file or none with one ⌘Z, commands run with the edits the server sends back
  (`workspace/applyEdit`, accepted only while an action runs); `editor.intentions.list`,
  `editor.intentions.apply` and `view.intentions.open`.
- Commit actions in the Git workspace (stage 2 of ADR-0057): `zen_vcs::Repository`
  gains `revert`, `reset` (`ResetMode`), `create_tag`, `is_ancestor` and `conflicted_files`, and
  `cherry_pick` takes a merge's mainline and returns the new HEAD; a cherry-pick or revert that
  stops on conflicts fails with `VcsError::Conflicts` (the command error code `conflicts`). New
  commands `vcs.commit.revert`, `vcs.branch.reset`, `vcs.tag.create`, `vcs.commit.copy` (moved
  together with `vcs.commit.cherryPick` into `zen_workbench::vcs::operations`), and the commit menu's
  `vcs.workspace.menu` and `vcs.workspace.commitAction`; the window takes clipboard requests
  (`ClipboardRequest`), the menu's dialogs and conflict reports from the session.

- Product bundles (ADR-0050): `BASE_PLUGINS` plus each IDE's `PRODUCT_PLUGINS` in
  `xtask/src/release.rs`; Settings → Plugins groups Downloaded and Bundled with this IDE and marks
  bundled plug-ins in the Marketplace; `list_plugins` reports `bundled` (and `installed`) for catalog
  plug-ins too; `cargo xtask bundle-plugins <product> <folder>` lays a bundle out for development
  builds (`ZEN_BUNDLED_PLUGINS`).
- Matching brackets in the editor,: the bracket next to the caret and its match are
  highlighted (an unmatched one in red), skipping strings and comments by the file's highlighting;
  Move Caret to Matching Brace (⌃⇧M); theme keys `editor.bracket.matched.*` and
  `editor.bracket.unmatched.*`, imported from JetBrains and VS Code themes; commands
  `editor.brackets.matching` and `editor.brackets.goToMatching`.
- Breadcrumbs under the editor: the declarations around the caret from the language server's
  document symbols or the outline (built-in and plug-in), clickable, read again only when the caret
  moves or an edit settles; Settings → Editor → Breadcrumbs (`breadcrumbs`), `editor.breadcrumbs.get`.
- Sticky lines in the editor: the first lines of up to 5 blocks the view is inside (functions,
  types, `if`/`for`/`switch`, classes; not calls, literals or parameter lists), from the folds, stay
  over its top with their line numbers; a click goes to the line. Settings → Editor → Sticky Lines
  (`stickyLines`, `stickyLinesMax`), commands `editor.stickyLines.list` and
  `editor.stickyLines.goTo`, theme keys `editor.stickyLines.background` and `.border` (imported from
  VS Code themes; the background from JetBrains colour schemes too).
- The Git workspace tab (ADR-0057, docs/git-workspace.md): stage 1 built on the platform's Git engine: the commit graph of every branch laid out
  once in the background and served in pages (`vcs.graph.page`, `vcs.graph.find`), the stashes
  (`zen_vcs::Repository::stashes`, `vcs.stash.list`), a refs stamp that ignores the index, and the
  tab's state as commands (`vcs.workspace.open`, `.close`, `.state`, `.select`, `.openDiff`).
- The egui spike's conclusion (ADR-0054, docs/behaviour/egui-spike.md): text rendering checked
  against the specified metrics, the performance numbers and an accessibility assessment; the
  `text_rendering` example of zen-ui draws the editor's and the UI's text at 100% and 200%.
- The non-affiliation notice of TRADEMARKS.md in every Zen IDE's About panel (`menu-app-about-notice`,
  the panel's credits) and in the `NSHumanReadableCopyright` of every bundle `cargo xtask release*`
  writes, the IDEs' and Zen Toolbox's.
- Language servers in every IDE (ADR-0055): declared by language modules (gopls) and
  plug-ins (`"languageServers"` in `plugin.json`; the bundled YAML, JSON, TOML, Shell and Docker
  plug-ins declare theirs), installed only on request, with a status bar widget and popup (state,
  version, files, Restart, Stop, Show Log, Install), a log dialog, a "not installed" banner (Install,
  Download Go and the like, Don't ask again) that replaces the gopls-only one, and Settings →
  Languages & Frameworks → Language Servers (on/off, path, arguments). Commands `lsp.servers.list`
  and `lsp.server.*` (install, restart, stop, log, dismiss, undismiss, configure).

- Basic web in every IDE (ADR-0050). Plug-in ABI v1 (ADR-0056): optional `zen_fold`,
  `zen_outline`, `zen_complete` and `zen_expand` exports, checked and limited by the host
  (`zen_syntax::TextFeatures`, `zen_workbench::plugins::PluginFeatures`): plug-in folds in
  `editor.fold.*`, plug-in outlines in `editor.structure.get`, plug-in items in
  `editor.completion.*`, and `editor.emmet.expand`, which Tab tries first. New bundled plug-ins,
  in `BUNDLED_PLUGINS` of every product: `html` (tags, attributes, values and character references
  completed; our own Emmet-style abbreviations), `css` (properties, values, units, at-rules and
  pseudo-classes completed), `javascript` and `typescript` (one analyser,
  `plugins/javascript/src/script.rs`; keywords, names and snippets completed), each with
  highlighting, folding and a Structure outline. The editor's fold arrows show on hover of the
  gutter, the actions are in the new Folding keymap group and in Code → Folding, and the theme key
  `editor.fold.text` colours a folded region's mark. Platform crates may dev-depend on plug-ins,
  so that tests run the real plug-ins natively (`zen_plugins::native`, feature `native`).
- IDE: the Debug tool window (⌘5, ADR-0052): a tab per debug session with Rerun, Stop, Resume, Pause,
  the steps and Run to Cursor, Threads & Variables (threads, frames, lazy variables with paging,
  watches) and the session's console; debug runs stay out of the Run tool window, and the
  breakpoints and watches are kept per project in `workspace.json`.
- IDE debugger front end: line breakpoints on the gutter's line numbers
  (`debug.breakpoint.toggle`), the execution point, Debug in the run widget, the run icon menu and
  the Run menu with the stepping actions, their keys (⌃D, ⌃⇧D, F7, F8, ⇧F8, ⌥⌘R, ⌥F9, ⌘F8, ⌘5), the
  theme keys `breakpoint`, `breakpoint.disabled`, `execution.line` and `execution.arrow`, and the
  offer to install Delve when it is missing.

- The debug service (ADR-0052): `zen_workbench::debug`, breakpoints that follow edits, debug
  sessions that are runs, and the `debug.*` commands.
- Go debug plans (ADR-0052): `GoRun::debug` builds `go.build` and `go.test` configurations
  with `-gcflags=all=-N -l` and execs them under `dlv dap --client-addr`; `VerboseParser` reads
  `-test.v` output into the test tree and `explain_adapter_error` explains known Delve errors.
- Delve in the Go toolchain: `zen_toolchain::go::find_dlv` (`GOBIN`, `GOPATH/bin`,
  `PATH`, Homebrew's folders), `dlv_version` with `MIN_DLV` 1.20, `GoInfo.dlv`, `dlvInstall` and
  `dlvPath`, the commands `toolchain.dlv.choose` and `toolchain.dlv.install` (an explicit
  `go install github.com/go-delve/delve/cmd/dlv@latest`), `Runs::set_debugger` filling
  `RunContext.debugger`, and a Delve row on Settings → Go.
- Named keymaps and JetBrains keymap import (ADR-0051, docs/jetbrains-compatibility.md#keymaps):
  `zen_core::keymap::KeymapImporter`, `zen_jb_import::JetBrainsKeymapImporter` with its action table
  (run actions included), `<data>/keymaps/<id>.json` and the `keymap` setting (kept by
  `set_settings`, listed read-only in its schema), the `ui.keymap.list`, `select`, `import`,
  `duplicate`, `rename`, `delete` and `reload` commands, the MCP tools `list_keymaps`,
  `select_keymap` and `import_keymap`, and the keymap combo with Duplicate, Rename, Delete and
  Import… in Settings → Keymap.
- Projects shared with JetBrains IDEs (ADR-0051, docs/jetbrains-compatibility.md):
  `zen_project::settings` has `ProjectSettings` and the read-only `SettingsProvider` and `ReadFiles`;
  `zen_jb_import::IdeaProject` reads a project's `.idea/`, `.run/` and `*.iml` files for every IDE,
  with the commands `project.settings.get` and `project.settings.refresh` (also run when those files
  change on disk). Never written: renames, Replace in Files and the run configuration writer refuse
  those files (`zen_workbench::guard`), and a test checks they stay byte-identical through open,
  edit, format, save, rename and run configuration changes.
  - Code style: `.idea/codeStyles/Project.xml` is laid over the user's Editor → Code Style
    (`project_settings::code_style_of`, `code_style_for_path`) for Enter, Tab, Indent, the editor's
    tab width, the status bar and Reformat Code.
  - Encodings: a file `encodings.xml` puts in another charset, or text that is not valid UTF-8,
    opens read-only with a message and is never saved; a binary file (not UTF-8, with NUL bytes) is
    not opened; the status bar shows the charset (`Document::charset`) and Read-only.
  - Excluded folders and patterns of `*.iml` files are excluded in the project tree, the file index,
    Go to File and Find in Files (`zen_index::Exclusions`, `FileIndex::with_excluded`);
    `.idea/scopes/` and `.idea/inspectionProfiles/` are read into `project.settings.get` (profiles
    are not applied).
  - Run configurations: `RunProvider::adopt`, implemented by `zen_lang_go::GoRun` for Go Application
    and Go Test; the `run.*` commands list adopted configurations read-only (`jb:` ids,
    `Configuration::adopted_from`, never written), and `run.configuration.saveAdopted` copies one into
    `.zen/runConfigurations.json`; the Run/Debug Configurations dialog shows them greyed out with Save
    as Zen Configuration.
- Run and test configurations (ADR-0048). `zen_project::run` is the configuration model
  and `.zen/runConfigurations.json` (permanent configurations only; the selection and at most five
  temporary configurations stay in the session's `zen_workbench::run::Runs` and the window's
  `workspace.json`). The new L2 crate `zen-run` holds the `RunProvider` extension point, output
  parsers, the console with file links, the test tree and `Process`: each command line in its own
  process group, output read line by line, Stop interrupts and then kills the group, every run builds
  in a scratch folder of its own that is removed when it ends, and closing the window kills what
  still runs (`rustix` on Unix). New commands: `run.configuration.*`, `run.context.get`,
  `run.targets.list`, `run.process.*`, `run.output.get`, `run.tests.get` and `run.tests.rerunFailed`;
  every start saves modified documents first.
- Go run configurations: `zen_lang_go::GoRun` runs `go.build` (build, then run the program) and
  `go.test` (`go test -json`, parsed into the test tree with file links), finds `func main`, tests,
  benchmarks, examples and fuzz tests for the gutter and the context with project-relative packages
  and their import paths, and reruns failed tests; `Toolchain::go_run_env` gives the SDK's `go` with
  its `PATH` and `GOROOT`.
- IDE: the run widget in the toolbar (the selected configuration, Run or Rerun, Stop), the Run/Debug
  Configurations dialog (Edit Configurations…), the Run menu, the Run tool window (⌘4) with a tab per
  run, Rerun, Stop, Rerun Failed Tests, the console with clickable file links and the test tree, and
  run icons in the editor gutter with the last test status; the gutter runs the context at its line
  (⌃⇧R), and the views read runs only when they change.
- The Settings dialog: Appearance, Editor, Go and search (ADR-0053). The dialog is a
  registry of pages (`ide/settings/page.rs`, one file and one Fluent file per page, shared widgets).
  Search: a field above the section tree (⌘F) filters the sections by their titles and page texts in
  the interface language, keeps the parents of matches, marks the matching labels on every page and
  clears with Escape before the dialog closes; an IDE without Go does not search the Go page.
  Appearance → Use custom font → Size (8–32, 13 by default) zooms the whole UI by `size / 13`.
  Editor → Font: the family from the installed monospaced fonts (`fontdb`, desktop only, read on a
  background thread; `ui.fonts.list`), size, line height, ligatures and a preview; the editor's
  metrics (`EditorMetrics`) ignore the UI zoom, and one gutter width counts the line numbers, run
  icons and blame annotations. Editor → Code Style → Go and Other File Types:
  `zen_project::settings::CodeStyle`, used by Tab, Enter, indenting and Reformat Code through
  `zen_workbench::code_style`. Languages & Frameworks → Go: Add SDK → Local… and Remove
  (`toolchain.go.add`, `toolchain.go.remove`, `customSdks` in `toolchain.json`), a custom gopls path
  (`toolchain.gopls.choose`, `goplsPath`) and the on-save options; when the SDK or gopls changes
  (`Toolchain::generation`), every window restarts gopls (`language.servers.restart`) and gives the
  project model and the runs the new SDK. MCP: `get_settings` returns every setting and
  `set_settings` merges nested objects key by key, with nulls for defaults, unknown keys refused and
  a schema of every field.
- Themes (ADR-0045, docs/themes.md). `zen-theme` (L1): the theme model and its keys, the built-in
  Zen Dark with the colours from before themes, the JSONC theme format with `parent` inheritance and
  its schema, the theme store of `<data>/themes` and the importer interface with the VS Code colour
  theme importer (docs/themes/import-vscode.md). `zen-jb-import` (L5): importers of JetBrains colour
  schemes (`.icls`, `.xml`) and UI themes (`.theme.json`) (docs/themes/import-jetbrains.md). The
  IDE draws every colour of its windows, the welcome screen, the editor and the terminal from the
  theme in use (`IdePalette::from_theme`, `IdeApp::set_theme`); Zen Dark looks as before. Themes
  are listed, chosen, imported and read again live with `ui.theme.list`, `select`, `import` and
  `reload` and the MCP tools `list_themes`, `select_theme` and `import_theme`; the choice is saved
  as `theme` in the settings. Settings → Appearance → Theme (in project windows and on the welcome
  screen) previews a theme at once, Cancel restores the previous one, and Load theme file… imports.
- DevopsDaruma, the DevOps IDE, joins the family (ADR-0049), with its own icon.
- Go modules and the Structure tool window (ADR-0046): `zen_project::model` (the project model and
  `ModelProvider`) and `zen_syntax::Outliner` are extension points an IDE lists in `IdeProduct`.
  `zen-lang-go` reads `go.work` and `go.mod` (`GoModules`: the modules, the Go SDK, the required
  modules in the module cache at their highest version, replacements, problems with their lines) and
  outlines Go files (`GoOutliner`); `Toolchain::go_locations` gives the SDK in use and `GOMODCACHE`.
  New commands: `project.model.refresh` (also run when `go.mod` or `go.work` changes on disk),
  `project.module.info`, `project.libraries.list`, and `editor.structure.get` (gopls' document
  symbols with methods under their receiver types; the outliner until gopls first answers, and the
  last symbols, `pending`, while it catches up after an edit).
- IDE: External Libraries in the Project tool window (the Go SDK and Go Modules, read-only, the ones
  not downloaded greyed out; Select Opened File finds library files), and the Structure tool window
  (⌘7) with Sort Alphabetically, Show Fields, Navigate with Single Click and Follow Caret, kept per
  project in the workspace file.
- IDE: Rename through the language server (⇧F6, Refactor → Rename…, ADR-0044). The Rename dialog
  renames at once or opens a Refactoring Preview tab in the Find tool window with Do Refactor and
  Cancel; `refactor.rename.prepare`, `preview`, `apply` and `cancel` change every file or none
  (`workspace/didChangeWatchedFiles` for the files written), and Undo reverts the rename in every
  file it changed.
- IDE: Reformat Code (⌥⌘L, `editor.code.reformat`, gofmt through gopls) and Optimize Imports (⌃⌥O,
  `editor.imports.optimize`, goimports through `source.organizeImports`, resolved when the edit
  comes later) in the Code menu, each one undo step; Settings → Tools → Actions on Save runs them
  before a save (`editor.onSave.reformat`, `editor.onSave.optimizeImports`), waiting at most a
  second for each.
- IDE: code navigation through the language server. Go to Declaration or Usages (⌘B, ⌘-click) and
  Go to Implementation(s) (⌥⌘B), with a chooser for several targets (`navigate.declaration.goTo`,
  `navigate.implementation.goTo`, `navigate.lookup.poll`, `navigate.lookup.cancel`); Find Usages
  (⌥F7, `navigate.usages.find`, `list`, `clear`) grouped by file in the new Find tool window (⌘3);
  quick documentation (F1, ⌃J, `editor.documentation.*`) and parameter info (⌘P,
  `editor.parameterInfo.*`) in popups at the caret, the Markdown read with `pulldown-cmark`. Every
  lookup takes a place (`path`, `offset`) and `waitMs` (at most 10 s); a go-to or Find Usages answer
  that comes after the file changed is dropped.
- IDE: Back and Forward (⌘[, ⌘], the toolbar arrows, the mouse's side buttons) across files;
  every jump goes through `navigate.place.goTo`, which records the place left
  (`navigate.history.back`, `forward`, `list`, `clear`).
- `zen_lsp::fake` (the `fake` feature), a scriptable language server for tests; the client announces
  location links, label offsets of parameters and signature help context, and positions accept
  `\r\n` and `\r` line breaks.
- Zen Toolbox settings (docs/toolbox-settings.md, ADR-0043): the tools location for new
  installations, the release channel, manual or automatic checks for releases, automatic
  self-updates, shell scripts and the theme, in the existing `settings.json`. The
  `toolbox.settings.get` and `toolbox.settings.set` commands read and change them; `--channel` and
  `--tools-dir` override them for one run. With manual checks Toolbox stays offline until asked and
  says that releases are not checked yet.
- Zen Toolbox shell launchers (docs/toolbox-launchers.md): `toolbox.shellScripts.generate` writes
  `ronin`, `garden`, ... (`.cmd` on Windows) for the installed tools into a chosen directory,
  `~/.local/bin` by default, and `toolbox.shellScripts.remove` deletes them; files without the
  Zen Toolbox marker are never touched, and the result says whether the directory is on `PATH`.
  Each tool lists its launcher name. With `toolbox.shellScripts.enabled` on, Toolbox keeps them
  current in `toolbox.shellScripts.location`: at once, at start-up and after installs, updates and
  uninstalls; turning it off removes the launchers it wrote.
- `zen-project` (L1) with the recent projects list, moved from `zen-workbench` (which re-exports it)
  so that Zen Toolbox can read the lists of all IDEs. The recent-projects file is now a documented,
  shared format: `docs/recent-projects.md`, ADR-0041 (proposed); `zen_project::recent::read` reads an
  IDE's list by its application id.
- `toolbox.projects.list` and `toolbox.project.open` in `zen-toolbox`: the recent projects of every
  Zen IDE, one row per folder owned by the IDE that opened it last (others in `alsoIn`), newest
  first, with missing folders marked and a case-insensitive `query`; opening uses the most recent
  IDE unless `toolId` is given. Demo projects for `--demo`. No Projects tab UI yet.
- `toolbox.tool.launch` takes an optional project folder (`path`) and starts the IDE with it, for
  the coming Projects tab; `zen_update::install::Installation::launch_with` builds the command line
  (`bin/<id> <folder>`, or `open -n -a <Name>.app --args <folder>` on macOS).
- `zen-cli` (`apps/zen-cli`), the command-line client (ADR-0007): `zen-cli commands` lists every
  workbench and toolbox command with its id and title (`--json` adds descriptions and parameter
  schemas), and `zen-cli run <id> --params '<json>'` (or `--params -` for standard input) runs one
  in a fresh session and prints its result as JSON. Errors are one line on standard error; exit
  code 2 means a usage error, 1 a failed command. Help and errors are in English and Russian
  (`ZEN_LANGUAGE`, otherwise the system language).
- Base UI components in `zen_ui::components` (primary and icon buttons, list row, tabs, popup menu,
  toggle, text field), now used by the Zen Toolbox window, and `zen_ui::offscreen`, which renders
  egui views on the CPU into images for checking them against the specification;
  `cargo run -p zen-ui --example components -- <folder>` writes the components as PNG files. The
  hint of the Toolbox's remote environment item is translated now.
- `zen-editor` (L3), the editor model: carets and selections that never overlap, caret movement
  (characters, words, lines with a goal column, smart line start, pages, the document) and edits at
  every caret, merged where they meet. The `editor.caret.*`, `editor.selection.*` and
  `editor.edit.*` commands drive it, also for AI agents; the editor's text field follows them.
- Complete changelogs in `changelog/`: one file per product and per plug-in with every release,
  newest first, recovered from the release server for the early ones. Releases read their notes
  from there and refuse to go without them; the gate checks the files; `cargo xtask changelog
  --json` writes them for the website.
- Bundled plug-ins (ADR-0036): Markdown, YAML, JSON, TOML, Docker, GitLab CI, Ignore files,
  Dotenv and Shell ship inside every IDE and are on from the start; they can be switched off in
  Settings → Plugins (and with `set_plugin_enabled`), not uninstalled.
- The JSON plug-in (`plugins/json`), also for JSONC, JSON5 and JSON Lines: keys, strings, escapes,
  numbers, `true`/`false`/`null`, punctuation and comments.
- The TOML plug-in (`plugins/toml`), also for `Cargo.lock`, `Pipfile`, `poetry.lock` and `uv.lock`:
  table headers, keys, strings with escapes, numbers, dates and times, booleans and comments.
- The Shell Script plug-in (`plugins/shell`) for sh, Bash, Zsh and ksh scripts and shell rc files:
  keywords, builtins, function definitions, variables, strings, heredocs, comments and operators.
- Plug-in "Dotenv" (`plugins/env`): highlighting for `.env`, `.env.*`, `*.env`, `.envrc` and
  `.flaskenv` (comments, `export`, names, quoted and multi-line values, `${VAR}` references).
- The Dockerfile plug-in (`plugins/dockerfile`): instructions, flags, strings, variables, heredocs,
  comments and parser directives in `Dockerfile`, `Containerfile` and their variants.
- The YAML plug-in (`plugins/yaml`), also for Docker Compose files and Kubernetes manifests: keys,
  strings, numbers, constants, comments, anchors, tags, block scalars and `${VAR}` interpolation.
- The GitLab CI plug-in (`plugins/gitlab-ci`) for `.gitlab-ci.yml`: keywords, job names, CI/CD
  variables, `rules: if` expressions, anchors and `!reference`, strings, numbers and comments.
- The SQL plug-in (`plugins/sql`): keywords and types, functions, strings (including PostgreSQL
  dollar quotes), numbers, comments and parameters, for standard SQL, PostgreSQL, MySQL and SQLite.
- IDE: a scroll gesture is locked to one axis, so text no longer wobbles when a trackpad moves both.
- IDE: Settings → Appearance & Behavior → Language (system, English, Russian), applied at once.
- IDE: Settings → Plugins with Marketplace (the signed catalog, search, install, update) and
  Installed (uninstall); agents use `list_plugins`, `install_plugin` and `uninstall_plugin`.
- Plug-in "Ignore files": highlighting for `.gitignore`, `.dockerignore`, `.hgignore` and other
  ignore files (comments, negations, globs, character classes, escapes).
- IDE: installed plug-ins update themselves in the background when the catalog has a newer version.
- Zen Toolbox has its own icon in the style of the Zen icon set (`assets/icons/products/toolbox.svg`).
- IDE: the workspace is restored at start: open files, expanded folders, tool windows, the Terminal
  with its tabs, the active project tab, and the main window's full screen, size and position.
- GoGarden: highlighting for `go.mod` and `go.work` (directives, versions, comments) and for
  `go.sum` and `go.work.sum` (versions in green, checksums in grey), built in.
- IDE: test files have their own icon, the language's icon with a muted green check mark:
  `*_test.go`, `*_test.py`, `test_*.py`, `*.test.ts` and `*.spec.ts` (and `.js`, `.tsx`, `.jsx`),
  `*Test.kt`, `*Test.java`, `*_spec.rb`.
- IDE: projects open as tabs of the main window by default; Settings → Appearance & Behavior
  chooses a new window instead.
- The product icons use the pastel tiles of the Zen icon set.
- IDE: a welcome window (sections, project search, recent projects with their
  branches, Remove from Recent Projects); the project widget lists open and recent projects;
  closing the last project window returns to the welcome window; the application icon in the main
  toolbar; every control that is not implemented yet says "In development".
- IDE: new files and folders from the project tree's context menu, Copy Path / Paste, and files
  dropped from the Finder; commands `file.fs.newFile`, `file.fs.newDirectory`, `file.fs.copyInto`.
- IDE: the Terminal tool window (⌥F12 or the stripe button): tabs with the login shell in the project
  folder; agents use it through `terminal_open`, `terminal_write` and `terminal_read`.
- IDE: the Settings dialog (⌘,, the gear) with Tools → MCP Server: turn the server on and off and
  choose its port, applied at once; `<ide> mcp` is a stdio bridge for agents (docs/automation.md).
- ADR-0033: every capability is reachable by AI agents. `zen-automation` serves MCP on
  `127.0.0.1` (`ZEN_AUTOMATION=1`; GoGarden on port 63530): list windows and commands, dispatch any
  command in a project window, open, focus and close projects, read documents with unsaved changes.

### Changed

- Code Vision's authors are off by default: `editor.codeVision.authors` and
  `codeVision.authors` default to false (`AUTHORS_BY_DEFAULT`); `IdeSettings::load` turns them off
  once in a file without the marker `codeVision.authorsDefaultV2` (older versions wrote
  `authors: true` without anyone choosing it) and saves the marker, so later choices are kept. The
  window action `view.codeVision.authors.toggle` (Show Code Authors, ⌃⌥A) switches it from View → Appearance (a check item in the
  native menu, `main_menu::CHECKED`), Find Action, `run_action` and a lit person button at the right
  end of the status bar.
- The Dockerfile plug-in is now "Docker" (0.2.0, not published yet): it also highlights Docker
  Compose files (`docker-compose*.yml`, `compose*.yml`), telling them from Dockerfiles by content.
- IDE: Markdown colours: heading text pink and italic with its `#` marks in the
  keyword orange, code in the string green (Markdown plug-in 0.2.0, not published yet).
- Terminal: the ANSI greens are toned down to the editor's restrained string green.

### Fixed

- Command and MCP tool descriptions no longer carry stray backslashes and line breaks.
- zen-ui: the Git Log's ref labels are never cut (`git_window::format::labels_layout`): the subject
  shortens first, down to 160 points, then the last labels collapse into "+N" and the first label's
  name ends in "…", with the names in the tooltip. The Git workspace draws the
  message, author, date, id and the branch pills' names truncated with "…" inside their columns,
  the full message in a tooltip.
- `cargo xtask bench-editor` runs the GoGarden it has just built, from the target directory Cargo
  used (`CARGO_TARGET_DIR` or `cargo metadata`'s `target_directory`), instead of always
  `target/release/gogarden`.
- zen-ui: a project window gives the keyboard back to what had it (the editor, or another widget)
  when a modal dialog or a popup closes (`ide/focus_return.rs`); a test opens Settings, Go to Line
  and Search Everywhere over an editor, closes them with ⎋ through `press_keys`, and runs ⌃G.
- zen-ui Settings: every page scrolls inside the page area (one `egui::ScrollArea` under the page
  title), so at the dialog's minimum size no page runs under OK, Cancel and Apply; a test checks
  every page at that size (`Offscreen::step` runs a frame without painting it).
- zen-ui Settings: the section tree scrolls inside the sidebar (`egui::ScrollArea`) and scrolls to
  the selected section when the dialog opens on it or the search moves to it; a test checks the
  tree at the dialog's minimum size.
- zen-ui automation: `ZEN_AUTOMATION` no longer writes `mcpEnabled` and its port into the saved
  settings (`IdeApp::enable_automation` keeps the port for the run; `apply_settings` runs the
  server when the setting or the variable asks for it).
- zen-run: a stopped or finished program reports its exit as soon as its process group is gone,
  without waiting for the end of pipes that another program spawned at the same moment inherited
  (macOS sets close-on-exec only after `pipe()`); `a_second_stop_kills_at_once` waited out another
  test's two-second grace under the gate.
- IDE status bar: the indent shows what Tab and Enter insert in the file, from the same source as
  editing (`project_settings::indent_unit_of`): Tab for Go and for files indented with tabs.
- IDE: a project's `.idea` code style reaches plain text and other files: `.txt` takes the `TEXT`
  style, then `OTHER_INDENT_OPTIONS`; a file type without a style of its own takes
  `OTHER_INDENT_OPTIONS` (Tab in a `.txt` inserted spaces where the project asked for a tab).
- Find Action and `find_action` list the image viewer's zoom by its steps (Zoom In, Zoom Out, Actual
  Size, Fit Zoom to Window: `editor.image.zoom/in` and so on, with their parameters); the single
  **Zoom Image** entry ran without the step it needs and failed.
- Completion: a generic function chosen right before `(` keeps the call's arguments without its
  type parameters (`Map[T any](xs []T)` at `|Map(xs)` gives `Map(xs)`, not `Map[T any](xs)`).
- IDE: `navigate.usages.find`, Go to Declaration and the other lookups that take `{ path, offset }`
  work for a file that is not open: it is opened in its language server, not in a tab, for the
  lookup (docs/automation.md). They failed with "No file is open".
- Automation: MCP calls are answered while the window is minimised, hidden or covered. They were
  answered inside a drawn frame, and macOS draws no frames for a window it does not show, so calls
  timed out after 30 s; `IdeApp::logic`, called from `eframe::App::logic`, now answers them without
  drawing (docs/automation.md).
- IDE: a second project window no longer freezes the first; windows are independent (deferred
  viewports) and the title is no longer reset every frame.
- Terminal emulator in the platform: `zen-terminal` (L2) runs the user's login shell (or any
  program) in a PTY, emulates VT/xterm with `alacritty_terminal` (pure Rust, Apache-2.0) and exposes
  screen snapshots (colours, attributes, cursor, scrollback), resizing, title and exit status, and the
  xterm key encoding. `zen_ui::terminal::TerminalView` draws it with the bundled editor font and sends it the
  keyboard; `cargo run -p zen-ui --example terminal` shows it alone. The bundled font's Bold and Italic
  are installed as font families for it.
- File and folder icons for the project tree and editor tabs, drawn in code by us in the New UI
  style (`zen_ui::file_icons`): language badges and symbols for Go, Rust, Python, JavaScript,
  TypeScript, web, data, config, build, image, archive, font, lock, Git, licence and other files,
  and folders tinted for sources, tests, docs, assets, VCS settings and excluded output. The
  `file_icons` example of `zen-ui` shows them all.
- Plug-ins (ADR-0031): sandboxed WebAssembly highlighters run by `wasmi`; a signed plug-in catalog
  on the release server (`releases/plugins.json`, `zen-releases publish-plugin`,
  `cargo xtask release plugin`); the IDE offers to install a plug-in when a file type has none.
  The first plug-in is Markdown (`plugins/markdown`). ABI: docs/plugins.md.
- The landing page lists every product's releases under "What's new", newest first, with the
  notes rendered from Markdown; release notes come from the product's `## <Product> <version>`
  section in CHANGELOG.md when `cargo xtask release` is given none (docs/releasing.md).
- What's new: Zen Toolbox shows the release notes from the manifest for each tool (the row link
  shows what an update brings, the ⋮ menu every release) and its own notes once after it updates
  itself; `toolbox.tool.releaseNotes` and a small Markdown reader in `zen-toolbox`.
- GoGarden: constants declared in a file are highlighted wherever they are used (italic purple).
- IDE: one window per project; the projects open at quit reopen at the next start; the welcome
  screen lists recent projects. A file passed on the command line opens with its folder.
- IDE: the window follows the New UI with islands: the main toolbar in the title bar with the
  project and VCS widgets, tool window stripes on both sides, the Project tool window header and
  root path, excluded folders, outlined editor tabs, the navigation path in the status bar.
- `ZEN_UI_SCREENSHOT=<file.png>` writes a snapshot of the window and quits, for checking the UI against its specification.
- `cargo xtask icon-import`: cleans a Figma export of a product tile into assets/icons; the new
  WebWave icon.
- Product icons from the Zen icon set (Figma) in Zen Toolbox, the IDE welcome screen, the app
  bundles and the landing page.
- ADR-0029: no rollback of IDEs for now; Toolbox keeps only the active version and prunes older ones.
- ADR-0028: Zen Toolbox deletes the replaced copy after updating itself and removes leftovers of
  earlier updates at start-up.
- Updates refresh icons: Zen Toolbox replaces its whole application bundle when it updates itself,
  and every replaced bundle is re-registered with Launch Services so Finder and the Dock drop cached icons.
- ADR-0027: the web IDE is WebWave instead of TsTorii.
- GoGarden 0.1, the first IDE offered (ADR-0026): `zen-lang-go` (Go highlighting) and `apps/go-garden`.
  RustRonin is withdrawn from the manifest for now with the new `zen-releases withdraw`.
- On macOS, IDE releases are application bundles and Zen Toolbox copies the active version into
  /Applications (or ~/Applications), launching it with `open` (ADR-0025).
- The IDE window in `zen-ui` (shared by every Zen IDE) and RustRonin 0.1: welcome screen, project
  tree, tabs, editor with line numbers and highlighting, save, undo/redo, status bar; the bundled editor font.
- Platform for the first IDE window: `zen-vfs` (L0: listing, UTF-8 reading, atomic saving),
  `zen-syntax` (L1: highlighting types and the `Highlighter` extension point), `zen-workbench` (L5:
  project, documents on `zen-text` buffers, `file.*` and `editor.*` commands), and `zen-lang-rust`
  (Rust highlighting with a forgiving lexer of its own).
- Localisation (ADR-0024): `zen-i18n` with Project Fluent, English and Russian for all of Zen
  Toolbox, a language picker in the settings menu, persisted settings, and `cargo xtask i18n` in the gate.
- `install.sh`: installs the latest Zen Toolbox from the terminal, without the Gatekeeper prompt
  (`curl -fsSL https://ide.ifzenelse.space/install.sh | sh`); ADR-0023 moves the site to ide.ifzenelse.space.
- A landing page stub at the root of the release server (deploy/releases/site), with a download
  button for the newest Zen Toolbox read from the manifest; the server now answers on
  https://ide.ifzenelse.space and keeps plain HTTP, without a redirect, on the domain and the IP.
- The production release server (deploy/releases: Docker, Caddy, deploy script) and
  `cargo xtask release-toolbox`: release build, `Zen Toolbox.app`, signed update artifact, `.dmg` installer.
- Zen Toolbox checks the production server by default, shows whether it is reachable and that there
  are no releases yet, and re-checks every 30 minutes.
- `zen-update` (L2, ADR-0021): release manifest schema 1, ed25519 signing and verification, version
  selection, downloads, installation, rollback and executable replacement.
- Zen Toolbox installs, updates, rolls back, uninstalls and launches tools from the release server,
  and updates itself (ADR-0022); `cargo xtask dev-server` and `dev-publish` for a local stand.
- `apps/zen-releases`: the release server (`serve`), key generation (`keygen`) and publishing (`publish`);
  ADR-0020 and docs/release-manifest.md.
- Zen Toolbox prototype: `zen-toolbox` (L5, tools and `toolbox.*` commands), `zen-ui` (L6, theme,
  Inter font, icons drawn in code, the main window) and `apps/zen-toolbox` with `--demo` data.
- ADR-0018: Zen Toolbox is the entry point.
- ADR-0019: product names alliterate; PyLantern becomes PyPetal and SharpKatana becomes SharpShuriken.
- Zen Toolbox as a tray widget: a frameless window that drops down from the tray icon, cannot be
  moved, hides on focus loss or Escape, and has no Dock icon on macOS; restyled with gradient
  product tiles and the indigo palette.
- `zen-core` (L4): command registry (ids, typed params with JSON schemas, dispatch by id or by type,
  availability, deprecated aliases), session with typed feature state, and JSON-backed settings.
- `zen-text` (L0): text snapshots on a rope, edits with inverses, undo/redo with typing groups, LSP
  position conversions (UTF-8 and UTF-16).
- GitLab CI: one job running `cargo xtask ci` on pull requests and on `main`.
- `cargo xtask ci`: the quality gate (format, clippy, test, layers, deny, wasm) and `deny.toml`.
- `cargo xtask layers`: enforces the layering rules; ADR-0014 refines the layer model.
- Cargo workspace skeleton: layered directories, workspace lints, stable toolchain pin, `cargo xtask`.

- Project charter: README, agent instructions, architecture overview, naming reference, roadmap,
  contribution and AI workflow guides, and the founding architecture decision records
  (ADR-0001 to ADR-0013), and the interface specification tracker.
