# ADR-0220: Code style options declared by languages

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

Until now Settings → Editor → Code Style → <language> had only Use tab character, Tab size and
Indent. The requirement: every language gets a page with the tabs Tabs and Indents, Spaces,
Wrapping and Braces, Blank Lines, Imports and Arrangement, a preview, and Code → Rearrange Code.
Our languages do not format code themselves: Go goes through gofmt (via gopls), Rust through
rustfmt (via rust-analyzer), Java and Kotlin through their language servers. Their options live in
different places: `textDocument/formatting` options, language server settings, and the formatter's
own file in the project (`rustfmt.toml`). Java and Kotlin are plug-ins, so whatever the platform
offers must be reachable as data from a `plugin.json`, without platform changes for each language
(ADR-0050).

## Decision

1. **A language declares its code style as data** (`zen_project::code_style::CodeStyleDeclaration`):
   its options (id, label, tab, group, kind: boolean, integer range, choice or text, default), a
   preview text, a note per tab, an optional configuration file of its formatter, and the kinds of
   declarations Rearrange Code sorts. A built-in module registers it with
   `ModuleHost::code_style_options`; a plug-in lists it under `"codeStyles"` in its `plugin.json`,
   carried through the catalog like the other declarations. Labels are a message id of the IDE's
   translations (built-in modules) or the text by language code (plug-ins).
2. **Each option says where its value goes**, any of: `formattingOption` (a key added to the
   `textDocument/formatting` options), `serverSetting` (a section and dot-separated path of the
   configuration of the language servers that serve the language, sent with
   `workspace/didChangeConfiguration` and answered to `workspace/configuration`, on top of the
   server's own settings of that section, or of `initializationOptions.settings.<section>` for a
   server that declares no section; only values that differ from the default are set), `configKey` (a
   key of the formatter's configuration file) and `editorConfig` (a property written by Export →
   EditorConfig File).
3. **The scheme decides where values are kept.** With Default (IDE) they are IDE settings
   (`editor.codeStyle.<language>.<id>`, `codeStyleOptions` for agents). With Project, a language
   that declares a configuration file reads and writes that file in the project's root folder,
   line by line, touching only its own keys (`editor.codeStyle.writeProject`); with Default, the
   file's keys that differ from their defaults go to the formatter on its command line through a
   declared server setting (for rustfmt: `rust-analyzer.rustfmt.extraArgs` with `--config`).
4. **Rearrange Code is a provider per language** (`zen_syntax::Arranger`): given the text and the
   order of declaration kinds, it answers the rearranged text or nothing when it cannot rearrange
   safely. The first one is Go's top-level declarations: constants, variables, types, functions, a
   stable sort that keeps the order inside each kind, so package variables are initialised and
   `init` functions run as before; comments directly above a declaration move with it; a file it
   does not understand is left alone. Rust has none yet: reordering items inside `impl` blocks
   needs to know macros and `cfg` attributes. `editor.code.rearrange` runs it; Reformat Code runs it
   first when `editor.code.reformat.rearrange` is on.

## Consequences

- Java and Kotlin options are a `"codeStyles"` list in their `plugin.json`.
- The page shows only what the formatter really reads: gofmt has no options, so the Go page shows
  gopls's `gofumpt` and `local` and says so on each tab; rustfmt shows its stable options only.
- The preview shows the declared text as it is; it is not reformatted while options change.
- A plug-in's page title is its own text, so its page in the Settings tree is named in the
  language of the plug-in's declaration, not through the IDE's translations.
