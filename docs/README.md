# Documentation

## Start here

- [../README.md](../README.md): what Zen IDE is.
- [../AGENTS.md](../AGENTS.md): rules and orientation for AI agents and new contributors.
- [../ROADMAP.md](../ROADMAP.md): phases and milestones.
- [development.md](development.md): toolchain setup and everyday commands.

## Design

- [architecture.md](architecture.md): the platform's layers, crates and key concepts.
- [naming.md](naming.md): product, crate, binary and command names, and the third-party trademarks they respect.
- [releasing.md](releasing.md): versions, release notes, and how to release the products and plug-ins.
- [licences.md](licences.md): the third-party notices in every release artefact, how `cargo xtask notices` makes them, and what is downloaded at run time instead.
- [network.md](network.md): no telemetry, and every network call the IDEs and Zen Toolbox make, with its code and the setting that turns it off (the website's privacy page says the same).
- [guide/](guide/README.md): the user guide, published as the documentation on the website.
- [performance.md](performance.md): how fast the editor is, how it is measured (`cargo xtask bench-editor`), and the baseline; rust-analyzer on this repository through a headless RustRonin, before and after its tuning.
- [../changelog/README.md](../changelog/README.md): the changelogs of every product and plug-in.
- [automation.md](automation.md): AI agents in the IDE: turning MCP on, connecting an agent, the tools.
- [plugins.md](plugins.md): plug-ins: what they are, ABI v0, the catalog, `cargo xtask release plugin`.
- [release-manifest.md](release-manifest.md): the signed release manifest and the release server.
- [toolbox-settings.md](toolbox-settings.md): Zen Toolbox settings: the file, every key, command-line overrides.
- [toolbox-launchers.md](toolbox-launchers.md): shell launchers (`gogarden`, `zen`, ...) from Zen Toolbox and Tools → Create Command-line Launcher…: names, the marker rule, the consented `PATH` line, commands, the settings that keep them current.
- [recent-projects.md](recent-projects.md): the recent-projects file every IDE writes and Zen Toolbox reads.
- [run-configurations.md](run-configurations.md): the run configurations file of a project, `.zen/runConfigurations.json`.
- [themes.md](themes.md): themes: the file format, inheritance, and every colour key with its Zen Dark value.
- [themes/import-vscode.md](themes/import-vscode.md): importing VS Code colour themes: the mapping tables, scope matching, alpha and includes.
- [themes/import-jetbrains.md](themes/import-jetbrains.md): importing JetBrains colour schemes (`.icls`) and UI themes (`.theme.json`): the mapping tables and what is not carried over.
- [git-workspace.md](git-workspace.md): the Git workspace tab: its specification, what we have, the delivery order and what is out of scope.
- [database-workspace.md](database-workspace.md): the Database workspace: its specification, where it is switched on, the workspace host it expands in, what is built, the delivery order and what is out of scope.
- [database-drivers.md](database-drivers.md): the external database driver protocol v1: how a plug-in's driver program talks to the IDE, the plug-in manifest, the reference driver and the conformance suite; the first-party driver programs in `drivers/` (SQL Server, MongoDB, DuckDB, and Oracle through SQLcl).
- [jetbrains-compatibility.md](jetbrains-compatibility.md): working side by side with JetBrains IDEs: the `.idea/` files read, the commands, and how they are never written.
- [behaviour.md](behaviour.md): the interface specification: the method and the status of every component.
- [behaviour/ide/main-window.md](behaviour/ide/main-window.md), [welcome.md](behaviour/ide/welcome.md), [toolbox/main-window.md](behaviour/toolbox/main-window.md): the main window, the welcome window and Zen Toolbox's window.
- [behaviour/ide/database.md](behaviour/ide/database.md): the Database workspace's layout, menus, connection types, navigator, SQL execution, data editor, shortcuts.
- [behaviour/egui-spike.md](behaviour/egui-spike.md): the egui spike: text rendering at the specified metrics, performance, accessibility.

## Process

- [ai-workflow.md](ai-workflow.md): how to work with AI agents: task format, working loop, definition of done.
- [../CONTRIBUTING.md](../CONTRIBUTING.md): branches, commits, sign-off, licence.

## Decision records

Architecture decision records (ADRs) explain why the project is the way it is. They are numbered,
and an accepted ADR is never edited: a later ADR supersedes it. Use [adr/0000-template.md](adr/0000-template.md)
for new records, with the number `cargo xtask adr next` names: the next one that no branch uses.

The records themselves are the index ([ADR-0188](adr/0188-release-notes-and-decision-records-in-files-of-their-own.md)):
[adr/](adr/) lists them by number, and `cargo xtask adr list` prints every record's title and
status as a table. A new record is not added to the table below, which lists the records up to
ADR-0179 as it stood then; the gate refuses two records under one number.

| ADR | Title | Status |
|---|---|---|
| [0001](adr/0001-record-architecture-decisions.md) | Record architecture decisions | Accepted |
| [0002](adr/0002-open-source-clean-room-pure-rust.md) | Open source, clean-room, pure Rust | Accepted; decision 2 (the sentence on software under an open licence) amended by [ADR-0280](adr/0280-clean-room-process.md) |
| [0003](adr/0003-hybrid-language-intelligence.md) | Hybrid language intelligence | Accepted |
| [0004](adr/0004-monorepo-and-layering.md) | A single monorepo with enforced layering | Accepted; layer numbering amended by [ADR-0014](adr/0014-refined-layer-model.md) |
| [0005](adr/0005-naming.md) | Naming | Accepted; product names amended by [ADR-0019](adr/0019-alliterative-product-names.md) |
| [0006](adr/0006-desktop-first-ui-stack.md) | Desktop first, egui on wgpu, web as a thin client | Accepted; the Phase 2 spike concluded by [ADR-0054](adr/0054-egui-spike-concluded.md) |
| [0007](adr/0007-everything-is-a-command.md) | Everything is a command | Accepted |
| [0008](adr/0008-toolbox-and-updates.md) | Zen Toolbox and signed updates | Accepted; decision 4 (timing) amended by [ADR-0015](adr/0015-toolbox-first.md); rollback removed for now by [ADR-0029](adr/0029-no-rollback-for-now.md) |
| [0009](adr/0009-english-only.md) | English only | Accepted |
| [0010](adr/0010-hosting.md) | Hosting on GitLab, moving to GitHub | Accepted |
| [0011](adr/0011-plugins-and-jetbrains-compatibility.md) | Plug-ins and JetBrains compatibility | Accepted |
| [0013](adr/0013-msrv-latest-stable.md) | Minimum supported Rust version is the latest stable | Accepted |
| [0014](adr/0014-refined-layer-model.md) | Refined layer model: the core gets its own layer | Accepted; the layer of `zen-update` amended by [ADR-0021](adr/0021-zen-update-is-a-service.md); `zen-core` in L3 and `zen-modules` in L4 by [ADR-0071](adr/0071-built-in-modules.md) |
| [0015](adr/0015-toolbox-first.md) | Build Zen Toolbox first | Superseded by [ADR-0280](adr/0280-clean-room-process.md) |
| [0016](adr/0016-toolbox-colour-scheme.md) | Zen Toolbox may use its own colour scheme | Accepted; decisions 1 and 3 amended by [ADR-0280](adr/0280-clean-room-process.md) |
| [0017](adr/0017-project-domain.md) | Project domain: ifzenelse.space | Accepted; decision 3 (the subdomain) amended by [ADR-0023](adr/0023-ide-subdomain.md) |
| [0018](adr/0018-toolbox-is-the-entry-point.md) | Zen Toolbox is the entry point | Accepted |
| [0019](adr/0019-alliterative-product-names.md) | Product names alliterate | Accepted; the web IDE renamed by [ADR-0027](adr/0027-webwave.md); DevopsDaruma added by [ADR-0049](adr/0049-devopsdaruma.md); TestTotami added by [ADR-0067](adr/0067-testtotami.md) |
| [0020](adr/0020-release-server.md) | The release server stores, the publisher signs | Accepted |
| [0021](adr/0021-zen-update-is-a-service.md) | zen-update is a service in layer L2 | Accepted |
| [0022](adr/0022-toolbox-updates-itself.md) | Zen Toolbox updates itself | Accepted; the kept copy (decision 2) removed by [ADR-0028](adr/0028-no-self-update-backup.md) |
| [0023](adr/0023-ide-subdomain.md) | Zen IDE lives at ide.ifzenelse.space | Accepted |
| [0024](adr/0024-localisation.md) | Localisation with Project Fluent, English and Russian from the start | Accepted |
| [0025](adr/0025-macos-applications.md) | On macOS, IDEs are applications in the Applications folder | Accepted; rollback removed for now by [ADR-0029](adr/0029-no-rollback-for-now.md) |
| [0026](adr/0026-gogarden-first.md) | GoGarden is the first IDE released | Accepted |
| [0027](adr/0027-webwave.md) | The web IDE is WebWave | Accepted |
| [0028](adr/0028-no-self-update-backup.md) | Zen Toolbox keeps no copy of itself after updating | Accepted |
| [0029](adr/0029-no-rollback-for-now.md) | No rollback of IDEs for now | Accepted |
| [0031](adr/0031-plugins.md) | Plug-ins: WebAssembly modules, a signed catalog and an offer on first open | Accepted |
| [0033](adr/0033-ai-access-to-every-api.md) | Every capability is reachable by AI agents | Accepted |
| [0034](adr/0034-restart-after-update.md) | An IDE restarts into its new version after an update | Accepted |
| [0035](adr/0035-update-offer-in-the-ide.md) | The IDE offers and installs its own updates | Accepted |
| [0036](adr/0036-bundled-plugins.md) | Popular plug-ins are bundled with every IDE | Accepted; extended by [ADR-0050](adr/0050-products-are-plugin-bundles.md) (products are bundles; basic web in the base set; the list is now `BASE_PLUGINS`) |
| [0037](adr/0037-image-viewer.md) | Images open in a built-in image viewer | Accepted |
| [0038](adr/0038-keymap.md) | One keymap drives every shortcut, and the user's changes live in keymap.json | Accepted |
| [0039](adr/0039-main-menu.md) | The main menu is built from keymap actions; on macOS it is the native menu bar | Accepted |
| [0040](adr/0040-https-pure-rust-tls.md) | The clients use HTTPS through rustls with RustCrypto's pure-Rust algorithms | Accepted |
| [0041](adr/0041-recent-projects-file.md) | The recent-projects file is a shared format | Proposed |
| [0042](adr/0042-zen-cli-output-and-exit-codes.md) | zen-cli output and exit codes | Proposed |
| [0043](adr/0043-toolbox-settings-and-shell-launchers.md) | Zen Toolbox settings and shell launchers | Proposed |
| [0044](adr/0044-how-a-refactoring-changes-files.md) | How a refactoring changes files | Proposed |
| [0045](adr/0045-themes.md) | Themes: model, file format, inheritance and importers | Proposed |
| [0046](adr/0046-project-models-and-outlines.md) | Project models and outlines from language modules | Proposed |
| [0047](adr/0047-version-control-gitoxide-and-git.md) | Version control reads with gitoxide and writes with the git program | Proposed |
| [0048](adr/0048-run-configurations-and-the-run-service.md) | Run configurations and the run service | Proposed |
| [0049](adr/0049-devopsdaruma.md) | The DevOps IDE is DevopsDaruma | Accepted |
| [0050](adr/0050-products-are-plugin-bundles.md) | Every capability is a plug-in; products are bundles; basic web is in every IDE | Accepted; the bundles are `BASE_PLUGINS` and `PRODUCT_PLUGINS` in `xtask/src/release.rs` ([docs/plugins.md](plugins.md#in-the-ide)); extended by [ADR-0071](adr/0071-built-in-modules.md) (Go, Rust and the next first-party specialisations are built-in modules switched on per product) |
| [0051](adr/0051-jetbrains-keymaps-and-idea-project-settings.md) | JetBrains keymaps and .idea project settings | Proposed |
| [0052](adr/0052-debugger-zen-dap-and-delve-over-reverse-dap.md) | Debugger: zen-dap and Delve over reverse DAP | Proposed |
| [0053](adr/0053-fonts-ui-scale-and-code-style.md) | Fonts, UI scale and code style per language | Proposed |
| [0054](adr/0054-egui-spike-concluded.md) | The egui spike is concluded: the stack stays, accessibility is our work | Accepted |
| [0055](adr/0055-language-servers.md) | Language servers are declared, found, and installed only on request | Proposed |
| [0056](adr/0056-plugin-abi-v1.md) | Plug-in ABI v1: folding, outline, completion and abbreviation expansion | Proposed |
| [0057](adr/0057-git-workspace.md) | A Git workspace tab | Accepted; decisions 3, 4 and 7 amended by [ADR-0280](adr/0280-clean-room-process.md) |
| [0058](adr/0058-code-actions.md) | Context actions (⌥Enter) and the edits a server sends | Proposed |
| [0059](adr/0059-go-mod-dependency-checks.md) | go.mod dependency checks over the network, and inspections with quick-fixes | Accepted |
| [0060](adr/0060-local-history-store.md) | Local History: a content-addressed store in the IDE's data folder | Proposed |
| [0061](adr/0061-http-client.md) | The HTTP Client: `.http` files in a plug-in, sending in the platform | Proposed |
| [0062](adr/0062-plugins-page.md) | Settings → Plugins: commands, release notes in the catalog, updates on request | Proposed |
| [0063](adr/0063-plugin-colours-and-typing.md) | Plug-in ABI v1 additions: colour literals and what typing adds | Proposed |
| [0064](adr/0064-undo-git-operations.md) | Undo and Redo of Git operations | Accepted |
| [0065](adr/0065-interactive-rebase-without-an-editor.md) | Interactive rebase without an editor | Accepted |
| [0066](adr/0066-background-tasks-and-go-mod-checks.md) | Background tasks, and go.mod checks for every common case | Accepted |
| [0067](adr/0067-testtotami.md) | The QA IDE is TestTotami, a bundle of separate QA plug-ins | Accepted; decision 6 (optional plug-ins of a bundle) is implemented by [ADR-0073](adr/0073-optional-bundle-plugins-areas.md) |
| [0068](adr/0068-changelists-and-shelf.md) | Changelists and the shelf | Proposed |
| [0069](adr/0069-toolbox-launch-at-login.md) | Zen Toolbox starts at login, in the tray | Accepted |
| [0070](adr/0070-worktrees-and-submodules.md) | Worktrees and submodules in the Git workspace | Proposed |
| [0071](adr/0071-built-in-modules.md) | First-party specialisation plug-ins are built-in modules, switched on per product | Accepted |
| [0072](adr/0072-merge-conflict-editor.md) | The merge conflict editor | Accepted |
| [0073](adr/0073-optional-bundle-plugins-areas.md) | Optional plug-ins of a bundle (areas) and their first-start choice | Accepted |
| [0074](adr/0074-git-workspace-avatars.md) | Authors' pictures in the Git workspace, fetched only when turned on | Accepted |
| [0075](adr/0075-avatars-on-by-default.md) | Authors' pictures on by default | Accepted |
| [0076](adr/0076-zen-light-and-readable-themes.md) | Zen Light, light bases and readable imported themes | Accepted |
| [0077](adr/0077-incremental-highlighting-with-line-lexers.md) | Incremental highlighting and folding with our own line lexers | Proposed |
| [0078](adr/0078-language-server-writer-thread.md) | Messages to language servers are written by a thread of their own | Proposed |
| [0079](adr/0079-zen-macos-unsafe-exception.md) | zen-macos, the one crate allowed `unsafe`, so that a system quit asks first | Accepted |
| [0080](adr/0080-inspections-follow-the-files-they-read.md) | Inspections follow the files they read; every failed fix is told | Accepted |
| [0081](adr/0081-search-everywhere-contributors.md) | Search Everywhere contributors, streaming results and holding them back | Proposed |
| [0082](adr/0082-trigram-index-for-find-in-files.md) | A trigram index for Find in Files | Proposed |
| [0083](adr/0083-search-scopes.md) | Search scopes, their pattern language and the project's own named scopes | Proposed |
| [0084](adr/0084-headless-ide.md) | A headless IDE for AI agents: the same app, sessions and MCP server without a window | Proposed |
| [0085](adr/0085-headless-ide-per-agent-session.md) | A headless IDE per agent session: `<ide> mcp --headless`, shared, ended by its last bridge | Proposed |
| [0086](adr/0086-ide-commands-on-path.md) | The IDE commands on PATH | Proposed |
| [0087](adr/0087-agent-sized-mcp-answers.md) | Agent-sized MCP answers: one-call edits, ranged reads, a catalogue on demand, no silent empty answers | Proposed |
| [0088](adr/0088-module-toolchains-side-by-side.md) | Built-in modules keep their toolchains side by side; diagnostics are pulled too | Proposed |
| [0089](adr/0089-mcp-answers-once-and-in-english.md) | MCP answers carry their payload once, and speak English to agents | Proposed |
| [0090](adr/0090-one-based-places-and-lean-test-results.md) | One-based places, one path per match, lean numbered reads and test results for agents | Proposed |
| [0091](adr/0091-agent-answers-round-three-and-command-tools.md) | Whole descriptions, checked files, grouped usages, compact one-based run targets, long symbol waits, and the main commands as MCP tools | Proposed |
| [0092](adr/0092-places-by-line-and-editing-through-the-ide.md) | Places by line and column, project-relative symbols, the end of a load waited for, a missing server named once, and editing through the IDE | Proposed |
| [0093](adr/0093-rust-analyzer-settings-tuned-and-layered.md) | rust-analyzer's settings are tuned for large workspaces, and the user and the project can change them | Proposed |
| [0094](adr/0094-file-indent-detection-and-indent-sources.md) | Detect a file's own indentation, and the order of the indentation sources | Proposed |
| [0095](adr/0095-search-everywhere-toolbar.md) | Search Everywhere's toolbar, options and the Text and Recent Files contributors | Proposed |
| [0096](adr/0096-two-keystroke-shortcuts-and-the-keypad-star.md) | Shortcuts of two keystrokes, and the numeric keypad's `*` read from the window events | Proposed |
| [0097](adr/0097-line-separators-of-documents.md) | Documents keep their line separators; new lines follow the file's | Proposed |
| [0098](adr/0098-extend-selection-without-a-parser.md) | Extend Selection from text units and the language server's selection ranges | Proposed |
| [0099](adr/0099-unwrap-remove-without-a-parser.md) | Unwrap/Remove from brackets and keywords, without a parser | Proposed |
| [0100](adr/0100-editorconfig-code-style.md) | EditorConfig is a per-file code style layer above the project's | Proposed |
| [0101](adr/0101-custom-folding-regions-and-manual-regions.md) | Custom folding regions from comments, and manual regions kept by the document | Proposed |
| [0102](adr/0102-search-everywhere-commands-calculator-shortcuts.md) | Search Everywhere's `/` commands, calculator, and shortcuts, abbreviations and synonyms of actions | Proposed |
| [0103](adr/0103-find-window-occurrences.md) | The Find tool window's occurrences are a background search kept by the workbench; every replacement goes through one guarded engine | Proposed |
| [0104](adr/0104-hierarchy-tool-window-tabs.md) | The Hierarchy tool window keeps one live hierarchy per tab | Proposed |
| [0105](adr/0105-search-everywhere-text-tab.md) | Search Everywhere's Text tab: Go to Text, its switches and file mask, and Text on All | Proposed |
| [0106](adr/0106-on-save-whitespace-options.md) | What a save does to whitespace: modified lines since the last save, caret lines, significant spaces and EditorConfig | Proposed |
| [0107](adr/0107-editorconfig-save-properties-and-files.md) | EditorConfig on save and on open, its status bar mark, and `.editorconfig` files in the IDE | Proposed |
| [0108](adr/0108-file-type-registry-and-ignored-files.md) | One file type registry with user-editable patterns, and the ignored files and folders | Proposed |
| [0109](adr/0109-bookmark-lists.md) | Bookmark lists are ordered, the default one first; the Bookmarks tool window reads them through commands | Proposed |
| [0110](adr/0110-autosave.md) | Autosave writes modified files by itself, never over a change on disk | Proposed |
| [0111](adr/0111-hand-files-to-the-running-ide.md) | A second start hands its folders and files to the running IDE | Proposed |
| [0112](adr/0112-macro-recording-and-playback.md) | Macros record dispatched editor commands and play them back as one undo step | Proposed |
| [0113](adr/0113-spelling-inspection-and-bundled-dictionary.md) | The spelling inspection and its bundled English dictionary | Proposed |
| [0114](adr/0114-fold-by-default-options-tagged-by-languages.md) | "Fold by default" options: General ones in the platform, a language's from its module, and folds tagged with the option that folds them | Proposed |
| [0115](adr/0115-file-templates.md) | File templates with a Velocity subset | Proposed |
| [0117](adr/0117-create-and-apply-patch.md) | Create Patch and Apply Patch | Proposed |
| [0118](adr/0118-find-usages-tab.md) | Find Usages lists its usages on the Find tool window's tabs of occurrences | Proposed |
| [0119](adr/0119-surround-with-as-live-templates.md) | Surround With puts code into live templates; language modules declare surrounders as data | Proposed |
| [0120](adr/0120-ide-zoom.md) | Zoom IDE is a percentage on top of the UI font size | Proposed |
| [0121](adr/0121-file-and-folder-bookmarks.md) | Bookmarks of files and folders share the line bookmarks' lists; the workspace file keeps them apart from the lines | Proposed |
| [0122](adr/0122-scratch-files-and-buffers.md) | Scratch files and buffers live in the IDE's data folder and are ordinary files of their language | Proposed |
| [0123](adr/0123-mark-directory-as-excluded.md) | Mark Directory as Excluded writes the module file, and folders are excluded by the kind of project, not by name | Proposed |
| [0124](adr/0124-associate-and-override-file-types.md) | Associate with File Type and Override File Type | Proposed |
| [0125](adr/0125-kozo-the-family-mascot.md) | Kozō, the family mascot | Proposed |
| [0126](adr/0126-vue-language-server-and-npm-paths-in-server-options.md) | The Vue plug-in runs the Vue language server in full mode; server options can name paths in npm packages | Proposed |
| [0127](adr/0127-npm-language-servers-in-the-ides-own-folder.md) | Language servers installed with npm go into the IDE's own folder, never among the global packages | Proposed |
| [0128](adr/0128-a-plugin-installs-its-language-servers.md) | Installing a plug-in installs its language servers, and every server says where it runs from | Proposed |
| [0129](adr/0129-project-plugins-npm-run-and-nuxt.md) | Plug-ins recognise projects, require other plug-ins and add run configurations; npm runs in every IDE; Nuxt is the first such plug-in | Proposed |
| [0130](adr/0130-typescript-and-javascript-through-typescript-language-server.md) | TypeScript and JavaScript through typescript-language-server, offered by the banner | Proposed |
| [0131](adr/0131-react-and-next-plugins.md) | The React and Next.js plug-ins; run configurations for some projects only; JSX end tags as you type | Proposed |
| [0132](adr/0132-svelte-plugin-and-language-server.md) | The Svelte plug-in reads markup with its expressions hidden, and runs the Svelte language server | Proposed |
| [0133](adr/0133-sveltekit-plugin.md) | The SvelteKit plug-in | Proposed |
| [0134](adr/0134-plugins-contribute-typescript-server-plugins.md) | A plug-in contributes TypeScript server plug-ins to another plug-in's server | Proposed |
| [0135](adr/0135-file-types-by-content-and-xml-in-every-ide.md) | File types by content, and XML in every IDE | Proposed |
| [0136](adr/0136-linters-and-formatters.md) | Linters beside a file's language server, the project's formatters, and the ESLint, Prettier and Biome plug-ins | Proposed |
| [0137](adr/0137-text-with-preview.md) | Text with a preview; an SVG opens as text and as a picture | Proposed |
| [0138](adr/0138-markdown-preview.md) | A full Markdown preview, drawn natively | Proposed |
| [0139](adr/0139-lean-mcp-answers-secrets-and-costs.md) | Compact lists, problem filters, usages by name, edit reports, hidden files and call costs over MCP | Proposed |
| [0140](adr/0140-ci-on-a-team-mac-runner.md) | CI on merge requests, on the team's own Mac runner | Proposed; decision 7 (no secrets on the runner) superseded by [ADR-0171](adr/0171-releases-from-a-protected-tag-through-gitlab.md) |
| [0141](adr/0141-the-gate-remembers-a-pass-by-its-tree.md) | The quality gate remembers a pass by its tree | Proposed |
| [0142](adr/0142-release-builds-on-the-runner-published-on-the-release-machine.md) | Release builds on the runner, signed and published on the release machine | Proposed; superseded by [ADR-0171](adr/0171-releases-from-a-protected-tag-through-gitlab.md) (releases from a protected tag), and kept as its fallback |
| [0143](adr/0143-hidden-files-out-of-agents-searches-and-short-tool-schemas.md) | Hidden files out of agents' searches; short tool schemas | Proposed |
| [0144](adr/0144-code-is-parsed-in-pure-rust.md) | Code is parsed by our own parsers in pure Rust, not tree-sitter | Proposed |
| [0145](adr/0145-jvmjizo.md) | JvmJizo, the Java-first IDE for the JVM | Proposed (the name and the Java-first scope: the owner, 2026-10-25) |
| [0146](adr/0146-one-calendar-version-for-every-ide-and-release-channels.md) | One calendar version for every IDE (release channels deferred) | Proposed (the scheme: the owner, 2026-10-25; channels deferred by the owner the same day) |
| [0147](adr/0147-jvmjizo-app-and-jdks.md) | The JvmJizo app, and JDKs in the platform | Proposed |
| [0148](adr/0148-java-plugin-and-eclipse-jdt-ls.md) | The Java plug-in, and Eclipse JDT LS installed from its archive | Proposed |
| [0149](adr/0149-maven-and-gradle.md) | Maven and Gradle: plug-ins for their projects, run kinds and tool windows in the platform | Proposed |
| [0150](adr/0150-linux-and-windows-builds-on-the-mac-runner.md) | Linux and Windows builds on the Mac runner, without Docker | Proposed (the owner, 2026-10-25) |
| [0151](adr/0151-kotlin-plugin.md) | The Kotlin plug-in: our own lexer, two Kotlin servers to choose from | Proposed |
| [0152](adr/0152-java-run-configurations-and-the-classpath.md) | Java run configurations and the classpath from the build tool | Proposed |
| [0153](adr/0153-java-debugging-and-tests.md) | Java debugging with java-debug, and Java tests run by Maven and Gradle | Proposed |
| [0154](adr/0154-quick-switch-scheme.md) | Quick Switch Scheme switches through the commands and settings of Settings | Proposed |
| [0155](adr/0155-new-menu-and-file-templates-page.md) | The New list by file type and the File and Code Templates page | Proposed |
| [0156](adr/0156-tool-window-view-modes-and-saved-layouts.md) | Tool window view modes and layouts saved per IDE | Proposed |
| [0157](adr/0157-export-import-and-restore-ide-settings.md) | Export, import and restore the IDE's settings | Proposed |
| [0158](adr/0158-power-save-mode-and-activity-monitor.md) | Power Save Mode, status bar widgets and the Activity Monitor | Proposed |
| [0159](adr/0159-project-structure-and-resource-roots.md) | Settings → Project Structure, resource roots and Exclude files write the module file | Proposed |
| [0160](adr/0160-external-tools.md) | External tools | Proposed |
| [0161](adr/0161-spelling-quick-fixes-and-dictionaries.md) | Spelling quick-fixes and the user's dictionaries | Proposed |
| [0162](adr/0162-compare-directories.md) | Compare Directories compares in the background and synchronises only inside the two folders | Proposed |
| [0163](adr/0163-workspaces-full-tools-in-the-editor-area.md) | Workspaces: full tools that expand over the editor area and come back as they were | Proposed |
| [0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md) | The database engine: `zen-db` in L2, one runtime of its own, drivers by data model, compiled in | Proposed |
| [0165](adr/0165-database-connections-types-and-secrets.md) | Database connections: global and project scopes, connection types, settings per driver, secrets in the Keychain | Proposed |
| [0167](adr/0167-java-hot-code-replace.md) | Java hot code replace through java-debug | Proposed |
| [0168](adr/0168-result-paging-without-held-locks.md) | Result paging that never holds a lock outside the person's own transaction | Proposed |
| [0169](adr/0169-missing-servers-symbols-and-run-options.md) | A missing server is named only where it could have answered, rust-analyzer returns enough symbols, run options are checked, and commits can be signed off | Proposed |
| [0170](adr/0170-generated-sql-in-each-drivers-dialect.md) | The SQL the IDE generates is written in each driver's dialect | Proposed |
| [0171](adr/0171-releases-from-a-protected-tag-through-gitlab.md) | Releases from a protected tag through GitLab, staged and made live by one button | Proposed (the owner, 2026-10-27) |
| [0172](adr/0172-production-safety-and-agents-on-databases.md) | Production safety: read-only connections, confirmations before statements run, and what agents may run on each connection type | Proposed |
| [0173](adr/0173-database-profiles-for-wire-compatible-servers.md) | Database profiles: wire-compatible servers as data over the compiled-in drivers | Proposed |
| [0174](adr/0174-external-database-driver-protocol.md) | External database drivers: a JSON-RPC protocol over a driver program's standard streams, supervised by the IDE, shipped in plug-ins | Proposed |
| [0175](adr/0175-first-party-database-driver-programs.md) | First-party database driver programs live in `drivers/`, speak TLS through rustls with RustCrypto themselves, and never bring C | Proposed |
| [0176](adr/0176-structure-scripts-and-migration-files.md) | Structure changes run as one previewed script; migration files are up-only SQL; SQLite rebuilds keep foreign keys | Proposed |
| [0177](adr/0177-crash-safe-database-journals.md) | Crash-safe journals for the database workspace: checked records, one recovery journal per process | Proposed |
| [0178](adr/0178-bind-parameters-variables-and-literals.md) | Bind parameters, script variables, and values written as literals where a driver cannot bind | Proposed |
| [0179](adr/0179-connections-closed-to-agents-hide-their-metadata.md) | A connection closed to agents hides its structure and statements too, not only its data | Accepted |
| [0181](adr/0181-database-transfer-and-parquet-export.md) | Database-to-database transfer and Parquet export through arrow-rs | Proposed |
| [0182](adr/0182-sql-plugin-abi-v1-and-dialects.md) | The SQL plug-in on ABI v1, the dialect a plug-in reads in, and where it is bundled | Proposed |
| [0183](adr/0183-language-injection.md) | Language injection: another language inside string literals, starting with SQL in code | Proposed |
| [0184](adr/0184-highlight-kinds-property-global.md) | Highlight kinds for declared properties and global variables | Proposed |
| [0185](adr/0185-english-by-default.md) | Every application speaks English until the person chooses another language | Proposed |
| [0186](adr/0186-database-keys-and-toolbar-in-the-database-workspace.md) | Database keys of its own and an always-present toolbar in the Database workspace | Accepted |
| [0187](adr/0187-git-workspace-on-the-workspace-host.md) | The Git workspace on the workspace host, and a tab in the editor strip for every open workspace | Accepted |
| [0188](adr/0188-release-notes-and-decision-records-in-files-of-their-own.md) | Release notes and decision records in files of their own, so merge requests stop conflicting | Proposed |
| [0189](adr/0189-splits-and-scripts-in-a-workspace.md) | Splits inside a workspace, scripts moving between a workspace and the editor, and Local History of consoles | Proposed |
| [0190](adr/0190-oracle-through-sqlcl-the-user-installs.md) | Oracle Database through SQLcl, the command-line program the user installs | Proposed |
| [0191](adr/0191-driver-programs-published-with-their-plugins.md) | Database driver programs are built, signed and published with their plug-in | Proposed |
| [0192](adr/0192-navigator-opens-properties-and-changes-objects-at-once.md) | The Database Navigator opens Properties and changes objects at once | Accepted |
| [0193](adr/0193-external-database-tools.md) | External database tools: found outside the project, passwords in private files, agents kept to the IDE's own choices | Proposed |
| [0194](adr/0194-comments.md) | No comments in code | Proposed |
| [0195](adr/0195-dependency-manager-and-the-build-model.md) | A dependency manager for every build system, and the build model from Maven and Gradle themselves | Proposed |
| [0196](adr/0196-load-changes-and-the-sync-view.md) | Load Changes and the Sync view, fed by the build plug-ins | Proposed |
| [0197](adr/0197-ant-plugin-and-its-tool-window.md) | Ant: a plug-in for its build files, the run kind and the tool window in the platform | Proposed |
| [0198](adr/0198-library-classes-and-documents-of-language-servers.md) | Library classes, and documents a language server shows under its own URIs | Proposed |
| [0199](adr/0199-project-structure-shows-the-model-of-every-language.md) | Project Structure shows the project model of every language | Proposed |
| [0200](adr/0200-kotlin-runs-tests-and-debugs-through-the-java-kinds.md) | Kotlin runs, tests and debugs through the Java kinds | Proposed |
| [0201](adr/0201-jar-artifacts-the-ide-writes.md) | Jar artifacts the IDE writes | Proposed |
| [0202](adr/0202-before-launch-tasks-are-an-extension-point.md) | Before-launch tasks are an extension point of the run service | Proposed |
| [0203](adr/0203-breakpoint-properties-the-ide-does-itself.md) | Breakpoint properties the IDE does itself | Proposed |
| [0204](adr/0204-run-anything.md) | Run Anything and double modifier shortcuts | Proposed |
| [0205](adr/0205-trusted-projects.md) | Trusted projects: a build runs by itself only in a project the person trusts | Proposed (the owner, 2026-11-04) |
| [0206](adr/0206-run-configuration-environment-macros-and-instances.md) | Environment files, macros and instances of run configurations | Proposed |
| [0207](adr/0207-file-watchers.md) | File watchers | Proposed |
| [0208](adr/0208-code-coverage-model-and-providers.md) | Code coverage: one model, read by the run providers | Proposed |
| [0209](adr/0209-test-naming-providers-and-test-history.md) | Test naming providers and the test history | Proposed |
| [0212](adr/0212-scratch-files-run-through-language-providers.md) | Scratch files run through language providers | Proposed |
| [0213](adr/0213-compound-configurations-templates-and-services.md) | Compound run configurations, templates and run configurations in Services | Proposed |
| [0214](adr/0214-properties-plugin-and-resource-bundles.md) | The Properties plug-in, keys from string literals and the Resource Bundle editor | Proposed |
| [0215](adr/0215-file-operations-in-language-server-edits.md) | File operations in language server edits | Proposed |
| [0216](adr/0216-generate-and-the-member-chooser.md) | Generate and the member chooser | Proposed |
| [0217](adr/0217-spring-boot-plugin-metadata-from-the-projects-jars.md) | Spring Boot: a plug-in for its files, the properties from the project's jars, a run kind | Proposed |
| [0218](adr/0218-editable-live-templates-postfix-and-statement-completion.md) | Editable live templates, postfix completion and statement completion as language data | Proposed |
| [0219](adr/0219-extract-and-inline-through-code-actions.md) | Extract and Inline through code actions, named in place | Proposed |
| [0220](adr/0220-code-style-options-declared-by-languages.md) | Code style options declared by languages | Proposed |
| [0222](adr/0222-safe-delete-and-the-refactoring-dialog.md) | Safe Delete and a refactoring dialog that providers fill | Proposed |
| [0223](adr/0223-plug-in-surrounders-thrown-types-and-method-statements.md) | Plug-in surrounders, translated template texts, thrown types and method statements | Proposed |
| [0224](adr/0224-jvm-test-rules-as-plugin-declarations.md) | Java and Kotlin test rules as plug-in declarations | Proposed |
| [0225](adr/0225-math-in-the-markdown-preview.md) | Math in the Markdown preview, typeset in pure Rust | Proposed |
| [0226](adr/0226-several-generators-per-server-action.md) | Several generators per server action, and Override/Implement Methods | Proposed |
| [0227](adr/0227-jacoco-coverage-for-maven-and-gradle.md) | JaCoCo coverage for Maven and Gradle runs | Proposed |
| [0228](adr/0228-formatter-profiles-and-editorconfig-sections-for-code-style.md) | Formatter profiles and EditorConfig sections for code style | Proposed |
| [0229](adr/0229-java-move-and-change-signature.md) | Java Move and Change Signature through JDT LS in the refactoring dialog | Proposed |
| [0230](adr/0230-show-bytecode-and-jvm-consoles.md) | Show Bytecode, the JShell console and Kotlin with the installed compiler | Proposed |
| [0231](adr/0231-collection-views-and-stream-trace-in-the-debugger.md) | Collection views and the stream trace in the debugger | Proposed |
| [0232](adr/0232-java-class-hierarchy-refactorings.md) | Java class hierarchy refactorings, from JDT LS where it has them | Proposed |
| [0233](adr/0233-build-triggers-and-keymap-actions-with-parameters.md) | Build triggers around the IDE's builds, and keymap actions that carry parameters | Proposed |
| [0234](adr/0234-dependency-manager-network-repositories-and-providers.md) | What the dependency manager asks, from whom, and how providers plug in | Proposed |
| [0235](adr/0235-dependency-analyzer.md) | The dependency analyzer: one model from Maven and Gradle, Exclude in the build file | Proposed |
| [0236](adr/0236-gradle-tool-window-model-run-and-sync-tasks.md) | The Gradle model loads after import, and tasks join the sync's Gradle run | Proposed |
| [0237](adr/0237-new-project-wizard-and-project-generators.md) | The New Project wizard and project generators | Proposed |
| [0238](adr/0238-saving-never-overwrites-outside-changes.md) | Saving never overwrites a change made outside the IDE | Proposed |
| [0239](adr/0239-cancelling-and-pausing-background-tasks.md) | Cancelling and pausing background tasks | Proposed |
| [0240](adr/0240-generators-with-a-service.md) | Project generators backed by a web service | Proposed |
| [0241](adr/0241-the-jdk-the-build-asks-for.md) | A project without a chosen JDK uses the JDK its build asks for | Proposed |
| [0242](adr/0242-java-and-kotlin-project-types.md) | Java and Kotlin project types come from their plug-ins | Proposed |
| [0243](adr/0243-new-project-wizard-pages-and-file-new-without-classes.md) | Wizard pages instead of a file preview, and File → New without classes | Proposed |
| [0244](adr/0244-projects-from-sources-and-modules.md) | Clone, existing sources, modules and generators that run a build tool | Proposed |
| [0245](adr/0245-the-ide-log-file.md) | The IDE log file | Proposed |
| [0246](adr/0246-file-encodings-and-writing-encodings-xml.md) | File encodings, and writing `.idea/encodings.xml` | Proposed |
| [0247](adr/0247-detached-editor-windows.md) | Detached editor windows | Proposed |
| [0248](adr/0248-safe-mode.md) | Safe mode: a project not trusted yet runs nothing of its own | Accepted (the owner, 2026-11-06) |
| [0249](adr/0249-local-git-operations-off-the-frame.md) | Local Git operations off the frame | Proposed |
| [0252](adr/0252-edit-starters-of-an-existing-project.md) | Editing the starters of an existing project | Proposed |
| [0253](adr/0253-welcome-sections.md) | The sections of the welcome window | Proposed |
| [0254](adr/0254-help-menu-reference-tips-and-local-usage-counts.md) | What's New, the shortcuts reference, tips and local usage counts | Proposed |
| [0255](adr/0255-code-style-schemes-in-files.md) | Code style schemes, the Project scheme in `.idea` and the formatter's exclusions | Proposed |
| [0256](adr/0256-inspect-code-over-a-scope.md) | Inspect Code over a scope | Accepted |
| [0257](adr/0257-one-proxy-setting-in-zen-net.md) | One proxy setting and one trust store in zen-net | Proposed |
| [0258](adr/0258-inspection-profiles-and-code-cleanup.md) | Inspection profiles and Code Cleanup | Accepted |
| [0259](adr/0259-actions-on-save-per-project.md) | Actions on Save per project, and fix-only tools | Proposed |
| [0260](adr/0260-menus-and-toolbars-customisation.md) | Menus and Toolbars customisation | Proposed |
| [0261](adr/0261-mermaid-diagrams-in-the-markdown-preview.md) | Mermaid diagrams in the Markdown preview, laid out in pure Rust | Proposed |
| [0262](adr/0262-safe-mode-refuses-a-persons-build.md) | Safe mode refuses a person's build too | Accepted (the owner, 2026-11-06) |
| [0263](adr/0263-keys-from-press-keys-count-as-the-agent.md) | Keys from press_keys count as the agent | Proposed |
| [0264](adr/0264-go-templates-and-embedded-files.md) | Go templates and embedded files are in the Go module | Proposed |
| [0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md) | Chosen versions, automatic updates, new-version notifications and waiting launchers | Proposed |
| [0266](adr/0266-go-scratch-files-in-the-projects-module.md) | Go scratch files run in the project's module | Proposed |
| [0267](adr/0267-kotlin-bundled-with-jvmjizo.md) | JvmJizo bundles the Kotlin plug-in | Accepted (the owner, 2026-11-07) |
| [0268](adr/0268-npm-and-nodejs.md) | npm and Node.js: the package.json model, the npm tool window, Node.js runs, a debugger over the DevTools protocol and test reporters | Proposed |
| [0269](adr/0269-suppressing-inspections-in-the-source.md) | Suppressing inspections in the source | Accepted |
| [0271](adr/0271-writing-idea-code-styles-with-compatibility.md) | The Project code style is written to `.idea`, and stays compatible | Accepted (the owner, 2026-11-07) |
| [0272](adr/0272-language-assists.md) | Language assists: completion and declarations from a language module | Proposed |
| [0273](adr/0273-profiles-one-model-read-by-the-run-providers.md) | Profiles: one model, read by the run providers | Proposed |
| [0274](adr/0274-what-a-modules-generator-can-add.md) | What a module's generator can add | Proposed (the decision of an earlier change, moved out of accepted ADR-0216, which it edited) |
| [0275](adr/0275-terminal-sessions-that-outlive-the-ide.md) | Terminal sessions that outlive the IDE, and agent sessions that come back after a reboot | Accepted (the owner, 2026-11-11) |
| [0276](adr/0276-sqlscroll.md) | SqlScroll, the IDE for SQL and databases | Proposed |
| [0280](adr/0280-clean-room-process.md) | The clean-room process: observers write a specification, implementers build from it | Proposed |
| [0282](adr/0282-third-party-notices-in-every-release-artefact.md) | Third-party notices in every release artefact | Proposed |
| [0283](adr/0283-a-callers-symbol-wait-is-honoured.md) | A caller's symbol wait is honoured after a server has loaded | Proposed |
| [0284](adr/0284-java-to-kotlin-conversion.md) | Java to Kotlin conversion is our own, in the Kotlin plug-in | Accepted (the owner, 2026-11-13) |
| [0285](adr/0285-devopsdaruma-app.md) | The DevopsDaruma app, a bundle of plug-ins for DevOps | Accepted (the owner, 2026-11-13) |
| [0286](adr/0286-plugin-run-targets.md) | Plug-ins offer run targets; the platform runs them | Accepted (the owner, 2026-11-13) |
| [0288](adr/0288-bundled-schemas-for-language-servers.md) | Schemas ship inside plug-ins; servers never download them by themselves | Accepted (the owner, 2026-11-13) |
| [0289](adr/0289-plugin-tool-windows.md) | Plug-ins declare tool windows over their declared programs | Accepted (the owner, 2026-11-13) |
| [0290](adr/0290-tool-window-choices-and-confirmations.md) | A plug-in tool window's header choices, and confirmations that name them | Accepted (the owner, 2026-11-13) |
| [0291](adr/0291-commands-only-a-person-starts.md) | Plug-in commands only a person starts, asked every time | Accepted (the owner, 2026-11-13) |
| [0292](adr/0292-plugin-tool-window-icons-and-sides.md) | Plug-in tool windows declare their icon and their side | Accepted (the owner, 2026-11-15) |
| [0293](adr/0293-gitops-project-index.md) | GitOps repositories: a plug-in's project index, its problems, navigation and graph | Accepted (the owner, 2026-11-15) |
| [0294](adr/0294-the-login-shells-environment.md) | The programs the IDE starts get the person's login-shell environment | Accepted (the owner, 2026-11-16) |
| [0295](adr/0295-values-a-person-types-into-tool-window-actions.md) | Values a person types into plug-in tool window actions | Accepted (the owner, 2026-11-16) |
| [0296](adr/0296-a-manifest-against-the-cluster.md) | Run targets that compare with a cluster and that only a person applies | Accepted (the owner, 2026-11-16) |
| [0297](adr/0297-index-completions-hidden-nodes-and-output-documents.md) | Index completions, nodes outside the graph and run output as a document | Accepted (the owner, 2026-11-16) |
| [0298](adr/0298-a-go-projects-environment.md) | A Go project's environment: a shared file, a personal layer, and where they reach | Accepted (the owner, 2026-11-17) |
| [0299](adr/0299-cluster-state-beside-a-project-index.md) | Cluster state beside a project index, with another plug-in's window values | Accepted (the owner, 2026-11-16) |
| [0300](adr/0300-releases-from-github.md) | Releases from GitHub | Accepted |

## Open questions

Decisions that are known to be needed but are not made yet.

| Question | Needed by |
|---|---|
| Trademark policy review for each language name used in a product name | Before each product's first public release |
| Logo and visual identity for the family and for each product | First release (Phase 6) |
| Legal review of trade-dress risk in the interface's look | First release (Phase 6) |
| Custody of the production signing key | First release (Phase 6) |
