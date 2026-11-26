# ADR-0135: File types by content, and XML in every IDE

- **Status:** Proposed
- **Date:** 2026-10-24
- **Extends:** [ADR-0108](0108-file-type-registry-and-ignored-files.md), [ADR-0124](0124-associate-and-override-file-types.md),
  [ADR-0031](0031-plugins.md), [ADR-0056](0056-plugin-abi-v1.md), [ADR-0063](0063-plugin-colours-and-typing.md),
  [ADR-0036](0036-bundled-plugins.md)

## Context

The owner, 2026-10-24, with a `.cyberduckprofile` file (an Apple property list:
`<?xml version="1.0"…?>`, `<!DOCTYPE plist …>`) opened as plain text: "by the extension you can't
tell, but by the content you can. Do it by extension and by content, and in the core — in every
project by default."

The registry (ADR-0108) knew a file's type by its name patterns and, for a file no pattern names,
by the hashbang of its first line, and only where the caller gave that first line (the editor's
highlighting, code style); the tree's icons, folding, the Structure and the language servers asked
by name alone. No IDE had an XML type at all: `.xml` files were plain text.

## Decision

1. **Content markers in the registry** (`zen_vfs::file_types::ContentMarker`): a type may declare
   what its files start with, after a byte order mark and white space: `starts` (letter case as
   written), `unless` (starts that do not count, any case) and `firstTag` (a word the text before
   the first `>` must hold). No regular expressions: what types need is a start, and a matcher that
   cannot backtrack cannot be made slow by a file.
2. **Order** (ADR-0124's, extended): the type the user gave the file itself (Override File Type);
   its name patterns, the user's changes included; the hashbang of its first line; then the content
   markers, in the order types were declared. A pattern the user adds therefore always beats the
   content.
3. **Reading the head.** Callers that have the text give its head (`file_types::head`, at most
   `HEAD_LIMIT` = 4 KiB) where they gave the first line before. For a file on disk that no
   pattern names, `known_name_at` and `type_at` read the head themselves (`disk_head`): only a
   regular file, only 4 KiB, never past a NUL byte (binary: no head), kept per path with its size and
   modification time (up to 4 096 files, then emptied), so the tree, icons and index read a file
   once per version. A file whose type is decided by its name is never read. `editor.fileTypes.typeOf`
   answers `byContent`; `editor.fileTypes.list` gives each type's markers as text (`contents`), and
   Settings → Editor → File Types shows them read-only under the patterns.
4. **Plug-ins declare them** in `plugin.json` (`"contents"`, carried by the catalog,
   `zen-releases publish-plugin --contents` and `cargo xtask release plugin`).
5. **Problems from plug-ins**: ABI v1 gets one more optional export, `zen_problems` (the text in;
   `[{"start", "end", "severity", "message"}]` out, byte offsets), `TextFeatures::problems` and
   `Feature::Problems`. Its problems join the file's (`problems.file.list`, underlines, the Problems
   tool window), found again when the document's version changes, and a file such a plug-in serves
   counts as checked without a language server. The host drops what does not fit and keeps the first
   line of each message (512 characters, 1 000 problems). Messages are the plug-in's words, in
   English, as a language server's are: a plug-in has no Fluent bundle of its own.
6. **An XML plug-in in every IDE's base set** (`plugins/xml`, `BASE_PLUGINS`), ABI v1:
   - the type by extension: `xml plist xsd xsl xslt svg wsdl pom csproj fsproj vbproj props targets
     resx xaml rss atom kml gpx opml dtd iml` (`pom.xml` and the like by `*.xml`). `.xhtml` stays
     HTML's. `.svg` is in it so that an SVG read as text is XML; an SVG still opens as a picture
     (images are decided before types), and keeps its picture icon;
   - by content: `<?xml`; `<!DOCTYPE` unless `<!DOCTYPE html` (any case); and `<` with `xmlns` in
     the first tag unless `<html`, `<!` or `<?`. None claims HTML (`<!doctype html>`,
     `<html xmlns=…>`) or a text that merely contains markup later;
   - highlighting (elements, attributes, values, references, comments, CDATA, the doctype,
     processing instructions, namespace prefixes as types), folding of elements, comments, CDATA and
     doctypes, the Structure (elements, `name#id`), end tags as you type (`>` adds `</name>` while
     the name's end tags after the caret are fewer than its start tags before; `</` completes the
     open element), and problems: end tags without a start tag or that do not match, elements not
     closed, tags, comments, CDATA sections and processing instructions not finished;
   - it reads the text with the HTML plug-in's tokenizer in a new XML mode (`lex_xml`: no raw text
     elements, `?>` ends a processing instruction, a doctype keeps its `[…]` subset, names may start
     with `_`), case-sensitive and with no void elements; the HTML, Vue and Svelte plug-ins keep
     `lex` as it was;
   - a New → XML File template; the platform's XML icon for every XML extension.

## Consequences

- Any plug-in can claim files by content (a JSON or YAML dialect by its first key, a format by its
  magic text) without platform code.
- Every IDE opens XML files highlighted, folded and checked, offline, with no server; a schema-aware
  XML language server (validation, completion from XSD) can come later as a plug-in's declaration.
- A file without an extension is now read (4 KiB, once per version) when it is shown in the tree or
  opened, if any type has content markers or hashbangs, which every IDE now has.
- The plug-in ABI's problems are text-only checks; anything needing the project (schemas, other
  files) belongs to a language server.
