# Scratch files and buffers

Scratch files are throwaway files for notes, snippets and quick experiments. They live outside your
projects, in the IDE's own data folder, so every project sees the same scratches and none of them
ends up in version control.

## Creating a scratch file

**File → New → Scratch File** (⇧⌘N) opens a list of languages: type to filter it, use ↑ ↓ and press
⏎. The languages you used last come first, then the others by name. The IDE creates `scratch.go`
for Go, `scratch.json` for JSON and so on; the next Go scratch is `scratch_1.go`, then
`scratch_2.go`. The list holds Plain Text, Go, Rust and the language of every plug-in that is
switched on (HTML, CSS, JavaScript, TypeScript, JSON, YAML, Markdown, SQL...).

To start from code you already have, select it in the editor, press ⌥⏎ (Show Context Actions) and
choose **Create new scratch file from selection**: the new scratch holds the selection and gets the
language of the file it came from.

A scratch file is highlighted, completed and checked like any file of its language. Where the
language has a language server, the server serves the scratch too: a Go scratch gets gopls even
though it is not in any Go module, and it checks all Go scratch files together as one package: two
Go scratches that both declare `main` report "main redeclared". A Go scratch that imports more than
the standard library is checked inside the project's Go module instead, the same way it runs (below):
its imports resolve, Go to Declaration reaches the project's code, and it is a package of its own.

A new Go scratch starts as a program, `package main` with `func main()`, the caret inside it.

## Running a scratch file

A scratch file with an entry point runs on its own, with the project's toolchain: a green ▷ in its
gutter next to `func main` (Go), `fn main` (Rust) or the class with `main` and its `main` method
(Java), **⌃⇧R** anywhere in it, and **Run '<name>'** at the top of its context menu under
Scratches and Consoles. The output goes to the Run tool window, and the run is kept as a temporary
configuration, which **Edit Configurations…** can save or change (program arguments, environment,
working directory: the project folder by default).

- **Go** builds the scratch alone with the project's Go SDK (`go build` of that one file, so other Go
  scratches with their own `main` do not get in the way) and runs the program. A scratch that imports
  more than the standard library builds inside the project's Go module, so its `go.mod` and `go.sum`
  resolve the imports and the project's own packages can be used; the scratch is mapped into the
  module with `-overlay`, and nothing is written to the project. In a project you have not trusted
  it builds alone.
- **Rust** compiles it with the `rustc` of the project's toolchain (edition 2024; the configuration's
  **Edition** changes it) and runs the program.
- **Java** runs it with the single-file source launcher of the project JDK (`java scratch.java`, JDK
  11 and later). Turn on **Use the classpath of the module** in the configuration to compile against
  the project: the module's classpath is made first, as for an Application, and the scratch runs
  with it.
- **Kotlin** (with the Kotlin plug-in) is compiled by the `kotlinc` installed on your computer, with
  the Kotlin runtime, into a JAR that the project JDK runs; **Use the classpath of the module**
  works as for Java. The IDE does not ship or download the Kotlin compiler: without `kotlinc`
  (`KOTLIN_HOME`, PATH, Homebrew or SDKMAN) the run says what is missing
  ([Kotlin](06-kotlin.md#kotlin-repl-and-kotlin-scratch-files)).

**View → Show Bytecode** works in a Java scratch file too: the file is compiled alone with the
project JDK's `javac` and its class shown ([Java](06-java.md#show-bytecode)).

The built program and other temporary files go to the run's own folder in the system's temporary
folder, readable only by you, which is removed when the run ends.

Closing a scratch file while it is empty deletes it, so trying the command costs nothing. A scratch
file that has text saved on disk, or that another IDE window still shows, is kept.

## Scratch buffers

**New Scratch Buffer** (in Find Action, ⇧⌘A) opens a plain-text buffer: `buffer1.txt`, then
`buffer2.txt`, up to `buffer5.txt`. When all five exist, the next one is used again in turn; if it
still has text, the IDE asks whether to **Clear** it or **Keep** what is there. A buffer you rename
is never used again as a buffer, so renaming one keeps it for good.

**Show Scratch Files** (in Find Action) lists every scratch file and buffer; type to filter and press
⏎ to open one.

## Scratches and Consoles in the Project tool window

The Project tool window shows **Scratches and Consoles** under the project and its External
Libraries, with **Scratches** and, once there are some, **Buffers**. Right-click a scratch file for:

- **Change Language…**: the language list again; the file gets the new language's extension;
- **Rename…** (⇧F6) and **Delete…** (⌫, to the Trash);
- **Move…** (F6): moves the scratch file into a folder of the project, which you type in the dialog
  (the project folder by default); its tab follows it;
- **Copy Path**, **Open in Right Split** and **Open In › Finder**.

To hide the node, turn off **Settings → Advanced Settings → Project View → Show Scratches and
Consoles in the Project tool window**.

## Where they are kept

Scratch files are in the `scratches` folder and buffers in the `buffers` folder of the IDE's data
folder: on macOS `~/Library/Application Support/space.ifzenelse.gogarden/scratches` for GoGarden
(each IDE has its own). Find in Files searches them with the **Scratches and Consoles** scope.

## For AI agents

Every action here is a command (see [AI agents](11-ai-agents.md)): `file.scratch.languages`,
`file.scratch.new { language, text? }`, `file.scratch.fromSelection`,
`file.scratch.newBuffer { clear? }`, `file.scratch.list`,
`file.scratch.changeLanguage { path, language }`, `file.scratch.move { path, directory }` and
`file.scratch.run { path, waitMs? }`; `run.targets.list` and `run.context.get` give a scratch file's
single-file run;
`file.fs.rename` and `file.fs.delete` take scratch files too.
