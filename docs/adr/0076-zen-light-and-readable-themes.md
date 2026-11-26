# ADR-0076: Zen Light, light bases and readable imported themes

- **Status:** Accepted
- **Date:** 2026-10-20
- **Supersedes:** the light-theme fallback of [ADR-0045](0045-themes.md) (point 4, "Light themes
  fall back to Zen Dark too until a built-in light theme exists", and the matching consequence);
  the rest of ADR-0045 stands
- **Relates to:** [ADR-0033](0033-ai-access-to-every-api.md)

## Context

ADR-0045 made Zen Dark the base below every theme until a built-in light theme existed. The IDE
needs a light theme as well as a dark one and has to follow the system's appearance.
Light themes imported from VS Code and JetBrains IDEs set few keys and fell back to Zen Dark: the
review of 2026-10-18 found a dark current-line band on a light editor, a dark project tree and the
run widget's text and the active terminal tab nearly invisible. A synthetic VS Code light theme
imported only four colours and left its text unreadable, and VS Code has no colour for a modified
diff line.

## Decision

1. **Zen Light is built in** (`zen-light`), next to Zen Dark, with every key set: the window, the
   editor, the code styles, the diff, file status and problem colours and the 16 ANSI colours. Its
   values are pinned key by key in a test and written out in `docs/themes/zen-light.json`.
2. **The base follows the appearance.** Below the last parent of a chain is Zen Light when the
   farthest theme of the chain is light (`"dark": false`), Zen Dark otherwise. Importers give a
   light theme `zen-light` as its parent and a dark one `zen-dark`, and code styles without a
   colour take the base theme's.
3. **Readability is checked.** `zen_theme::PAIRS` names every text colour and the background the
   IDE draws it on, with the least WCAG 2 contrast ratio it needs (4.5 for body text, 3.0 for
   coloured labels and marks, 2.0 for line numbers). Both built-in themes meet every pair, which
   their tests check.
4. **Imported themes are made readable.** When a theme of another editor is imported, every pair it
   fails is mended: on a surface (window, islands, popups, fields, editor, terminal) the text moves
   towards black or white, keeping its hue; on any other background (selection, hover, diff line,
   badge) the background moves away from the text. The import reports the keys it changed
   (`adjusted`). A theme written in the Zen format is taken as it is.
5. **VS Code diff colours are derived.** The diff colours a VS Code theme leaves out are tints of
   its gutter change markers; `diffEditor.diagonalFill` becomes the filler.
6. **The theme can follow the system.** `ui.theme.followSystem` (Settings → Appearance → Sync with
   OS, MCP `follow_system_theme`) uses one theme while the system is dark and another while it is
   light, switching without a restart; choosing a theme while following sets the one of its
   appearance. The choice is saved as `themeFollowSystem`, `themeDark` and `themeLight`.

## Consequences

- A light theme needs to set only the colours it changes; nothing dark shows through.
- Imported colours may differ from the source file where the source was unreadable; the report
  names them, and a user who wants the original colours can edit the theme file.
- Themes imported before this change keep `zen-dark` as their parent; importing them again gives
  them the light base and the readability fixes.
- Every new colour key needs a value in Zen Light too, and a pair in `PAIRS` when text is drawn on it.
