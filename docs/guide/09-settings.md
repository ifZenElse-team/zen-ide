# Settings

**⌘,** (or the gear in the toolbar) opens Settings. Changes apply with **OK** or **Apply**, except
on the Plugins page and for the theme, which apply at once. While Settings is open, the file under it
does not take typing; closing Settings gives the keyboard back to the editor.

Every page fits the dialog, down to its smallest size: a page longer than the dialog scrolls above
the OK, Cancel and Apply buttons, and on a narrow dialog a row that does not fit wraps (Sync with OS
goes under the theme list), long choices such as a GOROOT path end in "…", and a table wider than
the page scrolls sideways inside it.

If the settings file cannot be read (it is damaged, or edited by hand with a mistake), GoGarden
keeps it next to the original as `settings.json.corrupt`, starts with the default settings and tells
you where the copy is. If the settings cannot be saved, a notification says why.

## Search

The field above the list of sections (**⌘F** moves to it) searches the settings as you type. It looks
through the names of the sections and the texts on their pages, in the language of the interface.
Sections that do not match are hidden, the sections above a match stay in grey, and the
matching part of each label is marked. If the page you are on does not match, Settings shows the
first section whose name contains the text, else the first section whose own texts match (a page
with an option of that name comes before Keymap with an action of that name), else the first
section that matches. **Escape** clears the field first; pressed again, it closes Settings.
**Page Down**, **Page Up**, **Home** and **End** scroll the page, unless a field of the page has
the focus (the search field does not count).

The search also reaches the actions and the plug-ins:

- When the text is part of an action's name, as Keymap shows it or in English (type
  "duplicate line" in a Russian interface too), or one of its words begins a word of an action's
  name, **Keymap** stays in the list and its own search field gets the same text, so the page
  shows the actions with that text (none when only a word matched).
- When the text names an installed plug-in (a part of its name, or one of its file types such
  as `md`), **Plugins** stays in the list and opens on **Installed** with its search set to the
  text.

The page you are on stays while it is in the list, Keymap too. Clearing the field clears the
text it put into those pages' searches; what you typed there yourself stays. Agents search the same
way with `view.settings.open { query }`, which answers what was found.

## Project Structure

The first page of Settings (also **⌘;**): the project's folders as a tree, where you mark a folder
**Excluded** or as a **Resource Root**, and **Exclude files**, the name patterns left out of the
project. See [Project Structure](05-project.md#project-structure).

## Appearance & Behavior

- **Theme**: the colours of the whole IDE; see [Themes](#themes) below.
- **Use custom font** and **Size**: the size of the interface's text, from 8 to 32 points (13 when
  the box is clear). The whole interface grows or shrinks with it, menus and tool windows included;
  the editor keeps its own font size (Editor → Font).
- **Accessibility** → **Zoom**: scales the whole IDE, the editor included, on top of the UI font
  size: choose 70 %, 80 %, 90 %, 100 %, 110 %, 125 %, 150 %, 175 % or 200 % from the list, or type
  any value from 50 to 300 % in **Custom**. **Reset to default**, shown while the zoom is not 100 %,
  goes back to it. The same zoom is in View → Appearance → **Zoom IDE**; see
  [Zoom IDE](21-appearance.md#zoom-ide).
- **Presentation Mode** → **Font size**: the size of the editor's text in Presentation Mode, from 8
  to 96 points (24 by default); see [Presentation, Distraction Free and Zen modes](21-appearance.md).
- **Language** of the interface: English (the default, whatever your system's language), Russian,
  or **System** to follow your system's language. Your choice is remembered across restarts and
  updates. The welcome window's language switch (top right) sets the same setting; agents set it
  with `set_settings` (`language`: `en`, `ru`, `system`, or null for English).
- **System Settings** → **Show What's New after an update** (on by default, see
  [Updates](updates.html)) and **Show tips on startup** (off by default, see
  [Getting started](getting-started.html)).
- **System Settings** → **Confirm before exiting the IDE** (on by default): ⌘Q and **Quit** ask
  first; see [Exiting](getting-started.html#exiting). Turned off, they exit at once.
- **System Settings** → **Open project in**: **New window**, **Current window**, which opens the project
  as a new tab of the main window, or **Ask** each time.
- **System Settings** → **Default project directory**: where the Open dialog starts and where
  [New Project](05-project.md#new-project) proposes the project's folder; `~/Projects` when it is empty.
- **System Settings** → **Project Generators**: **Let project generators use the network** (on by
  default) and each generator's own settings, such as the [Spring Boot](31-spring-boot.md#new-spring-boot-project)
  service URL. With the network off, generators that need a service show their last saved choices and
  cannot create a project.
- **System Settings** → **Files**: **Save files if the IDE is idle for [15] seconds** and **Save
  files when focus moves to another application or a built-in terminal** (both on by default);
  see [Autosave](02-editor.md#autosave).
- **Notifications**: balloons for new notifications, on or off for every group or one by one; see
  [Notifications](notifications.html).
- **Trusted Locations**: every project under the folders listed here is trusted without asking.
  **+** adds a folder, **−** removes the selected one (its projects ask again at their next open)
  and **↑** **↓** reorder the list; **OK** or **Apply** keeps it. Adding your home folder or a folder
  above it shows a warning, as the trust check is then effectively off. See [Trust and safe
  mode](05-project.md#trust-and-safe-mode).

### Themes

The **Theme** list shows the built-in themes first, then yours. GoGarden comes with two:
**Zen Dark**, the default, and **Zen Light**, a light theme for the whole IDE — editor, tool
windows, the Git workspace, diffs and the terminal. Choosing a theme changes the colours at once,
without a restart, so you can look at it before deciding: **OK** or **Apply** keeps it, **Cancel**
(or Escape) goes back to the theme you had when you opened Settings. A theme file with errors is
listed but cannot be chosen; the error is shown under its name.

**Sync with OS**, next to the list, makes the theme follow the system's appearance: Zen Dark while
macOS is dark, Zen Light while it is light, switching the moment the system does. While it is on,
choosing a theme in the list makes it the theme for its own appearance (a light theme for the light
system, a dark one for the dark system); a line under the list names both.

**Load theme file…** imports a theme and switches to it. It reads:

- JetBrains colour schemes (`.icls`, or `.xml` exported from a JetBrains IDE) and UI themes
  (`.theme.json`);
- VS Code colour themes (`.json`);
- Zen IDE themes (`.json`), which set only the colours they change and name the theme they build on
  as their `parent` (Zen Dark when they name none, Zen Light for a light theme).

A light theme from VS Code or a JetBrains IDE takes every colour it does not set from Zen Light, a
dark one from Zen Dark, so nothing dark shows up in a light theme. Text the theme would leave too
faint on its background — grey on white, yellow in a white terminal, dark text on a dark selection —
is made readable on import.

After the import, a line under the list says how many colours were carried over, how many entries
of the file have no counterpart in GoGarden yet, and how many colours were made readable. Imported themes, and themes you write by
hand, are kept in the `themes` folder of GoGarden's data folder (`<data>/themes`); a file you put
there yourself appears in the list the next time Settings opens. The format, with every colour
key, is described in the [theme format reference](../themes.md).

### System Settings → HTTP Proxy

**Settings → Appearance & Behavior → System Settings → HTTP Proxy** is one setting for every network
request of the IDE: updates, the plug-in catalog, the Go module proxy, vulnerability data, tool
downloads and the HTTP Client. `git`, `go` and language servers are separate programs and keep their
own variables.

- **No proxy** connects directly.
- **Auto-detect proxy settings**, the default, uses the proxy variables the IDE was started with
  (`HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`), then the system's proxy on a Mac. **System proxy settings…**
  opens the system's page. **Automatic proxy configuration URL** and a script set in the system are
  not supported: the IDE cannot evaluate them, says so here and in **Check Connection**, and connects
  directly. **Clear Passwords** forgets the proxy password.
- **Manual proxy configuration** takes **HTTP** or **SOCKS** (SOCKS5), **Host name**, **Port number**
  and **No proxy for**, the hosts reached directly: `*.example.com, 192.168.*`. **Proxy authentication**
  takes **Login** and **Password**; with **Remember** the password is kept in the macOS Keychain,
  otherwise until the IDE quits. It is never written to a settings file.

**Check Connection** asks for an address and fetches it with what the page shows, before you press
**OK**. A request of an `.http` file with `# @no-proxy` connects directly, in a trusted project only.
This computer's own addresses never go through the proxy. A manual proxy with no usable host or port
stops connections rather than going direct. A remembered password belongs to that protocol, login, host
and port, so changing them never sends it elsewhere.

Agents read the setting as `proxy` in `get_settings` and use `settings.proxy.status`. Changing it,
`settings.proxy.check`, `settings.proxy.password` and `settings.proxy.forget` are yours alone.

### System Settings → Server Certificates

**Settings → Appearance & Behavior → System Settings → Server Certificates** is for networks that
intercept TLS with a certificate of their own. The IDE trusts Mozilla's roots, the roots and trusted
certificates of the macOS keychain, and the list on this page. It applies to updates, the plug-in
catalog, tool downloads and the HTTP Client; `git` gets the same lists through its configuration
and is never told to stop verifying, your own `http.sslCAInfo` is kept, and `http.sslVerify` in your Git
configuration still applies.

- **Accepted certificates:** **+** adds a `.crt`, `.cer` or `.pem` file, **−** removes the selected one,
  and its details are shown below. For a corporate root, add the root here or to the keychain: it covers
  every server at once.
- When the HTTP Client or Check Connection meets a server whose issuer is unknown, **Untrusted Server's
  Certificate** shows it. **Accept** keeps that certificate for that host only, listed under **Accepted for
  one host**; it signs for nobody else. **Reject** refuses it until the IDE restarts. An expired certificate
  or one for another name is refused without asking. A background connection, such as an update check,
  never asks: it fails, and you add the issuer here.
- **Accept non-trusted certificates automatically** is off by default. With it on, anyone on the network
  can read and change the IDE's connections; the page warns of that.

Agents use `settings.certificates.list`, `pending` and `remove`, and can turn
`acceptUntrustedCertificates` off. Turning it on, `add` and `answer` are yours alone, and an agent's
`press_keys` is refused while the dialog is open.

## Editor

- **Mouse Control → Change font size with Command+Mouse Wheel in:** (off by default; Ctrl on
  Windows and Linux) with **Active editor** or **All editors**: **⌘** and the mouse wheel change the
  editor's font size in the file in front only, or in every editor and in Editor → Font → Size
  ([Font size](02-editor.md#font-size)); for agents, `wheelFontSize` and `wheelFontSizeIn`.
- **Smart Keys → Use "CamelHumps" words** (off by default): ⌥← ⌥→, ⌥⇧← ⌥⇧→, ⌥⌫ and ⌥⌦ stop at the
  camel humps of a name, `Bar` in `fooBarBaz` ([Moving and selecting](02-editor.md#moving-and-selecting)).
  Under it, **A double-click selects CamelHumps words while they are turned on** (on by default,
  available only while the first is on): a double click selects the hump instead of the name. For
  agents, the settings `camelHumps` and `camelHumpsOnDoubleClick`.
- **Smart Keys: HTML/CSS → Insert closing tag on tag completion** (on by default): typing `>` of a
  start tag adds its end tag, and `</` the open element's name ([HTML](10-plugins.md#html)); for
  agents, the setting `closingTags`.
- **Gutter Icons → Show run icons in the gutter** (on by default): the green ▷ next to `main` and
  the tests ([Run icons in the gutter](15-run-and-test.md#run-icons-in-the-gutter)). Off, the
  gutter has no room for them.
- **Highlight on Caret Movement → Highlight usages of element at caret** (on by default): the
  usages of the name at the caret are highlighted by themselves, writes and reads in colours of
  their own ([The name at the caret](02-editor.md#the-name-at-the-caret)); for agents, the setting
  `highlightUsagesAtCaret`. A selection's other occurrences are highlighted either way.
- **Soft Wraps → Soft-wrap these files** (`*.md; *.txt; *.rst; *.adoc` by default): the files whose
  long lines wrap at the editor's edge when they open, by name patterns separated by `;`
  ([Soft wraps](02-editor.md#soft-wraps)). View → Active Editor → Soft-Wrap changes it for one file.
- **Appearance**: under Code Structure, **Show hard wrap and visual guides (configured in Code
  Style options)** (on), **Show line numbers** (on) with **Absolute**, **Relative** or **Hybrid**,
  **Show method separators** (off) and **Show indent guides** (on); under Presentation, **Show
  whitespaces** (off) with **Leading**, **Inner** and **Trailing** (on) and **Selection** (off),
  which pick the whitespaces drawn ([Line numbers and method
  separators](02-editor.md#line-numbers-and-method-separators), [Whitespaces and
  guides](02-editor.md#whitespaces-and-guides)). View → Active Editor → Show Whitespaces, Show Line
  Numbers and Show Indent Guides change them for one file; for agents, the settings `lineNumbers`,
  `lineNumbersMode`, `methodSeparators` and `editorAppearance`.
- **Breadcrumbs → Show breadcrumbs** (on by default): the declarations around the caret under the
  editor ([Breadcrumbs](02-editor.md#breadcrumbs)); for agents, the setting `breadcrumbs`.
- **Sticky Lines → Show sticky lines while scrolling** (on by default) and **Maximum number of
  lines** (5 by default, 1 to 10): the first lines of the blocks the view is inside stay at the top
  of the editor ([Sticky lines](02-editor.md#sticky-lines)).
- **Limits → Maximum number of entries in Paste from History** (20 by default, up to 100): how many
  copied texts [Paste from History](02-editor.md#paste-from-history) keeps.
- **Console → Override console cycle buffer size** (off by default) and **Buffer size, KB** (1024 by
  default): how much output a run or debug console keeps; when it is full, the oldest lines go
  ([Run and test](15-run-and-test.md)).
- **On Save → Remove trailing spaces on: Modified lines / All lines** (on, Modified lines by
  default), **Keep trailing spaces on caret line** (on by default, greyed out while the first one is
  off), **Drop the blank lines at the end of a file when it is saved** (off by default) and **End
  each file with a line break when it is saved** (off by default): what saving does to a file's
  whitespace ([What saving does to whitespace](02-editor.md#what-saving-does-to-whitespace)); a
  project's `.editorconfig` overrides them for the files it matches.

### Editor → General → Code Folding

**Editor → General** lists its pages: **Code Folding** and **Postfix Completion**.

- **Show code folding arrows** (on by default) with **On mouse hover** (the default: the arrows of
  unfolded regions show while the pointer is over the gutter, those of folded ones always) or
  **Always**; unchecked, the gutter shows no fold arrows. For agents, `showFoldArrows` and
  `foldArrows` (`onHover` or `always`).
- **Show bottom arrows** (off by default): an arrow on the last line of each unfolded region too;
  a click on it folds the region. For agents, `foldBottomArrows`.
- **Fold by default:** the regions that start folded when a file opens for the first time
  ([Fold by default](02-editor.md#fold-by-default)). **General**: Imports (on by default), File
  header, Documentation comments, Method bodies and Custom folding regions (off); for agents,
  `foldFileHeader`, `foldImports`, `foldDocComments`, `foldMethodBodies` and `foldCustomRegions`.
  **Go**, with the Go plug-in on: One-line 'if' blocks for error handling, One-line functions with a
  single 'return', One-line case clauses, Empty functions, Empty struct or interface type
  definitions and Formatted strings (off by default); for agents, `declaredSettings` with
  `go.folding.ifErrors`, `go.folding.oneLineReturns`, `go.folding.oneLineCases`,
  `go.folding.emptyFunctions`, `go.folding.emptyTypes` and `go.folding.formattedStrings`.

The settings search finds each of these labels and opens the page.

### Editor → General → Postfix Completion

**Enable postfix completion** switches [postfix completion](02-editor.md#postfix-completion) on and
off; under it, the postfix templates by language, each with a check box, its key and description.
Select one to see its text (`$EXPR$` is the expression before the dot). The choices are saved with
the live templates in `templates.json`.

### Editor → Font

- **Font**: JetBrains Mono, which comes with GoGarden, or any monospaced font installed on your
  computer. The list of installed fonts is read the first time you open it. A bold or italic face the
  font does not have is taken from the bundled font.
- **Size**: from 6 to 48 points (13 by default). **View → Increase / Decrease / Reset Font Size in
  All Editors** (**⌃⇧.**, **⌃⇧,**) change it too ([Font size](02-editor.md#font-size)).
- **Line height**: a multiple of the size, from 0.6 to 3.0 (1.2 by default, 24 pixels at 13 points).
- **Enable ligatures**: draw `!=` as `≠`, `<-` as `←` and so on (off by default).

The preview under them shows Go code with the font as you set it, before you apply it. The editor's
font does not change with the UI font size on Appearance & Behavior. AI agents list the fonts with
the `ui.fonts.list` command.

### Editor → Code Style

How a file is indented: by **Tab** and **⇧Tab**, by **Enter** after an opening bracket, and by
Reformat Code, which passes the same settings to the language server.

- **Scheme** (at the top of the page, before Show Scheme Actions): **Project** (the default) lets
  the code style a project keeps in `.idea` win where it sets one; **Default (IDE)** uses only your
  own settings below, in every project, and the copies you made of it are listed after it. Quick
  Switch Scheme (⌃`) switches between Project and Default too
  ([Quick Switch Scheme](21-appearance.md#quick-switch-scheme)); for agents, the settings
  `codeStyleScheme` (`project` or `default`) and `codeStyleCopy`. The pages below edit the selected
  scheme. A project that keeps no scheme yet stays that way, and edits go to your own scheme, until
  you pick **Project** in the combo or run **Copy to Project…**; **Apply** or **OK** then writes
  `.idea/codeStyles/Project.xml` and `codeStyleConfig.xml`, keeping whatever else the file says.
- **Show Scheme Actions** (the gear): **Copy to Project…** (it asks before replacing the project's
  scheme), **Copy to IDE…**, **Duplicate…** (a copy under a name you give), **Reset** (the built-in
  defaults), **Rename…** and **Delete** (copies only), **Export** → **IntelliJ IDEA Code Style XML**
  or **EditorConfig File**, and **Import Scheme…** (an IntelliJ code style XML file becomes a copy).
  For agents: `project.codeStyle.get`, `project.codeStyle.set`, `project.codeStyle.export`,
  `project.codeStyle.import`, and the settings `codeStyleGeneral` and `codeStyleSchemes`.
- **Hard wrap at** (120) is the column of the right margin the editor draws, **Visual guides** are
  more lines (columns separated by commas), and **Wrap on typing** moves a word typed past the
  margin to a new line, at the last gap before the margin. Editor → Appearance → Show hard wrap and
  visual guides turns the lines off.
- **Formatter** tab: **Do not format** takes file globs separated by `;` (a glob with `/` matches
  the path inside the project, any other the file name, `*` every file); Reformat Code, Optimize
  Imports and the save actions leave those files alone. **Turn formatter on/off with markers in code
  comments** keeps the lines from one with `@formatter:off` to one with `@formatter:on` as they are.
- **General**: **Line separator**, the one new files and files without line breaks get:
  System-Dependent (LF on macOS and Linux, CRLF on Windows; the default), Unix and macOS (\n),
  Windows (\r\n) or Classic Mac OS (\r). An `end_of_line` in the project's `.editorconfig` files
  wins for the files it matches; files that have line breaks keep their own
  ([Line separators](02-editor.md#line-separators)). **Enable EditorConfig support** (on by
  default) reads the project's `.editorconfig` files ([EditorConfig](#editorconfig)).
- **Code Style** (the General tab): **Follow the indentation a file already has** (on by
  default). A file whose lines are already indented with tabs, or with 2, 3, 4 or 8 spaces, is
  edited that way by **Tab**, **Enter** and **⇧Tab**, whatever the rows below say. Comments, text
  inside multi-line strings and continuation lines (the rest of a call or an expression that goes on
  over several lines) are not counted, and a file with fewer than three indented lines keeps the
  configured style. YAML never takes tabs and a Makefile never takes spaces, whatever its lines
  say. The file's indentation is read when it opens, when it is reloaded from disk and after
  **Reformat Code**, which still uses the configured style (and in Go, gofmt's tabs). Turning the option off returns every open file
  to the configured style at once.
- **Go**, **Rust** and every language that declares its formatter's options (a plug-in can too):
  a page with the tabs **Tabs and Indents** (Use tab character, Tab size and Indent, for **Tab**
  and **Enter**), **Spaces**, **Wrapping and Braces**, **Blank Lines**, **Imports** and
  **Arrangement**, and a **Preview** of sample code. Only options the formatter really reads are
  shown; a tab without any says why.
  - **Go**: gofmt has no options (tabs for indentation, its own spaces, line breaks and blank
    lines), so the page offers what gopls reads: **Format with gofumpt's stricter rules**
    (Wrapping and Braces) and the prefix of the imports to put after third-party ones
    (Imports, gopls's `local`). **Arrangement**: the order **Code → Rearrange Code** puts
    top-level declarations in (constants, variables, types, functions and methods by default;
    **Move Up** and **Move Down** change it).
  - **Rust**: the options of stable rustfmt (`hard_tabs`, `tab_spaces`, `max_width`,
    `use_small_heuristics`, `fn_params_layout`, `match_arm_leading_pipes`,
    `match_block_trailing_comma`, `newline_style`, the rewrites, `reorder_imports`,
    `reorder_modules`). With the **Project** scheme they are read from and written to the
    project's `rustfmt.toml` (or `.rustfmt.toml` when the project has that one) on **OK** or
    **Apply**; only these keys change, everything else in the file stays. With **Default (IDE)**
    they are yours and reach rustfmt as `--config`. Rust has no arrangement rules yet.
  - **Java** (with the Java plug-in): the Eclipse JDT Language Server formats with the Eclipse
    formatter, and the page sets its continuation indent, the indent of `case` branches, spaces
    before parentheses, around operators and before braces, the hard wrap, brace placement, how
    parameter lists, calls, chained calls, `extends`/`throws` lists, binary expressions and enum
    constants wrap, `else`/`catch`/`finally` on a new line, and the blank lines; under **Imports**,
    the import order (groups separated by `;`, `#` for static imports, an empty group for all
    others), the blank lines between import groups and how many classes or static names make an
    import use `*`. With **Default (IDE)** a changed page becomes a formatter profile the IDE keeps
    in its own folder; with **Project** the options are kept in the project's
    `.zen/java-formatter.xml`, an Eclipse formatter profile you can commit, and other settings in
    that file stay as they are. The import order and the `*` counts are yours in both schemes.
    Java has no arrangement rules yet ([Java](06-java.md#code-style)).
  - **Kotlin** (with the Kotlin plug-in): spaces, the hard wrap, trailing commas,
    `else`/`catch`/`finally` on a new line, the blank lines kept, and the import layout and `*`
    counts, as the `ij_kotlin_*` properties of `.editorconfig` that Kotlin LSP reads. With
    **Project** they are kept in the project's `.editorconfig` under `[*.{kt,kts}]`; with
    **Default (IDE)** **Export → EditorConfig File** writes them there. The fwcd server reads none
    of them ([Kotlin](06-kotlin.md#code-style)).
  - **Rearrange code on Reformat Code** (Arrangement) makes **Reformat Code** rearrange first.
  - For agents: `editor.codeStyle.list` lists every language's options, values and project file;
    `set_settings` with `codeStyleOptions` (`{"go": {"gofumpt": true}}`) and
    `rearrangeOnReformat` changes yours; `editor.codeStyle.writeProject` writes the project's file
    (`rustfmt.toml`, `.zen/java-formatter.xml`, `.editorconfig`).
- **SQL**: the same for SQL files and consoles (spaces, 4), and what **Reformat Code** does to
  them: **Keyword case** (As is, the default; Upper; Lower), **Commas in lists** (at the end of a
  line, the default, or at the start), **Clause keyword on its own line** (off) and **Each list item
  on its own line** (on). Every IDE has this page ([Formatting SQL](28-database.md#formatting-sql));
  for agents, the setting `sqlFormat`.
- **Other File Types**: the same for JSON, YAML and TOML (spaces, 2), Markdown (spaces, 4) and
  every other file type (spaces, 4).

Tabs are drawn as wide as the tab size of their file, also when the file's own indentation is used. A project that keeps its own code style in `.idea/` (shared with a
JetBrains IDE) overrides these settings for what it sets ([Projects shared with JetBrains
IDEs](05-project.md#projects-shared-with-jetbrains-ides)). The status bar shows what **Tab** and **Enter** insert in the open file: **Tab**, or the number of
spaces. Click it for a menu: when the file's own indentation differs from the configured one, it
switches the file between the two (*2 spaces (detected in the file)*, *4 spaces (configured)*);
**Configure Indents…** opens the file's Code Style page. The switch lasts until the file is closed.
AI agents read a file's indentation with `editor.indent.get`, switch it with `editor.indent.useDetected`
and `editor.indent.useConfigured`, open the menu with the action `view.indent.menu` and turn the
option off with `set_settings` and `detectIndents: false`. The editor's text, its line numbers, run icons and Git blame
annotations follow the font size and line height as soon as you apply them.

#### EditorConfig

A project's `.editorconfig` files (<https://editorconfig.org>) set the code style of the files they
match, above the project's own code style and yours: what they set wins, what they leave out comes
from the project and then from these settings.

- **Which files**: the `.editorconfig` in the file's folder and in every folder above it, up to the
  project's root or to a file with `root = true`; the nearest one wins, and within a file a later
  section wins over an earlier one. Sections match by EditorConfig globs: `*`, `**`, `?`, `[abc]`,
  `[!abc]`, `{a,b}` and `{1..3}`; `[*.go]` matches Go files in every folder below,
  `[/src/*.go]` only those directly in `src`.
- **What they set**: `indent_style` (`tab` or `space`), `indent_size` (a number, or `tab` for the
  tab size), `tab_width`, `max_line_length` (the right margin; `off` for none), `end_of_line`,
  `charset`, `trim_trailing_whitespace` and `insert_final_newline` (see [Saving](#saving-and-encodings)). `unset` takes a property back, so that the project's or your setting applies.
  Of the IntelliJ properties, `ij_continuation_indent_size` and `ij_visual_guides` are read, with
  the language's own (`ij_go_...`) before `ij_any_...` before the general one; other keys are
  ignored.
- **Where it shows**: **Tab**, **Enter**, **⇧Tab**, the width of a tab, the status bar's indent and
  Reformat Code. Editing, adding or deleting an `.editorconfig` file applies to open files at once.
- **Mistakes**: a line that cannot be read is skipped; AI agents see it in the project's settings
  (`project.settings.get`), and what applies to a file with `project.editorConfig.get`.

Turn **Enable EditorConfig support** off on the General page to ignore every `.editorconfig` file.

##### Saving and encodings

- `end_of_line` (`lf`, `crlf` or `cr`) is the line separator of new files, and a modified file is
  written with it when you save it: every line break is rewritten, as one step that one **⌘Z**
  takes back. A file you only opened is not rewritten.
- `trim_trailing_whitespace` and `insert_final_newline` win over **Editor → General → On Save** for
  the files they match: `true` removes the trailing spaces of every line or ends the file with a
  line break, `false` leaves trailing spaces or a missing final line break alone whatever On Save
  says. Both keep the file's line separator.
- `charset` (`utf-8`, `utf-8-bom`, `latin1`, `utf-16be` or `utf-16le`) is the encoding the file is
  opened and saved in; the status bar shows it (`UTF-8 BOM`, `ISO-8859-1`, `UTF-16LE`...). A file
  whose bytes are not text in that encoding opens read-only. When you type a character the
  encoding cannot hold (say `я` in a `latin1` file), the save is refused with the character, its
  line and column: the file on disk stays as it was and your text stays in the editor. A changed
  `charset` applies when the file is opened again. **Replace in Files** and **Line Separators**
  leave closed `latin1` and UTF-16 files alone (open them to change them), **Find in Files** reads
  closed files as UTF-8, and **Local History** keeps such files' versions as they are on disk:
  **Revert** gives the bytes back exactly, though the versions show as binary.

#### File encodings

A file's encoding comes from the first of these that says something: its byte order mark (BOM);
`charset` in `.editorconfig`; the nearest entry of the Path / Encoding table; Project Encoding;
Global Encoding; UTF-8. Every encoding of the web standard is read and written (windows-1251,
KOI8-R, ISO-8859-x, Shift_JIS, GBK, Big5 and others), plus US-ASCII, ISO-8859-1 and UTF-16; UTF-32
is not, and such a file opens read-only.

- **Settings → Editor → File Encodings** sets **Global Encoding** (for every project), and for the
  project **Project Encoding**, the **Path / Encoding** table and **Create UTF-8 files** (with BOM,
  with NO BOM, or with BOM under Windows, with no BOM otherwise). The project's part is kept in
  `.idea/encodings.xml`, where a JetBrains IDE reads it too; the rest of that file stays as it was.
  Files already open keep their encoding until they are opened again.
- **The status bar's encoding** opens a searchable list of encodings (also **File → File Properties
  → File Encoding**). A change that gives the same text and the same bytes is made at once. Any
  other asks: **Reload** reads the file again in the encoding and leaves it as it is, **Convert**
  writes it in the encoding, with a version from before in **Local History**. Reload is off while
  the file has unsaved changes or its bytes are not text in that encoding; Convert is off when the encoding cannot hold a character, and the dialog
  names the first one. The choice goes into the Path / Encoding table, except for a file outside
  the project.
- **A BOM** is never shown in the text and is kept on save; the status bar shows `UTF-8 BOM`. **File
  → File Properties → Remove BOM / Add BOM** work on the open file, or on the files and folders
  selected in the Project tool window (a folder: all its text files, recursively). They change only
  the mark: unsaved edits stay unsaved, and the change is in **Local History**.
- **A character the encoding cannot hold** stops the save: the message names the character, its
  line and column, the file stays as it was and your text stays in the editor.

Agents use `file.encoding.get`, `file.encoding.list`, `file.encoding.set` (`charset`, `path`,
`action`: `reload` or `convert`; without `action` a change that loses something answers
`applied: false` with `canReload`, `canConvert` and `unmappable`, changing nothing),
`file.bom.add`, `file.bom.remove` (`paths`), `project.encodings.get`, `project.encodings.set`
(`project`, `files`, `bomForNewFiles`: `with`, `without` or `onWindows`) and the IDE setting
`globalEncoding`.
- Documents opened read-only and files that are read-only on disk are never changed by a save.

##### The status bar, new files and export

- When an `.editorconfig` sets the open file's code style, the status bar's indent shows the
  EditorConfig mark. Click it for **Open .editorconfig** (the nearest file that matches),
  **Show Files Related to Project…** (the project's `.editorconfig` files, `root` ones and ones with
  unreadable lines marked; click one to open it) and **Disable EditorConfig** (turns Enable
  EditorConfig support off).
- `.editorconfig` files are highlighted (sections, comments, known properties and values) and
  complete: before `=` the known properties, after it their values and `unset`.
- **File → New → EditorConfig File**, or **New → EditorConfig File** in the Project tool window,
  asks which stubs to add (`root = true`, the standard properties, the IntelliJ ones) and creates
  `.editorconfig` in the folder with each property set to what its files get now, so that the
  code style does not change; its `end_of_line` does apply from then on, so a file with other line
  breaks gets the new file's ones when you next save it. A folder that has one is refused.
- **Code Style → General → Show Scheme Actions** (the gear next to **Scheme**) → **Export** →
  **EditorConfig File** writes your code style, as last applied, to `.editorconfig` at the
  project's root: `[*]` with the default style and a section for each language indented
  differently. An existing file is replaced only after you click **Replace**, and Local History
  keeps the one it replaced.

AI agents use `project.editorConfig.get` (what applies to a file, `save` included),
`project.editorConfig.open`, `project.editorConfig.list`, `project.editorConfig.create` and
`project.editorConfig.export`, and the actions `view.editorConfig.new`,
`view.editorConfig.showFiles` and `view.editorConfig.disable` with `run_action`.

### Editor → Inlay Hints

- **Show inlay hints** (on by default): the labels the language server puts in the text
  ([Inlay hints](02-editor.md#inlay-hints)).
- **Hints**: **Parameter names** (on by default), **Field names in composite literals**, **Types of
  variables**, **Types of composite literals**, **Inferred type arguments** and **Constant values**.

gopls switches each kind itself. A language server that cannot shows its parameter hints while
parameter or field names are on, and its type hints while any of the types is on. AI agents change
these settings with `set_settings` (`inlayHints`).
### Editor → Inlay Hints → Code Vision

- **Show Code Vision above declarations** (on by default): the grey row with the usages and the
  author ([Code Vision](02-editor.md#code-vision)).
- **Usages** (on by default): how many usages each declaration has, from the language server.
- **Code author** (off by default): who changed the declaration last, from Git blame. The status
  bar's person button, **View → Appearance → Show Code Authors** and **⌃⌥A** switch the same
  setting. Updating to 0.38.7 turned the authors off once for everyone; whatever you choose after
  that is kept.

AI agents change these settings with `set_settings` (`codeVision`).

### Editor → Inspections

The inspections by group, each with a check box; the selected one shows its description, its
severity and its options. The inspections come from the IDE and from language servers, so the gopls
analyzers are here when you work on Go. The field above the tree filters by name, description or
group; **Modified only**, **Cleanup only** and the severity choice narrow it further. What differs
from the defaults is shown in blue.

**Severity** starts at **As reported**, the severity each finding comes with; choosing another
applies it to the editor, the Problems tool window and Inspect Code. An inspection you turn off
stops there too, and a gopls analyzer is turned off (or on) in gopls itself.

**Profile** chooses the set of these choices that applies to the project. **Project Default** is
always there; a profile marked (Project) is kept in `.zen/inspectionProfiles/` and shared with the
project, one marked (IDE) stays with this IDE. The gear next to it copies the profile to the IDE or
the project, duplicates, renames, deletes (not Project Default), exports to a JSON file, imports
one, and restores the defaults; these take effect at once. The choices in the tree and the choice
of profile take effect with **Apply** or **OK**.

**Proofreading → Spelling** ([Spelling](02-editor.md#spelling)), severity
**Typo**:

- the check box turns the spelling inspection on and off (on by default);
- **Process code** (on by default): the names where they are declared;
- **Process literals** (on by default): string literals;
- **Process comments** (on by default): comments.

Plain text and Markdown are always checked while the inspection is on. AI agents change these
settings with `set_settings` (`spelling`).

### Editor → File and Code Templates

The file templates that New (**⌘N**) and File → New create files from: the Scheme (Default or
Project), the tabs Files, Includes and Code, the toolbar (Create Template, Create Child Template
File, Remove Template, Copy Template, Revert to Original Template) and the selected template's
fields, with the variables described under them. See
[File and Code Templates settings](05-project.md#file-and-code-templates-settings).

### Editor → Live Templates

The [live templates](02-editor.md#live-templates), by language (Go, the languages of your plug-ins,
and Other for every file): each row has a check box that switches the template on and off, its
abbreviation and its description.

- **Add Template** adds one to the selected template's group; **Duplicate** copies the selected one
  as your own (with a free abbreviation, such as `sout2`); **Remove** removes a template you added;
  **Restore Defaults** brings back the selected built-in template as GoGarden has it, or every
  built-in template of the selected group.
- Under the list, the selected template's **Abbreviation** (fixed for a built-in one),
  **Description** and **Template text**. In the text, `$NAME$` is a variable (every `$NAME$` is
  filled in at once), `$END$` is where the caret goes at the end, `$SELECTION$` is the code the
  template wraps ([Surround With](02-editor.md#surround-with), ⌥⌘J), `$$` is a dollar sign, and a tab at
  the start of a line becomes the file's indentation.
- **Applicable in**: **Code**, **Comments**, **Strings** — where in a file the template expands;
  code only unless you tick more.
- **Variables** lists the variables of the text, in the order **Tab** visits them, each with an
  **Expression**, a **Default value** (used when there is no expression or it gives nothing) and
  **Skip if defined** (Tab passes it by when it has a value). An expression is a quoted text,
  another variable's name, or a function: `camelCase()`, `capitalize()`, `concat()`, `date()`,
  `decapitalize()`, `fileName()`, `fileNameWithoutExtension()`, `lineNumber()`, `lowercase()`,
  `snakeCase()`, `time()`, `uppercase()`, `user()`; one that does not read is shown in red and
  refused.
- A template from a plug-in is read-only: switch it off with its check box, or **Duplicate** it to
  change the copy.

Changes take effect with **OK** or **Apply**. Built-in templates stay in GoGarden; what you change
and add is saved in `templates.json` in GoGarden's data folder. When that file is damaged, GoGarden
says so at start, uses the built-in templates and keeps the file as `templates.json.corrupt`; a
template in it that is not valid is left out.

### Editor → File Types

What a file is decides how it is highlighted and indented, which language server opens it and which
icon it gets. Two tabs:

- **Recognized File Types**: the file types of GoGarden, its built-in modules (Go, Go Module, Go
  Checksums, Rust) and its plug-ins (Markdown, YAML, Shell Script, ...), Text, and two types of
  their own: **File type auto-detected by file content** (its files open as text) and **Files opened
  in associated applications** (its files open in the application macOS chooses, not in the editor),
  which [Associate with File Type](05-project.md#associate-with-file-type-and-override-file-type)
  fills. For the selected
  type, **File name patterns** (`*.go`, `Dockerfile`; `*` stands for any characters, `?` for one)
  and **HashBang patterns**: the interpreter in the first line of a script (`bash` for
  `#!/usr/bin/env bash`), which gives a type to a file no name pattern names. **+**, **−** and **✎**
  add, remove and edit a pattern; Enter or **OK** next to the field takes it. A pattern another type
  has is moved here, and the line under the field says so before you take it. A type whose plug-in
  says what its files start with shows it under **Recognized by content (read-only)**: XML's
  `<?xml`, a doctype other than HTML's (`<!DOCTYPE plist …`), and a root element with an `xmlns`
  namespace. A file that no name pattern and no hashbang name gets the type its first bytes show,
  so that an Apple property list saved as `timeweb-cloud.cyberduckprofile` is XML: highlighted,
  folded, in the Structure, with the XML icon. Only the first 4 KiB of a file are read, once per
  version of the file, never of a binary one; your own patterns and **Override File Type** still
  come first.
- **Ignored Files and Folders**: names the IDE does not see. *Ignored files and folders are not
  visible in the IDE and not indexed*: the project tree leaves them out, and Go to File, Search
  Everywhere and Find in Files never find them. By default: `*.pyc`, `*.pyo`, `*.rbc`, `*.yarb`,
  `*~`, `.DS_Store`, `.git`, `.hg`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.svn`, `CVS`,
  `__pycache__`, `_svn`, `vssver.scc` and `vssver2.scc`. A name matches a whole file or folder name,
  with `*` and `?`, letter case as written. Taking `.git`, `.hg` or `.svn` off the list shows the
  folder in the tree, but its contents are still never indexed, searched or watched.

Changes take effect with **OK** or **Apply**, at once: open files change their highlighting, icon
and indentation without being opened again, and the project is indexed again in the background
(**Indexing…** in the status bar) while you keep working. When a pattern decides the type
of a file differently from GoGarden's own, the file is treated in every way as a file of its new
type: `*.gotmpl` given to Go is highlighted and indented as Go, gets the Go icon and is opened in Go's
language server. A type given by a hashbang changes the highlighting and indentation of the script;
its icon and language server still go by its name. A pattern removed from its type leaves its files
without a type: plain text, a plain icon. The patterns are GoGarden's,
for every project, and are saved in `filetypes.json` in GoGarden's data folder (only what differs
from the defaults). AI agents use the `editor.fileTypes.*` commands: `list`, `typeOf` (the type of
a file, by its name, hashbang or content, and whether it is ignored), `addPattern`, `removePattern`,
`addHashbang`, `removeHashbang`, `setIgnored` and `reset`.

### Editor → Natural Languages → Spelling

**Natural Languages** lists the language checked, English (USA), and **Configure 'Proofreading'
inspections…** opens the spelling inspection's options. Its **Spelling** page keeps the
[dictionaries](02-editor.md#dictionaries):

- **Use single dictionary for saving words** (off by default) with **project-level** (the default)
  or **application-level**: where Save to dictionary (⌥⏎ on a typo) puts words. Off, Save asks each
  time (Select Dictionary).
- **Custom dictionaries (plain text word lists, hunspell)**: the built-in application-level and
  project-level dictionaries first, then yours. **+** adds a file (type its path, or **Browse…**),
  **−** removes it from the list (the file stays), **✎** opens it in the editor (the dialog applies
  and closes). Each shows how many words it spells or why it cannot be read.
- **Accepted words**: the words of the application-level and project-level dictionaries. **+** adds
  a word (type it, then Enter), **−** removes the selected one. The changes are made on **OK** or
  **Apply**: new words go to the dictionary chosen above (application-level while it is off),
  removed words leave both.
- **Configure 'Spelling' inspection…** opens Editor → Inspections.

AI agents change these settings with `set_settings` (`spelling`: `customDictionaries`,
`useSingleDictionary`, `savingDictionary`) and the words with `spelling.word.save` and
`spelling.word.remove`.

## Keymap

The **Keymap** list at the top chooses the keymap in use: the built-in **macOS** keymap or one of
yours. The gear button next to it has:

- **Duplicate…**: a copy of the keymap shown, with your changes, which becomes the keymap in use.
- **Rename…** and **Delete** for your own keymaps (the macOS keymap cannot be renamed or deleted).
- **Import…**: choose a keymap `.xml` exported from a JetBrains IDE. It becomes a keymap of its own
  and the keymap in use, with its shortcuts for editing, navigation, search, version control and
  running; a short report says how many actions were carried over and lists the
  shortcuts that were not (actions GoGarden does not have, two keystrokes in a row, mouse shortcuts,
  keys it cannot name). The details are in [Working side by side with JetBrains
  IDEs](../jetbrains-compatibility.md#keymaps).

Below it, every action with its shortcuts, in groups; search by name (as shown, or in English) or
by shortcut. Changed actions are blue.

- Double-click an action (or use its context menu) and choose **Add Keyboard Shortcut**, then press
  the keys. If another action has them already, you can remove them from it or leave both.
- The context menu also removes a shortcut, or resets the action to its defaults.
- **Add Abbreviation** in the context menu gives the action a short name of your own (`gl` for
  **Go to Line:Column…**): typed in Find Action or Search Everywhere, it lists that action first.
  The abbreviations show next to the shortcuts, **Remove Abbreviation** takes one away, and they
  are kept in `abbreviations.json` for every keymap at once. A command you gave a shortcut with
  **⌥⏎** in Find Action is listed here too.

Changes to the macOS keymap are kept in `keymap.json` in GoGarden's data folder; your own keymaps
are files in its `keymaps` folder, one per keymap, and the choice is remembered. Choosing another
keymap and the gear menu act at once; Cancel goes back to the keymap Settings opened with. The full list of default
shortcuts is on the [Keyboard shortcuts](keyboard-shortcuts.html) page.

## Plugins

**Marketplace** lists the plug-ins you can install, with a search; **Built-in** the plug-ins built
into every Zen IDE, such as Go and Rust, each switched on or off from the next start, with **Restart
IDE**; **Installed** those you have,
with a switch to turn each off, in two groups: **Downloaded** and **Bundled with this IDE**, and
**Check for Updates**, **Update** and **Update All**. The plug-in selected in the list shows its
details on the right, with its release notes under **What's New**. Plug-ins that come with
GoGarden are marked **Bundled** (in the Marketplace too) and can be switched off but not
uninstalled; those another Zen IDE comes with are installed here from the Marketplace. When you
open a file that a plug-in supports, a notification above it offers to install it. More in
[Plug-ins](10-plugins.md#settings--plugins).

## Languages & Frameworks → Go

- **GOROOT**: the Go SDK in use; **Add SDK → Local…** adds one from a folder, **Remove** takes an
  added one off the list. A Homebrew Go is kept as `/opt/homebrew/opt/go/libexec`, so it follows
  `brew upgrade go`; a chosen SDK whose folder is gone is shown in red with a button that uses the one
  found instead ([Go and gopls](06-go.md#go-and-gopls)).
- **gopls**: its version and path, a **Custom path** to use another gopls, **Install gopls** or
  **Update gopls**, and **Look again**.
- **Reformat code on save** and **Optimize imports on save**, the same options as
  [Tools → Actions on Save](#tools--actions-on-save).
- **Dependencies**: **Check for newer versions of dependencies** and **Check dependencies for
  known vulnerabilities** in `go.mod` (both on). Each says which servers it contacts; turned off, it
  contacts nothing ([go.mod checks](go.html#gomod-colours-newer-versions-and-vulnerabilities)).
- **Inspections**: **Check with go vet**, **Check with staticcheck** and **Check when a file is saved**
  (all on). They use no network ([go vet and staticcheck](go.html#go-vet-and-staticcheck)).
- **staticcheck**: the program in use, and **Install staticcheck** or **Update staticcheck**.

The SDK and gopls change at once, without **OK**, in every window: gopls restarts, and the project's
modules and libraries and the next run or test use the new SDK. The details are in
[Go settings](go.html#go-settings).

## Languages & Frameworks → Markdown

How [Markdown files](02-editor.md#markdown-preview) are previewed:

- **Preview layout**: how a Markdown file you never switched opens: **Editor Only**, **Editor and
  Preview** (the default) or **Preview Only**. A file you switched keeps its own mode.
- **Sync scroll in the editor and preview** (on by default): scrolling either side brings the other
  to the same source line. Off, each side scrolls alone.
- **Preview font size** (8 to 32 points, 14.5 by default): the size of the preview's body text;
  headings, tables, code and formulas follow it in proportion.

AI agents change these through `markdown` in `set_settings`.

## Languages & Frameworks → Language Servers

Every language server of the project with its state, version, path and arguments; edit the path
and arguments, install or restart a server: see [Language servers](language-servers.html).

## Tools → Actions on Save

The page is a table of actions and when they run; all of them run on **Any save**.

- **Reformat code**: as Code → Reformat Code (⌥⌘L) does. **Files:** narrows it to chosen file types
  (All file types by default). In a project under version control, **Whole file** can be changed to
  **Changed lines**, which reformats only the lines that differ from the last commit; a file that is
  new to the repository is reformatted whole, and so is SQL.
- **Optimize imports**: as Code → Optimize Imports (⌃⌥O) does, with its own **Files:**.
- **Rearrange code** and **Run code cleanup** are shown disabled: the IDE does not have them yet.
- **Update copyright notice** says *No copyright is configured*.
- **Run stylelint --fix** and **Run eslint --fix** can be turned on when their plug-ins support the
  open project, and otherwise say *integration disabled*. They run the project's own tool on the
  files it serves. Stylelint needs the Stylelint plug-in, which leaves Reformat Code alone.
- **File Watcher**: the enabled file watchers, or "No File Watchers configured"; **Configure…**
  opens **Tools → File Watchers**.

The actions are kept per project: once you change them in a project, it keeps them in
`.zen/actionsOnSave.json`. Until then a project takes the defaults, which **File → New Projects
Setup → Settings for New Projects…** opens. Reformat code and Optimize imports are off by default.
The on-save options on a language's page (Go, Rust) are these same actions. For agents: `project.actionsOnSave.get` and
`project.actionsOnSave.set`, and the settings of the defaults through `set_settings`.

**ESLint, Prettier and Biome on save: Code Quality Tools…** opens
**Languages & Frameworks → Code Quality Tools**, where **Run eslint fixes on save** and Prettier's
**Run on save** are set ([Linters and formatters](06-web.md#linters-and-formatters)).

**Configure autosave options…** at the bottom opens **Appearance & Behavior → System Settings**,
where autosave is set.

## Tools → External Tools

The programs you run on the current file or the project from the Project view's and the editor's
context menus and shortcuts: see [External tools](15-run-and-test.md#external-tools).

## Tools → File Watchers

The programs that run by themselves when matching files are saved or change on disk: see
[File watchers](15-run-and-test.md#file-watchers).

## Tools → MCP Server

Lets AI agents use GoGarden: see [AI agents](ai-agents.html). **Language for agents** is the
language of what agents get over MCP (errors, reasons, messages): English by default, whatever the
interface language.
**Files hidden from agents** lists, one pattern per line, the files whose text agents never get
(environment files, keys, certificates and credential files by default; `!` before a pattern keeps
a file readable): see [Files agents never read](11-ai-agents.md#files-agents-never-read).

## Advanced Settings

- **Editor → Show the zoom level when it changes** (on by default): after the editor's font size changes, a label at
  the bottom of the editor shows the size, the default size and **Reset**
  ([Font size](02-editor.md#font-size)); for agents, `zoomIndicator`.
- **Editor → Tab character rendering**: how a tab shows while whitespaces are shown: **Horizontal
  line** (the default) or **Arrow** ([Whitespaces and guides](02-editor.md#whitespaces-and-guides));
  for agents, `editorAppearance.tabRendering`.
- **Editor → Let Extend Selection take in whitespace**: with the caret in spaces
  rather than at a word, the first ⌥↑ selects the spaces. Off by default.
- **Markdown → Hide the formatting toolbar over a selection** (off by default): no [toolbar](02-editor.md#markdown-preview)
  over text selected in a Markdown file; for agents, `markdownHideFloatingToolbar`.
- **Bookmarks → List only line bookmarks in the popup**: Show Line Bookmarks… (⌘F3) lists the line
  bookmarks only. On by default; off, it lists the breakpoints too, as a list of their own.
- **Search Everywhere → Show results only after every source has answered**: Search Everywhere shows new results
  only once every tab's search is done. Off by default: the results show as soon as Classes, Files
  and Actions have enough of them, or after two seconds, and Symbols join in as they come.
- **Search Everywhere → Add text matches to All when there are few other results**: on by default;
  the **Text** group comes at the bottom of **All** when the other groups found fewer than six
  results ([Go to Text](03-navigation.md#go-to-text)). Off, All never searches text.

## Export, import and restore settings

**File → Manage IDE Settings** has three items. They work on GoGarden's own
settings only: a project's settings (`.zen`, and `.idea` from JetBrains IDEs) are never read, written
or reset by them. The settings come in five parts, the *components*: **Settings** (this dialog's
choices and the language servers), **Keymaps**, **Themes**, **Live templates** and the **Plug-in
list** (which plug-ins and built-in modules are on, and which plug-ins are installed).

**Export Settings…** lists the components with check boxes (and **Select All**) and how many files
each has, and writes them into a ZIP file, `settings.zip` in your home folder unless you choose
another (the **…** button). A path inside a project's `.zen` or `.idea` folder is refused.

**Import Settings…** (also on the welcome window, under **Customize**) asks for a file or a folder
(**Choose File…**, **Choose Folder…**), then shows **Select Components to Import**: the components it can give, with check boxes, and the entries
it will skip, each with why. It reads:

- a ZIP exported from GoGarden or another Zen IDE: the chosen components replace yours (keymaps and
  themes of the same name are replaced, others are kept). A plug-in on the exported list that is not
  installed here is named among the skipped entries; install it from **Settings → Plugins**;
- a ZIP made by **File → Manage IDE Settings → Export Settings…** in a JetBrains IDE: its keymaps
  (`keymaps/*.xml`) become keymaps and its colour schemes (`colors/*.icls`) become themes, as
  **Import…** on the Keymap page and **Load theme file…** make them; everything else in it is
  skipped and listed;
- one file: a JetBrains keymap `.xml`, a colour scheme `.icls` or `.xml`, or a theme `.json`. It
  becomes a new keymap or theme, and the last step says whether it was carried over and, if not, why;
- the backup folder that Restore Default Settings made (below).

Keymaps and themes are there at once; the settings, live templates and plug-in list are used after a
restart, so the last step offers **Restart** or **Not Now**. An archive that could do harm is refused
as a whole and nothing is changed: one larger than 64 MB, with more than 4096 entries or an entry over
8 MB, or with an entry whose name would leave the folder (such as `../` or `/etc/...`).

**Restore Default Settings…** asks first; **Restore and Restart** copies every component to a new
folder `settings-backups/<date-time>` in GoGarden's data folder, removes them and restarts with the
defaults. Recent projects, local history and installed plug-ins stay. To go back, choose that folder
in **Import Settings…** (**Choose Folder…** opens in `settings-backups`).

Agents use `settings.ide.components`, `settings.ide.export { path, components? }` and
`settings.ide.inspect { path }`, and the menu items `view.ideSettings.export`,
`view.ideSettings.import` and `view.ideSettings.restoreDefaults` with the same parameters to open the
dialogs filled in. **Importing and restoring are yours alone:** `settings.ide.import` and
`settings.ide.restoreDefaults` refuse an agent with `agentNotAllowed`, because both replace settings
wholesale, including what an agent may not change, such as the files hidden from agents. Only you
answer **Restore Default Settings…** and **Import Settings…**: an agent's call opens them, its
`confirmed` is ignored, and its `press_keys` is refused while they are open. A headless IDE has no
such dialogs, so settings are not imported or restored there.
