# Roadmap

> **Last updated:** 2026-10-17

The roadmap is a sequence of phases, each ending with something that can be demonstrated. A phase is
finished when its exit criteria are met; the target months below are a plan, not a promise. Every
item is a GitHub issue labelled with its phase.

GoGarden is the first IDE ([ADR-0026](docs/adr/0026-gogarden-first.md)), so the language work of the
next phases targets Go; everything else is built in the platform and shared by every IDE.

## Plan for October 2026 – April 2027

| Months | Phase | Outcome |
|---|---|---|
| Oct – Dec 2026 | 3. The editor (finish) | Our own editor view with multiple carets, find and replace, file watching, Go to File, Find in Files, Find Action, keymaps, the main menu; measured typing latency |
| Jan – Feb 2027 | 4. GoGarden understands Go | gopls: diagnostics, completion, navigation, rename, formatting; the Go project model; run and test configurations |
| Mar – Apr 2027 | 5. The workbench | Git (status, commit, diff, log, blame), the debugger (Delve), the Settings dialog, the light theme |
| All along | Release track | Work in short branches merged to `main`; CI back on; Windows and Linux builds; signing and notarisation; an HTTPS client without C; the changelog on the website |

## Phase 0: Foundations (done)

- [x] Names for the team, family, platform, products and installer
- [x] Architecture decision records for the founding decisions
- [x] Architecture overview, naming reference, contribution and AI workflow guides

## Phase 1: Platform skeleton (done)

- [x] Cargo workspace with `crates/platform`, `crates/lang`, `apps`, `xtask`
- [x] `cargo xtask ci`: formatting, Clippy, tests, layering, translations, changelogs, licences, the
  WebAssembly build of the core
- [x] CI as a thin wrapper around `cargo xtask ci`, on every pull request; a green pipeline is
  required to merge
- [x] `zen-text`: rope buffer, edits, undo

## Phase 2: Zen Toolbox (done; Toolbox 0.3.0 released)

The first application: install, update and launch Zen IDEs ([ADR-0015](docs/adr/0015-toolbox-first.md)).

- [x] `zen-core`: command registry, session, settings
- [x] `zen-update`: signed release manifest, channels, atomic installation (rollback removed for
  now, [ADR-0029](docs/adr/0029-no-rollback-for-now.md))
- [x] Zen Toolbox: tools list, install and update flows, self-update, What's new, tray icon
- [ ] Projects tab, Settings (waiting for priority)
- [x] `zen-cli`: lists and runs commands headless, JSON output, fixed exit codes
  ([ADR-0042](docs/adr/0042-zen-cli-output-and-exit-codes.md))

## Phase 3: The editor (current, until December 2026)

A fast, comfortable editor in a window shared by every IDE. GoGarden is at 0.9.

Done:

- [x] The IDE window: welcome screen, projects in tabs of one window or in their own windows,
  restored at start with their files, panels and terminal tabs; the New UI frame with islands
- [x] Project tree: icons, several selected files, keyboard, copy, paste (also screenshots), drag
  and drop from the Finder, New File and Directory, Delete to the Trash
- [x] Editor tabs (scrolling, All Tabs), line numbers, current line, Go highlighting, save, undo
- [x] Plug-ins ([ADR-0031](docs/adr/0031-plugins.md), [ADR-0036](docs/adr/0036-bundled-plugins.md)):
  WebAssembly highlighters, a signed catalog, ten plug-ins, nine bundled
- [x] The built-in terminal; the image viewer ([ADR-0037](docs/adr/0037-image-viewer.md))
- [x] Settings: language, plug-ins, MCP, where projects open
- [x] Updates while the IDE runs ([ADR-0034](docs/adr/0034-restart-after-update.md),
  [ADR-0035](docs/adr/0035-update-offer-in-the-ide.md)); release notes in `changelog/`
- [x] Every capability reachable by AI agents through MCP ([ADR-0033](docs/adr/0033-ai-access-to-every-api.md))

To do:

- [ ] Decide how code is parsed (tree-sitter is C; ADR-0002 forbids C in the default build)
- [ ] `zen-editor` (L3): carets, selections, edits; our own editor view in place of egui's text
  field: multiple carets, column selection, large files
- [ ] Editing actions: find and replace, duplicate, move and delete lines, comment, indent, brackets
- [ ] File watching: external changes reload open files and refresh the tree
- [ ] Incremental highlighting and folding for Go
- [x] Go to File, Recent Files, Find in Files, Search Everywhere and Find Action
- [x] Keymaps (the macOS default first; ADR-0038)
- [x] The main menu from commands (ADR-0039)
- [ ] Measured typing latency and a large-file benchmark

**Exit:** editing a large Go project in GoGarden is pleasant, with measured typing latency.

## Phase 4: GoGarden understands Go (January – February 2027)

- The Go toolchain (now `zen_lang_go::toolchain`, ADR-0071): find the Go SDK and gopls, offer to install gopls (done: GoGarden 0.21)
- `zen-lsp`: the Language Server Protocol client (the client is done; the IDE uses it with diagnostics)
- Diagnostics in the editor and the Problems tool window; completion; go to definition, find (diagnostics and Problems done: GoGarden 0.22)
  usages, quick documentation, parameter info; rename; formatting on save
- The Go module project model and the Structure tool window
- Run and test configurations, the Run tool window, gutter run icons, the test tree (done: GoGarden 0.31)

**Exit:** a developer can write, navigate, run and test a Go module without leaving GoGarden.

## Phase 5: The workbench (March – April 2027)

- Git: file status colours, the branch widget, commit, diff, log, blame
- The debugger through `zen-dap` and Delve
- The Settings dialog (appearance, editor, keymap, Go); the light theme
- Import of JetBrains colour schemes and keymaps

**Exit:** a developer can use GoGarden as their only tool for a working day.

## Phase 6: First public release (after April 2027)

- Installers for macOS, Windows and Linux; signed and notarised
- Production signing key with a documented custody procedure
- Website, logos; trademark review; legal review of the interface's look
- Move to GitHub ([ADR-0010](docs/adr/0010-hosting.md)); contributor onboarding

## Later

- RustRonin offered again: rust-analyzer, Cargo, run and test configurations
- WebWave, the web IDE: the app exists (basic web editing); then the TypeScript language server,
  npm, Node.js run and debug, frameworks and linters, each a plug-in for every IDE
- JvmJizo, the IDE for Java and the JVM: the app and the platform's JDKs exist
  ([ADR-0147](docs/adr/0147-jvmjizo-app-and-jdks.md)); then Java with Eclipse JDT LS, Maven and
  Gradle, each a plug-in for every IDE
- A stable plug-in ABI and its SDK; plug-ins beyond highlighting
- A rollback mechanism for IDE updates ([ADR-0029](docs/adr/0029-no-rollback-for-now.md))
- Browser thin client; the remaining IDEs
