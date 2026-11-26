# IDE main window

> **Last updated:** 2026-11-26 · Implemented in `crates/platform/ui/src/ide.rs`.

Measurements are in logical pixels, for a 1512 × 950 window in the Zen Dark theme.

## Layout

| Element | Specification |
|---|---|
| Main toolbar | Shares the title bar with the window buttons; ~40 high. Left: window buttons, back, forward, project widget (badge with initials + name + ▾), VCS widget (branch icon + branch + ▾). Right: run (green), debug (green), ⋮, then search and settings. The run configuration and build widgets are not built yet. |
| Tool window stripes | Left and right, ~40 wide, no border. Left top: Project (active: blue rounded square), Commit, Structure, ⋯. Left bottom, from the top: Find, Debug, Run, Terminal, Problems, Git. Right: the Git workspace, Notifications, Database (the last two in development). Every tool window's button can be dragged to another stripe or side (docs/guide/22-tool-windows.md). |
| Islands | Tool windows and the editor are rounded panels (~10 radius) on a darker window background, ~5 apart; no separator lines. |
| Project tool window header | "Project ▾" on the left; locate, collapse, ⋮, hide on the right; ~36 high. + and expand come later. |
| Project tree | Rows ~24 high, indent ~18. The root shows the folder name in bold and the path relative to `~` in grey. Excluded folders (`target`, `node_modules`, `dist`, `build`, `vendor`) are orange with an orange-tinted row. Hidden: `.git`, `.DS_Store`. |
| Editor tabs | ~38 high; each tab: file icon, name, × on the active one; the active tab is outlined (1 px, rounded ~7), not filled. |
| Editor | the bundled editor font at 13, rows 22 pt (44 px at 2x); the gutter: right-aligned numbers ending 28.6 pt from its left edge, 30 pt of room for icons, the fold arrows (› 5×8 pt, ⌄ 8×5 pt; expanded ones only while the pointer is over the gutter), a 1 pt separator 12.25 pt after them, the text 1.5 pt after it; a breakpoint is a 14 pt dot over the line number; the first row right under the tabs; current line slightly lighter (`metrics::gutter`, `LINE_HEIGHT`). An inspections widget at the top right is not built yet. |
| Status bar | ~28 high, no border. Left: the navigation bar (project › path › file). Right: `line:column`, `LF`, `UTF-8`, indent (`4 spaces`), platform. |

## Colours (Dark, approximate)

| Role | Value |
|---|---|
| Window background (between islands, toolbar, stripes, status bar) | `#25282C` |
| Island (tool windows, editor) — darker than the window | `#191A1D` |
| Main toolbar glow in the project colour, strongest at the project widget | e.g. `#304852` fading to `#25282C` by the middle |
| Active stripe button | `#3771E1`, white icon |
| Current line | `#1F2024` |
| Excluded folder row | `#291F1C` |
| Text | `#DFE1E5` |
| Secondary text | `#868A91` |
| Line numbers | `#4B5059` |
| Selection (tree row, active stripe button) | `#2E436E` |
| Excluded folder text | `#D88A3C` |
| Keyword | `#CF8E6D` |
| String | `#6AAB73` |
| Number | `#2AACB8` |
| Comment | `#7A7E85` |
| Function / macro | `#56A8F5` |
| Constant (italic) | `#C77DBB` |

## Behaviour

- Projects open as tabs of the main window (or in windows of their own, by a setting). Opening a
  project that is already open shows it.
- The IDE comes back as it was left (`workspace.json`): per project the open files and the active
  one, the split editors, expanded folders, the layout of the tool windows (each one's side, order on its stripe, size
  and whether it is shown) and the Terminal with its tabs (names, splits, folders); the main
  window's full screen, maximised state, position and size; the active project tab.
- The projects that were open when the IDE quit open again at the next start; the welcome screen
  lists recent projects (most recent first).
- Dragging the empty part of the main toolbar moves the window.
- ⌘1 shows or hides the Project tool window; File → Open… opens a project (no shortcut: ⌘O is Go
  to Class).
