# ADR-0272: Language assists: completion and declarations from a language module

- **Status:** Proposed
- **Date:** 2026-11-06

## Context

Plug-ins answer completion through `TextFeatures`, and Spring Boot, resource bundles and Ant have their
own branches in the completion and declaration commands. A built-in language module had no way in. Go
needs one: gopls does not complete versions in `go.mod` or the keys of a struct tag. `TextFeatures`
does not fit, because a plug-in that serves a file replaces how the file's words are read, and gopls
must keep answering everywhere else in a Go file.

## Decision

1. `zen_modules::assist::LanguageAssist` is a small trait a module registers with
   `ModuleHost::language_assist`: `handles`, `complete` and `declaration`. Both answers are optional and
   `None` means "not here".
2. An assist answers only at the places it knows. Where it answers completion, the language server is
   not asked, as for the keys of a resource bundle, so the list holds what fits the place and nothing
   else. Everywhere else the file works as before.
3. An assist is synchronous and runs on the UI thread, so it reads what is already known. It never
   starts a program or contacts a server: `go.mod` versions come from what the go.mod checks kept.
4. A declaration is a file, opened at its start. **Go to Declaration** asks the assists before the
   language server.

## Consequences

Completion and navigation reach agents through the commands they already have,
`editor.completion.start` and `navigate.declaration.goTo`. A language with a place its server does not
serve adds an assist instead of a branch in those commands.
