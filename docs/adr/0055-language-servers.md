# ADR-0055: Language servers are declared, found, and installed only on request

- **Status:** Proposed
- **Date:** 2026-10-18
- **Extends:** [ADR-0003](0003-hybrid-language-intelligence.md), [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

Until now the IDE started one language server, gopls, from a spec that GoGarden's window built in
code. The requirement is what every IDE needs: which servers there are and how they are, what to
install, a log, restart and stop, settings for the program and its arguments, and the same servers
in every IDE because plug-ins declare them (ADR-0050). The IDE never downloads on its own
(AGENTS.md rule 10): the existing gopls install runs `go install` only when the user clicks.

## Decision

1. **A server is a declaration** (`zen_lsp::ServerDeclaration`): `id`, `name`, `languages`
   (protocol language ids, the first is the default; a file whose extension equals one of them gets
   that one), `extensions`, `names` (patterns with `*`), `command` (a program name or an absolute
   path), `args`, `versionArgs`, the optional `install` (`kind`: `go`, `npm`, `cargo` or `manual`,
   `package`, and for `manual` a `hint` that says how) and the optional `initializationOptions`. A plug-in declares its servers as
   `"languageServers"` in `plugin.json`; the field travels into the catalog and the installed
   manifest. A language module declares its servers in the same JSON shape
   (`zen_lang_go::LANGUAGE_SERVERS` holds gopls) and its product passes them in `IdeProduct`.
2. **One registry per IDE**, `zen_workbench::language::servers::LanguageServers`, shared by the
   windows' sessions like the toolchain. It holds the declarations (the product's first, then the
   enabled plug-ins'; the first declaration of an id wins), finds each program through
   `zen_lsp::ServerTools`, reads versions in the background, runs installations and keeps a log per
   server. `ServerTools` has defaults (`PATH` and the folders tools install into, such as
   `/opt/homebrew/bin`, `~/.cargo/bin`, `~/go/bin` and nvm's folders, which an application started
   from the Dock does not have on its `PATH`); `zen_toolchain::Toolchain` implements it for servers
   installed with `go install`: found in `GOBIN` or `GOPATH/bin`, started with the SDK in use first
   on `PATH` and as `GOROOT`, installed with that SDK's `go`.
3. **Installing is explicit.** `lsp.server.install` is the only thing that installs: `go install`,
   `npm install -g` or `cargo install --locked` with the tool found on this computer, in the
   background. When it cannot, `ServerTools::install_command` says why as a
   `zen_lsp::NotInstallable`: `manual` (the hint, or a translated "install it yourself") or
   `ToolMissing` (no `go`, `npm` or `cargo`; for Go, no Go SDK). The workbench turns it into a
   translated message, and `lsp.servers.list` gives `installable: false` with the missing tool and
   its download page (`installer`), so the UI can offer "Download Go" as the gopls banner did.
   Nothing installs on its own, and a missing server is not started: it is looked for again every
   30 seconds while it is listed, after an installation, and after `lsp.server.restart`.
4. **The user's choices** live in `language-servers.json` in the IDE's data folder, next to
   `settings.json`: `dismissed` (the ids of "Don't ask again") and `servers` (per id an optional
   `path`, a program name or an absolute path, an optional `args`, and `disabled` when the user
   turned the server off). `lsp.server.configure` changes them and restarts the server in the window
   it runs in (other windows use them from the server's next start); an empty `path` goes back to
   the program found. The IDE's `settings.json` stays the UI's.
5. **Each window keeps its own running servers** (`Languages`), keyed by server id; a server
   declared without its program present does not serve files, so features behave as without a
   server. A server stopped by the user stays stopped until restarted. A server that cannot start
   (or stops before it answered `initialize`) is shown as failed once and is not started again on a
   timer: only when its program or its arguments change while a file it serves is open (it was
   configured, installed or found anew) or on `lsp.server.restart`. A server that crashes after it ran is
   restarted by the client, three times in three minutes at most. A window remembers which
   servers had an open file and scans its project once in the background (at most 20 000 entries,
   skipping hidden folders, `node_modules`, `target`, `build`, `dist` and `out`) to say which
   servers the project needs.
6. **Logs.** A server's standard error and a one-line-per-message JSON-RPC trace (direction, kind,
   method or id, and the text of `window/*` messages; not the bodies) go into its log, as do its
   installations' output. The log keeps the last 2 000 lines, numbered, so that a reader asks only
   for the lines after the last it saw. The log is per IDE: two windows running the same server
   write into one log.
7. **Commands** (ADR-0007, ADR-0033): `lsp.servers.list`, `lsp.server.install`,
   `lsp.server.restart`, `lsp.server.stop`, `lsp.server.log`, `lsp.server.dismiss`,
   `lsp.server.undismiss` and `lsp.server.configure`; `language.servers.status` stays and gives
   the started servers by server id (`gopls`; it was the language id, `go`, before). The shapes are in [automation.md](../automation.md).

## Consequences

- Every IDE offers the same servers for the same plug-ins; the bundled YAML, JSON, TOML, Shell and
  Docker plug-ins declare yaml-language-server, vscode-json-language-server, Taplo,
  bash-language-server and docker-langserver, installed with npm (Taplo by hand).
- A server the user has installed starts for its files without a question, in every IDE, as gopls
  does (kept by the owner's decision: the plug-ins declare the same servers for every IDE). The
  user turns a server off in Settings → Languages & Frameworks → Language Servers
  (`lsp.server.configure` with `enabled: false`); a server turned off is listed as `disabled`, is
  not started and is not offered for installation.
- The UI (status bar widget, banner, Settings → Languages & Frameworks → Language Servers) is built on
  `lsp.servers.list` and the other commands only.
- A plug-in installed during a session declares its servers from the next start.
- A bundled plug-in that the user updated from the catalog before its manifest declared servers
  keeps the older manifest (the user's copy wins), so its servers appear only once the plug-in is
  republished with `"languageServers"` and updated again.
- A `manual` hint comes from the manifest and is shown as written (English); the messages the IDE
  writes itself (a missing `go`, `npm` or `cargo`, "install it yourself") are translated.
