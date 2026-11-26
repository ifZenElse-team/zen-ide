# ADR-0258: Inspection profiles and Code Cleanup

- **Status:** Accepted
- **Date:** 2026-11-06
- **Relates to:** [ADR-0256](0256-inspect-code-over-a-scope.md) (extended), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

Every inspection ran at the severity it reported, with no way to turn one off, and Inspect Code had
no profile behind its dialog. The requirement (an Inspection profiles page, suppression and Code
Cleanup) is profiles and fixes in bulk. This is its first part: scope-specific severity,
importing a `.idea/inspectionProfiles` profile, suppression and Actions on Save come later.
The Inspections page, its gear menu and the Code Cleanup dialog are not settled in every detail, so
they may change.

## Decision

1. **A profile says only what it changes.** It is a JSON file, the name and a map from inspection id
   to `enabled` and `severity`, either of which may be absent. A new inspection therefore has its
   declared defaults in every existing profile, and what the IDE no longer declares stays in the
   file (`unknown` in `inspect.profile.get`), so a profile is not damaged by an IDE without the
   plug-in that declared it.
2. **Where they live.** Project profiles in `.zen/inspectionProfiles/`, IDE profiles in
   `inspectionProfiles` of the IDE's data folder. Project Default always exists and needs no file; it
   cannot be renamed or deleted. The profile that applies is chosen per project, in
   `.zen/inspectionProfiles/profiles_settings.json`, and when it is gone Project Default takes over.
   Export writes the same JSON, not the XML of `.idea/inspectionProfiles`.
3. **One place applies a profile.** `inspections::joined`, through which the editor's highlighting and
   the Problems tool window get their problems, and the end of an Inspect Code run. A problem belongs
   to an inspection by the ids of ADR-0256 (`source/code`, the source or the code), so a plain
   analyzer id such as `simplifyrange` covers gopls's `simplifyrange/default`. A run may name another
   profile. Run Inspection by Name runs what the profile turned off, because it was asked for by name.
4. **Severity is an override, not a declared value.** An inspection declares no severity, since a
   server rates each finding itself, so the page's first choice is "As reported". Hints and information
   are offered; Server Problem, Grammar Error, Consideration and No highlighting are not, because we have
   no such severities.
5. **Analyzers are switched in the server as well as filtered.** A filter alone would leave a server
   doing the work, and could not turn on what is off by default. A server declares
   `inspectionSettings` (section and path, `gopls`/`analyses`) and `off` for what it leaves off. Only
   switches a profile sets are sent. gopls keeps an `analyses` entry it is not told again (seen with
   gopls 0.23.0 on 2026-11-06: a removed entry stayed in force), so a switch that leaves a profile is
   sent once more with its default, and stays sent until the IDE restarts.
6. **Code Cleanup is an Inspect Code run with a different end.** `inspect.cleanup.run` inspects the
   scope, keeps what declares `cleanup` and is enabled, and hands it to the fix queue of ADR-0256
   without opening results. An analyzer switched off by the project's profile finds nothing even if
   the profile asked for has it on, as the server has one configuration.
7. **One undoable change per file.** The queue takes each fix's journal entry back and merges a file's
   entries with `edits::journal` when the batch ends, which merges the undo steps of an open document
   as a multi-step context action does, so ⌘Z takes back the whole file. A file that is not open has no
   Undo in an editor, so the notification offers Undo and `inspect.cleanup.undo` takes back the last
   cleanup, by file or whole, leaving alone a file changed since. An inspector's own fix is a job of
   ADR-0059 that writes files, with no Undo; no inspector declares `cleanup` yet.

## Consequences

- A cleanup is as slow as ADR-0256's fixes, at least `diagnosticsSettleMs` per fix, and can be cancelled;
  what was fixed by then stays, merged the same way.
- If someone types in an open document during a cleanup, its undo steps are not next to each other any
  more and the merge can take that typing with it.
- The settings page saves changes on Apply, but the gear's actions take effect at once, after saving
  what was changed so far.
