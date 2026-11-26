# ADR-0128: Installing a plug-in installs its language servers, and every server says where it runs from

- **Status:** Proposed
- **Date:** 2026-10-23
- **Extends:** [ADR-0055](0055-language-servers.md), [ADR-0127](0127-npm-language-servers-in-the-ides-own-folder.md),
  [ADR-0031](0031-plugins.md)
- **Supersedes:** the part of ADR-0055 and `docs/plugins.md` by which a plug-in's servers are read
  only at start, so that installing such a plug-in asks for a restart

## Context

A plug-in such as Vue, YAML, JSON, Bash or Docker declares a language server, but installing the
plug-in left the server to a second step: a restart, then a banner above the next file with its own
**Install**.
The owner (2026-10-22): installing a plug-in installs its language server in the same step, and the
list of language servers says that the server is installed locally.

## Decision

1. **One step.** When a plug-in is installed or updated (Settings → Plugins → Marketplace, the
   editor's offer banner, MCP `install_plugin`, `plugins.plugin.install`/`update`, the update at
   start), and it is on, the IDE's plug-ins (`zen_ui::plugins::Plugins`, which the window gives the
   IDE's `LanguageServers` with `set_language_servers`) take each server it declares:
   - a server that is **found** is kept and not installed again: at the path the user set, in the
     IDE's own folder (ADR-0127), or on `PATH` and the usual tool folders. A copy the user installed
     on `PATH` counts as found: the IDE does not install a second one beside it. Updating a plug-in
     therefore keeps its server;
   - a missing server whose installer is here is installed in the background with
     `LanguageServers::install` (the same installation as `lsp.server.install`: into the IDE's own
     folder for npm), whose progress the status bar's language server widget and the Language
     Servers list show; a notification says when it is installed or why it failed, and the server's
     banner never appears for it;
   - when the installer is missing (no Node.js with npm, no Go, no Rust), **one** notification per
     tool says which plug-in needs what and where to download it; nothing runs, and the server's
     banner stays as before, so that the user installs it later from there or from the settings;
   - a server installed by hand (`manual`) is left to its banner, as before.
2. **No restart for a new plug-in's servers.** A plug-in installed for the first time whose server
   ids are new declares them in the IDE's registry at once, and the windows serve its files without
   a restart. An update, an uninstall, switching a plug-in on or off, and a plug-in whose server id
   was declared already still ask for a restart, as before (a declaration cannot be taken back).
3. **Where a server runs from** is part of every server's state: `ServerSource` — `local` (the
   IDE's own folder, "installed locally"), `path` (`PATH` or the usual tool folders, "Global
   (PATH)"; also a server a toolchain finds, such as gopls in `GOPATH/bin`), `custom` (the path set
   in its settings) or `missing`. `lsp.servers.list` gives it as `source`, the table of Settings →
   Languages & Frameworks → Language Servers has an **Installed** column with it, and the status
   bar list names it beside the version.
4. **Uninstalling a plug-in** removes the IDE's own copies of its servers
   (`<data>/language-servers/<id>`) unless another installed plug-in declares the same server; a
   server on `PATH` or at a custom path is never touched. A bundled plug-in is not uninstalled, so
   its servers stay. Updating keeps the server (point 1).

## Consequences

- One click on **Install** in the Marketplace or the editor's banner gives a working Vue (or YAML,
  JSON, Bash, Docker) with its language server, without touching global packages.
- A plug-in install may now run npm, which needs the network; it happens only because the user
  asked for the plug-in (AGENTS.md rule 10), and the notification says so.
- Tests use a fake installer (`ServerTools::install_command_into`), never npm or the network.
