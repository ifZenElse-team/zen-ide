# ADR-0077: Incremental highlighting and folding with our own line lexers

- **Status:** Proposed
- **Date:** 2026-10-20
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (pure Rust),
  [ADR-0003](0003-hybrid-language-intelligence.md) (the syntax layer), the open parsing decision
  "syntax: Decide how code is parsed (tree-sitter is C)"

## Context

ADR-0003 plans a syntax layer of incremental tree-sitter parsing. Tree-sitter is written in C,
which ADR-0002 keeps out of the default build, and how code is parsed is an open decision. It
names three options: allow tree-sitter as an exception, write our own incremental parsers per
language in Rust, or port a parser framework; it recommends the second for Go now, as the Go
grammar is small and gopls gives the semantics, and an evaluation of the third before the next
language.

Meanwhile the editor highlighted the whole file again after every keystroke and rebuilt its line
index, and read the folds from the whole text (as characters) once typing paused: on a
100 000-line Go file, 11 ms of the 12 ms a keystroke took (docs/performance.md).

## Decision

1. **Highlighting follows edits line by line.** `zen_syntax::Lines` lexes a text one line at a time
   with a `LineLexer`, keeping each line's start, the lexer's state at the start of each line and
   what each line lexed into. An edit lexes the lines it touched and then the following lines
   until one starts in the state it had before; the rest only move. A language offers this through
   `Highlighter::incremental`, which returns an `Incremental`: its edits, the spans of a byte
   range, and optionally the fold regions. A highlighter without it is highlighted whole, as before;
   `zen_syntax::Highlighting` reads both the same way.
2. **The spans are those of the whole-text highlighter.** A span over several lines (a block
   comment, a raw string) comes as one piece per line. What the whole-text lexer decides from far
   away is settled when spans are asked for: in Go, a declared constant is coloured wherever it is
   used (the declarations are counted over the lines as they change), and a name followed by `(`
   on a later line is a call. Tests compare the incremental spans with the whole-text spans after
   thousands of random edits.
3. **Edits are known, not guessed.** A document keeps the byte edits of its last 64 versions
   (`Document::changes_since`); the highlighting and the editor's line index follow them, and read
   the whole text only when they are unknown (another file, a version too old).
4. **One highlighting per open file, shared.** `zen_workbench::brackets::Highlighters` keeps the
   highlighting of the files shown; the editor draws from it, and the commands (matching brackets,
   folding) read it, so that a file is lexed once whoever asks.
5. **Folds come from the lexer's tokens.** A language whose `Incremental` gives fold regions (Go:
   blocks in brackets that span lines, `import ( ... )`, runs of `//` lines) is folded from the
   tokens its highlighting holds, without reading the text again; the regions are those
   `zen_editor::folds` finds in well-formed code, which a test checks. Other files fold by their
   brackets as before; a plug-in's folds still come first.
6. **Go is first.** `zen-lang-go` lexes Go line by line with the same rules as its whole-text
   lexer. This is option 2 of the parsing decision for Go; the decision itself, and whether a later
   language uses a ported framework (option 3), stays open.

## Consequences

- Typing in a large Go file lexes one line a keystroke (a block comment opened or closed lexes the
  rest of the file, as it changes every line after it); `cargo xtask bench-editor` reports the most
  lines a keystroke highlighted again.
- Go has two lexers with the same rules, the whole-text one (still used for the Structure, run
  targets and imports) and the line lexer; the random-edit tests keep them in step.
- A new language can be made incremental by writing a `LineLexer` for it; nothing in the editor
  changes.
- There is still no syntax tree: structural selection and a parser-based outline need the parsing
  decision.

## Alternatives considered

- **Tree-sitter now.** Fastest route to many languages, but it is C, against ADR-0002, and the
  owner has not chosen it.
- **Highlight only the visible lines.** Wrong inside a block comment or raw string that starts
  above the view, and the constants of a file need the whole file.
- **Diff the old and new text to find the edit.** Costs a pass over the text per keystroke; the
  document knows its edits already.
