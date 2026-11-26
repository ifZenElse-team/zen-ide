# Presentation, Distraction Free and Zen modes, Zoom IDE

**View → Appearance** leaves the editor alone in the window, for showing code to others or for
writing without anything around it, and turns full screen on and off.

| Item | What it does |
|---|---|
| **Enter Presentation Mode** | The editor fills the screen in a large font; everything else is hidden |
| **Enter Distraction Free Mode** | The editor alone in the window, its text centred, without line numbers |
| **Enter Full Screen** (⌃⌘F) | The window takes the whole screen |
| **Enter Zen Mode** | Distraction Free Mode in full screen |
| **Zoom IDE (Current: 100%)…** | Opens the Zoom IDE dialog, which scales the whole IDE; see [Zoom IDE](#zoom-ide) |
| **Navigation Bar** › **Top** / **In Status Bar** / **Don't Show**, **Show Members** | Where the [navigation bar](03-navigation.md#navigation-bar) is, and whether it shows the declarations at the caret |
| **Show Code Authors** (⌃⌥A) | Shows or hides who wrote each declaration above it ([Code Vision](02-editor.md#code-vision)); checked while shown |

While a mode is on, its item reads **Exit …**: choose it again to leave. **Escape** leaves the mode
too, when nothing else takes it: Escape first closes a popup, a dialog or the search bar, or removes
the other carets, and only an Escape pressed in the editor with none of these open leaves the mode.

In every mode the toolbar, the navigation bar, the tool window stripes, the status bar, the editor
tabs and the breadcrumbs are hidden, and so are the tool windows; ⌘↑ shows the navigation bar over
the editor. Their shortcuts still work: ⌘1 shows the Project
tool window, ⌘6 Problems, and so on, while the mode stays on; the Branches popup opens at the top of
the screen. When you leave the mode, the tool
windows come back as they were before it; the layout GoGarden remembers for the next start is that
one too, and a mode is never on when GoGarden starts.

- **Presentation Mode** goes full screen and draws the editor's text at the size set in Settings →
  Appearance & Behavior → **Presentation Mode** → **Font size** (24 points by default). The editor's
  own font size does not change.
- **Distraction Free Mode** keeps the window as it is. The text is centred in a column 120
  characters wide; on a narrower window the editor takes the whole width.
- **Zen Mode** is Distraction Free Mode and full screen together. Choosing one mode while another
  is on switches to it.

A mode that turned full screen on turns it off when you leave it; a window that was full screen
before stays full screen. Turning full screen off yourself while a mode is on keeps the mode.

On macOS, **Enter Full Screen** with ⌃⌘F is the system's own item, the same as the window's green
button: macOS names it in the system's language and switches it to **Exit Full Screen**. Give Full
Screen another shortcut in Settings → Keymap, and the item becomes GoGarden's, with that shortcut.

Double-clicking the window's title bar — the toolbar's empty space, or the strip at the top of the
Welcome window or of a mode — does what macOS is set to do in System Settings → Desktop & Dock,
"Double-click a window's title bar to": **Zoom** (or **Fill**) makes the window fill the screen
and a second double-click puts it back, **Minimize** puts it in the Dock, **Do Nothing** does
nothing. Dragging that space moves the window.

None of these items has a shortcut except Full Screen; give them one in Settings → Keymap (group
Window), or run them from Find Action (⇧⌘A) by name: Toggle Presentation Mode, Toggle Distraction
Free Mode, Toggle Zen Mode, Toggle Full Screen Mode.

AI agents run them as `view.presentationMode.toggle`, `view.distractionFreeMode.toggle`,
`view.zenMode.toggle` and `view.fullScreen.toggle` with `run_action`, and `get_window_state` reports
the mode as `appearance` and the full screen as `fullScreen`.

## Zoom IDE

**View → Appearance → Zoom IDE (Current: 100%)…** scales the whole IDE at once: menus, tool
windows, dialogs, the status bar and the editor's text all grow or shrink together. The item shows
the zoom in use. It opens a small dialog:

- **−** and **+** step to the next smaller or larger zoom of 70 %, 80 %, 90 %, 100 %, 110 %, 125 %,
  150 %, 175 % and 200 %; the change shows at once.
- **Reset** goes back to 100 %.
- **Close** (or ⏎, or ⎋) closes the dialog and keeps the zoom.

The zoom is separate from the UI font size (Settings → Appearance & Behavior → **Use custom font**)
and from the editor's font size (Editor → Font): it works on top of both, so a larger UI font zoomed
to 150 % is 150 % of that font, and the editor keeps its own size relative to the rest. The zoom is
saved for the IDE (GoGarden, RustRonin, … each keep their own) and comes back at the next start.
Settings → Appearance & Behavior → **Accessibility** → **Zoom** sets it too, with any value from 50 to
300 %.

**Zoom In**, **Zoom Out** and **Reset** are actions of their own with no shortcut in the macOS
keymap: run them from Find Action (⇧⌘A) or give them shortcuts in Settings →
Keymap (group Window). A JetBrains keymap imported in Settings → Keymap brings its shortcuts for them.

AI agents run `view.ideZoom.in`, `view.ideZoom.out`, `view.ideZoom.reset` and `view.ideZoom.open` with
`run_action` (each answers the zoom in percent), set any value with `set_settings` and `ideZoom`, and
`get_window_state` reports it as `ideZoom` and the dialog as the popup `zoomIde`.

## Quick Switch Scheme

**⌃`** (or **View → Quick Switch Scheme…**, or Find Action) opens the **Switch** popup in the middle
of the window, to change how the IDE looks and behaves without opening Settings:

| Number | Row | Lists |
|---|---|---|
| 1 | **Theme** | The themes of Settings → Appearance & Behavior, built-in first, the theme in use selected |
| 2 | **Editor Color Scheme** | The themes again: in Zen IDE the theme gives the editor its colours too; the one in use reads *Theme default* |
| 3 | **Code Style Scheme** | **<Project>** (the code style the project keeps in `.idea`, where it sets one) and **Default** (your own Editor → Code Style) |
| 4 | **Keymap** | The keymaps of Settings → Keymap, then **Configure Keymap…**, which opens that page |
| 5 | **View Mode** | Enter or Exit Presentation Mode, Distraction Free Mode, Full Screen and Zen Mode |
| 6 | **Zoom** | The IDE zoom from 70% to 200%, the zoom in use selected (rows without numbers) |

Press a row's number, or move with **↑** and **↓** and press **Enter**, or click it: its list takes
the popup's place under its own title. In Editor Color Scheme, Code Style Scheme and Keymap an
arrow marks what is in use and the row after it is selected. In the list, a row's number or
**Enter** or a click chooses at once and closes the popup; **Escape** closes it. Type to filter the
rows: a search field appears at the top, and a name typed with the other keyboard layout on (Russian
or English) is found too; **Backspace** edits the filter and **Escape** clears it. Pressing ⌃` again
while the popup is open does nothing. A choice is made exactly as in Settings and the View menu,
and saved the same way.

AI agents open the popup with `run_action` `view.quickSwitchScheme.open`, which answers every row
with its list, the choice in use and what each runs (`ui.theme.select`, `ui.keymap.select`, an
action such as a View Mode one, or `set_settings` with `codeStyleScheme` or `ideZoom`);
`get_window_state` names the popup `quickSwitchScheme`.

## Menus and Toolbars

**Settings → Appearance & Behavior → Menus and Toolbars** shows the Main Menu with its menus, the
Main Toolbar and the popup menus as a tree, with a search field: Editor, Editor Gutter, Editor Tab,
Project View, Scratches, Terminal, Git, Commit, Shelf, Debug, Database, Find Results, Markdown
Preview, Bookmarks and Tool Window Button. Select a group or an item, then:

- **Add…** picks an action from the list, with a search field, or a separator, or brings back a
  built-in item you removed, and puts it after the selected item, or at the end of the selected
  group.
- **Edit Icon** gives the selected action one of the built-in icons; without one, a toolbar button
  shows the first letter of the action's title.
- **Move Up** and **Move Down** reorder, **Remove** removes.
- **Restore** brings back the selected group's defaults, and **Restore All** those of every group.

**Apply** or **OK** applies the changes without a restart. On the Main Toolbar, the list holds Back,
Forward and what you add; the project, Git and Run widgets, Search and Settings stay in place. In a
popup menu the built-in items can be moved and removed like any other; a submenu moves as a whole, and
an item still shows only where it applies, for example Reformat Code on a file. The menus of bookmark
rows, tool window buttons and workspace tabs differ by what you click, so they list one Built-in
Items entry you can move or remove as a whole, with your own actions around it. A separator shows only
between items that are shown. Any action or command can be added to the main menu, also one that
is not in the Keymap.

Right-click an empty part of the main toolbar for **Add Action to Main Toolbar…** and **Customize
Toolbar…**, which open the same page for the toolbar only.

Customisations are saved in `customization.json` next to `settings.json`, for this IDE; an action id
the IDE does not know is skipped. For agents: the `ui.customization.*` commands, or the setting
`customization` of `set_settings`.
