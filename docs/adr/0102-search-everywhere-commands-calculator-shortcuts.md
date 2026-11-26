# ADR-0102: Search Everywhere's `/` commands, calculator, and shortcuts, abbreviations and synonyms of actions

- **Status:** Proposed
- **Date:** 2026-10-22
- **Amends:** [ADR-0081](0081-search-everywhere-contributors.md), [ADR-0095](0095-search-everywhere-toolbar.md)
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md) (AI access),
  [ADR-0038](0038-keymap.md) (keymap)

## Context

Search Everywhere is to be the one entry point of the IDE: its field says "Type / to see
commands", `/` lists the groups of Settings and `/plugins` the plug-ins with a switch each; an
arithmetic expression shows its value; ⌥⏎ on an action assigns it a shortcut; and Find Action
matches an action's abbreviations (Settings → Keymap → Add Abbreviation) and synonyms, not only its
name. Our popup had none of it, our keymap
held only its own actions, so a command found in Find Action could not get a shortcut, and nothing
kept abbreviations.

## Decision

1. **Three more kinds of result.** `zen_modules::search::SearchTarget` gets `settings { page }`
   (choosing it runs `view.settings.open { page }`), `switch { on, command, params }` (choosing it
   dispatches the command, which switches the thing to the other state, and the popup stays open
   and searches again; only Enter on the row shown or a click does, never an Enter pressed before
   the results came, which opens the first result of other kinds) and `value { value }` (choosing it copies the value and closes the popup).
2. **`/` commands are a contributor of their own.** A contributor says it answers commands
   (`SearchContributor::commands`); on All, a query that starts with `/` is searched by those
   contributors only, with the limit of a tab (50) instead of 6, and they are not searched for any
   other query nor listed in the Filter. The window's Commands contributor (`commands`, in
   `zen-ui`, where the Settings tree lives) lists the top-level groups of the Settings tree as
   `/<name>` (`/vcs` for Version Control) for `/` and the start of a name, and for `/plugins` also
   the built-in modules (`plugins.modules.list`) and the installed plug-ins (`plugins.installed.list`),
   the rest of the query filtering them, each a switch over `plugins.module.*` or `plugins.plugin.*`.
3. **The calculator is a platform contributor** (`calculator`, weight 20, All only, in the
   Filter): `zen_workbench::calculator::evaluate` reads numbers, `+ - * / %`, `^` or `**`, brackets
   and signs, needs at least one operation and a finite value, and shows at most 12 digits. It
   gives up past 64 levels of brackets, signs or powers, so a pasted text cannot exhaust the
   stack. Where the interface's language writes a decimal comma
   (`workbench-calculator-decimal-separator`), `1,5` is a number too and the value of an
   expression written with a comma is shown with one; in English a comma is not a number.
4. **A command can have a shortcut.** A keymap's user changes may name an action the keymap lacks;
   `Keymap::bindings` lists it as a global binding that runs the command of that name, and the
   window resolves it to what choosing the command in Find Action runs (its window action, or a
   step's parameters). `ui.keymap.setShortcut { action, keystrokes? }` sets keystrokes for any
   action, and `set_keymap` accepts the commands `find_action` lists. ⌥⏎ on an action row opens the
   Keyboard Shortcut dialog of Settings → Keymap over the popup (popup `keyboardShortcut`), which
   takes every key; the popup takes ⌥⏎ before the window's Show Context Actions.
5. **Abbreviations belong to no keymap**: `abbreviations.json` in the data
   folder, kept by the keymap store, read with `ui.keymap.abbreviations` and changed with
   `ui.keymap.addAbbreviation` and `ui.keymap.removeAbbreviation`; Settings → Keymap adds and
   removes them from an action's context menu and shows them beside its shortcuts. An action whose
   abbreviation is the query comes first in Find Action, one whose abbreviation starts with it next.
6. **Synonyms are messages.** `synonyms-<action>` in `synonyms.ftl` lists an action's synonyms,
   comma-separated, in each language; Find Action matches them in the interface's language and in
   English when the name does not match, ranks such a match a little below a name match of the same
   kind, and shows the synonym matched as the row's detail.

## Consequences

- `SearchTarget` is matched exhaustively in fewer places: code that only knows files and places
  treats the new kinds as having no file.
- A stale action in a keymap file (a command that no longer exists) is listed by `get_keymap` and
  Settings → Keymap under its id, and its shortcut does nothing.
- The synonyms are a small table of our own; a plug-in cannot add any yet. `/` lists groups only:
  the options inside a group (`/appearance` with its switches) are not listed yet.

## Alternatives considered

- **Settings rows as `action` targets.** Running `view.settings.open` as an action would have
  needed no new kind, but it would be remembered as a recent action and rank Settings… first in
  Find Action.
- **Abbreviations in each keymap file.** It would change the keymap file format and lose them on
  switching keymaps.
- **A registry of extra bindings filled by the UI.** Resolving a command's shortcut where the keys
  are handled keeps `zen-core` free of the window's actions and needs no second list.
