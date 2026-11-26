# ADR-0157: Export, import and restore the IDE's settings

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

The requirement: File → Manage IDE Settings with Import Settings…, Export Settings…, Restore
Default Settings… and Backup and Sync…. Export writes the chosen components into a
ZIP file; Import reads such a ZIP (or a configuration folder), asks which components to take in
Select Components to Import and restarts; Restore Default Settings asks with Restore and Restart and
first copies the configuration to a backup folder that Import Settings can read back. Backup and
Sync would need an account.

A Zen IDE keeps its own settings in its data folder (`ZEN_DATA_DIR/<app id>` or the system's):
`settings.json`, `language-servers.json`, `keymap.json` and `keymaps/`, `themes/`, `templates.json`,
`modules.json` and the plug-in switches in `plugins/`. The same folder also holds things that are not
settings: recent projects, local history, caches, shelves, Git history, scratches, window layouts.
Project settings live in the project (`.zen`, and `.idea` read from JetBrains IDEs).

No ZIP library is in `Cargo.lock`; flate2 (raw deflate and CRC-32, pure Rust) is.

## Decision

1. **Components.** The IDE's settings are five components: `settings` (`settings.json`,
   `language-servers.json`), `keymaps` (`keymap.json`, `keymaps/*.json`), `themes`
   (`themes/*.json`), `templates` (`templates.json`) and `plugins` (`modules.json`,
   `plugins/enabled.json`, `plugins/disabled.json`, `plugins/ignored-extensions.json`, and in an
   archive `plugins/installed.json`, the list of installed plug-ins). Nothing else of the data folder
   is exported, imported or reset, and nothing of a project, ever: Export refuses a path inside a
   `.zen` or `.idea` folder, and Import and Restore write only into the data folder.
2. **Our archive** is a ZIP whose first entry, `zen-settings.json`, says
   `{ formatVersion: 1, kind: "export" | "backup", product, version, created, components }`; the
   other entries are the files at their paths relative to the data folder. Importing it replaces the
   files it holds (keymaps and themes by file name) and leaves the rest. The plug-in list is only
   compared: a plug-in it lists that is not installed is reported as skipped, never downloaded
   (AGENTS.md, rule 10).
3. **A JetBrains Export Settings ZIP** (no `zen-settings.json`; `keymaps/`, `colors/`, `options/` or
   the "IntelliJ IDEA Global Settings" marker) gives its `keymaps/*.xml` through the existing
   JetBrains keymap importer and its `colors/*.icls` (or `.xml`) through the existing colour-scheme
   importer, as new keymaps and themes. Every other entry is listed as skipped with the reason.
4. **Restart.** Keymaps and themes apply at once; settings, live templates and the plug-in list are
   read when the IDE starts, so an import of them ends with Restart / Not Now. Restore Default
   Settings copies every component's files to
   `settings-backups/<YYYY-MM-DD-HHMMSS>/` *inside* the data folder (with a `zen-settings.json` of
   kind `backup`), removes them only when the copy succeeded, and restarts. Keeping the backup
   inside the data folder means the action
   never writes outside the folder it resets, which also makes it safe in tests. Import Settings…
   reads that folder back.
5. **A headless IDE never restarts** into a window (ADR-0084): it does the import or the restore and
   answers `restartRequired: true`. Under test, the commands get a data folder only when it is in the
   system's temporary folder, and a restart is only counted.
6. **Our own ZIP reader and writer** (`zen_workbench::ide_settings::zip`, on flate2) instead of a new
   dependency: stored and deflated entries, no ZIP64, no encryption. An archive is refused as a whole
   when it is over 64 MB, lists more than 4096 entries, an entry unpacks to more than 8 MB (or all of
   them to more than 64 MB), or an entry's name is not a plain relative path (absolute, a drive, `..`,
   `.`, a backslash, an empty part). Entries are unpacked in memory, never more than their declared
   size, with the size and the CRC-32 checked; nothing is written by an entry's name except the fixed
   paths of decision 1.
7. **Commands** (ADR-0033): `settings.ide.components`, `settings.ide.export { path, components? }`,
   `settings.ide.inspect { path }`, `settings.ide.import { path, components? }` and
   `settings.ide.restoreDefaults`; the window actions `view.ideSettings.export`,
   `view.ideSettings.import` and `view.ideSettings.restoreDefaults` (the menu items, with the same
   parameters to fill the dialogs, and `confirmed` to restore at once). `settings.ide.import` and
   `settings.ide.restoreDefaults` are a person's only (`agentNotAllowed`): both replace settings
   wholesale, past what `set_settings` refuses (ADR-0033 keeps them reachable as the dialogs an agent
   opens and a person answers). Backup and Sync is left out:
   it needs an account (AGENTS.md, rule 10); syncing through a folder or Git would need an ADR of its
   own.

## Consequences

- Settings move between machines and between Zen IDEs with one ZIP, and a reset can be undone.
- People coming from a JetBrains IDE bring their keymaps and colour schemes from the ZIP they
  already have; what is not read is listed, not lost silently.
- The ZIP code is ours to keep small and safe; it reads what our writer and JetBrains IDEs write,
  not every ZIP (no ZIP64, no encryption, no other methods).
- Agents export, import and restore the settings through `settings.ide.*`, although ADR-0143 keeps
  the data folder out of their searches: these commands touch only the files of decision 1, which
  hold no secrets and nothing of a project (accepted by the owner, 2026-10-26).
- A new component (for example code styles or file templates) is added to the list of decision 1,
  with its files; the archive format stays at version 1 while files are only added.
