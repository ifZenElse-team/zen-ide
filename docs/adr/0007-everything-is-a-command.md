# ADR-0007: Everything is a command

- **Status:** Accepted
- **Date:** 2026-10-16

## Context

An IDE has thousands of actions reachable from menus, shortcuts, the action search, context menus
and toolbars. The project also wants every action to be scriptable and available to AI agents, and
it is developed largely by agents, which work best with a uniform, discoverable surface.

## Decision

Every user-visible action is a command registered in `zen-core`, with:

- a stable id (`<area>.<group>.<action>`, for example `editor.caret.moveToLineEnd`),
- a typed parameter struct that serialises to JSON, with a schema,
- a title, a description, an optional default shortcut per keymap, and an availability check.

Menus, toolbars, keymaps, the action search, macros, `zen-cli`, the MCP server and plug-ins dispatch
commands by id. They never call feature code directly. Command ids are part of the public interface:
renaming one requires a deprecation alias.

## Consequences

- Any action can be tested headless, recorded, replayed, bound to a key or called by an agent.
- Adding a feature always includes registering its commands, which is a small, predictable cost.
- The list of commands doubles as a progress measure of the feature surface.

## Alternatives considered

- **Direct calls from the UI to features:** less ceremony, but every other client (CLI, MCP,
  plug-ins, tests) would need its own integration.
