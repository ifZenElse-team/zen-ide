# ADR-0024: Localisation with Project Fluent, English and Russian from the start

- **Status:** Accepted
- **Date:** 2026-10-16
- **Refines:** [ADR-0009](0009-english-only.md) ("user-facing text ... localised through the localisation system")

## Context

ADR-0009 keeps everything in the repository in English and requires user-facing text to go through
a localisation system. The owner asked to build that system now, while the interface is small, so
that no feature is written with hard-coded text.

## Decision

1. **Format: [Project Fluent](https://projectfluent.org)** (`.ftl`), through the `fluent-bundle` crate
   (MIT OR Apache-2.0, pure Rust). Fluent handles plural forms and grammatical variants per language,
   which Russian needs ("1 обновление, 3 обновления, 5 обновлений").
2. **`zen-i18n`** (L0) owns the mechanism: crates register their messages at start-up; code asks for
   text with `t!("message-id", name = value)`; lookup goes current language → English → the id.
   Unicode isolation marks are disabled because the UI renders them as boxes.
3. **Files:** `<crate>/locales/<language>/<crate>.ftl`, compiled into the binary. **English is the
   source**; message ids are kebab-case and start with the area (`toolbox-…`).
4. **Languages from the start: English and Russian** (`zen_i18n::LANGUAGES`). A change that adds or
   changes user-facing text updates every language in the same commit.
5. **The language follows the system** unless the user picks one; the choice is the setting
   `ui.language` (`system`, `en`, `ru`). Language names are shown in their own language.
6. **`cargo xtask i18n`** (part of `cargo xtask ci`) fails if a translation lacks a message, has an
   extra one, or uses different variables than the English source.
7. Not translated: product names, command ids, log output, and text in the repository (ADR-0009).

## Consequences

- No user-visible string literal in code; reviewers and agents check for `t!`.
- Adding a language means adding it to `LANGUAGES` and a directory per locale folder; the check then
  lists every missing message.

## Alternatives considered

- **gettext (`.po`):** widely known, but weaker plural and grammar handling and C tooling.
- **Plain key-value JSON:** no plurals or grammatical cases; would be outgrown at once.
