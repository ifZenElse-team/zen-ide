# JSON plug-in changelog

Every release of the JSON plug-in (`plugins/json`), newest first; the format is in
[../README.md](../README.md).

## 0.2.0 - 2026-11-13

Needs Zen IDE 26.11 or later.

### Changed

- The JSON language server no longer downloads a schema a file names with a web address
  (`"$schema": "https://…"`): it asks the IDE, which refuses unless you allowed schema downloads
  (Settings → Languages & Frameworks → Schemas), and the file says why. Schema files on disk still work.

## 0.1.0 - 2026-10-16

### Added

- The first release: highlighting for JSON, JSONC, JSON5 and JSON Lines: keys, strings, escapes,
  numbers, `true`/`false`/`null`, punctuation and comments.
