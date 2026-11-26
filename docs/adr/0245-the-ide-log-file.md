# ADR-0245: The IDE log file

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

An IDE that misbehaves left no trace: only language servers had logs, kept in memory, and a panic
was written nowhere. A user had nothing to attach to a bug report. No telemetry may leave the
machine (AGENTS.md, rule 10).

## Decision

1. **Every IDE writes a local log**, `zen.log`, in `~/Library/Logs/<app id>/` on macOS, under
   `%LOCALAPPDATA%\<app id>\logs` on Windows and `$XDG_STATE_HOME/<app id>/logs` elsewhere. With
   `ZEN_DATA_DIR` set it goes to `<data folder>/logs`, so a throwaway start never touches the real
   folder.
2. **The `log` crate is the facade.** It is a new direct dependency, already in the tree: egui, wgpu
   and winit write to it, so their warnings reach the file without any wrapper of ours. The sink is
   `zen_workbench::ide_log`, installed once by `zen_ui::ide::read_command_line`, the path every IDE's
   `main` goes through. Starts that only hand paths over or print a version write nothing.
3. **Rotation:** at 5 MiB `zen.log` becomes `zen.1.log`, and 12 old files are kept, 65 MiB at most.
4. **Levels:** Info for our crates and Warn for the others. Debug Log Settings lists categories, one
   per line, each a module path matched as whole `::` segments; a category writes Debug, or everything
   with `:trace`. They are kept per IDE in `log-categories.txt` in the data folder and apply at once.
5. **A panic hook** writes the panic and a forced backtrace, then calls the hook it replaced.
6. **Collect Logs and Diagnostic Data** writes a ZIP locally and uploads nothing. Keys of
   `settings.json` that name a password, passphrase, secret, token, API key or credential are
   replaced before it is written. The archive goes to the log folder unless a path is given.

## Consequences

- The file holds what the IDE reports, including failed commands, so paths and project names can be
  in it. It stays on the machine; a person decides whether to attach it.
- Most of the platform writes no log lines yet; adding one is now a `log::info!` away.
- The user name and password of a URL are taken out of every log line, of the settings' values and of
  the logs in the archive; other secrets a program prints in a console stay there.
- Hiding secrets works by key name. A secret under an unusual key in `settings.json` would be kept;
  real secrets live in the system's secret store, not there.
