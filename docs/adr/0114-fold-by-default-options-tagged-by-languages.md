# ADR-0114: "Fold by default" options: General ones in the platform, a language's from its module, and folds tagged with the option that folds them

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0059](0059-go-mod-dependency-checks.md), [ADR-0071](0071-built-in-modules.md),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md),
  [ADR-0101](0101-custom-folding-regions-and-manual-regions.md)

## Context

The requirement is a page Settings → Editor → General → Code Folding: Show code folding arrows
(Always / On mouse hover), Show bottom arrows, and "Fold by default:" options in groups: General
(File header, Imports, Documentation comments, Method bodies, Custom folding regions) and one group
per language (Go: One-line 'if' blocks for error handling, One-line functions with a single
'return', One-line case clauses, Empty functions, Empty struct or interface type definitions,
Formatted strings). They apply when a file is opened for the first time; after that its folds are
remembered.

We had one option, Fold imports when a file opens (`editor.folding.imports`), on the Editor page.
Our folds are bracket blocks, imports and comments with a kind, from a plug-in, a lexer (Go) or
the brackets. Telling a method body, a one-line `if err != nil` block or a format string apart is
language knowledge, and the platform must not know Go (ADR-0050, ADR-0071). Some of Go's options
also fold what is no fold today (a `case` clause, a format string inside one line) and show
something other than `...` in its place.

## Decision

1. **A fold carries what its language knows.** `zen_syntax::FoldRegion` and `zen_editor::Fold` gain
   `placeholder` (what shows in place of the fold when folded, instead of the kind's mark) and
   `option` (the setting of the "Fold by default" option that folds it). A language's lexer sets
   them; the Go module's (`zen_lang_go`, `incremental::fold_options`) tags declared function bodies
   with the General Method bodies, and with its own options the bodies of one line (`return`, the
   `if` that checks an error against `nil`) shown as that line, empty function bodies and empty
   `struct`/`interface` types shown as nothing, and adds two kinds of region that only its options
   fold: `case`/`default` clauses of one statement, shown as the statement, and the format strings
   of `fmt.Printf`-like calls on one line with as many arguments as verbs, shown with the arguments
   in braces in place of the verbs.
2. **The General options are the platform's.** Their settings are named in
   `zen_modules::folding` (`editor.folding.fileHeader`, `.imports`, `.docComments`,
   `.methodBodies`, `.customRegions`), so that a language can tag folds with them; the workbench
   lists them (`zen_workbench::folding::GENERAL`) and classifies a fold no language tagged: imports
   by kind, custom regions by kind, a comment that starts the file and documents nothing as the
   file header, a doc comment as documentation, and a block in braces whose line starts a function
   (`fn`, `func`, `function`, `fun`, `def`, after modifiers such as `pub` and `async`, or a
   signature over several lines that started such a line) as a method body. These guesses are made
   only in files whose comments are C-like (`//` or `/* */`, `zen_syntax::comment_syntax`); in any
   other file (Markdown, YAML, JSON, TOML, a plug-in's language) only the folds its language tagged
   and custom regions start folded: no folding is better than a wrong one.
3. **A module gives its group as data.** `ModuleHost::folding_options(FoldingOptions)` adds a titled
   group of `FoldOption`s (setting, Fluent label, default); the window keeps the groups of the
   modules that are on in its session (`FoldingGroups`), the Settings page draws them under the
   General group, and the settings search finds their labels. A module's option is an on/off
   setting stored with the other declared settings (`declaredSettings` by key, as ADR-0059's), so no
   new storage is needed and agents change it with `set_settings`.
4. **Applied on the first open only.** A file without remembered folds opens with the folds whose
   option is on (as set, else its default) folded; a file with remembered folds opens with those
   (ADR-0101), so what the user folded or unfolded is kept and turning an option on changes only
   files opened for the first time.
5. **Arrows are settings the editor reads.** `editor.folding.showArrows`, `editor.folding.arrows`
   (`onHover`, the default, or `always`) and `editor.folding.bottomArrows`; the bottom arrow sits on
   the last line of an unfolded region of several lines and folds it. A fold on one line (a format
   string) has no arrow; its mark unfolds it.
6. **Commands** (ADR-0007, ADR-0033): `editor.folding.options` lists the arrows and every option
   with its key, label, default and value; `editor.fold.list` gives each fold its `option` and, for
   one with a placeholder, the `placeholder`; the settings are `showFoldArrows`, `foldArrows`,
   `foldBottomArrows`, `foldFileHeader`, `foldImports`, `foldDocComments`, `foldMethodBodies`,
   `foldCustomRegions` and a module's keys in `declaredSettings`.
7. **Where the page sits.** Editor → General (a page that lists its pages as
   links, the Editor page keeping its own groups for now) → Code Folding. Defaults: Imports on (as
   `editor.folding.imports` was), the rest off, Go's off. File header is off: on it would fold, for
   every user, the licence headers that opened unfolded before.

## Consequences

- A language module adds options without the platform knowing it: the Rust module can tag its
  folds and add a group the same way; until then Rust gets the General options from the bracket
  folds and the heuristics above, and runs of `use` declarations now fold as imports.
- The method-body heuristic is lexical and may miss a function whose header is unusual (a `where`
  clause on its own line in Rust); a language that tags its folds is exact.
- A bracket scan now treats a `'` that is not a character literal (a Rust lifetime, `'_`, `'a`) as
  no quote, so the folds after it are found.
- A placeholder is plain text: a fold folded inside another fold's placeholder is not shown folded
  there (an `if err != nil` block shows its format string as written).

## Alternatives considered

- **Classify by language in the platform** (keywords per file type): would make the platform know
  Go, against ADR-0071.
- **A module's options as a settings page of its own** (Languages & Frameworks → Go): the General
  group and the module's would be found in two places.
- **New fold kinds for each option**: the kind says how a fold is drawn (brackets kept or not); the
  option is why it starts folded. Keeping them apart lets a language tag any kind.
