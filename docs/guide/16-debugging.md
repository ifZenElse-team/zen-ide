# Debugging

GoGarden debugs Go programs and tests with Delve: stop at breakpoints, step through the code, and
look at goroutines, stack frames, variables and watched expressions in the Debug tool window.
Java programs and tests are debugged the same way in every Zen IDE, with java-debug, which comes
with the Java plug-in's language server ([Java](06-java.md#debug)), which also reloads changed
classes into the running program (**Run → Reload Changed Classes**). JavaScript and TypeScript are
debugged with Node.js itself, with the [Node.js plug-in](06-web.md#nodejs). The rest of this page
shows Go.

## Install Delve

The debugger is Delve (`dlv`), version 1.20 or later. GoGarden looks for it next to the SDK's `go`,
in `GOBIN`, in `GOPATH/bin` and on `PATH`; **Settings → Languages & Frameworks → Go → Delve** shows
the one in use and takes a custom path. If it is missing, starting a debug session shows a
notification with **Install Delve**, which runs `go install github.com/go-delve/delve/cmd/dlv@latest`;
that downloads code, so GoGarden never installs it by itself. You can also install it yourself from
a terminal.

## Breakpoints

Click a line number in the gutter, or press **⌘F8** on a line, to set a line breakpoint: a red dot
in place of the number. Click it again to remove it. Breakpoints move with the lines when you edit
the file. Breakpoints belong to you, not to the project: they are kept with the window's state and
come back when GoGarden reopens the project, with all their properties, and they are never written
to `.zen/`. Changing a breakpoint while a session runs takes effect at once.

| In the gutter | Does |
|---|---|
| Click a line number | Sets a breakpoint, or removes the one there |
| **⇧-click** a line number | Sets a logging breakpoint that does not stop the program: it logs "Breakpoint reached at main.go:12" to the console. ⇧-click no longer selects the line; a click beside the numbers still does |
| **⌥-click** a breakpoint | Disables it, or enables it again; a middle click does the same |
| Drag a breakpoint | Moves it to another line, with its properties; **⌘-drag** copies it |
| Right-click a breakpoint | Opens its properties (below) |

**Settings → Build, Execution, Deployment → Debugger → Remove breakpoint** chooses how a breakpoint
is removed: **Click** (the default), or **Drag it into the editor, or middle-click it**;
then a click on a breakpoint leaves it, dragging it off the gutter or a middle click removes it.

The mark shows the state: a red dot, a red dot with **?** when it has a condition, an orange dot for a
breakpoint that does not stop the program, a red ring when it is disabled (or waits for another
breakpoint), a red dot with a cross when the debugger could not set it (pointing at it shows why),
and grey dots while breakpoints are muted.

**Run → Toggle Breakpoint** has **Toggle Line Breakpoint** (⌘F8), **Toggle Temporary Line
Breakpoint** (⌥⇧⌘F8: a breakpoint removed once it stops the program) and **Toggle Breakpoint
Enabled**. Right-click the gutter beside a breakpoint for **Remove Other Breakpoints** and **Disable
Other Breakpoints**. **Add Conditional Breakpoint…** and **Add Logging Breakpoint…** are in
**Settings → Keymap** without a shortcut: they set a breakpoint at the caret and open its properties
at the condition or the expression to log.

### Breakpoint properties

Right-click a breakpoint to change it in a popup; every change applies at once, in a running session
too:

- **Enabled**.
- **Suspend**: whether the program stops. With a debugger that can let threads run alone, **All**
  stops every thread and **Thread** only the one that hit the breakpoint.
- **Condition**: an expression that must be true for the breakpoint to stop. While you type, the
  language server suggests what can be typed at the breakpoint's line (↑ ↓ to choose, Enter or Tab
  to take one, Esc to close the list); **Evaluate and log** suggests the same way.
- With Suspend off, **Log**: the **"Breakpoint hit" message**, the **Stack trace**, and **Evaluate
  and log** an expression; and **Remove once hit**.

**More** (⇧⌘F8) opens the Breakpoints dialog at that breakpoint; **Done** closes the popup.

### The Breakpoints dialog

**Run → View Breakpoints…** (⇧⌘F8) lists every breakpoint by kind: line breakpoints (by your groups,
then the others), function breakpoints, exception breakpoints and data breakpoints, each with a
check box to enable it. Select one to see its properties on the right, with the code around a line
breakpoint; **Go to Source** (or a double click) opens the line. **+** adds a function breakpoint
(by name, such as `main.run`) or an exception breakpoint the debugger offers; **−** removes the
selected one. Right-click a line breakpoint for **Move to Group** (an existing group, **New
Group…** or **No Group**) and **Edit Description…**.

A line breakpoint has, beside the popup's properties:

- **Log** and **Remove once hit**, also for a breakpoint that stops.
- **Disable until hitting the following breakpoint**: the breakpoint stays off in a session until
  the chosen one is hit; **After hit** it waits again (**Disable again**) or stays on (**Leave
  enabled**).
- **Pass count**: how many hits it takes to stop, as the debugger reads it (Delve takes `5`, `> 5`
  or `% 2`); shown when the debugger has hit conditions.

A debugger without log points still logs: GoGarden stops the program at the breakpoint, writes the
log lines to the console and lets the program run on. Fields a debugger cannot use (pass count,
Thread, conditions of exception breakpoints) are hidden while its session runs.

## Exception breakpoints

A debugger that can stop where an exception is thrown (Java's java-debug can: **Caught Exceptions**
and **Uncaught Exceptions**) offers its kinds in the Breakpoints dialog's **+** once a session of
it has run; turn one on with its check box, and give it a condition when the debugger takes one.
The choice holds for the window's next sessions and takes effect at once in the current one. Delve
offers none: it stops on an unrecovered panic by itself. Agents use
`debug.exceptionBreakpoint.add` and `debug.exceptionBreakpoint.list`.

## Mute Breakpoints

**Mute Breakpoints**, the toggle at the end of the Debug toolbar, lets the program run past every
breakpoint without removing them: the debugger gets no breakpoints while it is on, and the gutter
shows them grey. Click it again to let them stop the program.

## Data breakpoints

A debugger with data breakpoints can stop when a variable of the stopped
program is written: agents add one with `debug.dataBreakpoint.add`, and the Breakpoints dialog
lists it while the session lasts. Delve and java-debug have none.

## Start debugging

- **⌃D** debugs the selected run configuration, as **⌃R** runs it.
- **⌃⇧D** debugs what is at the caret: the test or `main` function around it.
- **Debug** (the bug) in the run widget, **Run → Debug**, and **Debug '<name>'** in the menu of a
  run icon in the gutter do the same.

GoGarden builds the program or the test binary without optimisations, in the run's own folder, with
the build shown in the console, and then starts Delve on it. Delve connects back to GoGarden on the
loopback interface; nothing listens for other programs and nothing leaves the machine. When Delve
cannot start the program, the console says why, and for the problems GoGarden knows (no
`debugserver` on macOS, a Delve too old for the Go version, a Go version too old for Delve, no
permission to trace, a program without debug information) what to do about it.

## Step through the code

When the program stops at a breakpoint, GoGarden opens the file at the line, highlights the line
with an arrow in the gutter, and opens the Debug tool window. Then:

| Action | Shortcut |
|---|---|
| Step Over: run the current line | F8 |
| Step Into: go into the function called on the line | F7 |
| Step Out: run to the end of the current function | ⇧F8 |
| Force Step Over: run the current line, passing every breakpoint in the functions it calls | ⌥⇧F8 |
| Force Step Into: go into the function called on the line; with Delve and other debuggers that step into every function, the same as Step Into | ⌥⇧F7 |
| Smart Step Into: choose which of the calls on the line to go into | ⇧F7 |
| Run to Cursor: run to the line of the caret, or to a breakpoint on the way | ⌥F9 |
| Force Run to Cursor: run to the line of the caret, passing every breakpoint on the way | ⌥⌘F9 |
| Resume Program: run to the next breakpoint | ⌥⌘R |
| Show Execution Point: back to the line where the program stopped | ⌥F10 |
| Pause Program | the toolbar button |
| Stop | ⌘F2 |

The force actions are in **Run → Debugging Actions**. They ignore line, function and data
breakpoints only for their own run: the breakpoints are back as soon as the program stops.

### Smart Step Into

When a line calls several functions, such as `total := add(twice(n), 4)`, **Smart Step Into**
(**⇧F7**, **Run → Debugging Actions**) lists the calls on the line in a popup under the caret.
Move through them with **↑** and **↓** or the pointer: the editor frames the chosen call on the
line. **Enter** or a click steps into it; **Esc** closes the popup without stepping. With one call
on the line it steps into it at once, and with none it is a plain Step Into.

Smart Step Into needs a debugger that lists the calls of a line (the Debug Adapter Protocol's step
in targets). lldb-dap from the Command Line Tools does not offer it and Delve does not document it,
so the action is grey for them; it works with any debugger that has it, for example from a plug-in.

While the program is stopped, resting the pointer on a line of the editor shows a small **Run to
Cursor** button at the line's start: click it to run to that line. **Advanced Settings → Debugger →
Show a Run to Cursor button on lines while debugging** turns the button off.

## Values in the editor

While the program is stopped, the editor shows the values of the selected frame's variables at the
end of the lines of its function, up to the line where the program stopped: `count = 3, name =
"Ann"` in grey after the code. A value that changed since the previous stop in the same function
is shown in another colour. Long values are cut after 80 characters; the Variables pane shows them
whole. **Settings → Build, Execution, Deployment → Debugger → Data Views → Show values inline**
turns them off.

## The Debug tool window

**⌘5**, or the bug icon on the left stripe, shows the window. It
opens by itself when a debug session starts and whenever the program stops. Each session has a tab
with its state (a green bug while it runs, a pause icon while it is stopped) and **×**: closing the
tab of a session that still runs stops it. Debug sessions do not appear in the Run tool window.

Under the tabs, **Threads & Variables** and **Console** switch the content, and the toolbar holds
**Rerun**, **Stop**, **Resume Program**, **Pause Program**, **Step Over**, **Step Into**, **Step
Out**, **Run to Cursor**, **Reset Frame** (only with a debugger that can reset frames) and the
**Mute Breakpoints** toggle; a button is grey when the session's state does not allow it, and
hovering it shows its shortcut.

- **Threads & Variables**: on the left, the goroutine the program stopped in (pick another one in the
  list above the frames; the first 100 are listed) and its stack frames. Click a frame to see its
  variables and its line in the editor. On the right, the variables of the selected frame, by
  scope: click the arrow (or press →) to see the fields or elements of a value; long slices and maps
  are read 100 at a time, with **…more** for the next ones.
- **Async stack traces**: when the debugger gives the frames of the awaited or scheduled call that
  led to the current one, they follow the current stack below a grey separator with the debugger's
  name for it (or **Async stack trace**). They can be selected like the others, but not reset.
  Frames the debugger plays down, such as runtime internals, are grey.
- **Console**: the build's output and the program's output, with clickable file links, as in the
  Run tool window.

## Reset a frame

**Reset Frame** (formerly Drop Frame) drops the frames above the selected one and runs that frame
again from its start, as if the function had just been called: select a frame and press the
toolbar button or choose **Run → Debugging Actions → Reset Frame**, or right-click a frame and
choose **Reset Frame**. The program stops at the start
of the frame. Only the frame's own variables go back: what the dropped code changed elsewhere, in
memory, files or the network, stays changed.

Reset Frame needs a debugger that can restart frames (the Debug Adapter Protocol's restart frame);
without one the button and the frame's menu item are not shown and the Run menu's item is grey.
lldb-dap does not offer it and Delve does not document it, so it is hidden for Go and Rust today. A frame the debugger cannot restart, and the frames of an async
stack trace, have the item grey.

## Collections, maps and arrays

With a debugger that can show them by their content (java-debug for Java can), a collection, a map
or an array opens to its **elements**, not to the fields that hold them inside: a list shows `0`,
`1`, `2`… with the values, an array its indexes, and a map its entries as **key → value** (`"ann" →
30`). An object whose text the debugger gives as well is shown as `{Integer@79} 3`: the object in
grey, then its text. This works the same in the Variables pane, the watches, the Evaluate dialog and
the value popups.

To see what is inside, right-click a collection and choose **Show Raw Fields**: that one shows its
fields (`elementData`, `size`…), the others keep their elements; choose it again to go back. **Show
Raw Fields of Every Collection** in the same menu switches all of them, as does **Settings → Build,
Execution, Deployment → Debugger → Data Views → Enable alternative view for Collections classes**
(on by default; off shows the raw fields). Debuggers that cannot switch show what they give either
way.

## Trace a stream chain

For a Java stream on the line where the program stopped, such as
`names.stream().filter(n -> n.length() == 3).map(String::length).collect(Collectors.toList())`,
**Run → Debugging Actions → Trace Current Stream Chain** opens the **Stream Trace** dialog: one
column for the stream's source and each operation, with the values after it (the first 100, and how
many there are in all), and a last column with what the terminal operation returns. The chain may
span several lines; the columns fill one after another while the debugger works, and the window
never waits. The dialog closes when the program goes on.

To get the values, the IDE evaluates the chain in the stopped program once per column, up to that
operation, collecting the elements with `toArray()`. So:

- the stream must be made in the statement (`list.stream()`, `Stream.of(…)`, `Arrays.stream(…)`,
  `IntStream.range(…)`…): a stream kept in a variable can be used only once, and is refused;
- a chain with `peek(…)` is refused, because tracing would run its side effects again; a terminal
  `forEach(…)` is not run, its column says so;
- an infinite stream (`Stream.generate`, `Stream.iterate` without a condition, `random.ints()`) is
  traced only with a `limit(…)` or `takeWhile(…)`, and only from there on;
- the lambdas run again for each column: keep them free of side effects.

Only Java is traced; Kotlin sequences and other languages are not.

## Change a value

When the debugger can change variables, select a variable in the Variables pane and
press **F2**, or right-click it and choose **Set Value…**: the value becomes a field. Type the new
value in the program's language (`42`, `"text"`) and press **Enter**; **Esc** leaves the value as it
was. The variables, the watches and the values in the editor are read again. When the debugger
refuses the value, the reason is shown in red under the variable. **F2** on a watch still edits the
watch's expression. The action is not offered when the debugger cannot change variables.

## Watches

The field at the top of the variables, **Type an expression: ⏎ evaluates it, ⇧⌘⏎ adds it to the watches**, does
both: **Enter** evaluates the expression in the selected frame and shows its value under the field
(open it with the arrow to see its fields), and **⇧⌘Enter** or **+** adds it to the watches.
Watches are shown above the variables and are evaluated again in the selected frame at every stop;
an expression that cannot be evaluated shows the reason in red. Select a watch and press **F2** (or
double-click it) to edit it, or press **Delete** (or click **−**) to remove it. Watches, like
breakpoints, come back when GoGarden reopens the project.

## Evaluate expressions

While the program is stopped:

- **Run → Debugging Actions → Evaluate Expression…** (**⌥F8**) opens the **Evaluate** dialog with
  the selection, or the name at the caret, in its field. Press **Enter** (or click **Evaluate**) to
  evaluate it in the selected frame; the result is a tree like the variables, read as you open it.
  The language server suggests what can be typed at the frame's line, as in a breakpoint's
  condition. **▾** lists the recent expressions of the project, the newest first; **Add to
  Watches** adds the expression to the watches. The dialog does not block the window: you can
  step, edit and open more dialogs while it is open; **Esc** in its field or **×** closes it.
- **Quick Evaluate Expression** (**⌥⌘F8**, or **⌥-click** on a name in the editor) shows the value
  of the selection, or of the name at the caret, in a popup at the expression; **Esc** or a click
  elsewhere closes it.
- Resting the pointer on a name in the editor shows its value in a tooltip: a name with what it is
  selected from (`order.items`), never a function call. The debugger must be able to evaluate
  values for hovers (Delve can).
- **Run → Debugging Actions → Show Execution Point** (**⌥F10**) selects the frame where the program
  stopped and opens its line.

Evaluation never makes the window wait: a value that takes long shows "Evaluating…" until the
debugger answers. A value longer than 1,000 characters is shown cut, ending with "…", in the
variables, the watches, the dialog and the tooltip. **Settings → Build, Execution, Deployment → Debugger → Data Views** turns the value
tooltip on or off and sets its delay (700 ms by default); **Advanced Settings → Debugger → Recent expressions to remember** sets how many expressions the project keeps (50 by default). The
recent expressions come back when GoGarden reopens the project.
