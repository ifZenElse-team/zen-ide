# The editor

Typing, selecting and changing code, with several carets.

## Typing

- **Enter** keeps the line's indentation, indents after `{`, `(` and `[`, and puts the closing
  bracket on its own line.
- Typing `(`, `[`, `{` or a quote adds the closing one, or wraps the selection in the pair; typing
  the closing one steps over it; **⌫** between an empty pair deletes both. To wrap code in an `if`,
  a loop or a template, use [Surround With](#surround-with).
- **Tab** indents the selected lines (or types the indentation), **⇧Tab** unindents. Go files use
  tabs; a file already indented another way keeps its own indentation ([Code
  Style](09-settings.md#editor--code-style)). After an abbreviation, Tab expands it first: an Emmet
  abbreviation in a file whose plug-in has them, then a [live template](#live-templates), then a
  [postfix template](#postfix-completion) such as `ok.if`.
- **⇧⌘⏎** (Code → Code Completion → Complete Current Statement) finishes the statement you are
  typing, see [Complete Current Statement](#complete-current-statement).
- Typing the opening of a documentation comment on an empty line above a declaration puts the
  language's stub, see [Documentation comment stubs](#documentation-comment-stubs).
- Undo with **⌘Z**, redo with **⇧⌘Z**.
- Typing stays smooth in large files with a language server running, however long you type: the
  completion list and the fold arrows are worked out again only when what they show changes, and
  the fold arrows follow your edits until you pause.
- **⌘S** (File → Save All) saves every modified file. **Save**, in
  Find Action, saves only the current one; it has no shortcut, and you can give it one in
  Settings → Keymap.
- You rarely need ⌘S: GoGarden saves by itself, see [Autosave](#autosave).

## Live templates

A live template is a piece of code behind a short abbreviation: type `fori` in a Go file and press
**Tab**, and it becomes a counting loop with its variables to fill in.

- **Tab** after an abbreviation expands it, when the abbreviation is a whole word in a place the
  template is applicable in: code, unless the template says comments or strings too (Settings →
  Live Templates, **Applicable in**); elsewhere Tab stays Tab. The completion list (as you type, or **⌃Space**) offers
  the templates of the file's language too, marked **t**; **⌘J** (Code → Insert Live Template...)
  lists only the templates.
- The first variable is selected. Type to replace it: a variable used several times (the `i` of
  `for i := 0; i < n; i++`) changes everywhere at once. **Tab** or **Enter** goes to the next
  variable, **⇧Tab** back; after the last one the caret goes to where the template ends.
  **Escape** ends the template where the caret is, and so does moving the caret out of it.
- With several carets, the template expands at the primary one only.
- A template that needs a package adds its import to the file (Go's `print` adds `fmt`), in the
  same undo step.
- The variable you are on is framed; the others have a fainter frame.
- **⌘Z** ends the template and undoes your typing in it, then the whole expansion, back to the
  abbreviation.
- A variable can be computed by an **expression**: `decapitalize(TYPE)` follows the `TYPE`
  variable, so when you leave `TYPE` with Tab, the variable takes its new value (until you reach it
  and type your own); `camelCase(fileNameWithoutExtension())`, `date()`, `time()`, `user()`,
  `lineNumber()` read the place. A variable marked **Skip if defined** is passed by when its
  expression gives a value.

Each language brings its own templates ([Go's](06-go.md#live-templates)), plug-ins bring theirs
(Java's and Kotlin's come with their plug-ins); the **Other** group holds templates for every file.
Edit them, switch them off and add your own in [Settings → Editor → Live
Templates](09-settings.md#editor--live-templates). A plug-in's template can only be switched off
there; **Duplicate** makes your own copy to change. AI agents use `editor.templates.list`,
`editor.templates.set`, `editor.templates.expand` and `editor.templates.reset`.

## Postfix completion

Type an expression, a dot and a key, and a postfix template wraps the expression: `ok.if` becomes
`if ok { }`, `err.nn` becomes `if err != nil { }`, `items.for` becomes a loop over `items`.

- After `expression.` the completion list offers the language's postfix templates (marked **t**)
  next to what the language server offers; choose one with **Enter** or **Tab**. Typing the whole
  key and pressing **Tab** expands it too.
- The expression is the whole chain before the dot: `a.b(c).d[1].if` takes `a.b(c).d[1]`; in Rust
  `vec![1, 2].for` and `env::args()?.for` too. An operator or a space ends it, so `x == y.if` takes
  only `y`.
- The result is a live template: its variables are filled in with Tab, the caret goes to its end,
  and a template that needs a package imports it (Go's `print`). One **⌘Z** takes it back.
- In Go: `var`, `if`, `else`, `not`, `nn`, `nil`, `for`, `fori`, `switch`, `return`, `print`,
  `par`, `len`. In Rust: `let`, `if`, `else`, `not`, `while`, `for`, `match`, `ifl` (`if let Some`),
  `ref`, `refm`, `some`, `ok`, `err`, `box`, `dbg`, `println`, `return`, `par`.

Switch each template, or postfix completion as a whole, off in [Settings → Editor → General →
Postfix Completion](09-settings.md#editor--general--postfix-completion). Plug-ins declare postfix
templates for their languages. AI agents use `editor.postfix.list`, `editor.postfix.set` and
`editor.postfix.expand`.

## Complete Current Statement

**⇧⌘⏎** (Code → Code Completion → Complete Current Statement) finishes the statement at the caret
as its language writes it:

- open brackets and an open string on the line are closed: `fmt.Println("a` becomes
  `fmt.Println("a")`;
- an `if`, a loop, a `switch` or `match`, a function, a type with a body get their block, with the
  caret on an indented line inside it: `if ready(x` becomes `if ready(x) {` … `}`; a line that
  ends with `{` gets the line inside (and the closing `}` when it is missing);
- a statement gets the language's terminator (`;` in Rust and Java, nothing in Go) and the caret
  goes to its end; a statement that is complete already gets a new line below.

Go and Rust declare their rules; plug-ins declare them for their languages, and a file of another
language gets its brackets closed and a new line. AI agents use `editor.statement.complete`.

## Type-Matching completion

**⌃⇧Space** (Code → Code Completion → Type-Matching) opens the completion list with only what fits
the place: the items the language server marks as the ones for it. When the server marks none, the
list keeps its values, functions and types and leaves out keywords, snippets and templates. AI
agents use `editor.completion.smart`.

## Documentation comment stubs

Type the opening of a documentation comment on an empty line right above a declaration, and the
language puts its stub with the caret where the text goes:

- in Rust, `///` above a `fn`, `struct`, `enum`, `trait` and the like gives `/// `; above a
  function that returns a `Result` it adds an `# Errors` section;
- in Go, `//` above a `func`, `type`, `var` or `const` gives `// Name `, the declared name first,
  as Go's documentation comments start.
- in Java and Kotlin (with their plug-ins), `/**` and **Enter** above a class, method, field,
  function or property gives a Javadoc or KDoc stub with an `@param` line per parameter, `@return`
  when there is a result, and an `@throws` line per exception after `throws` (Java) or in
  `@Throws(...)` (Kotlin).

The stub is its own undo step: **⌘Z** leaves the opening you typed. Under a comment line the
opening types as usual. Plug-ins declare stubs for their languages, also ones that come on **Enter**
after the opening (`/**` and Enter in Java), with a line per parameter and one for the result.

## Surround With

**⌥⌘T** (Code → Surround With…) wraps the selection in a construct of the file's language. Without
a selection it wraps the statement at the caret: the caret line, down to the line that closes the
brackets it opens, so a whole `for { ... }` block goes in at once. On a closing `}` or `} else {`,
inside a call's arguments or a struct literal, or in a multi-line string, the whole statement
around the caret is wrapped; brackets in strings and comments do not count.

- The popup numbers its items; press the item's digit or letter, or choose it with the arrows and
  **Enter**, or click it. Beside the list, the hovered or selected item shows the lines as they
  will read.
- In Go: **if**, **if / else**, **for**, **func() { … }()**, and, for a selection, **(expr)** and
  **!(expr)**, which wrap exactly what is selected.
- In Java and Kotlin (with their plug-ins): **if**, **if / else**, **while**, **do / while**,
  **for**, **try / catch**, **try / finally**, **try / catch / finally**, **synchronized**,
  **Runnable**, a block, and, for a selection, **(expr)** and **!(expr)**, with **((Type) expr)**
  in Java and **"${expr}"** in Kotlin.
- Every file with comments gets **<editor-fold…> Comments** and **region…endregion Comments**: a
  custom folding region around the lines, written with the file's comment (`// region Description`
  … `// endregion` in Go).
- After them come the live templates that hold `$SELECTION$`, and **Configure Live Templates…**.
- A selection is widened to the whole statements it touches; the wrapped lines are indented one
  level inside the construct, and the construct takes the indentation of the first line. The
  lines inside a multi-line string or comment stay exactly as they are, and a file with CRLF line
  breaks keeps them.
- The caret goes where the construct asks for something: into the condition of an `if` or a `for`
  (**Tab** then goes on, into the `else` branch of an **if / else**), onto the region's
  description. One **⌘Z** undoes the whole wrapping.

**⌥⌘J** (Surround with Live Template, in Find Action and in Settings → Keymap under Other; not in
the Code menu) lists only the live templates that hold `$SELECTION$`. Write
`$SELECTION$` in a [template's text](09-settings.md#editor--live-templates) where the code goes: on
a line of its own it wraps whole lines, otherwise exactly the selection.

An imported JetBrains keymap keeps its keys for both. AI agents use `editor.surround.list` and
`editor.surround.apply`.
## Cyclic Expand Word

**⌥/** (Code → Code Completion → Cyclic Expand Word) finishes the word you are typing from words
already in your open files, with no popup and no language server, so it works the same in code,
comments, strings and plain text. Type `cou` and press ⌥/: the part of the word left of the caret
becomes the nearest word above it that starts with `cou`, say `counter`.

- Press ⌥/ again to replace it with the next word: the words above the caret come first, nearest
  first, then the words below it, then the words of your other open files. Each word comes once;
  after the last one your own `cou` comes back, and the round starts again.
- **⌥⇧/** (Cyclic Expand Word (Backward)) looks below the caret first, then above, then in the other
  files. Pressed during a round started with the other key, it steps back to the previous word.
- Typing, moving the caret, undo or any other change to the text or the carets ends the round; the
  next ⌥/ starts from the word part that is there then.
- Letters of any alphabet, digits and `_` make up a word, and case counts: `cou` does not become
  `Counter`.
- In the editor ⌥/ and ⌥⇧/ do not type the character the key gives with ⌥ (`÷`, or `–` and `—` on a
  German keyboard, where that key is `-`). To keep typing it, give the actions other keys in
  [Settings → Keymap](09-settings.md#keymap).
- With several carets, every caret with the same word part left of it changes the same way.
- Each press is one step for **⌘Z**.

Agents run it with `editor.completion.expandWord` (`forward: true` for ⌥⇧/), which answers with the
word part, the word now in its place, its number in the round and how many words there are.

## TODO comments

Start a TODO comment and the completion list helps you spell it the way the TODO tool window and
its highlighting find it.

- In a comment, typing the start of **TODO** or **FIXME** in any case (`to`, `Fix`) offers the word
  in the completion list; **⌃Space** after `// ` offers both. Code and strings get nothing extra.
- **Enter** puts the word in upper case in place of what you typed (`to` becomes `TODO`), **Tab**
  also replaces the rest of the word. With several carets, every caret whose letters start
  the word gets it; a caret after another word is left as it is.
- A word typed out in full in upper case is not offered again, so Enter after it starts a new line.
- The words are those the TODO patterns start with: TODO and FIXME by default, or those of the
  patterns in **Settings → Editor → TODO**.

TODO text is highlighted with the colour scheme's `todo` colour, or the pattern's own, with its
indented following lines, and marked along the scroll bar ([TODO](22-tool-windows.md#todo)).

## Line actions

They act on every caret.

| Action | Shortcut |
|---|---|
| Duplicate the line or the selection | ⌘D |
| Delete the line | ⌘⌫ |
| Move the line up or down | ⌥⇧↑, ⌥⇧↓ |
| Comment or uncomment lines | ⌘/ |
| Comment the selection with a block comment | ⌥⌘/ |

## Macros

A macro repeats what you did in the editor. **Edit → Macros → Start Macro Recording** starts
recording; the status bar shows **Recording a macro** with a stop button. Everything you do in the
editor of the file in front is recorded: typing, caret moves, selections, line actions, comments,
Reformat Code and saving. Clicks, popups, tool windows, menus and dialogs are not, and neither is
choosing a completion item or an ⌥⏎ fix: type the text instead when the macro needs it.

**Stop Macro Recording** (the same menu item, or the status bar's stop button) asks for a name in
**Save Recorded Macro**. A name saves the macro; names are unique. Leave it empty (or press Cancel) to
keep the macro only until the IDE closes, for **Play Back Last Macro**.

- **Play Back Last Macro** plays the last macro recorded or played on the file in front.
- **Play Saved Macros…** lists the saved macros; choose one to play it.
- **Edit Macros** lists the saved macros on the left (**Rename…**, **Remove**) and the actions of
  the selected one on the right (**Remove**); OK keeps the changes.

One ⌘Z takes a whole playback back. Saved macros belong to the IDE, not to a project: each is an
action of its own in [Settings → Keymap](09-settings.md) under **Macros** and in Find Action (⇧⌘A),
so you can give it a shortcut. None of the Macros items has a shortcut of its own.

AI agents use the `editor.macro.*` commands: `startRecording`, `stopRecording` with a `name`,
`play` with a `name`, `playLast`, `list`, `rename`, `remove` and `removeStep`. A recording keeps
only what its starter does: what an agent changes while you record stays out of your macro, and
what you type while an agent records stays out of its.

## Several carets

- **⌃G** selects the word at the caret, then each next occurrence with a caret of its own; **⌃⇧G**
  takes the last one back; **⌃⌘G** selects every occurrence.
- **⌥⇧-click** adds or removes a caret.
- **Drag with ⌥** to select a column.
- **⌥⌥↑** and **⌥⌥↓** (press ⌥ twice and hold it, then ↑ or ↓) clone the caret to the line above
  or below: **Clone Caret Above** and **Clone Caret Below**, also in Edit → Multiple Carets and
  Find Action.
- **⇧⌘8** turns **Column Selection Mode** on and off (Edit → Column Selection Mode): then **⇧** with
  the arrows, and a drag, select a column, also past the ends of short lines.
- **Escape** keeps only the primary caret.
- Pasting as many lines as there are carets gives each caret its own line.

## The gutter

Left of the text: the line numbers, right-aligned; room for icons and the change
markers of Git; the fold arrows; then a thin line before the text. A breakpoint is a red dot in
place of the line number. The wheel and the trackpad scroll the text with the pointer over the
gutter too. Next to `main` and the tests, a green ▷ runs them (Settings → Editor → Gutter Icons turns the
run icons off).

### Line numbers and method separators

**Settings → Editor → Appearance → Show line numbers** turns the numbers off in every editor, and
its list chooses how they count:

| Mode | The caret's line | The other lines |
|---|---|---|
| **Absolute** (default) | its number | their numbers |
| **Relative** | 0 | how many lines away from the caret they are |
| **Hybrid** | its number | how many lines away from the caret they are |

A folded region counts as one line, so a relative number is the count of lines you see. **View →
Active Editor → Show Line Numbers**, and **Appearance → Show Line Numbers** in the gutter's context
menu, show or hide the numbers of the file shown only, whatever the setting. Without numbers the
gutter narrows, and breakpoints keep a column of their own. Distraction Free and Zen modes hide the
numbers whatever is chosen ([Appearance](21-appearance.md)).

**Show method separators** (off by default) draws a thin line, in the theme's `editor.methodSeparator`
colour, above each top-level function, method and type declaration, and above the comments,
attributes and decorators right above it. The declarations are the file's structure, the same
symbols as the Structure tool window and the breadcrumbs: the language server's (gopls for Go), or
the language's own outline. The lines are read once per version of the text, a moment after you
stop typing; meanwhile they move with the lines you add or remove, and one whose place was edited
is left out until they are read again. Agents read them with `editor.methodSeparators.list` (each separator's line, the
declaration's name and kind), the numbers with `editor.lineNumbers.get`, and turn the numbers of the
active editor on or off with `editor.lineNumbers.toggle`; `set_settings` changes `lineNumbers`,
`lineNumbersMode` and `methodSeparators`.

## Breadcrumbs

Under the text, a bar shows where the caret is: the declarations around it, outermost
first, each with its kind's badge, for example `Server › Start` inside a method of a type. They come
from the language server's document symbols (gopls for Go), or from the text while it has not
answered and for files of a plug-in that outlines them, the same symbols as the Structure tool
window. A click on a crumb moves the caret to that declaration's name. The bar follows the caret as
it moves, and catches up a moment after you stop typing. Files nothing can be read from have no bar.
**Settings → Editor → Breadcrumbs → Show breadcrumbs** turns it off. Agents read the same path with
`editor.breadcrumbs.get` (the active file and its caret by default, or a `path` and an `offset`).

## Moving and selecting

The arrows move the caret; with **⌥** by words, with **⌘** to the line's start and end or the
file's start and end; **⇧** selects as it moves. **⌘A** selects all. A double click selects a word,
a triple click the line; a click on a line number selects the line.

With **Settings → Editor → General → Smart Keys → Use "CamelHumps" words** on, a word stops at its
camel humps: in `fooBarBaz`, **⌥←** and **⌥→** (with **⇧**, selecting) stop at `Bar` and `Baz`, and
**⌥⌫** and **⌥⌦** delete one hump at a time. A hump starts at a capital after a small letter or a
digit (`utf8|Decode`), at the last capital of an abbreviation (`HTTP|Server`) and after `_`
(`foo_|bar`). **A double-click selects CamelHumps words while they are turned on** (on by default,
available only with the first) makes a double click select the hump (`Bar`) instead of the whole
name. For agents: the settings `camelHumps` and `camelHumpsOnDoubleClick`, and
`editor.selection.word { at }`, which selects a word as a double click does.

**⌥↑** (Edit → Extend Selection) selects the word at the caret, then with each press the next
larger piece of code around it: a string's text, the string with its quotes, the argument, the
argument list without and with its parentheses, the call or expression, the statement, a block's
content, the block, the declaration with its doc comment, and the whole file. In comments and text
files it goes by word, sentence, paragraph and the whole comment. With Use "CamelHumps" words on,
the first press selects the hump at the caret (`Amount` in `totalAmount`, `retry` in
`max_retry_count`) and the next one the whole word. **⌥↓** (Shrink Selection) steps
back the same way; after a selection made by hand, it narrows it to the largest piece inside it.
Every caret grows on its own. Where a language server offers selection ranges (gopls, rust-analyzer),
its steps join in, for example `a + b` inside a longer expression. A JetBrains keymap you import
brings its own keys for both (⌘W and ⇧⌘W in IntelliJ IDEA Classic). Agents use
`editor.selection.extend` and `editor.selection.shrink`.

## Soft wraps

Long lines can wrap at the right edge of the editor instead of running past it.
**View → Active Editor → Soft-Wrap** turns wrapping on or off for the file shown. Markdown and text
files open wrapped: **Settings → Editor → Soft Wraps → Soft-wrap these files** lists the file name
patterns that do (`*.md; *.txt; *.rst; *.adoc` by default; `*` stands for any characters, `?` for
one).

A line breaks after a space where it can, a word too long for the row where it overflows, and
Chinese, Japanese and Korean text between any two characters. The rows after a line's first start
as far in as the line's own indent. The line number, breakpoints and other gutter icons stay on a
line's first row. **↑** and **↓** move by rows on the screen, and so do **Page Up** and **Page
Down**, a screenful of rows at a time; **End** goes to the end of the row (where the caret stands
before the wrap) and, pressed again, to the end of the line; **Home** goes to the start of the row,
then to the line's start. With **⇧** they select, and with several carets every caret moves. A
click past the end of a row puts the caret at that row's end. When the window gets wider or
narrower, the line at the top of the editor stays there. A folded region is one row and does not
wrap. Agents turn wrapping on and off with `editor.softWrap.toggle`, read it with
`editor.softWrap.get`, and move by rows on the screen with `editor.caret.move` and `"visual": true`
(without it, or when the file is not shown, carets move by lines).

## Whitespaces and guides

Thin vertical lines mark each indent level of the code: a block's guide runs down its
body at the column of the line that opens it, over the blank lines inside it, and the guide of the
block the caret is in is brighter. A line further right marks the right margin: the **Hard wrap
at** column of the code style (the project's in `.idea`, or Settings → Editor → Code Style, 120 by
default), with a line for each of its **Visual guides** too. The **Appearance** group of **Settings → Editor** turns both on and off (**Show hard
wrap and visual guides**, **Show indent guides**, both on by default).

**Show whitespaces** (off by default) draws each space as a small dot in the middle of its place and
each tab as a horizontal line across it, or as an arrow with **Settings → Advanced Settings → Editor
→ Tab character rendering → Arrow**. **Leading** (before a line's text), **Inner** (between its
words) and **Trailing** (after it) pick which are drawn, all three by default; with **Selection**,
the whitespaces inside a selection are drawn too, whatever their place. A line of whitespace only
counts as leading and trailing.

**View → Active Editor → Show Whitespaces** and **Show Indent Guides** turn them on or off for the
file shown only, with a check mark while on; the gutter's context menu has **Appearance → Show
Indent Guides** as well. The colours are the theme's `whitespace`, `indentGuide`,
`indentGuide.selected` and `rightMargin` ([theme format](../themes.md)). Agents turn them on and
off with `editor.whitespaces.toggle` and `editor.indentGuides.toggle` (`"enabled": true` or
`false`, or without it to toggle), read what the active file shows, the settings and its right
margin and visual guides with `editor.appearance.get`, and change the settings with `set_settings`
and `editorAppearance`. Only the lines on the screen are drawn, so a long file costs nothing more.

## The name at the caret

When the caret stands on a name, its usages in the file are highlighted. In a file
the language server knows (Go files with gopls, for example) they are the symbol's real usages: the
places it is written (its declaration, assignments, `x++`) get a reddish background, the places
it is read a grey one, and a word that only looks the same (in a comment, or another variable of
the same name) is left alone. In any other file, every place the name occurs as a whole word gets
the grey background. They are shown once you stop typing. **Settings → Editor → General →
Highlight on Caret Movement → Highlight usages of element at caret** turns this off.

When you select text, every other place the same text occurs in the file is highlighted too,
inside longer words and in comments as well (a selection of a single line, up to 1000 characters).

To keep the highlight while you move around, use **Edit → Find Usages**:

- **Highlight Usages in File** (**⇧⌘F7**) pins the usages of the name at the caret (or the selected
  text's occurrences); pressed again on one of them, it takes the highlight away.
- **Find Usages in File** (**⌘F7**) pins them too, and pressed again keeps them.
- **Next Highlighted Usage** (**⌃⌥↓**) and **Previous Highlighted Usage** (**⌃⌥↑**) move the caret
  to the next or previous pinned usage, round the end of the file; with nothing pinned, they step
  through the usages at the caret.
- **Escape** clears the pinned usages; so does any edit of the file.

The colours are the theme's `editor.symbolHighlight` (reads and plain text) and
`editor.symbolHighlight.write` (writes). Agents read the highlights at the caret with
`editor.highlights.get` (each range `read`, `write` or `text`, and where they come from), the
pinned ones with `editor.highlightUsages.list`, and run the actions above with
`editor.highlightUsages.toggle`, `navigate.usages.findInFile`, `editor.highlightUsages.next`,
`editor.highlightUsages.previous` and `editor.highlightUsages.clear`; `editor.occurrences.list`
still lists the name's whole-word occurrences.

## Matching brackets

When the caret is next to a bracket (`(`, `)`, `[`, `]`, `{` or `}`), the bracket and the one that
matches it are highlighted; a bracket with no match is highlighted in red instead.
An opening bracket right after the caret counts first, then a closing one right before it (so
that in `f(g(x)|)` the pair just closed is shown), then any other bracket next to the caret; only
the primary caret's bracket is highlighted. Brackets inside strings
and comments do not count: the file's highlighting says where those are, for Go and for the
languages of plug-ins alike. **⌃⇧M** (Move Caret to Matching Brace) moves the caret to the
matching bracket (past a closing one, before an opening one) and, pressed again, back; with
several carets every one of them moves. The colours are the theme's `editor.bracket.matched.*`
and `editor.bracket.unmatched.*` keys. Agents
read the pair with `editor.brackets.matching` and move the caret with `editor.brackets.goToMatching`.

## Inlay hints

A language server can put small grey labels in the text: the name of the parameter before each
argument of a call (`name:`), the type of a variable declared without one, the value of a constant.
They are not part of the file: the caret steps over a hint, a click on one puts the caret where it
stands, and typing before it pushes it along. They come a moment after the text stops changing and,
while you type, move with your edits instead of disappearing. Which kinds you see is set in
[Settings → Editor → Inlay Hints](09-settings.md#editor--inlay-hints); a hint with more to say shows
it when the pointer rests on it. A plug-in can show hints too, such as the value of a `.properties`
entry without its escapes ([Properties files](30-properties.md)); they follow the switch for all inlay
hints. Agents read the hints of a file, with their lines and columns, with
`editor.inlayHints.list`.

## Code Vision

A small grey row above each function, method, type, constant and variable declared at the top of a
file says how many usages it has (**3 usages**, **no usages**) and, when you ask for it, who wrote
it: the author of the latest change among its lines, with **+N** when others changed it too, from
Git blame. The authors are off by default, as they are noisy; the person button at the right end of
the status bar shows and hides them (it is lit while they are shown), and so do **View → Appearance
→ Show Code Authors**, **⌃⌥A** and Find Action (**Show Code Authors**). Click the
usages to show them in the [Show Usages](06-go.md#show-usages) popup, as **⌥⌘F7** does; click the author to open that latest
commit in the Git tool window's Log. The usages come from the file's language server, so a file without one
shows only its authors; lines changed since the last commit have no author yet. The counts are
asked for the declarations on screen a moment after the text stops changing; while you type, the
rows move with your edits. [Settings → Editor → Inlay Hints → Code Vision](09-settings.md#editor--inlay-hints--code-vision)
turns Code Vision or each kind on and off. Agents switch the authors with
`run_action` (`view.codeVision.authors.toggle`, which answers whether they are shown now) or
`set_settings` (`codeVision.authors`), and read the same rows with `editor.codeVision.list`
(`path`, `fromLine`, `toLine`, `waitMs`) for a file open in the editor; files that are not open
have no Code Vision.

## Folding

Blocks in brackets that span lines, and runs of comment lines, can be folded: a folded block shows
as `{...}` (Go's imports as `import ...`). A comment of several lines, a `/* ... */` block or a
run of `//` lines, folds whole into `/.../`;
when it starts with the name the next line declares (a doc comment such as
`// TestLimitStopsWalk checks...` above `func TestLimitStopsWalk`), the name stays in view:
`// TestLimitStopsWalk ...`. Such a name is shown a little brighter than the rest of the comment.

In a file whose plug-in finds the regions (HTML, CSS and the other languages of plug-ins, see
[Plug-ins](10-plugins.md)), the plug-in's regions are folded instead: a folded region keeps its
first line in view, followed by a `...` mark, and hides the lines after it; the line numbers skip
the hidden lines.

The fold arrows have their own column of the gutter, between the line numbers and the text: the
arrow of a folded region (›) is always there; those of regions you can fold (⌄) show
while the pointer is over the gutter. A click on an arrow folds or unfolds; a click on the `...`
mark unfolds too, and resting on it shows the folded lines.

Moving a caret or a selection into a folded region opens it, whether by the keyboard, a click, Go
to Line, a search match or a jump to a declaration or a problem. The same actions are in
**Code → Folding** in the main menu and in the Folding group of **Settings → Keymap**.

| Action | Shortcut (macOS) | Windows and Linux |
|---|---|---|
| Unfold at the caret (Expand) | ⌘= or ⌘+ | Ctrl+= or Ctrl+NumPad+ |
| Unfold the region at the caret and every region inside it (Expand Recursively) | ⌥⌘= or ⌥⌘+ | Ctrl+Alt+NumPad+ |
| Unfold everything (Expand All) | ⇧⌘= or ⇧⌘+ | Ctrl+Shift+NumPad+ |
| Fold the region at the caret (Collapse) | ⌘− | Ctrl+NumPad− |
| Fold the region at the caret and every region inside it (Collapse Recursively) | ⌥⌘− | Ctrl+Alt+NumPad− |
| Fold everything (Collapse All) | ⇧⌘− | Ctrl+Shift+NumPad− |
| Expand to Level 1–5 | ⌘NumPad*, then the digit | Ctrl+NumPad*, then the digit |
| Expand All to Level 1–5 | ⌥⌘NumPad*, then the digit | Ctrl+Shift+NumPad*, then the digit |
| Expand Doc Comments, Collapse Doc Comments | none | none |
| Toggle Folding | none | none |
| Fold the selection, or remove the manual region at the caret (Fold Selection / Remove Region) | ⌘. | Ctrl+. |
| Fold the code block around the caret (Fold Code Block) | ⇧⌘. | Ctrl+Shift+. |
| Go to a custom region (Custom Folding…) | ⌥⌘. | Ctrl+Alt+. |

The region at the caret is the smallest one around the caret or starting on its line, so on the
first line of a function or an `if` it is that block; Expand Recursively takes a folded region on
the caret's line first. With a selection, **Collapse All** and **Expand All** act only on the
regions inside the selection; without one, on the whole file.

**Expand to Level** unfolds the region at the caret down to a depth and folds what is deeper: level
1 unfolds the region itself and folds the regions inside it, level 2 unfolds one level more, and so
on. **Expand All to Level** does the same for the whole file, where level 1 is the regions inside no
other (functions, types, imports, comments). Both are submenus of Code → Folding with the levels 1
to 5, and shortcuts of two keystrokes: the numeric keypad's `*` with ⌘ (or ⌥⌘), then the digit,
pressed without a modifier. A digit that follows another key is typed as usual. After the `*` the
editor waits two seconds for the digit; any other key, or a digit pressed later, does what it
always does (a letter is typed), so a `*` pressed by mistake loses nothing.

**Expand Doc Comments** and **Collapse Doc Comments** unfold or fold every doc comment of the
file: a comment of several lines directly above a line of code, with no blank line between, as Go's
doc comments sit above what they document, and any `/** */`, `///` or `//!` comment. A comment
followed by a blank line, such as a licence header, is not a doc comment.

**Toggle Folding** unfolds what is folded on the caret's line or around the caret, and otherwise
folds the region at the caret. **Fold Code Block** folds the smallest block in brackets around the
caret, never a comment or the imports.

Agents run the same actions with `editor.fold.expand`, `editor.fold.expandRecursively`,
`editor.fold.expandAll`, `editor.fold.collapse`, `editor.fold.collapseRecursively`,
`editor.fold.collapseAll`, `editor.fold.expandToLevel` and `editor.fold.expandAllToLevel` (with
`level`), `editor.fold.expandDocComments`, `editor.fold.collapseDocComments`,
`editor.fold.toggleAtCaret`, `editor.fold.collapseBlock` and `editor.fold.selection` (Fold
Selection / Remove Region, on the selection they set with `editor.selection.set`); each returns the
folds as `editor.fold.list` lists them, where a custom region is of kind `region` with its name as
`placeholder` and a manual region of kind `manual`. `editor.fold.regions` lists the custom regions
as Custom Folding does (name, start, line from 0, depth, style, folded); `navigate.place.goTo` with
a region's `start` goes to it, and `run_action` with `view.customFolding.open` opens the list and
answers with the regions.

### Fold by default

When a file opens for the first time, some of its regions start folded, as **Settings → Editor →
General → Code Folding → Fold by default** says ([Settings](09-settings.md#editor--general--code-folding)):

- **General**: **File header** (the comment that starts the file, such as a licence, when it
  documents nothing; off by default), **Imports** (Go's `import ( … )`, a run of Rust `use`
  declarations; on by default), **Documentation comments**, **Method bodies** (the bodies of
  functions and methods) and **Custom folding regions** (every one, not only those written with
  `defaultstate="collapsed"`). The file header, imports, documentation comments and method bodies
  are found in files with `//` or `/* */` comments (Go, Rust, JavaScript, TypeScript, CSS and the
  like); in Markdown, YAML, JSON, TOML and other files only custom regions start folded.
- **Go**, from the Go plug-in: **One-line 'if' blocks for error handling** (`if err != nil {
  return err }` on one line), **One-line functions with a single 'return'** (`func Name() string {
  return s.name }`), **One-line case clauses** (`case 80: return errors.New("privileged")`), **Empty
  functions** and **Empty struct or interface type definitions** (`{}`), and **Formatted strings**:
  the format string of `fmt.Printf`, `fmt.Sprintf`, `fmt.Errorf`, `fmt.Fprintf`, `t.Logf` and the
  like, written on one line with as many arguments as verbs, shows the arguments in place of its
  verbs, `fmt.Printf("{name}: {done} of {total}\n")`. All of them are off by default.

A folded region of one line shows that line in its place, an empty one nothing, and a click on it
unfolds it. The options apply only the first time a file opens: after that it opens with the folds
you left, so a region you unfolded stays unfolded, and turning an option on changes the files you
open for the first time. Agents read the options and their values with `editor.folding.options`,
change them with `set_settings` (`foldFileHeader`, `foldImports`, `foldDocComments`,
`foldMethodBodies`, `foldCustomRegions`, and Go's in `declaredSettings`, such as
`go.folding.ifErrors`), and `editor.fold.list` gives each region the `option` that folds it.

The fold arrows in the gutter show while the pointer is over the gutter, and those of folded regions
all the time. **Show code folding arrows** on the same page chooses **Always** instead of **On mouse
hover**, or, unchecked, hides the arrows; **Show bottom arrows** adds an arrow on the last line of
each unfolded region, which folds it too.

Each file keeps what you folded: close it and open it again, or quit and start GoGarden again,
and the same regions are folded (imports you unfolded stay unfolded). A region comes back folded
when the file still has it on the same lines; after the file changed elsewhere, the regions that
moved open unfolded. Manual regions are kept with them, folded or not, by their lines and columns,
and come back where the file still has those places with the same text; one whose text changed
while the file was closed is dropped. Custom regions are part of the text, so they
are there whenever the file is.

### Custom folding regions

Comments can mark a region of their own in every language with comments: write
`// region Name` above it and `// endregion` below it, or `//<editor-fold desc="Name">` and
`//</editor-fold>`, with the comment syntax of the file (`# region Name` in Python, YAML or shell,
`-- region Name` in SQL, `<!-- region Name -->` in HTML and Markdown, `/* region Name */` in CSS).
`#region` and `#endregion` work too, a space after the comment start is optional, and the name can
be left out. Each comment must be alone on its line, and one inside a string or a block comment
does not count. Regions nest, and a closing comment closes the innermost open region written the
same way.

A custom region folds whole, from its opening comment to the end of its closing one, and shows its
name in place of the lines (`...` when it has none), with its fold arrow like any other region; the
folding actions above act on it too. `//<editor-fold desc="Name" defaultstate="collapsed">` starts
folded when the file is opened for the first time. **Surround With** (⌥⌘T) offers
`<editor-fold…> Comments` and `region…endregion Comments` to wrap the selection in such a region,
with a name you type.

**Custom Folding…** (⌥⌘., in the Navigate menu) lists the regions of the file by name, nested ones
indented, with their line numbers: ↑ and ↓ choose one, Enter or a click puts the caret on its
opening comment, Escape closes the list. A file without custom regions says so.

### Fold Selection

**Fold Selection / Remove Region** (⌘., in Code → Folding) folds what is selected, any text, even
part of a line: it becomes a manual region, folded to `...`, with a fold arrow, and the caret goes
before it. A selection of whole lines that ends at the start of the next line stops at the end of
the last line it selects, so the line break stays. Pressed without a selection on a manual region's
line or inside it, it removes the region (the text unfolds and the arrow goes). A manual region
moves with your edits, Undo and Redo, and a reload of the file changed on disk: it keeps what it
holds and what you type inside it, while what you type or paste right before or right after it
stays out. It unfolds when a caret goes into it like any fold, and goes away when all its text is
deleted or replaced.

## Sticky lines

While you scroll, the first lines of the blocks the view is inside (a function, a type, an `if` or a
loop) stay over the top of the editor, outermost first, with their line numbers: up to
five lines, and never more than half the lines the editor shows, with a line and a shadow under
them. The blocks are those the editor can fold that open a block: a line that ends with `{` after a
space or a `)`, such as `func`, `type … struct`, `if`, `for`, `switch`, `select`, a class or a
method, or a plug-in's fold of a line that leaves no bracket open (an HTML element, a YAML key).
The arguments of a call, a parameter list, a literal (`T{`, `= {`, `return {`), comments and
imports do not stick, nor does a folded block; a block whose header takes several lines (a long
parameter list, a condition split after `&&`) sticks with the header's first line. A click on a
sticky line moves the caret to it and scrolls it to just under the lines that stay above it.

**Settings → Editor → Sticky Lines** turns them off (**Show sticky lines while scrolling**) or sets
how many lines may stick (**Maximum number of lines**, 1 to 10). Agents read what sticks with
`editor.stickyLines.list` and go to a line with `editor.stickyLines.goTo`; the settings are
`stickyLines` and `stickyLinesMax`.

## Rename and formatting

| Action | Shortcut |
|---|---|
| Rename the symbol at the caret (Refactor → Rename…) | ⇧F6 |
| Reformat the file (Code → Reformat Code) | ⌥⌘L |
| Remove unused imports and sort the rest (Code → Optimize Imports) | ⌃⌥O |
| Put declarations in the order of Code Style → <language> → Arrangement (Code → Rearrange Code) | — |
| Show the quick fixes and intentions at the caret (Show Context Actions) | ⌥⏎ |
| Write a constructor, a test or other code for the code at the caret (Code → Generate…) | ⌘N |
| Override or implement methods of the class's supertypes (Code → Override Methods…, Implement Methods…) | ⌃O, ⌃I |
| Take away the construct around the caret, keeping or dropping its body (Code → Unwrap/Remove…) | ⇧⌘⌦ |

With no file open in the editor, ⌥⌘L and ⌃⌥O do nothing, as Go to Line does; no notification is
posted.

### Extract and Inline

| Action | Shortcut |
|---|---|
| Put an expression into a new local variable (Refactor → Extract/Introduce → Variable…) | ⌥⌘V |
| Put an expression into a new constant (Refactor → Extract/Introduce → Constant…) | ⌥⌘C |
| Put an expression into a new field of the class (Refactor → Extract/Introduce → Field…) | ⌥⌘F |
| Make an expression a new parameter of its function (Refactor → Extract/Introduce → Parameter…) | ⌥⌘P |
| Move the selected code into a new function or method (Refactor → Extract/Introduce → Method…) | ⌥⌘M |
| Put a variable's value or a function's body in place of its uses (Refactor → Inline…) | ⌥⌘N |

The language server of the file does the change, so what is offered depends on it:

| Server | Variable | Constant | Field | Parameter | Method | Inline |
|---|---|---|---|---|---|---|
| Java (Eclipse JDT LS) | yes | yes | yes | yes | yes | yes |
| Go (gopls) | yes, also every occurrence | yes | no | no | function or method | calls, variables |
| Rust (rust-analyzer) | yes | yes | no | no | function | variables, calls, aliases |
| TypeScript and JavaScript | the enclosing scope | the module | a readonly class field | no | function or method | variables |

Select the code first, or leave the caret in it: when several expressions around the caret
qualify, a popup lists them, smallest first, and shows the chosen one in the text; **↑**/**↓** and
**Enter** pick one, **Escape** gives up. When the server offers several ways (this occurrence or
every occurrence, a scope or another), a second popup lists them in its words.

After the change, the new name is selected at every place it is used, the declaration and its
uses at once: type to rename them all. A list under the caret offers the server's name and names
made from the expression (`user.getFullName()` gives `fullName` and `name`); **↑**/**↓** choose,
**Enter** takes the chosen one and ends the naming, **Escape** ends it keeping the name as it is.
Moving the caret out of the name ends it too. ⌘Z undoes the typing first, then the refactoring.
When the server offers none of the kind, a message says so, or gives the server's reason.

Agents call `refactor.extract.variable`, `refactor.extract.constant`, `refactor.extract.field`,
`refactor.extract.parameter`, `refactor.extract.method` and `refactor.inline.apply` with `start` and
`end` (the selection by default) and an optional `name`; the answer lists `expressions` or
`variants` to choose from, or the new `name` and its `occurrences`.

### Safe Delete

**Refactor → Safe Delete…** (⌘⌦) deletes the declaration whose name is at the caret, or the files
and folders selected in the Project tool window, but first asks the language servers for its
usages:

- **Nothing outside uses it:** it is deleted at once. A declaration goes with its whole text: the
  attributes, annotations and doc comment above it, and the blank line after it. Files go to the
  Trash through the same path as **Delete** (⌫), and the language servers that ask to hear of
  deletes are told first. One ⌘Z brings a declaration back; files come back from the Trash or
  Local History.
- **Something uses it:** the **Usages Detected** dialog says how many usages keep it from being
  deleted safely and lists them. **Delete Anyway** deletes it all the same, **View Usages** shows
  them on the **Refactoring Preview** tab of the Find tool window, where **Do Refactor** deletes
  it after all, and **Cancel** leaves it.

Usages inside the declaration itself (a recursive call) or inside the deleted files do not count.
For a file, the usages of its declarations from other files count. A file no language server
reads has no usages, so it is deleted at once.

### The refactoring dialog

Refactorings that need more than a name share one dialog: the fields the refactoring asks for
(text, check boxes, choices, and tables whose rows **Add**, **Remove**, **Move Up** and **Move
Down** change, or lists of members to tick), **Preview**, which lists every change it would make on the
**Refactoring Preview** tab (text changes by file and the files it would create, rename or
delete), **Cancel**, and **Refactor**. Before it changes anything, **Refactor** looks for
conflicts; when there are some, a list shows them with **Continue** (or the refactoring's own
word, such as **Delete Anyway**), **Show Conflicts in View**, and **Cancel**. Safe Delete and Rename
run through it, and language plug-ins add their refactorings to it: in Java, **Refactor → Move…**
(**F6**, in the editor and on the Project tool window's selection) and **Refactor → Change
Signature…** (**⌘F6**), see [Java](06-java.md#move-and-change-signature), and **Pull Members
Up…**, **Push Members Down…**, **Encapsulate Fields…** and **Extract/Introduce → Parameter
Object…**, **Interface…** and **Superclass…**, see [Java](06-java.md#class-refactorings). Where no refactoring
answers, F6 and ⌘F6 say so; F6 on a scratch file still moves it into the project.

Agents call `refactor.safeDelete.find` and `refactor.safeDelete.apply` (`force: true` deletes in
spite of usages), or the dialog's own `refactor.dialog.list`, `refactor.dialog.form`,
`refactor.dialog.preview` and `refactor.dialog.apply` with a refactoring's id (`safeDelete`,
`rename`, `javaMove`, `javaChangeSignature`, `javaPullUp`, ...).

**Settings → Tools → Actions on Save** can run Reformat Code and Optimize Imports on every save.

**Rearrange Code** works in Go files: it puts the top-level constants, variables, types and
functions in the order Code Style → Go → Arrangement sets, keeping the order of declarations of
the same kind and the comments directly above each, so the program does not change; a file it
cannot read safely (a brace that is not closed, code it does not know) is left as it is. Undo
brings the old order back. For agents: `editor.code.rearrange`, and `rearrange: true` on
`editor.code.reformat`.

## What saving does to whitespace

Saving a file (⌘S, Save All, closing GoGarden, a commit) first runs the
[Actions on Save](09-settings.md#tools--actions-on-save), then tidies the file's whitespace as
**Settings → Editor → General → On Save** says, and only then writes it:

- **Remove trailing spaces on: Modified lines** (on by default): the spaces and tabs at the end of
  the lines changed since the file was last saved or opened go; **All lines** takes them from every
  line. A line you changed back to what it was is no longer a modified line.
- **Keep trailing spaces on caret line** (on by default): a line with a caret keeps its trailing
  spaces, with several carets every caret line does, so saving never moves the text you are typing
  at. It also keeps the caret's line when trailing blank lines go.
- **Drop the blank lines at the end of a file when it is saved** (off by default): the empty and
  whitespace-only lines after the last line with text go, down to the caret's line.
- **End each file with a line break when it is saved** (off by default): a file that does not end with
  a line break gets one; an empty file stays empty.

Trailing spaces that mean something stay: two spaces or more at the end of a Markdown line (a hard
line break), and the spaces inside a string literal that goes on to the next line (a Go raw string
in back quotes, a Rust or JavaScript multi-line string), as the file's highlighting knows them; a
file the IDE does not highlight (a Python file, for now) has no such protection.
Line breaks are kept as the file has them: a CRLF file stays CRLF, and a line break added at the end
is the file's own.

All of it is one undo step: **⌘Z** after saving brings the spaces back (the file then has unsaved
changes again). A read-only file is never touched. A project's `.editorconfig` files win for the
files they match: `trim_trailing_whitespace = true` removes trailing spaces from every line,
`false` keeps them all; `insert_final_newline = true` or `false` turns the final line break on or
off. Agents read what applies to a file with `editor.onSave.options` and change the options with
the settings `removeTrailingSpaces`, `removeTrailingSpacesOn` (`modified` or `all`),
`keepTrailingSpacesOnCaretLine`, `removeTrailingBlankLines` and `ensureLineBreakAtEnd`.

## Context actions

**⌥⏎** (Show Context Actions) opens a list of what the file's language server can do at the caret:
the quick fixes of the problem there first, then intentions such as refactorings and source
actions. **↑** and **↓** choose, **Enter** applies, **Escape** closes. When the caret stays on a line
that has actions, a bulb shows in the gutter of that line, red when one of them fixes a problem; a
click on it opens the same list. ⌥⏎ anywhere on a line with a problem offers that problem's fixes,
and ways to [suppress it](22-tool-windows.md#suppressing-a-problem) where the language has them.

This list, completion, parameter info, quick documentation, the chooser of declarations and Show
Usages open next to the caret where it is now: when the caret has just moved off the screen, as when
an AI agent moves it and opens one at once, the popup appears next to the caret once the editor has
scrolled there, never where the caret was. Near the bottom of the window such a popup opens above
the caret. It follows the caret while the text scrolls or the font size changes; when the caret is
scrolled off the screen, the popup waits in the middle of the editor until the caret is back.

An action changes every file it needs or none, and one **⌘Z** undoes it in all of them; open files
keep the change unsaved. It never changes a file outside the project (scratch files excepted). A file that cannot be edited (one opened read-only because of its encoding) is
left as it is, with a message saying why. Actions come from the language server, so a file without
one has none, except one: with text selected, the list ends with **Create new scratch file from
selection** ([Scratch files](25-scratches.md)), and ⌥⏎ on a selection always opens this list.

## Generate

**⌘N** in the editor (Code → Generate…) lists what can be written for the code at the caret: first
the generators of the file's language (Go: **Constructor** inside a struct type), then the source
actions of its language server (Go: **Add test for …** on a function). **↑** and **↓** choose,
**Enter** runs one, **Escape** closes. Outside the editor ⌘N stays **New…**. **Override
Methods…** (**⌃O**) and **Implement Methods…** (**⌃I**), above Generate… in the Code menu, open the
member chooser of that generator at once when the language offers it (Java does; Go offers
Implement Methods), and say so when there is nothing to override or implement.

A generator whose name ends with **…** first opens a member chooser: the fields or methods it can
use, under the type they belong to, with check boxes. Everything works from the keyboard: **↑** and
**↓** move, **Space** checks or unchecks the member (on a type, all of its members), **⌘A** checks
them all, **Tab** and **⇧Tab** go to **Select All**, **Sort Alphabetically** and the generator's own
options (Go's constructor: **Return a pointer**), where **Space** turns them on or off; **Enter** is
**OK** and **Escape** cancels. A generator that needs a member refuses OK with none checked.

A generator that picks one thing from a long list (Go's **Implement Methods…**) asks with a search
field instead: type words of the name or package, **↑** and **↓** choose, **Enter** or a double
click picks.

The code is written formatted as the language writes it (gofmt's layout for Go, with the file's
indentation), the caret goes to it, and one **⌘Z** takes it away again, in every file it changed.
A read-only file is left as it is. A language plug-in adds its own generators through its language
server ([Plug-ins](10-plugins.md)); agents use `editor.generate.list`, `editor.generate.members` and
`editor.generate.apply`.

## Unwrap/Remove

**⇧⌘⌦** (forward Delete; Code → Unwrap/Remove…) lists the constructs around the caret, innermost
first, and takes one away:

- **Unwrap 'if…'**, **Unwrap 'for…'** (also `while` and `loop`), **Unwrap 'case…'** (or
  `default…`, in a `switch` or `select`), **Unwrap 'func…'** (a function literal, with the statement
  it is in, such as `go func() { … }()` or `t.Run("…", func(t *testing.T) { … })`), **Unwrap braces**
  and **Unwrap 'f(…)'** (a call with one argument) keep the body and drop what is around it. An `if`
  goes with its `else` branches; a case takes the whole `switch` with it.
- **Unwrap 'else…'** keeps that branch's body instead of the whole `if`; **Remove 'else…'** and
  **Remove 'case…'** drop that branch or case with its body and keep the rest.

Remove entries are on a background of their own. **↑** and **↓** choose, **Enter** or a click
applies, **Escape** closes; the code that stays is highlighted in the editor for the entry chosen or
under the mouse. When only one entry applies, ⇧⌘⌦ applies it at once. The body is re-indented to
where the construct was, the caret stays on its code, and one **⌘Z** undoes it all. Braces and
parentheses in strings and comments do not count, and the lines of a multi-line string keep their
indentation. It works in every language with C-like braces (Go, Rust, JavaScript, TypeScript,
Java...), without a language server.

## Spelling

The spelling inspection underlines misspelt words with a wavy line in the theme's **Typo** colour
(green in Zen Dark and Zen Light): in comments, in string literals, and in the names you declare.
It is never an error: the counter at the top right of the editor shows typos with their own mark,
**F2** and **⇧F2** step through them with the other problems, and the **Problems** tool window lists
them on its **File** tab as typos ("Possible misspelling: 'Hadnler'"), never under **Project Errors**.
Hovering a typo shows the same message.

- **Words.** Names are split at capitals, `_` and digits, so in `parseUrlHadnler` only `Hadnler`
  is underlined. A name is checked where it is declared (a function, a type, a variable, a
  parameter or a field), not where it is used, so `fmt.Sprintf` or a library's names are never
  marked. With a language server the declarations are its own; without one, a name after `func`,
  `type`, `var`, `const`, `fn`, `let`, `struct` and the like, or before Go's `:=`.
- **Skipped.** Words of one or two letters, words in capitals (`HTTP`, `MAX_SIZE`), words of one
  repeated letter (`aaaa`), URLs, e-mail addresses, file paths (`docs/guide.md`, `~/.zprofile`),
  dotted and qualified names (`fmt.Sprintf`, `main.go`, `egui::Context`), hexadecimal numbers and
  colours (`0xFF`, `3fa9c0d2`, `#ff00aa`), escapes and format verbs in literals (`\n`, `%d`),
  letters written against digits in comments and literals (`aarch64`) and words with letters
  outside English (other languages are not judged).
- **Dictionary.** English, bundled with the IDE and used offline: Webster's Second International
  with the modern and programming words it lacks; regular endings (`parsed`, `files`,
  `tokenizer`), prefixes (`unmarshal`), two words written together (`filename`) are known,
  and so are both American and British spellings (`color` and `colour`, `initialize` and
  `initialise`), so code and comments in either are left alone.
- **Plain text and Markdown** are checked word by word, code and link targets left out.

The check runs in the background, so typing is never slowed down, even in a file of ten thousand
lines; marks move with the text until the check of the new text is done. **Settings → Editor →
Inspections → Proofreading → Spelling** turns it off or chooses **Process code**, **Process
literals** and **Process comments** ([settings](09-settings.md#editor--inspections)).

### Fixing a typo

Put the caret on an underlined word and press **⌥⏎** (Show Context Actions):

- **Fix typo** (a word of a comment, a string or text), greyed, heads up to ten words close to it,
  the closest first, in the word's case (`Hadnler` gives `Handler`); pick one and it replaces the
  word, one change that **⌘Z** undoes.
- **Typo: Rename to '…'** (a name you declare) names the whole name with the word corrected
  (`parseUrlHadnler` gives `parseUrlHandler`) and renames it everywhere through the language
  server, as **Rename** (⇧F6) does; ⌘Z undoes it in every file. For another name, use Rename.
- **Save '<word>' to dictionary** saves the word, so it is no longer a typo anywhere. Unless
  **Use single dictionary for saving words** is on, **Select Dictionary** asks where:
  **application-level** (this IDE's, for every project) or **project-level** (the project's, shared
  with the team). Saved by mistake? **⌘Z** right away takes it out again (before you change the
  file), or remove it from **Accepted words** in the settings.

The typo's actions are offered on the word only; elsewhere on its line ⌥⏎ shows the line's other
fixes and the language server's actions.

### Dictionaries

Besides the bundled English dictionary, the check knows the words of:

- **the application-level dictionary**, this IDE's own (GoGarden and RustRonin each have theirs),
  for every project: `dictionaries/application.dic` in the IDE's data folder, one word a line;
- **the project-level dictionary**: the files of `.idea/dictionaries/` in the project, the same files
  JetBrains IDEs use, so the team shares the words through Git and a JetBrains IDE sees the words saved in
  GoGarden and the other way round. Words are saved to `.idea/dictionaries/project.xml`; only its
  word list changes, everything else in the file stays as it was;
- **custom dictionaries** you add in the settings: plain word lists (one word a line) and Hunspell
  dictionaries (a `.dic` file with its `.aff` beside it). They are only read.

Saved words are in lower case and cover their regular forms (`goroutine` also accepts
`goroutines`). A dictionary changed outside the IDE, by a JetBrains IDE or a `git pull`, counts within a
second. Save to dictionary asks which dictionary unless **Settings → Editor → Natural Languages →
Spelling** names one ([settings](09-settings.md#editor--natural-languages--spelling)).

**Hunspell.** The IDE reads the common part of the format: the character set (`SET`, UTF-8 or
ISO 8859-1), the flags (`FLAG long`, `num`, `UTF-8`, `AF` aliases), prefixes and suffixes (`PFX`,
`SFX`) with their conditions and cross products, `NEEDAFFIX` and `FORBIDDENWORD`. Compound words,
affixes on affixes, `ICONV`/`OCONV`, `IGNORE` and the suggestion tables are not read, and, as with
the bundled dictionary, only words in English letters count. No dictionary is downloaded: add the
files you have.

AI agents read a file's typos with `spelling.typos.list` (it waits for the check of the text as
it is now), see them as problems of severity `typo` in `problems.file.list` with their quick-fixes
(`problems.fixes.list`), judge words with `spelling.words.check`, get a typo and its suggestions
with `spelling.typo.get` (and `spelling.suggestions.list` for any word), change it with
`spelling.typo.change` or rename the name with `refactor.rename.apply`, save and remove words with
`spelling.word.save` and `spelling.word.remove`, read the dictionaries with
`spelling.dictionaries.get`, and change the settings with `set_settings` (`spelling`).

## Ligatures

The editor's bundled font can draw `!=` as `≠` and `<-` as `←`. That is off by default;
**Settings → Editor → Enable ligatures** turns it on.

## Font size

The size of the editor's text changes on the fly, for every editor or for the one in
front:

- **View → Increase Font Size in All Editors** (**⌃⇧.**) and **Decrease Font Size in All Editors**
  (**⌃⇧,**) make the text one point larger or smaller in every editor and write the size to
  **Settings → Editor → Font → Size**, so it stays after a restart. **View → Reset Font Size in All
  Editors** brings back 13 points.
- **View → Active Editor → Increase Font Size**, **Decrease Font Size** and **Reset Font Size**
  change the size of the file in front only. Other files keep the size of the settings; the file's
  own size is forgotten when you close it. These three have no shortcut by default; give them one in
  [Settings → Keymap](09-settings.md#keymap).
- With **Settings → Editor → General → Mouse Control → Change font size with Command+Mouse Wheel
  in:** turned on (off by default, Ctrl on Windows and Linux), **⌘** and the mouse wheel over the
  editor make the text larger (wheel up) or smaller (wheel down), in the **Active editor** or in
  **All editors**, as chosen there.

In Presentation Mode the actions for all editors change **Settings → Appearance & Behavior →
Presentation Mode → Font size** instead (from 8 to 96 points, Reset brings back 24), and the
active editor's size starts from it. A file open in two editor groups has one size in both.

The size goes from 6 to 48 points. After it changes, a label at the bottom of the editor shows the
new size and the default one, with **Reset** to go back to it, for a few seconds;
**Settings → Advanced Settings → Editor → Show the zoom level when it changes** turns the label off. This is the
editor's text only: the size of the interface is **Settings → Appearance & Behavior → Use custom
font**. A keymap imported from a JetBrains IDE keeps its shortcuts for these actions.

Agents change the size of the file in front with `editor.fontSize.increase`, `editor.fontSize.decrease`
and `editor.fontSize.reset`, read it with `editor.fontSize.get`, and change every editor with the
actions `view.editorFontSize.increase`, `.decrease` and `.reset` (`run_action`) or `set_settings`
(`editorFont.size`, `wheelFontSize`, `wheelFontSizeIn`, `zoomIndicator`).

## Copy and paste

**⌘C** and **⌘X** without a selection copy or cut the whole line.

### Paste from History

GoGarden remembers the texts you copy and cut in it, in the editor and in its other text fields, the
newest first. **⇧⌘V** (or **Edit → Paste Special → Paste from History…**) lists them, each by its
first line, with the chosen one shown in full under the list:

- type to filter the list, **↑** and **↓** to choose;
- **Enter** or a double click pastes the entry at every caret, and it becomes what **⌘V** pastes;
- **⇧↑**, **⇧↓** or **⌘**-click choose several entries, pasted one per line;
- **Delete** removes the chosen entries from the list, **Escape** closes it.

The list keeps the last 20 entries (**Settings → Editor → Limits**, up to 100) and only for as long
as GoGarden runs: it is never written to disk, and what other programs copy never gets into it.

## Tabs

Open files are tabs above the editor. **⌘W** closes the active one, and so does a middle click on a
tab; **⌥**-click on a tab's **×** closes all the other tabs instead. When they do not fit, the row
scrolls, and the arrow at its end lists every tab. **⇧⌘]** or **⌃→** selects the next tab, **⇧⌘[**
or **⌃←** the previous one, going round at the ends.

Right-click a tab for its menu:

- **Close Tab**, **Close Other Tabs** (all but this one and the pinned ones), **Close All Tabs**,
  **Close Unmodified Tabs** (the ones without unsaved changes), **Close All but Pinned**, **Close
  Tabs to the Left** and **Close Tabs to the Right** (sparing the pinned ones) and **Close All
  Read-Only Tabs** (files locked in the IDE, not writable on disk, or library sources), each for
  the tabs of its split;
- **Copy Path/Reference…** with Absolute Path, File Name, Path From Project Root and Path From
  Repository Root;
- the split items, **Move to Opposite Group** (the tab goes to the next split), and Unsplit and
  Unsplit All while the editor is split;
- **Pin Active Tab** (**Unpin Active Tab** on a pinned tab), **Keep Tab Open** (on a preview tab),
  **Configure Editor Tabs…**;
- **Reopen Closed Tab**, **Bookmarks →** (Toggle Bookmark, Toggle Bookmark Mnemonic…, Delete and
  Bookmark Open Tabs…, see [Bookmarks](03-navigation.md#files-folders-and-open-tabs)), **Open In →
  Finder** (Explorer on Windows, the file manager on Linux), which shows the file selected in its
  folder, **Open In → Terminal**, which opens a terminal tab in the file's folder, and **Local
  History**; agents do the same with `file.fs.reveal` and `view.project.openInTerminal`, each with
  the file's `path`.

**Window → Editor Tabs** has Select Next Tab, Select Previous Tab, Pin Tab, Keep Tab Open, the close actions and
Reopen Closed Tab for the split with the keyboard, then the split items; every one of them can get
a shortcut in Settings → Keymap.

A pinned tab shows a pin where the **×** is and moves to the start of the row, after the other
pinned tabs; dragging never puts an unpinned tab before a pinned one. Click the pin to unpin the
tab. Pinned tabs stay pinned when the tab moves to another split or the splits are merged, and are
kept with the project when it closes.

**Reopen Closed Tab** brings back the file of the tab closed last, in its split (the main one when
that split is gone) at its place in the row, pinned again if it was; each use goes one tab further
back. The last 30 closed tabs are kept per project, also when it closes; a file deleted since is
skipped. It has no shortcut: **⇧⌘T** is Go to Test.

Closing never loses what you typed. A tab with unsaved changes is saved as it closes, as **Save
All** (⌘S) saves it: with the **Actions on Save** (Reformat Code, Optimize Imports) when they are
on, and a version in Local History. This holds for ⌘W, the tab's ×, every Close item, the last tab of a
split, closing the project's window and quitting. When a file cannot be saved (its folder is
read-only or was deleted), its tab stays open with the changes, a notification says why, and the
project's window and the IDE stay open too.

Agents close tabs with `file.document.close`, `.closeOthers`, `.closeAll`, `.closeUnmodified`,
`.closeAllButPinned`, `.closeLeft`, `.closeRight` and `.closeReadOnly`; each answers the files closed
and the files saved, or fails saying which file could not be saved. They pin and unpin with
`editor.tab.pin` (`pinned`: `true`, `false`, or absent to toggle), reopen with
`file.document.reopenClosed`, cycle with `editor.tab.next` and `editor.tab.previous`, and move a tab
to the next split with `editor.split.moveToOpposite`.

### Settings for tabs

**Settings → Editor → General → Editor Tabs** (**Configure Editor Tabs…** in the tab menu and in
**Window → Editor Tabs**):

- **Tab placement**: Top, Bottom, Left, Right (a column beside the text) or None.
- **One row**, with **Scroll the tabs panel** or **Squeeze tabs** when they do not fit, or **Multiple
  rows**; **Show pinned tabs in a separate row**.
- **Show file icon**, **Show file extension**, **Show directory for non-unique file names**, **Mark
  modified** (an asterisk before the name), **Show full path on mouse hover**, **Close button
  position**.
- **Sort tabs alphabetically**, **Open new tabs at the end** (off, a new tab opens right of the active
  one).
- **Enable preview tab** (off by default): a single click on a file in the Project tool window, and
  the place where the debugger stops, show the file in one reused tab with an italic name. An edit, a
  double click on the tab or on the file, or **Keep Tab Open** turns it into an ordinary tab.
- **Tab limit** (30) for each editor group: past it, **Close unused** closes the least recently used
  tab and **Close unchanged** one without unsaved changes first. Pinned tabs and the active tab never
  close for the limit; changes are saved, and **Reopen Closed Tab** brings the tab back.
- **When the current tab is closed, activate**: the tab on the left (the default), on the right, or
  the most recently opened one.

**Settings → Advanced Settings → Editor Tabs** has **Keep pinned tabs on the left** (on by default)
and **Open declaration source in the same tab** (off by default): Go to Declaration replaces a tab
without unsaved changes that is not pinned.

Agents change all of this with `set_settings` and `editorTabs`, open a file in the preview tab with
`file.document.open` and `"preview": true`, and keep it with `editor.tab.keepOpen`.

## Autosave

GoGarden saves your modified files by itself, so what you typed is on disk
even if you never press ⌘S:

- **After a pause**: once you have not typed, clicked or scrolled for 15 seconds, every modified
  file is saved. While you keep working, the save waits.
- **When you leave**: switching to another application (⌘⇥, a click on another window) or to the
  **Terminal** tool window saves every modified file of every project window.

An autosave is an ordinary save: the **Actions on Save** (Reformat Code, Optimize Imports) run
when they are on, a formatter that fails or takes longer than a second gets a notification (once,
not again at every autosave while it keeps failing) and the file is saved anyway, the file's line
separators are kept, a file opened through a symbolic link is written through the link, and each
autosave keeps one version in **Local History**. Formatting never happens under your fingers: autosave runs only once
you have stopped typing or left the editor.

Autosave leaves some files alone, with their changes kept in the editor:

- a file that changed on disk since GoGarden read or saved it (another program, `git checkout`):
  it is never written over; the editor offers **Load File from Disk** or **Keep My Edits**,
  and after Keep My Edits autosave writes your text;
- a read-only file (no write permission, or a file GoGarden opened read-only because of its
  encoding);
- a file that is no longer on disk.

A file that cannot be written (its folder is read-only) gets a notification naming it; the other
files are saved. Autosave tries it again after your next pause, without a new notification, and
saves it once it can be written.

Saving on switching to a different application waits for the focus to leave GoGarden: moving
between GoGarden's own windows, detached tool windows among them, saves nothing.

Both kinds of autosave are on by default; turn them off or change the 15 seconds in **Settings →
Appearance & Behavior → System Settings → Files** (also reached from **Tools → Actions on Save →
Configure autosave options…**). Autosave cannot be turned off completely: a modified file is still
saved before its tab or project closes and before GoGarden exits or restarts.

Agents change the options with `set_settings` (`autosaveOnIdle`, `autosaveIdleSeconds`,
`autosaveOnFrameDeactivation`) and save at once with `file.document.autosave`, which answers the
files saved, the files skipped and why (`readOnly`, `conflict`, `missing`), and the files that
failed. The pause also counts an agent's edits: a headless GoGarden saves them 15 seconds after the
agent's last edit.

## Text with a preview

An SVG file opens as text, with its picture beside it. The switch at the
editor's top-right corner has three modes:

- **Editor Only**: the text alone.
- **Editor and Preview** (how SVG files open): the text on the left, the picture on the right; drag
  the line between them to share the width differently. **Change Split Orientation**, the button
  left of the switch, puts the picture under the text and back, for each file.
- **Preview Only**: the picture alone.

The mode is kept for each file, also after a restart: switching one SVG leaves the others as they
were, and a file you never switched opens as **Editor and Preview**. The three are in **Find
Action** (⇧⌘A) too. The picture follows the text as you type, before it is saved: it is drawn in the background,
so typing never waits for it, and the last picture stays until the new one is ready. When the text
is not a valid SVG, the preview says why instead. The preview's toolbar zooms as the image viewer's
does (**Zoom In** ⌘=, **Zoom Out** ⌘−, **Actual Size** ⌘/, **Fit Zoom to Window**, ⌘ with the scroll
wheel or a pinch), over a chessboard where the picture is transparent. Text in an SVG is not drawn
(no fonts), as in the image viewer. Other images still open in the [image viewer](08-images.md).

## Markdown preview

A Markdown file (`*.md`, `*.markdown`) opens with its preview beside it, with the same switch as an
SVG (**Editor Only**, **Editor and Preview**, **Preview Only**), its mode kept for each file. While
both are shown, **Change Split Orientation**, the button left of the switch, puts them one above the
other or back side by side; each file, SVG files too, keeps its own. The mode of files you never
switched, sync scroll and the preview's font size are in
[Settings → Languages & Frameworks → Markdown](09-settings.md#languages--frameworks--markdown).
The preview reads GitHub Flavored Markdown and is drawn by the IDE itself, in the theme's colours:

- **Text**: headings (with GitHub's anchors), emphasis, strikethrough, inline code, links, bare web
  addresses as links, nested lists, block quotes, rules, footnotes (numbered, gathered at the end).
- **GitHub alerts**: `> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` and `[!CAUTION]`, each with
  its colour, mark and title.
- **Tables** with their columns' alignment, a header and stripes.
- **Task lists**: a click on a checkbox ticks the task in the text (`- [ ]` becomes `- [x]`), one
  **Undo** step.
- **Code blocks** highlighted by the IDE's own highlighters, by the fence's language (`rust`, `go`,
  `bash`, `ts`, `json`…), with a **Copy** button at their top-right corner.
- **Math** in TeX, typeset by the IDE itself: inline (`$…$`) in the line of text, display
  (`$$…$$`, or a fenced block labelled `math`) centred on its own: fractions, sub- and superscripts,
  roots, Greek letters and symbols, sums, integrals and products with their limits, `\left…\right`
  delimiters, matrices, `aligned` and `cases`, `\text{…}`, `\mathbb{…}` and other alphabets. A
  formula that cannot be typeset (an unknown command, an unclosed brace) shows its source: inline
  underlined, with the reason when you hover it; a display formula with a note under it. Selected
  and copied, a formula gives its TeX (`$…$`, `$$ … $$`).
- **Mermaid diagrams**: a ```` ```mermaid ```` block is drawn in the theme's colours, on your machine
  and without the network: flowcharts and sequence, class and state diagrams among others. One the
  IDE cannot read keeps its source, with a note saying why. Selected and copied, a diagram gives its
  source.
- **HTML**: the safe part of it: bold, italic, code and keys, line breaks, links, images and
  `<details>` (shown open); scripts, styles and frames are never shown or run.
- **Images**: pictures next to the file (relative paths) are shown. A picture on the web is never
  downloaded until you allow its site: its frame offers **Load images from <site>**, remembered for
  the project.

Selecting text in the editor shows a floating toolbar above it: **Text** and **H1** to **H6**,
**Bold**, **Italic**, **Strikethrough**, **Code**, **Create Link**, and bullet, numbered and check
lists. A button takes its style off again when the text already has it. In Markdown files **⌘B** is
Bold, **⌘I** Italic, **⇧⌘S** Strikethrough, **⇧⌘C** Code and **⇧⌘U** Create Link (with Ctrl on Linux
and Windows), with a selection or on the word at the caret; everywhere else those keys keep their
usual actions, and all five are in Settings → Keymap. Advanced Settings → Markdown → **Hide the
formatting toolbar over a selection** turns the toolbar off.

The preview and the text scroll together: scrolling either brings the other to the same place (turn
**Sync scroll** off in the settings to scroll each alone). A
click on a paragraph, heading or table in the preview puts the caret on its line; a click on a link
follows it: `#anchors` scroll the preview, a relative file opens in the IDE (another Markdown file
at its anchor), a web address opens in your browser. The preview follows your typing, before
saving. The **Structure** tool window lists the headings, and sections and fenced code fold.

Text in the preview can be selected as in a browser: drag across paragraphs, lists, tables and code,
double-click a word, triple-click a paragraph or a whole table, or click in the preview and press
**⌘A**. **⌘C** copies the selection as plain text: list items keep their numbers and bullets, tasks
their `[x]` and `[ ]`, table cells are split by tabs and rows by lines, code keeps its lines and
indentation. The preview's context menu adds **Copy as Markdown**, which copies the source lines of
the selected blocks, and **Select All**. A click without a drag still puts the caret on the line and
follows links and checkboxes; it clears the selection, and the keyboard stays with the preview until
you click the editor. The three actions are in Settings → Keymap (**Select All in Markdown Preview**,
**Copy from Markdown Preview**, **Copy from Markdown Preview as Markdown**) and reachable by agents
(`view.markdownPreview.selectAll`, `.copy`, `.copyMarkdown`); `markdown.preview.getSelection` answers
the selected text and its Markdown source.

## Split editors

Right-click a tab and choose **Split Right** or **Split Down** (also in **Window → Editor Tabs**) to
show the file in a second editor beside or below the first one. **Split and Move Right** and **Split
and Move Down** move the tab there instead. Each split has its own tabs, its own carets, folds and
scrolling; a file open in two splits is one file, so what you type in one shows in the other at once.

- Click in a split, or use **Goto Next Splitter** and **Goto Previous Splitter** (no shortcut by
  default; give them one in Settings → Keymap), to move the keyboard there. A file you open goes to the split that has the keyboard, or to the
  split that already shows it.
- Drag a tab along its row to put it elsewhere in the row: it lifts with a shadow and stays under
  the pointer where you took it, the other tabs slide aside to open its new place, and it glides
  there when you let go. With Reduce motion on in
  the system settings, nothing slides. Agents use `editor.tab.move`. Drag a tab onto another split
  to move it there; drag the line between two splits to resize them. While tabs are sorted
  alphabetically, they stay in that order.
- Closing the last tab of a split removes the split. **Unsplit** (in the tab menu and Window →
  Editor Tabs) removes the split the current editor is in, keeping the tabs of both sides; **Unsplit
  All** brings every tab back into one editor.
- The splits are remembered per project and come back when you open it again.

Agents split and unsplit with the `editor.split.*` commands (`editor.split.right`, `.down`,
`.unsplit`, `.unsplitAll`, `.next`, `.previous`, `.focus`, `.moveTab`, `.resize`, `.list`), and
`get_window_state` tells how the editor is split. Commands that name no split act on the one that
has the keyboard; `file.document.open` takes a `group` to open a file in a given split.

## Detached editor windows

**Open Tab in New Window** in a tab's context menu, or **⇧F4**, moves the tab into a window of its
own with its own tab row; dragging a tab out of the IDE window does the same. The file stays one
document: edits, saves and reloads show wherever it is open. Drag a tab onto the main window to dock
it again. Closing a window's last tab closes the window; closing the window itself gives its tabs
back to the main window, so nothing unsaved is lost. Detached windows come back with their tabs,
size and place when the project opens again. A detached window has one tab row, without splits.

A file opened from a detached window, such as **Go to Declaration**, opens in the main window, unless
the window already shows it. **Settings → Advanced Settings → Editor Tabs → Open declarations reached from a detached window in the main window** turns that off.

Agents use `editor.window.openTab` (`group`, `index`, `bounds`), `editor.window.list`,
`editor.window.close` and `editor.split.moveTab` with the window's `group`.

## Workspaces

A workspace is a full tool that opens over the editor and closes back without closing anything,
such as the [Database workspace](28-database.md). While it is open it takes the editor's place, with its own
toolbar, a panel on the left and tabs of its own; the tool windows, the stripes, the toolbar and
the status bar stay. Your splits, tabs, carets and scrolling wait untouched underneath and come
back exactly as they were. Opening any code file (from Project, Recent Files, Go to File, Find in
Files, Problems) puts the workspace away and shows the file. Its text tabs are real editors, with
undo, find and completion as in code; a middle click or × closes a tab, dragging a tab along the
row reorders it, and a tab cannot be dragged between a workspace and the editor yet. Distraction
Free, Zen and Presentation modes keep an open workspace open, with its row of tabs, and Find Action (⇧⌘A) opens and
closes it there by name ("Database Workspace").

Each workspace has a button at the top of the right stripe, under the Git workspace's: a click opens
it, and the button stays lit while it is open; a second click puts it away. **⇧⎋** puts it away too
while you work in it (when a tool window has the keyboard, ⇧⎋ hides that tool window as before);
**⎋** never does. **⌘W** closes the workspace's active tab, never a code tab hidden beneath it. The
Switcher (⌃⇥) and Recent Files (⌘E) list a workspace's tabs too, and picking one opens the workspace
on it. Commands that need a text file say so when a workspace tab that is not a text file is in
front.

A workspace comes back after a restart as you left it: open or not, its tabs in order, each
console's caret and scroll. Its tabs belong to the project: a window that switches to another
project keeps them with the first one and shows them again, put away, when it switches back.
Nothing connects when it comes back. Consoles are saved a second after
you stop typing, even with autosave off, so their text survives a crash. Details:
[Database workspace](28-database.md).

Agents open, close and drive workspaces with `workspace.host.list`, `.state`, `.expand`,
`.collapse` and `.toggle`, and their tabs with `workspace.tab.open`, `.select`, `.close` and
`.move` (see [Automation](../automation.md#database)).

## Line separators

The status bar shows the open file's line separator, next to the caret position: **LF** (Unix and
macOS), **CRLF** (Windows) or **CR** (classic Mac OS), read from the file's first line break, so a
file that mixes them shows the first one. Typing **Enter**, pasting, completion, live templates and
formatting all write new lines with that separator, and saving keeps it (unless an `.editorconfig`
`end_of_line` says otherwise).

Click it to choose **CRLF - Windows (\r\n)**, **LF - Unix and macOS (\n)** or **CR - Classic Mac
OS (\r)** (the current one is greyed out); **File → File Properties → Line Separators** has the
same items. The whole file is rewritten with the new separator and saved at once, unsaved changes
included; **⌘Z** brings the old separators back in one step, and Local History keeps the text before
and after (**Change Line Separators**). With the keyboard in the Project tool window, the menu
changes the selected files instead, and a selected folder changes all its text files, folders inside
it included (not `.git`, nor `target`, `node_modules`, `dist`, `build` and `vendor`, nor the folders
and names the project excludes); links, read-only files, files over 16 MB and files that are not
UTF-8 text are left as they are and listed as skipped. When the file cannot be written (a read-only
folder, say), nothing changes: the editor keeps the text and its unsaved changes, and the error
says why.

A new file, and a file without any line break, takes the separator of Settings → Editor → Code
Style → **Line separator** (System-Dependent by default: LF on macOS), or `end_of_line` of the
project's `.editorconfig` files when they set it for the file and EditorConfig support is on
([Editor → Code Style](09-settings.md#editor--code-style)); with `end_of_line` set, saving a
modified file also rewrites its line breaks with it ([Saving and
encodings](09-settings.md#saving-and-encodings)). Agents read the separator with
`file.lineSeparator.get` and change it with `file.lineSeparator.set`.

## Files changed outside GoGarden

Files changed by other programs show up at once: the project tree follows, and open files without
unsaved changes reload (**⌘Z** brings the old text back). If a file with unsaved changes changes on
disk, a banner above the editor offers **Load File from Disk** or **Keep My Edits**. This
happens while the window is covered by other windows, minimised or hidden as well, not only when you
look at it. After a big checkout, when the system says it lost track of what changed, GoGarden
compares every open file. If it cannot follow the project's folder at all, a notification says so and
it keeps trying.

Saving never writes over such a change. Every save compares the file on disk with what GoGarden last
read or wrote, just before writing: **⌘S**, **Save All**, closing a tab, quitting, the save before a
commit and the saves of AI agents. When they differ, the save stops with an error naming the file,
your changes stay unsaved, nothing is formatted, and the banner asks which to keep: **Load File from
Disk** takes the file (**⌘Z** brings your text back), **Keep My Edits** lets the next save write
your text. The other files of a Save All are saved. A file that was deleted is written again.

## Read-only files

A lock at the right of the status bar shows whether the open file can be edited: open for a
writable file, closed for a file that is read-only on disk or that cannot be edited for another
reason, such as bytes that are not text in the file's encoding (hover over the lock to see why).

- **Click the lock**, or choose **File → File Properties → Make file read-only**, to turn the file's
  write permission on disk off; unsaved changes are saved first, and if they cannot be saved the
  file stays writable and an error says why. The same item then reads **Make file writable** and
  turns the permission back on, as does a click on the closed lock: the file gets back the write
  permission it had (for its group too); for a file made read-only elsewhere, its owner gets it. A
  file that cannot be edited because of its encoding is not changed by a click. A file you may not
  change (another user's, or on a read-only disk) stays as it is, with an error that says why.
- **Typing into a read-only file** changes nothing; a notification says the file is read-only and
  offers **Make Writable**. Replace in Files, renames and other refactorings leave read-only files
  alone too and say which file is read-only.
- A file made read-only or writable by another program is followed as soon as the IDE notices the
  change on disk; once it is writable, the notification is put away.

Agents use `file.document.toggleReadOnly` (`readOnly: true` or `false`, or nothing to turn it the
other way; `path` for another file) and read the state with `file.document.readOnlyState`.
