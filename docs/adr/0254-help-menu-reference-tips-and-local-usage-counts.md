# ADR-0254: What's New, the shortcuts reference, tips and local usage counts

- **Status:** Proposed
- **Date:** 2026-11-05

## Context

The Help menu gets What's New, Keyboard Shortcuts PDF, Tip of the Day and My
Productivity. The release notes are only in the signed manifest, held in memory; a static PDF of
shortcuts would not follow the user's keymap; My Productivity needs counts of what
the person does, and rule 10 of AGENTS.md allows that only if nothing leaves the machine.

## Decision

1. **Release notes are kept.** When the updater has a verified manifest, the notes of the product are
   written to `release-notes.json` in the data folder. What's New reads only that file, so it needs
   no network. A page `release-notes/<product>-<version>.md` is written and opened as an editor tab.
2. **Once after an update.** `whats-new-shown` holds the last version shown. A first start shows
   nothing. A newer version opens What's New in the first project window, unless
   `whatsNewOnUpdate` is off; a version without notes yet is tried again at the next start.
3. **The shortcuts reference is generated**, from the main menu's structure and the active keymap,
   the rest of the bound actions after the menus under their keymap groups. PDF and HTML come from one model.
4. **PDF without a PDF library.** A small writer embeds the bundled Inter font, which has the
   Cyrillic letters and the key symbols, through `ttf-parser` (in `Cargo.lock` already, pure Rust)
   and `flate2`. PDF is simple enough at this size, and no library was needed for one fixed layout.
5. **Tips** are Fluent messages with the shortcut as an argument, each tied to an action. **Off on
   startup by default**: a dialog nobody asked for is not shown unless the person turns it on.
6. **Usage counts are local.** `productivity.json` in the data folder holds, per action, how many
   times a person ran it and when last. It is written at most every 30 seconds and at exit, never
   read by anything but this dialog, not part of Export Settings, and cleared by Reset. What an agent
   runs is not counted.

## Consequences

- `ttf-parser` and `flate2` become direct dependencies of `zen-ui`.
- The cached page is rewritten from `release-notes.json` at every opening, so changing the tab
  changes nothing lasting.
