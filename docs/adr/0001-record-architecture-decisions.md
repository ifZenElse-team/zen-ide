# ADR-0001: Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

Zen IDE is a long-lived project. Most of its code will be written with AI agents, by people who
did not take part in the early discussions. Agents and new contributors need the reasons behind
the architecture, not only its current shape, or they will reopen settled questions and undo
decisions without knowing it.

## Decision

Every significant decision is recorded as an Architecture Decision Record in `docs/adr/`, using
`0000-template.md`. Records are numbered sequentially. An accepted record is never edited, except
to fix typos or to mark it as superseded; changing a decision means writing a new record that
supersedes the old one. The index in `docs/README.md` lists every record and its status.

A decision is significant if it affects the architecture or layering, the dependency policy,
licensing, file formats, public behaviour, the release process or the project's conventions.

## Consequences

- Agents are told (in `AGENTS.md`) to read the relevant records before changing an area, and to
  propose a new record instead of silently deviating from an accepted one.
- Writing a record costs a little time on every significant change.

## Alternatives considered

- **Decisions in issues and chat only:** they get lost and are invisible to agents working from
  the repository.
- **One large design document:** it is rewritten over time, so the history of reasons disappears.
