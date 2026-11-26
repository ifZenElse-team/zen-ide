# ADR-0214: The Properties plug-in, keys from string literals and the Resource Bundle editor

- **Status:** Proposed
- **Date:** 2026-11-04
- **Extends:** [ADR-0031](0031-plugins.md), [ADR-0056](0056-plugin-abi-v1.md) (the plug-in ABI)

## Context

JvmJizo had no support for `.properties` files, which Java projects use for configuration and for
message bundles (`java.util.ResourceBundle`). The IDE should highlight them, report duplicate
keys, complete and navigate the keys used in code (`bundle.getString("key")`), group the files of
a bundle (`messages.properties`, `messages_ru.properties`…) in the Project tool window and edit
them together. The format is public: `java.util.Properties.load` documents the separators (`=`,
`:` or white space), the continued lines (an odd number of `\` at the end), the comments (`#`, `!`)
and the escapes (`\t`, `\n`, `\f`, `\r`, `\uXXXX`, `\` before any other character); since Java 9
`PropertyResourceBundle` reads files as UTF-8, while `Properties.load(InputStream)` reads ISO-8859-1.

Two things were out of a plug-in's reach: showing a value unescaped (the ABI had no way to put text
next to the code) and anything about other files (a plug-in sees only the text it is given).

## Decision

1. **A Properties plug-in** (`plugins/properties`, ABI v1) gives highlighting (keys as declared
   properties, kind 17; values as strings; escapes as constants), folding (continued values and
   comment blocks), the outline (the keys, kind `property`), problems (a duplicate key is a warning on
   every occurrence; a malformed `\u` an error) and two new exports. It works in every IDE and
   JvmJizo bundles it. Its reader is one self-contained file that another plug-in may include
   (`#[path]`), as Maven includes the XML plug-in's code.
2. **`zen_inlays`**, an optional ABI v1 export: the text in, `[{"offset", "text"}]` out. The host
   lists them through `editor.inlayHints.list` for a file no language server gives hints for, cached
   by document version, and they follow the global inlay hint switch only (they are not one of the
   protocol's kinds). The Properties plug-in shows a value unescaped where it differs from the text:
   an inlay is where a value's meaning is shown without changing the file, and hover
   would hide it until asked.
3. **`zen_key_references`**, an optional ABI v1 export read once at load:
   `[{"files", "keys", "bundles"}]` — in files matching `files`, a string literal that is the first
   argument of a call named in `keys` is a key of the declaring plug-in's files, in `bundles` a
   bundle name. The platform does the rest generically: it lists the project's files the plug-in
   handles (the file index), reads their keys from the plug-in's own outline (cached by
   modification time), and answers completion inside such a literal and Go to Declaration on it
   (tried before the language server, as Ant's targets are). A bundle's name is its path under the
   nearest `resources`, `java` or `kotlin` folder, dotted, without a locale suffix; when the file
   names bundles with `getBundle`, only their keys are offered. The Java names live in the plug-in's
   declaration, not in the platform, so another plug-in can declare its own calls (a templating
   language's message functions, say) without platform changes.
4. **Key completion for a file name** (Spring Boot's `application.properties`, a later task) needs
   no new interface: the framework's plug-in takes those files by name, which wins over the
   Properties plug-in's extension, includes `properties.rs` and answers `zen_complete` with
   `complete_keys(text, caret, &KEYS)`.
5. **Resource bundles are the platform's** (`zen_workbench::resource_bundles`), since a bundle is
   several files and an editor view, which no plug-in can draw: a bundle is two files or more of one
   folder named `base.properties` or `base_<locale>.properties`, the locale a two-letter lower-case
   language, then optionally an upper-case country or a three-digit region, then variants. The
   platform reads and writes the format with its own small reader (keys, values, the ranges to
   change), as it reads EditorConfig: the editing operations need byte ranges the outline does not
   carry, and the platform cannot depend on a plug-in.
   - Commands: `properties.bundle.list`, `get` (the table: values per locale, unescaped, and the
     locales missing each key, absent or empty), `setValue`, `addKey` (to every file lacking it,
     the value in the default file, empty elsewhere), `renameKey` (refused when the new name is
     taken), `removeKey` and `undo`. Each change goes through the refactoring edit path: open files
     change in their documents, others on disk, and the change is one journal entry, so
     `properties.bundle.undo` takes it back in every file at once (⌘Z in a file's editor still undoes
     that file's part).
   - The Project tool window groups a bundle's files under a **Resource Bundle 'name'** node while
     some plug-in handles `.properties` files; double-click or ⏎ opens the Resource Bundle editor, a
     front tab of the editor area like the folder comparison, with a column per locale and the
     `view.resourceBundle.*` actions for agents.

## Consequences

- Plug-ins can show inlays and make their keys reachable from other languages' code without new
  platform code per language; the platform's key provider is generic but only knows "first string
  argument of a named call", which covers `ResourceBundle` and `MessageSource` usage but not keys
  built from expressions or passed through constants.
- The bundle editor works on files, not on an index: a bundle with many locales is read on each
  change, which is fine for the sizes message bundles have.
- The `.properties` reading rules exist twice (the plug-in's reader and the platform's); both are
  tested on the same rules, and a change to the format's reading changes both.
