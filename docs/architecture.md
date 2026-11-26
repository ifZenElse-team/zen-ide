# Architecture

> **Status:** design target, partly implemented · **Last updated:** 2026-10-26

This document describes the architecture the Zen Platform is built towards. Where the implementation
differs, the "Current state" section below takes precedence. The reasons behind each choice are in
the [decision records](adr/).

## Current state (2026-10-17)

- **Crates that exist:** `zen-text`, `zen-vfs`, `zen-i18n`, `zen-image`, `zen-macos`, `zen-mascot`, `zen-spell`, `zen-secrets`, `zen-env` (L0); `zen-syntax`,
  `zen-project`, `zen-theme`, `zen-net` (L1); `zen-index`, `zen-lsp`, `zen-dap`, `zen-terminal`, `zen-run`, `zen-update`, `zen-http`, `zen-db`, `zen-automation` (L2);
  `zen-editor`, `zen-core` (L3); `zen-modules` (L4); `zen-plugins`, `zen-jb-import`, `zen-toolbox`,
  `zen-workbench`, `zen-lang-go`, `zen-lang-rust` (L5); `zen-ui` (L6). The others in section 3 are planned.
  `zen-project` holds the recent projects list ([ADR-0041](adr/0041-recent-projects-file.md)) and
  the project model that language modules build ([ADR-0046](adr/0046-project-models-and-outlines.md)),
  and the run configurations file ([ADR-0048](adr/0048-run-configurations-and-the-run-service.md)).
  `zen-theme` holds the theme model, the built-in Zen Dark, the theme file format and the store of
  `<data>/themes`; every colour `zen-ui` draws comes from the theme in use ([ADR-0045](adr/0045-themes.md)).
- **Run:** `zen-run` is the run service: the `RunProvider` extension point, output parsers, the
  console with file links and the test tree; the `run.*` commands of `zen-workbench` drive it.
- **Debug:** `zen-dap` is the Debug Adapter Protocol client; a `RunProvider` describes how to debug a
  configuration (`DebugPlan`), and the adapter (Delve) connects back to a loopback listener of the IDE;
  the `debug.*` commands drive it ([ADR-0052](adr/0052-debugger-zen-dap-and-delve-over-reverse-dap.md)).
- **Editor:** `zen-editor` holds carets and selections, caret movement and edits at every caret, and
  the `editor.*` commands drive it. The view is ours (`zen-ui`, `ide/editor_view.rs`): it lays out
  only the visible lines and turns keys, mouse, clipboard and input methods into those commands.
- **Syntax:** highlighting is hand-written in Rust per language (`zen_syntax::Highlighter`); there
  is no tree-sitter. A language can lex line by line (`zen_syntax::LineLexer`, `Lines`,
  `Incremental`), so that an edit lexes only the lines it changed and those whose state at the start
  changed with it; Go does, and its folds come from the same tokens
  ([ADR-0077](adr/0077-incremental-highlighting-with-line-lexers.md)). Other highlighters highlight
  the whole text after an edit. Tree-sitter is written in C, which ADR-0002 keeps out of the default
  build, so how code is parsed into a syntax tree is still an open decision.
- **Automation:** the MCP server runs inside the IDE (`zen-automation`, ADR-0033); `gogarden mcp`
  bridges an agent's standard input and output to it. `zen-cli` (`apps/zen-cli`, headless) lists
  and runs the commands of `zen-workbench` and `zen-toolbox`, each run in a fresh session, with JSON
  output and fixed exit codes ([ADR-0042](adr/0042-zen-cli-output-and-exit-codes.md)).
- **Updates:** installation is atomic; rolling back installs an older release from the signed
  manifest ([ADR-0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md)).

## 1. Goals

1. **One platform, many IDEs.** Every Zen IDE is the same platform with a different set of language
   modules and its own branding. A new IDE costs a language module, not a new application.
2. **Native performance.** Typing latency, startup time and memory use are first-class requirements,
   measured and tracked from the first release.
3. **Headless first.** Every feature works without a window: from tests, the CLI and the MCP server.
   The desktop UI is one client of the platform.
4. **Portable core.** Everything below the client layer (L6) compiles to `wasm32`, which keeps a browser
   client possible and keeps the core free of operating-system assumptions.
5. **A precise UI.** The interface is built clean-room to an exact specification of layout,
   metrics and behaviour ([behaviour.md](behaviour.md)). The UI layer must
   therefore be able to reproduce precise metrics, text rendering and behaviour.

## 2. Shape of the system

```text
┌────────────────────────────────────────────────────────────────────────────┐
│ apps   rust-ronin · go-garden · web-wave · jvm-jizo · …  zen-toolbox zen-cli│  branding, bundling
├────────────────────────────────────────────────────────────────────────────┤
│ L6  ui (egui on wgpu): windows, tool windows; answers agents' tool calls    │  clients
├────────────────────────────────────────────────────────────────────────────┤
│ L5  plugins (WASM host) · jb-import · toolbox · workbench · lang/*         │  features
├────────────────────────────────────────────────────────────────────────────┤
│ L4  modules: the host built-in modules (Go, Rust…) register into           │  module host
├────────────────────────────────────────────────────────────────────────────┤
│ L3  core: commands · session · settings · extension points                 │  the engine
│     editor model: carets, selections, folding, soft wrap (no UI)           │
├────────────────────────────────────────────────────────────────────────────┤
│ L2  index · lsp · dap · vcs · terminal · run · update · http · db · MCP    │  services
├────────────────────────────────────────────────────────────────────────────┤
│ L1  syntax (tree-sitter) · project model · themes                          │
├────────────────────────────────────────────────────────────────────────────┤
│ L0  text (rope, edits, undo) · vfs (files, watching, overlays)             │  foundation
└────────────────────────────────────────────────────────────────────────────┘
```

A crate may depend only on crates in strictly lower layers. Language modules (`crates/lang/*`) sit at
L5 and depend on the platform through the extension points that `zen-core` declares; no platform
crate depends on a language module. The full rules are in [ADR-0014](adr/0014-refined-layer-model.md);
they are enforced by `cargo xtask layers`, and the registry in `xtask/src/layers.rs` is the source of
truth for layer membership.

## 3. Crates

| Crate | Layer | Responsibility |
|---|---|---|
| `zen-i18n` | L0 | Localisation with Project Fluent: languages, lookup, plural forms ([ADR-0024](adr/0024-localisation.md)) |
| `zen-text` | L0 | Rope-based text buffers, edits, line index, undo history |
| `zen-image` | L0 | Decoding images (PNG, JPEG, GIF, WebP, BMP, ICO, SVG) for the image viewer ([ADR-0037](adr/0037-image-viewer.md)) |
| `zen-macos` | L0 | The macOS application hooks without a safe binding: a system quit (the Dock, logging out) asks as ⌘Q does; the only crate allowed `unsafe` ([ADR-0079](adr/0079-zen-macos-unsafe-exception.md)) |
| `zen-mascot` | L0 | Kozō, the family's pixel-art mascot: the frames of every product's variant and state as grids of inks, for the UI and the website ([ADR-0125](adr/0125-kozo-the-family-mascot.md)) |
| `zen-secrets` | L0 | Secrets in the system's credential store ([ADR-0165](adr/0165-database-connections-types-and-secrets.md), decision 4): the `SecretStore` interface, a `Secret` that is never printed and is cleared when dropped, the macOS Keychain through `keyring-core` and `apple-native-keyring-store` behind a per-process cache (one Keychain read per secret, a denial not asked again until retried), an unsupported store elsewhere and a memory store for tests; the Database workspace's items. At L0 so that `zen-db` (L2) and `zen-workbench` (L5) both use it; it depends on no workspace crate |
| `zen-spell` | L0 | Spelling: the bundled English dictionary (public domain, ADR-0113), the user's dictionaries (word lists, a Hunspell subset, JetBrains `.idea/dictionaries` files; ADR-0161), suggestions by edit distance and the words of names, literals and comments to check |
| `zen-vfs` | L0 | Virtual file system: disk access, file watching, unsaved overlays, encodings |
| `zen-env` | L0 | The person's login-shell environment ([ADR-0294](adr/0294-the-login-shells-environment.md)): read once per start in the background (`$SHELL -l -i -c` printing `env -0` between markers, 10 s), what is taken and never taken, the on/off switch, and `apply` for every program the IDE starts (runs, servers, tool windows, the terminal); it depends on no workspace crate |
| `zen-syntax` | L1 | Incremental parsing with tree-sitter: highlighting, folding, outline, structural selection |
| `zen-project` | L1 | Workspace model: the recent projects list and its shared file ([recent-projects.md](recent-projects.md)); the project model (modules, workspace file, libraries) and its `ModelProvider` extension point ([ADR-0046](adr/0046-project-models-and-outlines.md)); the `Inspector` extension point for problems with quick-fixes ([ADR-0059](adr/0059-go-mod-dependency-checks.md)); run configurations and their file ([run-configurations.md](run-configurations.md)); the code style of a language ([ADR-0053](adr/0053-fonts-ui-scale-and-code-style.md)); later project settings |
| `zen-theme` | L1 | Themes: colour keys, the built-in Zen Dark, the theme file format and inheritance, the theme store, the importer interface and the VS Code importer ([themes.md](themes.md), [ADR-0045](adr/0045-themes.md)) |
| `zen-index` | L2 | The project's file names and the trigrams of their text, held in memory: built on a worker thread respecting `.gitignore` and the excluded folders, kept current from file-change events; the names matched by the shared name matcher (`zen_index::Pattern`) for Go to File and Search Everywhere, the trigrams naming the candidate files Find in Files reads ([ADR-0082](adr/0082-trigram-index-for-find-in-files.md), section 4.8). There is no symbol index: Go to Class and Go to Symbol ask the language servers (`workspace/symbol`) |
| `zen-lsp` | L2 | Language Server Protocol client: process management, capabilities, request routing, a writer thread per server (ADR-0078) |
| `zen-dap` | L2 | Debug Adapter Protocol client: the protocol's messages, framing, typed requests, the session handshake, the loopback listener the adapter connects back to, and a fake adapter for tests ([ADR-0052](adr/0052-debugger-zen-dap-and-delve-over-reverse-dap.md)) |
| `zen-run` | L2 | Run and test configurations: the `RunProvider` extension point, command lines, output parsers, the console with file links, the test tree; processes on desktop only ([ADR-0048](adr/0048-run-configurations-and-the-run-service.md)) |
| `zen-vcs` | L2 | Version control: git status, branches and contents at `HEAD`, read with gitoxide; commits, checkouts, fetch, pull and push through the `git` program; desktop only ([ADR-0047](adr/0047-version-control-gitoxide-and-git.md)). Its `vcs.*` commands are `zen-workbench`'s |
| `zen-terminal` | L2 | The built-in terminal: shell sessions in a PTY, VT/xterm emulation (`alacritty_terminal`), screen snapshots and key encoding. Desktop only; the view is `zen_ui::terminal` |
| `zen-editor` | L3 | Editor model: carets, selections, folding, soft wrap, inlays. No drawing |
| `zen-core` | L3 | Command registry, session, settings, extension points |
| `zen-modules` | L4 | Built-in modules ([ADR-0071](adr/0071-built-in-modules.md)): the one entry point a module registers through (`ModuleDescriptor`, `ModuleHost`), what it gives the IDE, the settings pages as forms, which modules are on (`modules.json`) and the `plugins.module*` commands; a product's optional plug-ins, its areas, and their choice (`Areas`, `areas.json`, the `plugins.areas.*` commands, [ADR-0073](adr/0073-optional-bundle-plugins-areas.md)) |
| `zen-plugins` | L5 | Sandboxed WebAssembly plug-in host |
| `zen-jb-import` | L5 | Import of JetBrains formats: colour schemes (`.icls`) and UI themes (`.theme.json`) ([themes/import-jetbrains.md](themes/import-jetbrains.md)), and the reader of a project's `.idea/` settings, which never writes them ([jetbrains-compatibility.md](jetbrains-compatibility.md)); keymaps and live templates planned |
| `zen-http` | L2 | The HTTP Client: `.http` and `.rest` files, their environments and variables, sending one request (desktop only, the TLS stack of ADR-0040) and the per-project history in the data folder ([ADR-0061](adr/0061-http-client.md)) |
| `zen-db` | L2 | The database engine of the Database workspace ([ADR-0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md)): connections, folders, personal overrides and connection types in their files ([ADR-0165](adr/0165-database-connections-types-and-secrets.md)), secrets through `zen-secrets` (a saved password read on its runtime), the driver interface with capabilities by data model (relational, key-value, document, search, wide-column) and one value type, the PostgreSQL driver, and the lifecycle (Main, Metadata and console contexts, guards, keepalive and ping, reconnects with backoff, cancel on the server, idle close) with operations, cancellation and deadlines on one tokio runtime of its own; drivers and runtime on desktop only. Statement splitting, the result store and the other drivers are planned. Its `db.*` commands are `zen-workbench`'s, its views `zen-ui`'s |
| `zen-net` | L1 | The proxy setting of the process and the HTTP agent every request of the IDE is built from ([ADR-0257](adr/0257-one-proxy-setting-in-zen-net.md)) |
| `zen-update` | L2 | The signed release manifest, version selection, artifact checks, downloads and atomic installation ([ADR-0021](adr/0021-zen-update-is-a-service.md); rollback from the manifest, [ADR-0265](adr/0265-versions-updates-notifications-and-waiting-launchers.md)) |
| `zen-workbench` | L5 | The open project, open documents, its project model, runs and terminal sessions, and the `file.*`, `editor.*`, `project.*`, `run.*` and `terminal.*` commands |
| `zen-toolbox` | L5 | The tools Zen Toolbox manages, their states, and the `toolbox.*` commands |
| `zen-lang-go` | L5 | The Go plug-in, a built-in module of every IDE ([ADR-0071](adr/0071-built-in-modules.md); `MODULE` and its `register`): highlighting of Go, `go.mod` and `go.sum`, the Go modules model of `go.mod` and `go.work` (`GoModules`), the outline of Go files (`GoOutliner`), run and test configurations (`GoRun`: `go build`, `go test -json`, gutter targets), the `go.mod` dependency checks (`GoModInspector`: newer versions and known vulnerabilities, [ADR-0059](adr/0059-go-mod-dependency-checks.md)), the Go toolchain (`toolchain`: the SDKs, gopls and Delve, found and installed on request, and where they keep downloads, `GOMODCACHE`) and Settings → Go |
| `zen-lang-rust` | L5 | The Rust plug-in, a built-in module of every IDE: highlighting today; rust-analyzer integration, Cargo, run and test configurations, inspections planned |
| `zen-ui` | L6 | The egui shell: main window, tool windows, editor view, dialogs |
| `zen-automation` | L2 | The local MCP server for AI agents (ADR-0033); the application answers its tool calls |
| `zen-testkit` | — | Test helpers shared across crates |

## 4. Key concepts

### 4.1 Commands

Every action is a `CommandSpec` registered in `zen-core`: a stable id such as
`editor.caret.moveToLineEnd`, a typed and serialisable parameter struct, a title, a default shortcut
and an availability check. Menus, the action search ("Find Action"), keymaps, macros, the CLI, the
MCP server and plug-ins all dispatch commands by id. A command that is not registered does not exist.

### 4.2 Language modules

A language module implements extension points declared by `zen-core`: file type detection, the
tree-sitter grammar and highlight queries, the language server to start and how to configure it,
project model detection (for example `Cargo.toml`), run and test configurations, and language-specific
commands and inspections. An IDE is a list of language modules plus branding.

Implemented today, each listed by the IDE in `zen_ui::ide::IdeProduct`: `zen_syntax::Highlighter`
(highlighting), `zen_syntax::Outliner` (the Structure tool window's outline from the text alone) and
`zen_project::model::ModelProvider` (modules, the workspace file and External Libraries, read by
`project.model.refresh`, `project.module.info` and `project.libraries.list`) and `zen_run::RunProvider`
(run and test configurations, gutter targets and test output, driven by the `run.*` commands,
[ADR-0048](adr/0048-run-configurations-and-the-run-service.md)); the language server is given as a
`zen_workbench::language::ServerSpec` ([ADR-0046](adr/0046-project-models-and-outlines.md)), and
`zen_project::inspect::Inspector` finds problems with quick-fixes and declares its settings (the
`problems.*` commands, the hover popup and ⌥↵; Go's `go.mod` dependency checks,
[ADR-0059](adr/0059-go-mod-dependency-checks.md)); its `unused_diagnostic` names the language
server diagnostics of its files that mean unused code without the Unnecessary tag (gopls's "… is not
used in this module"): they are greyed out, and left out where its own unused finding covers them.
Every IDE also reads the settings a project's own files give through `zen_project::settings::SettingsProvider`
(`project.settings.get`, `project.settings.refresh`; [ADR-0051](adr/0051-jetbrains-keymaps-and-idea-project-settings.md)).

### 4.3 Hybrid language intelligence

Two layers answer questions about code ([ADR-0003](adr/0003-hybrid-language-intelligence.md)):

- **The syntax layer** (`zen-syntax`) is ours, always available and fast: highlighting on every
  keystroke, folding, the structure view, structural selection and syntax-level inspections. It never
  waits for the language server.
- **The semantic layer** comes from the language server through `zen-lsp`: completion, go to
  definition, find usages, rename and other refactorings, diagnostics, inlay hints.

For Rust, rust-analyzer is also published as Rust libraries (`ra_ap_*`). Embedding it in-process is a
later step that removes the process boundary without changing the architecture.

### 4.4 Documents, snapshots and threads

Text buffers are immutable snapshots that share structure (a rope). The UI thread never blocks on
I/O, parsing, indexing or the language server: those run on background workers against snapshots,
and their results are applied only if the snapshot they were computed for is still current.

### 4.5 Automation

Every command is reachable by AI agents through the MCP server inside the IDE (`zen-automation`,
[ADR-0033](adr/0033-ai-access-to-every-api.md), [automation.md](automation.md)). The `zen-cli`
binary runs the platform headless: `zen-cli commands` lists the commands, `zen-cli run <id>` runs one
in a fresh session and prints its result as JSON, and the exit code tells usage errors (2) from
failed commands (1) ([ADR-0042](adr/0042-zen-cli-output-and-exit-codes.md)). Attaching to a running
IDE, opening a project first and running several commands in one batch are later steps. The desktop
app can expose an authenticated, loopback-only control channel for driving and inspecting the UI in
tests.

### 4.6 Updates and distribution

Releases are described by a signed manifest of channels (stable, nightly) and versions. `zen-update`
verifies the signature and installs atomically (rollback is an older release from the manifest, ADR-0265). Zen Toolbox uses it to manage every
installed IDE; an IDE installed without Toolbox can use it to update itself
([ADR-0008](adr/0008-toolbox-and-updates.md)).

### 4.7 Tool windows

The layout of a project window's tool windows is one model, `ToolWindowLayout`
(`crates/platform/ui/src/ide/tool_windows.rs`): each tool window's side, its order on the stripe,
its size and whether it is shown, kept per project as `toolWindows` in `workspace.json`. The stripes,
the panels, the drag and drop and the layout actions are in `tool_stripes.rs`; people's view of it
is the guide's [Tool windows](guide/22-tool-windows.md), agents' is
[docs/automation.md](automation.md#tool-window-layout).

**How to add a tool window:**

1. Declare it in `DECLARED` (`tool_windows.rs`): its id, its default side (the declarations' order is
   the order on the stripes), whether it shows in the default layout, and whether it shows again
   when the project opens (`false` for tool windows of results, such as Run).
2. Give it a stripe button in `KINDS` (`tool_stripes.rs`): the same id, its glyph, the Fluent
   message of its name and the keymap action that toggles it. A test checks that every declared tool
   window has one.
3. Draw it: an arm in `draw_tool_window` (`tool_stripes.rs`) that calls its drawing function with the
   `Ui` of its part of the edge. It is drawn wherever the person put it, so it lays itself out in the
   room it is given.
4. Bind its toggle (`view.toolWindow.<id>` in `zen_core::keymap`, group "Window") and handle it in
   `perform_action` with `self.toggle_tool("<id>")`.

Code shows, hides and moves tool windows only through `IdeView::show_tool`, `hide_tool`,
`toggle_tool` and `move_tool`, never by changing the layout's fields. The Git and Commit code
(`git_window.rs`, `commit_window.rs`) still writes the older flags (`project_open`,
`structure_open`, `commit_open`, and `bottom = None` to hide itself); the layout takes those writes
after the tool window is drawn and at the start of each frame, and gives the flags its own state
after every change.

A mode that hides every tool window (Distraction Free, Zen) calls `IdeView::hide_all_tools`, which
returns the layout as it was, and gives it back to `IdeView::restore_tools` when the mode ends; the
same pair is Hide All Tool Windows (⇧⌘F12). While a mode is on, the workspace should keep the
layout it returned rather than the hidden one.

### 4.8 The project's indexes

`zen_index::FileIndex` is the project's index, held by the workbench and built again when the
project or its exclusions change. On a worker thread it first walks the project's file names
(respecting `.gitignore`, leaving out `.git`, the folders a kind of project excludes such as a Cargo
package's `target`, and the excluded folders, ADR-0123), which Go to
File and Search Everywhere read; then it reads every file's text into an in-memory **trigram
index**: for each three folded bytes, the ascending ids of the files that contain them, delta-encoded
([ADR-0082](adr/0082-trigram-index-for-find-in-files.md)). Binary files, files that are not UTF-8
and files over 2.5 MB are skipped. The build is the background task "Indexing…", with
its progress in the status bar; `search.index.status` reports it.

Find in Files (`FileIndex::search_text`) turns its query into what a match needs (the text, or a
regular expression's literal runs with "and" and "or"), keeps the files that contain every trigram
of it, and searches only those, plus the files the index does not know yet and the open files;
a query without trigrams searches every file. The file watcher's changes (`file.index.refresh`)
forget the changed files at once, so they are searched in full until a background read puts them
back.

### 4.9 Popups at the caret

A popup that opens at the editor's caret (context actions, the quick-fix list, completion, parameter
info, quick documentation, the targets chooser, Show Usages) never keeps a screen position taken when
it opens: the caret may have moved in the same frame, by a command or an agent, and the editor scrolls
to it only while drawing. Each such popup keeps a `PopupAnchor` (`crates/platform/ui/src/ide/caret_popup.rs`):
`PopupAnchor::UNDER_CARET` (or `Caret(offset)`) for the caret, `At(point)` for a click, a gutter icon
or a hint. It places its `egui::Area` while drawing, after the text, with
`PopupAnchor::area(ctx, area, EditorView::caret_place())`; a popup that reads the caret's rectangle
itself (quick documentation, parameter info) takes `CaretPlace::rect`. The rules:

- While the editor scrolls to a caret the frame does not show yet, both answer `None` and the popup
  waits a frame instead of appearing where the caret was. The editor gives up scrolling after 120
  frames in a row, so a popup never waits for ever.
- The caret is read again in every frame, so the popup follows it when the text scrolls, the editor
  font or the interface zoom changes, soft wrap moves it, or the window is resized; each editor group
  has its own `EditorView`, so a popup follows the caret of its own group.
- A popup at the caret opens under it, and above it when the popup drawn in the last frame has no room
  below; it is kept inside the window, so at the right edge it moves left.
- When the caret is not on the screen (the user scrolled it away, or a popup is opened while it is
  off the screen), the popup stays open in the middle of the text, and goes back to the caret when
  the caret is shown again. It is never hidden while it takes the keys.

A new popup at the caret (Unwrap/Remove, Surround With) stores a `PopupAnchor` instead of a `Pos2`
and calls `PopupAnchor::area` in its `show`.

### 4.10 Workspaces

A **workspace** is a full tool that expands over the editor area and collapses back without closing
anything ([ADR-0163](adr/0163-workspaces-full-tools-in-the-editor-area.md)). The Database workspace
is the first ([database-workspace.md](database-workspace.md)); the Git workspace moves
onto the same host later (it is an editor tab today, [ADR-0057](adr/0057-git-workspace.md)).

- **Where:** while expanded, `editor_area` (`crates/platform/ui/src/ide/mod.rs`) draws the
  workspace host instead of the split editors; tool windows, stripes, toolbar, navigation bar and
  status bar stay. The split editors are untouched and come back exactly as they were.
- **One fact decides what is in front:** each workspace owns editor groups in a split tree of its
  own, kept by `EditorGroups` (`crates/platform/workbench/src/groups.rs`) outside the main split
  tree; the workspace is expanded exactly when the focused group is one of its own, and the editor's
  split commands then act on its area (ADR-0189). Expanding focuses it, collapsing
  focuses the main group that had the keyboard, and focusing any main group (opening a code file
  from anywhere) collapses it.
- **Tabs:** text tabs (consoles, scripts) are file-backed documents in the owned group, so the
  editor works in them as in code; other tabs are drawn by the feature, which the workspace state
  knows by kind, title and a little kind data. Consoles live in `<data folder>/consoles/`, never in
  the project.
- **Keys:** ⇧⎋ collapses while the workspace has the keyboard (and still hides the active tool
  window otherwise); ⎋ never collapses; ⌘W closes the active inner tab.
- **State and commands:** `zen_workbench::workspaces` holds the registered workspaces, which one is
  expanded and each one's tabs; `workspace.host.list`, `.state`, `.expand`, `.collapse`, `.toggle`
  and `workspace.tab.*` drive it, and `get_window_state` reports it to agents. The host
  (`ide/workspace_host.rs`) draws the inner tabs with the editor tabs' widget (`ide/tab_strip.rs`)
  and a feature's toolbar, left panel and non-file tabs through its `WorkspaceView`.
- **Kept per project** in `workspace.json` (`workspaces`): expanded or not, the tabs and the active
  one, carets and panel sizes; never data or secrets, and restoring connects nowhere.

**How to add a workspace:** register it (id, title, icon) with the workspace state, give the right
stripe a button that runs `workspace.host.toggle { id }`, register the views of its non-file tab kinds
and its toolbar and left panel, and list it in `docs/behaviour.md` as our own design.

### 4.11 Databases

`zen-db` (L2, planned, [ADR-0164](adr/0164-database-engine-zen-db-runtime-and-drivers.md)) runs
all database I/O on **one tokio runtime on a thread of its own**, started at the first connection.
A `db.*` command starts an operation and returns its id; rows and progress arrive as events in the
session, a `zen_run::Waker` wakes the window, and every operation has a cancellation token and a
deadline. Drivers are compiled in (no downloads) and describe themselves by data model and
capabilities, so the navigator and views ask a driver what it can do instead of assuming SQL:
PostgreSQL (`tokio-postgres`), SQLite (the system `sqlite3` program), MySQL and MariaDB
(`mysql_async`), Redis and Valkey (`redis`), ClickHouse, Elasticsearch and OpenSearch (over the HTTP
stack of ADR-0040) first; MongoDB, Cassandra/ScyllaDB and SQL Server once their TLS can run on our
pure-Rust provider. Any other database comes as an external driver program over standard input and
output, declared by a plug-in like a language server. Connections are global (the IDE's data
folder) or per project (`.zen/data-sources.json`), and their secrets live only in the Keychain
([ADR-0165](adr/0165-database-connections-types-and-secrets.md)), through `zen-secrets` (L0):
items of the service `space.ifzenelse.zen.database`, accounts `<connection id>/<kind>` (a project
connection's prefixed by its project key), read at most once per session by the process's cached
store (`zen_secrets::system()`). Deleting a connection calls
`zen_secrets::database::delete_all(store, project key, connection id)`.

## 5. Platforms

- **Desktop first:** macOS, Windows and Linux, rendered with wgpu (Metal, DirectX 12, Vulkan).
- **Web later, as a thin client:** an IDE needs processes, a real file system and a terminal, which a
  browser does not have. The browser client will run the UI while files, builds and language servers
  run on a server or in a container ([ADR-0006](adr/0006-desktop-first-ui-stack.md)).
