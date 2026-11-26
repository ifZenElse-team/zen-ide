# ADR-0260: Menus and Toolbars customisation

- **Status:** Proposed
- **Date:** 2026-11-06
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The main menu is a static table, the toolbar is drawn in code, and the context menus are closures
with items that depend on what was clicked and carry parameters. Users must be able to add, remove
and reorder actions, and agents must be able to do the same.

## Decision

1. **A customisation is a whole list per group**, `customization.json` next to `settings.json`:
   `groups` maps a group id to its items, and `icons` maps an action to an icon. An item is an action
   or command id, `-`, `slot:<built-in item>`, `submenu:<menu key>`, `special:new` or
   `special:layouts`. A group without a
   list keeps its defaults, so Restore only removes the list. An id the IDE does not know is skipped
   when a menu is built, and stays in the file.
2. **It travels with the settings.** `IdeSettings.customization` goes through the dialog's draft,
   Apply and OK and `set_settings`, so changes need no restart; only saving splits it off into its
   own file. The `ui.customization.*` commands (`list`, `add`, `remove`, `move`, `restore`, `setIcon`)
   give agents one typed step at a time (ADR-0007): the setting alone replaces a whole list.
3. **A popup menu is a list of slots.** A built-in item depends on the click and carries its
   parameters, so it stays code; the list names it as `slot:<name>`, and the menu's code draws a slot
   when the list reaches it. Moving and removing then need no data form of the item. A slot also says
   whether it applies to this click, and a separator shows only between items that are shown. A
   submenu is one slot: its inner items do not move. Popup menus are per list, so the Project view
   and scratches, and database connections and objects, have a group each. Menus whose whole content
   changes with the click (bookmark rows, tool window buttons, workspace tabs) are one `Built-in
   Items` slot. Popups inside dialogs (Keymap, Breakpoints) are not groups: the dialog is modal, and
   an added action could not act on the IDE behind it.
4. **The main toolbar's widgets stay.** Its list covers Back, Forward and added actions; the project,
   Git and Run widgets, Search and Settings are drawn apart, anchored to the two ends of the bar and
   each with a popup of its own.
5. **What an agent adds runs as the agent's.** An item an agent adds, with `ui.customization.add` or
   a whole list through `set_settings`, is kept in `fromAgents`; when the person clicks it, it runs
   as the agent would, so a command agents may not run (`project.trust.grant`, say) stays refused
   and an agent cannot place it for the person to press. The person adding the same item clears the mark.
6. **Icons come from a short built-in list**, the IDE's own glyphs; an action without one shows the
   first letter of its title. Icon files are not loaded.

## Consequences

- A new context menu joins by naming its slots in `POPUPS` and drawing them through
  `customization::popup`; a built-in item whose title needs arguments gets a plain title of its own.
- Other groups (Navigation Bar, Run and VCS toolbars) can be added as their code gets
  such lists.
- A command without a keymap action shows in the native main menu with no shortcut and runs as a
  command.
