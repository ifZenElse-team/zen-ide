# ADR-0185: Every application speaks English until the person chooses another language

- **Status:** Proposed
- **Date:** 2026-10-28
- **Supersedes:** decision 5 of [ADR-0024](0024-localisation.md) ("the language follows the
  system unless the user picks one"), and the default of decision 6 of
  [ADR-0042](0042-zen-cli-output-and-exit-codes.md) (an unset `ZEN_LANGUAGE` follows the system)

## Context

The IDEs, Zen Toolbox and `zen-cli` took the system's language when the person had chosen none, so
a Mac set to Russian opened them in Russian. The owner, 2026-10-28: every application starts in
English by default; Russian is an option the person picks, and once picked it is remembered.

## Decision

1. **English is the default.** `zen_i18n::set_language(None)` is English, whatever the system's
   language; an unknown code is English too.
2. **Following the system is a choice.** `zen_i18n::SYSTEM` (`system`) follows the system's language
   when the platform is translated into it, English otherwise. It is offered in the pickers next to
   the languages, never chosen by default.
3. **The choice is remembered** where each application keeps its settings: the IDE's
   `settings.json` (`language`: a code, `system`, or absent for English), Zen Toolbox's settings
   (`ui.language`, `en` when absent), and `ZEN_LANGUAGE` for `zen-cli` (`en`, `ru`, `system`; unset
   is English). Updates keep the settings files, so the choice survives them.
4. **Agents set it too:** the IDE's `set_settings { language }` (a code, `system`, or null for
   English) and Zen Toolbox's `toolbox.settings.set { language }`.

## Consequences

- A person who never chose a language and whose system speaks Russian now sees English; choosing
  Русский (or System) once brings Russian back for good.
- A person who chose a language before keeps it. One who had picked the IDE's "System" entry kept no
  value for it (it was the absence of a choice), so that person sees English until choosing again.
- Tests that set no language run in English, whatever the machine's locale.

## Alternatives considered

- **Follow the system, as before.** The owner wants one default language for every user and
  machine.
- **Ask on first start.** One more dialog before the first window; the setting is one click away.
