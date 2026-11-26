# ADR-0033: Every capability is reachable by AI agents

- **Status:** Accepted
- **Date:** 2026-10-16
- **Refines:** [ADR-0007](0007-everything-is-a-command.md)

## Context

The owner (2026-10-16), calling it the most important requirement: "all the API must be available to
AI". ADR-0007 already makes every user-visible action a command. Two gaps remain: some actions live
above a single session (windows, the list of open and recent projects, plug-in installation, the
terminal), and nothing outside the process can call commands yet.

## Decision

1. **No feature is UI-only.** A change that adds a capability adds its command first, with a typed
   parameter struct, a JSON schema and a description written for agents; the UI dispatches it. A
   review rejects a feature whose effect cannot be reached through a command.
2. **Two scopes of commands.** Session commands act on one project window (`file.*`, `editor.*`, ...,
   in `zen-core`'s registry). Application commands act on the IDE as a whole: open, focus and close
   project windows, list open and recent projects, install plug-ins, open terminals. They use the
   same `Command` trait and ids under `app.*`.
3. **An automation endpoint** (`zen-automation`, L6) exposes both scopes to agents over the Model
   Context Protocol: list the windows, list commands with their schemas, dispatch a command to a
   window or to the application, and read state (open documents, the project tree, diagnostics as
   they arrive). It is local only, off unless enabled, and visible in the status bar while an agent
   is connected (AGENTS.md rule 10).
4. **Definition of done** for any feature includes its command ids in the change description and a
   test that dispatches them headless.

## Consequences

- File operations added with this ADR are commands from the start: `file.fs.newFile`,
  `file.fs.newDirectory`, `file.fs.copyInto`.
- Window management currently lives in `zen_ui::ide::IdeApp`; it moves behind `app.*` commands when
  the automation endpoint is built, which is the next step.
