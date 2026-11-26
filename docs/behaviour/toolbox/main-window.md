# Toolbox: main window

> **Last updated:** 2026-11-26 · **Status:** specified (partly)

Colours are not tracked for Toolbox ([ADR-0016](../../adr/0016-toolbox-colour-scheme.md)).
Sizes below are points (the 2x pixels halved). Labels are given by their English meaning; the
Russian translation lives in the locale files.

## Window behaviour

- The window is a **tray widget**: it has no title bar, drops down from the tray icon (centred under
  it on macOS; above it when the tray is at the bottom of the screen, as on Windows), cannot be
  moved or resized, and hides when it loses focus. A left click on the tray icon toggles it.
- Implemented in `apps/zen-toolbox` and `zen-ui::tray`; on Linux the tray backend does not report the
  icon position, so the window opens at the top right of the screen.

## Structure, top to bottom

1. **Header**
   - Left: the Zen Toolbox mark and its two-line wordmark.
   - Right: an environment selector: a laptop icon, the label *Local* and a chevron that opens a
     dropdown.
   - Far right: the settings button (gear icon) with a small accent dot when there is a
     notification.
2. **Tab bar**
   - Tabs *Tools* and *Projects*. The active tab has bold text and an underline.
   - *Tools* carries a superscript counter: the number of available updates.
   - Right: a search button (magnifier icon).
3. **"Installed" panel**: a rounded panel, lighter than the window background, with a thin
   vertical scrollbar at its right edge when the list overflows.
   - Section header: *Installed* on the left in secondary text, and the link *Update all* on the
     right in the accent colour.
   - **Tool rows**, one per installed tool:
     - product icon, about 44 px, with an optional status dot at its lower right (green: running);
     - name in bold; an optional vendor suffix in secondary text;
     - version line in secondary text: `current → available`;
     - the link *What's new* in the accent colour below the version;
     - on the right, the primary button *Update* and a vertical ellipsis (⋮) menu button.
   - **Row hover:** the row gets a rounded highlight across the panel width, and a drag handle (⠿)
     appears at its left edge: installed tools can be reordered by dragging.
4. **"Available" panel**: a second rounded panel with a collapsible header and a chevron, listing
   the tools that are not installed.

## What's new

- The row link "What's new" appears when an update is available and shows the release notes of
  every release newer than the installed one; on a tool that is not installed it shows every
  release. An up-to-date row has no link and is compact (60 px instead of 76 px).
- The version line carries the download size (`0.38.27 · Update to 0.38.31 · 32 MB`), so the name
  and the version line end 10 px before the Update or Install button, or the ⋮ menu: a version line
  that does not fit wraps at its " · " separators and the row grows by the extra lines; past three
  lines it ends in "…" and the row's hint holds the full text (`crates/platform/ui/src/toolbox/row.rs`).
- The header is laid out before it is drawn: the gear keeps its place at the right edge and the
  environment selector ends 6 px before it, as wide as its name needs (at least 98 px), growing
  left into the free space by the wordmark. A name that does not fit between the wordmark and the
  gear ends in "…" and the selector's hint holds it in full, so the name, the chevron and the gear
  never overlap in any language (`crates/platform/ui/src/toolbox/header.rs`).
- The ⋮ menu starts with "What's new", which shows every release, the installed one marked.
- The settings menu has "What's new in Zen Toolbox". After Toolbox updates itself, the first start of
  the new version shows its own notes once (the last version shown is the `toolbox.whatsNewShown`
  setting in `settings.json`).
- A modal dialog, 340 px of content: the title "What's new in <tool>", then per release, newest
  first, the version (semibold 14), the date (secondary 12.5) and an "installed" tag, then the notes
  rendered from Markdown (headings, lists, paragraphs, code, bold, italic), scrolling past 460 px,
  and a Close button.

## Metrics

- Window 440 × 700 pt, at the top right under the menu bar (x 782, y 38 on a 1512 pt wide screen).
- Header: wordmark at the left; the environment selector 126 × 32 pt; the gear button 32 × 32 pt,
  16 pt from the right edge, with a blue dot when a Toolbox update waits.
- Tab bar: a rounded panel 392 pt wide at y 82; the tabs *Tools* (with a superscript count)
  88 × 36 pt and *Projects* 57 × 36 pt; the search button 24 × 24 pt at the right.
- Installed list: a panel from y 132, 8 pt inside the window; the section header row 24 pt with
  *Update all* 88 × 16 pt on the right; tool rows 81 pt with an update (name, `current → new`,
  *What's new*) and 72 pt without; the *Update* button 81 × 24 pt and the ⋮ button 24 × 24 pt, both
  8 pt from the row top; product icons about 48 pt.
- Available tools: rows 99–113 pt with a description and an *Install* button 91 × 24 pt.
- In the light theme the window keeps a gradient background (pale pink to lilac), panels are white
  with a slight transparency, buttons keep the blue fill.

## Tool menu (⋮ on an installed tool)

A popup menu 188 pt wide, items 32 pt high (168 pt wide inside an 8 pt padding), opening below the
⋮ button and right-aligned with it (above it when there is no room), separators between groups:

- **A tool with an update** (188 × 257 pt): *Settings* · *Other versions* · *Show log files* · — ·
  *Update to <version>* · *Skip <version>* · *Downgrade to <version>* · *Uninstall…* (in red). No
  shortcuts.
- **An up-to-date tool**: *Settings* · *What's new* · *Other versions* · *Show log files* · — ·
  *Uninstall…*.

No item opens a submenu; two open pages in the same window, with a back link "‹ Tools":

- **Other versions** → a page titled "<tool> versions": one row per release, 65 pt high, newest
  first: the version (pre-releases named, for example `Beta`), its date in the middle, the build
  number under the version, an *Install* button 91 × 24 pt and a ⋮ *Actions* button whose menu has
  one item, *What's new*.
- **Settings** → a page titled with the tool's name, tabs *Settings* / *About*:
  - *Install location* with the path and a *Show…* link;
  - toggle *Update the tool automatically*, off;
  - *Update channel*: radio *release* (selected, with a one-line description) and *Early Access
    Program (EAP)* (with a warning icon and a one-line description), then a note that updates go to
    any newer version with the same or a higher stability level;
  - toggle *Stay on <major version>* (no updates past the current version), off;
  - a collapsible *Configuration*: a JVM options field; a radio between one shared JVM settings
    file (selected, the IDE's configuration folder with a copy button) and a separate one per tool;
    *Maximum heap size* 2048 MiB (the default shown); *Edit JVM options…*; the shell script name
    field and its location; *Configure shell scripts…*;
  - the button *Uninstall…* in red outline;
  - a bottom bar while an update waits: *Update* (primary), *Skip*, and on the right
    *What's new in <version>*.
  - The About tab: the tool's icon, name, version and build, *Show log files*, then links to the
    tool's tour, overview, what's new, getting started, help and other versions.

## Tray icon

The status item in the macOS menu bar (24 × 24 pt). A left click toggles the window. A right click
opens a native menu (187 × 58 pt, drawn in the system's appearance, not Toolbox's theme) with two
items and no recent projects: *Open Zen Toolbox* · *Quit Zen Toolbox*. The light Toolbox theme
gives the same dark menu on a dark macOS.

## Projects tab

- One collapsible group per environment (*Local*), then one row per recent project, 59 pt apart: a
  28 pt rounded square with the project's initials on a colour of its own, the name in bold, under
  it the IDE's small icon and the path with `~`; a ⋮ *Actions* button on every row. The search
  button on the tab bar filters.

## Settings menu (the gear)

A popup menu 370 × 299 pt under the gear, items 32 pt high and 350 pt wide, separators between
groups: *Restart to finish the update* ⌘T (with a refresh icon; only when a Toolbox update is
downloaded) · *Check for updates* ⌘R · *Settings* ⌘, · *About* · *Questions and answers ↗*
(browser) · *Quit* ⌘Q. No account items (AGENTS.md rule 10).

## Settings

Settings replaces the list in the same window: a back link "‹ Tools" (or the tab it came from), the
title "Zen Toolbox", tabs *Settings* and *About*, then collapsible sections (headers 24 pt, a
chevron on the left; expanding one keeps the others as they are):

- **Appearance and behaviour**: *Language* ⌄; *Region* ⌄; *Theme*: a segmented control *Light /
  System / Dark* (73 / 89 / 68 × 24 pt); toggles (24 × 16 pt, the label 8 pt to the right, 36 pt
  apart): *Gradient background* on, *Start at login* on, *Update automatically* on; a shortcut field
  to open Zen Toolbox; *Scale* ⌄ 100%. No usage statistics (AGENTS.md rule 10).
- **Tools**: *Update tools automatically* off; the install location with *Change…*; *Keep previous
  versions for rollback* on; *Generate shell scripts* on, with a description and the scripts folder
  with *Change…*; disk usage: *Clean up leftover directories…* with its size, *Clear the download
  cache* with its size, *Total*; *Notify when removable data exceeds a threshold* on, *Threshold* ⌄
  10 GiB.
- **Proxy**, **Plugins**, **Advanced**: not specified yet.

## Not specified yet

- The same window with no updates (row without the Update button).
- The available (not installed) tools section in detail, and an installation in progress.
- What "What's new" opens in place of our dialog, if anything else.
- The environment selector dropdown.
- The Projects tab's search.
- Window size, resizing behaviour, and the window's position relative to the tray icon.
