# ADR-0264: Go templates and embedded files are in the Go module

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0031](0031-plugins.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0071](0071-built-in-modules.md)

## Context

Go projects keep `text/template` and `html/template` files (`.tmpl`, `.gotmpl`, `.gohtml`) and name
files in `//go:embed` lines. Highlighting a template could be a WebAssembly plug-in or part of the
built-in Go module. gopls answers Go to Declaration on a `//go:embed` pattern with one file even
when the pattern matches several, as seen by asking it (0.23), so it cannot list them.

## Decision

1. **Built-in, in the Go module (`zen-lang-go`), not a plug-in.** Templates belong to Go projects, so
   they are on exactly when Go is, in every IDE that has the module, and need no catalog entry.
   The highlighter reads both template kinds, with the HTML around the actions when the file looks
   like HTML; a highlighter is not told the file's name, and `page.html.tmpl` is common. A plug-in
   would gain nothing today and would need the Go module's on/off state besides.
2. **A module can name the files at a place.** `zen_modules::file_references::FileReferences` answers
   `files_at(path, text, offset)`: `None` leaves the place to the language server, `Some(files)` is
   the answer of Go to Declaration (one file opens, several are listed, none says so), through the
   same `navigate.declaration.goTo` for people and agents. Go's `GoEmbedFiles` implements
   `//go:embed`: the pattern at the caret, or all of the line's on `//go:embed` itself, with Go's
   rules: `path.Match` patterns in the file's folder, a folder embeds its files without those
   starting with `.` or `_` unless `all:` is given, nothing leaves the folder or enters another
   module.

## Consequences

- Moving templates to a plug-in later changes no file format.
- Go to Declaration lists only files it can read as text: a binary embedded file is left out of the
  list.
- `{{if}}` and its `{{end}}` are not matched with each other; only braces are.
