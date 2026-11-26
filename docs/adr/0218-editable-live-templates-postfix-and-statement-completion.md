# ADR-0218: Editable live templates, postfix completion and statement completion as language data

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0119](0119-surround-with-as-live-templates.md)

## Context

Live templates exist in the platform (`zen_workbench::templates`, ADR-0119): language modules
declare them as JSON (`TemplateSet`), the user changes and adds them in Settings → Live Templates,
and they are saved in `templates.json`. More is needed: template variables are
computed by expressions and follow each other, a template is applicable only in some places of a
file, plug-ins bring templates of their own languages, postfix templates wrap the expression before
a dot (`x.if`), Complete Current Statement (⇧⌘⏎) finishes a statement, Type-Matching completion
(⌃⇧Space) keeps only what fits the expected type, and typing a documentation comment's opening
above a declaration puts a stub. Every language needs these; Java and Kotlin must
plug in without platform changes, and the platform has no parser for every language.

## Decision

1. **Everything a language gives is data**, declared next to its live templates: a module's
   `TemplateSet` has `postfix`, `doc_comments` and `statements` (JSON of
   `templates::PostfixTemplate`, `DocComment`, `StatementRules`); a plug-in declares the same four
   lists in `plugin.json` under `liveTemplates` (`templates`, `postfix`, `docComments`,
   `statements`), carried by its manifest and catalog entry. The platform reads declarations from
   the plug-ins that are on whenever the plug-in set changes.
2. **Template variables have an expression and Skip if defined.** An expression is a quoted text,
   another variable's name or one of a fixed set of functions (case changes, `concat`, the file's
   name, the line, date, time, user); it is checked when a template is saved. Values are worked
   out in Tab order at expansion; a variable computed from another one is worked out again, in all
   its places, when Tab leaves a variable, as long as Tab has not reached it. A variable whose
   value is defined and marked Skip if defined is not visited.
3. **A template has contexts** `code`, `comment`, `string` (code only when none), told apart by
   the file's highlighter, the same way Tab already refused to expand in strings and comments.
4. **A plug-in's templates are read-only.** Only switching one off is saved (the plug-in's text is
   always the current one); Duplicate makes a template of the user's own. A module's templates
   stay editable as before. On the same language and abbreviation, a module's template wins.
5. **Postfix templates** are live templates whose `$EXPR$` is the expression before the dot. The
   expression is found from the text, without a parser: the chain of names, calls, indexes,
   macros, `?`, `::` and string literals before the dot; an operator or a space ends it. They join
   the completion list after `expression.` (a server's snippet with the same label gives way, as
   for live templates), and Tab expands a fully typed key. Settings → Editor → General → Postfix
   Completion switches each one, or all, off; the choices are saved in `templates.json`
   (`postfixOff`, `postfixDisabled`).
6. **Complete Current Statement is rule-driven** (`StatementRules`: terminator, block words,
   parenthesized words): it closes the line's open brackets and string, gives a block statement
   its block with the caret inside, or adds the terminator; a complete statement gets a new line.
   A language without rules gets its brackets closed and a new line.
7. **Type-Matching completion filters the server's answer**: it keeps the items the server marks
   `preselect`; when the server marks none, it keeps values, functions and types and drops
   keywords, snippets, live and postfix templates. The protocol has no expected-type field, so
   this is what a language server can tell us today.
8. **Documentation comment stubs** are declared per language (`DocComment`): the opening, whether
   it comes on typing or on Enter, the declaration keywords, the name rule of parameters, where the
   result type is, and a template with `$NAME$`, a `$PARAMETERS$` line and a `$RESULT$` line. The
   declaration below is read from the text (keyword or `name(`, the first parameter list, the
   result after it or before the name). The stub is a live template inserted as its own undo step.
9. **Commands** (ADR-0007): `editor.postfix.list`, `editor.postfix.set`, `editor.postfix.expand`,
   `editor.statement.complete` (⇧⌘⏎), `editor.completion.smart` (⌃⇧Space); the doc stub comes from
   `editor.edit.type` and `editor.edit.newline`; templates keep `editor.templates.*`.

## Consequences

- Java and Kotlin get templates, postfix templates, Javadoc and KDoc stubs and statement rules by
  writing `liveTemplates` in their `plugin.json`.
- Without a parser, a postfix expression that needs an operator (`a + b.if`) takes only its last
  operand, a statement split over lines is finished one line at a time, and a declaration whose
  head is unusual (a macro that declares a function) gets no stub.
- A catalog published before the release server knows `liveTemplates` drops the field; bundled
  plug-ins carry it in their manifest from the build.

## Alternatives considered

- **Code providers per language** (a trait in the module API): more exact, but a WebAssembly
  plug-in could not provide one without a new ABI, and Java's Javadoc would need a platform change.
- **Asking the language server for the expected type**: no request of the protocol gives it.
