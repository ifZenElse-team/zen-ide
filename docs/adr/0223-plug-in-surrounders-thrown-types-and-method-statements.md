# ADR-0223: Plug-in surrounders, translated template texts, thrown types and method statements

- **Status:** Proposed
- **Date:** 2026-11-04
- **Relates to:** [ADR-0218](0218-editable-live-templates-postfix-and-statement-completion.md),
  [ADR-0119](0119-surround-with-as-live-templates.md), [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

ADR-0218 lets a plug-in declare live templates, postfix templates, documentation comment stubs and
Complete Current Statement rules in `plugin.json` under `liveTemplates`. Java and Kotlin are the
first plug-ins to use it, and they need four things the declaration did not have:

- **Surround With** (⌥⌘T) lists a language's constructs (`if`, `try / catch`, `synchronized`...),
  but only a language module could declare them (`Surrounder`); a plug-in could only add live
  templates with `$SELECTION$`, which the list shows last and by abbreviation.
- A plug-in has no Fluent messages, so its template descriptions could be English only, while
  every text the IDE shows is in English and Russian (golden rule 9).
- A Javadoc or KDoc stub has a `@throws` line per exception the declaration throws: Java lists them
  after `throws`, Kotlin in a `@Throws(...)` annotation. Kotlin also writes type parameters before
  a function's name (`fun <T> f()`) and a receiver before it (`fun List<T>.second()`), and Java
  documents fields, which have no keyword.
- A Java method declaration (`public int size(`) gets a block from Complete Current Statement, but
  it starts with no keyword a rule could name.

## Decision

1. **`liveTemplates.surrounders`** is a list of `Surrounder` (as a language module declares
   them: `language`, `id`, `title`, `text` with `$SELECTION$`, `variables`, `expression`). Surround
   With lists a plug-in's surrounders after the module's ones for the same language; a module's
   surrounder with the same id wins.
2. **A text shown to people may be given per language:** `description` of a template or postfix
   template and `title` of a surrounder is either a string or an object of language codes
   (`{"en": "...", "ru": "..."}`). The IDE takes the interface language, then its base language,
   then `en`, when it reads the plug-in's declarations.
3. **`DocComment` gets thrown types and fields:** `throws` is the word after the parameters that
   lists them (`throws`), `throwsAnnotation` the start of an annotation line above that lists them
   (`@Throws(`, each `X::class` read as `X`), `thrown` the line per type with `$EXCEPTION$`, put
   where the text has a line that is only `$THROWS$`. `fields` makes a line `Type name;` or
   `Type name = value` a declaration. The name after a keyword passes over type parameters
   (`<...>`) and a receiver (`Type.`), and a result type before the name keeps its type arguments
   (`Map<String, Integer>`).
4. **`StatementRules` gets `methods` and `notDeclarations`:** with `methods`, a line with two or
   more words before its first parenthesis and no `=` (`public List<String> names(int count`)
   declares a method and gets a block, unless its first word is one of `notDeclarations`
   (`return`, `throw`, `new`...).

## Consequences

- Java and Kotlin bring their templates, postfix templates, surrounders, stubs and rules as data
  in their `plugin.json`, with no code of their own; the hard-coded Java completion snippets go.
- These fields need an IDE with this change: an older IDE leaves a plug-in's surrounders out and,
  reading a `description` object instead of a string, leaves that template out. A plug-in that
  uses them says which IDE it needs in its changelog. The release server keeps them only once it
  runs the `zen-releases` of this change (its `liveTemplates` is the platform's
  `LiveTemplateDeclarations`).
- Texts chosen per language are taken when the plug-ins are read, so after the interface language
  changes they follow when the plug-ins are read again (the next start or plug-in change).

## Alternatives considered

- **Fluent files in a plug-in**, registered when it loads: the same as the platform's texts, but
  a WebAssembly plug-in has no files of its own beside `plugin.json` today, and the release server
  would have to carry them.
- **Live templates with `$SELECTION$` instead of surrounders:** they work already, but are listed
  after the region comments by abbreviation, unlike a language's own constructs.
