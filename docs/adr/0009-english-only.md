# ADR-0009: English only

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

The project will be public and international, and most of its code is written with AI agents. A
single language keeps the repository searchable, reviewable by anyone, and consistent for agents.

## Decision

Everything in the repository is written in English: code, identifiers, comments, documentation,
commit messages, change-request and issue descriptions, and log and error messages. User-facing text
in the product is written in English and localised through the localisation system, never
hard-coded in another language.

Discussion outside the repository (chat, calls) may be in any language; its outcome is recorded in
English.

## Consequences

- Contributors and agents must write in English even when the conversation that led to a change
  was in another language.

## Alternatives considered

- **Mixed languages:** convenient for the founding team, but a barrier to every future contributor.
