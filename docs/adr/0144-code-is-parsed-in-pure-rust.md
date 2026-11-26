# ADR-0144: Code is parsed by our own parsers in pure Rust, not tree-sitter

- **Status:** Proposed
- **Date:** 2026-10-25
- **Supersedes:** the tree-sitter part of [ADR-0003](0003-hybrid-language-intelligence.md) (its syntax
  layer); **relates to** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (pure Rust),
  [ADR-0077](0077-incremental-highlighting-with-line-lexers.md), [ADR-0098](0098-extend-selection-without-a-parser.md),
  [ADR-0099](0099-unwrap-remove-without-a-parser.md)

## Context

ADR-0003 split language intelligence into a syntax layer owned by the platform (highlighting,
folding, the Structure view, structural selection, indentation) and a semantic layer from the
language server (completion, navigation, rename, diagnostics). It planned the syntax layer as
incremental tree-sitter parsing. Tree-sitter and its grammars are written in C, which ADR-0002
keeps out of the default build, so the choice stayed open with three options:

1. allow tree-sitter as a documented exception to ADR-0002: many ready grammars, the fastest
   route, but the default build is no longer pure Rust;
2. keep pure Rust and write our own incremental lexers and parsers per language, as the
   highlighters, folds and outlines already are (ADR-0077; the plug-ins' own lexers);
3. keep pure Rust with a Rust parser framework, if a mature one is found.

Meanwhile the platform and the plug-ins grew their own line lexers and structure readers for Go,
Rust, the web languages, XML, Markdown and others, and the language servers cover the semantics.

## Decision

1. **No tree-sitter, no C.** The default build stays pure Rust (ADR-0002); tree-sitter is not
   added as an exception.
2. **Our own parsers, per language, in Rust** (option 2, the owner, 2026-10-25). A language's
   syntax layer — highlighting, folding, the Structure view, structural selection, indentation —
   is a lexer (ADR-0077's `LineLexer`) and, where a feature needs more than tokens, a small
   incremental parser of that language, written for it, in the platform (built-in languages) or in
   the language's plug-in. Semantics stay with the language server (ADR-0003, layer 2).
3. **Before the next language that needs a real parse tree**, evaluate a pure-Rust parser
   framework (option 3) once, and record the outcome in a new ADR; until then, option 2.

## Consequences

- Each language costs its own lexer and, where needed, parser; ready-made grammars are not
  available. The Go and web plug-ins show this is affordable for the features we have.
- Syntax features that need a full tree (structural selection across nested expressions, precise
  indentation) are built per language when they are needed, as ADR-0098 and ADR-0099 did without
  a parser.
- ADR-0003's layering stands; only how its syntax layer is built changes.
