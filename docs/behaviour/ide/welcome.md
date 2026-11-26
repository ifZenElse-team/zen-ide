# IDE welcome window

> **Last updated:** 2026-11-26 · Implemented in `crates/platform/ui/src/ide/welcome.rs`.

| Element | Specification |
|---|---|
| Window | Title "Welcome to <product>", drawn in our own strip (the window shares the title bar); shown when no project is open; closing the last project window returns to it. 960 wide, so that the Russian button labels and the language switch fit in one row. |
| Left column | ~226 wide: product logo, name and version; sections Projects, Customize, Plugins and Learn, each with its pane; the selected section is highlighted in blue; settings gear at the bottom. Remote Development is hidden until it exists ([ADR-0253](../../adr/0253-welcome-sections.md)). The product's Kozō, the family's pixel-art mascot, meditates above the gear while there is room ([ADR-0125](../../adr/0125-kozo-the-family-mascot.md)). |
| Top of the right side | Search field "Search projects" with a magnifier; outlined buttons New Project, Open, Clone Repository. A language switch in the top-right corner, right of the buttons: a globe, the current language's own name and ▾; its menu lists English and Русский and sets Settings → Appearance & Behavior → Language. When the row is too narrow the search shrinks first (to 120), then the buttons move to rows under it. |
| Customize | Color theme with Sync with OS, IDE font, Keymap with Configure…, Accessibility (Support screen readers), Import Settings…, All settings…; applied at once, the settings of Settings → Appearance and Keymap. Support screen readers is shown on and cannot be changed: there is nothing to turn on ([ADR-0253](../../adr/0253-welcome-sections.md)). |
| Plugins | The Settings → Plugins page itself, with its tabs. |
| Learn | The guide's chapters and the keyboard shortcuts page, opened on the website in the browser. |
| Help → Welcome | Brings the welcome window forward while a project is open; when a project shares the main window, in a window of its own. |
| Project list | Rows: initials badge in a colour, name, path relative to `~`, branch with a branch icon; hover shows ⋮ with "Remove from Recent Projects"; the selected row is blue. The search filters by path. |

Initials: words split at `-`, `_`, `.`, spaces and lower-to-upper case
changes; two words give two letters (`DataHub` → `DH`), one word gives one (`tinyapp` → `T`).

## Project widget menu

Open…, Clone Repository…; "Open Projects" (the current one highlighted); "Recent Projects", each
with badge, name and path. Choosing an open project focuses its window, a recent one opens in a new
window.
