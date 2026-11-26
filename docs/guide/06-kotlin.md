# Kotlin

Kotlin is a plug-in for every Zen IDE: JvmJizo comes with it, and the other IDEs offer it when a
project needs it: opening a `.kt` or `.kts` file offers the Kotlin plug-in in a banner, and the
Marketplace has it ([Plug-ins](10-plugins.md)).

## What the plug-in reads itself

With the plug-in installed, Kotlin files (`.kt`) and Kotlin scripts (`.kts`) are read by the IDE's
own Kotlin reader, so this works at once, without a language server:

- **Highlighting:** keywords and modifiers; strings with their `$name` and `${…}` templates (the code
  inside a template is highlighted as code); raw strings; numbers; comments and KDoc; annotations;
  labels; types; and function names.
- **Folding:** blocks, block comments and the imports.
- **The Structure view (⌘7):** classes, interfaces, objects, enum classes, type aliases, functions,
  properties and constants, nested as declared.

The plug-in also brings, without a language server:

- **Live templates** ([Live templates](02-editor.md#live-templates)): `main`, `maina` (with
  `args`), `maino` (`@JvmStatic` in an object), `sout`, `serr`, `soutv` (a value with its name),
  `fun0`, `fun1`, `fun2`, `void`, `ifn`, `inn`, `iter`, `fori`, `interface`, `singleton`,
  `closure`, `anonymous`, `exfun`, `exval` and `exvar`, listed in Settings → Editor → Live
  Templates under Kotlin, read-only: switch one off, or **Duplicate** it to change it.
- **Postfix completion** after an expression and a dot: `x.val`, `x.var`, `ok.if`, `ok.else`,
  `ok.not`, `x.nn` and `x.notnull`, `x.null`, `items.for` and `items.iter`, `ok.while`, `x.when`,
  `x.return`, `e.throw`, `x.sout`, `call().try`, `x.cast` (`as`), `x.par`, `x.lambda`, `x.let`
  (`?.let { }`), `x.arg`, `array.spread` and `ok.assert`
  ([Postfix completion](02-editor.md#postfix-completion)).
- **Surround With** (**⌥⌘T**): **if**, **if / else**, **while**, **do / while**, **for**,
  **try / catch**, **try / finally**, **try / catch / finally**, **synchronized**, **Runnable**, a
  function literal, and for a selection **(expr)**, **!(expr)** and **"${expr}"**.
- **KDoc stubs:** `/**` and **Enter** above a function, class, object, interface, property or type
  alias puts `/** … */` with an `@param` line per parameter (of a class's primary constructor too),
  `@return` when a function declares a result other than `Unit`, and an `@throws` line per class in
  its `@Throws(...)` annotation. Type parameters and a receiver (`fun <T> List<T>.second()`) are
  passed over to the function's name.
- **Complete Current Statement** (**⇧⌘⏎**) closes the brackets and adds the block of an `if`, a
  loop, `when`, `try`, `catch`, a `fun`, class, interface or object, with the condition put in
  parentheses when it has none; Kotlin statements need no `;`
  ([Complete Current Statement](02-editor.md#complete-current-statement)).

Gradle's Kotlin DSL (`build.gradle.kts`, `settings.gradle.kts`) is a Kotlin script, so it is
highlighted the same way. Its top-level blocks (`plugins`, `dependencies`,
`tasks.register("hello")`) are listed in the Structure view
([Maven and Gradle](26-maven-and-gradle.md)).

## The Kotlin language servers

Problems as you type, completion, Go to Declaration, Find Usages, Rename (⇧F6), Reformat Code
(⌥⌘L) and Optimize Imports (⌃⌥O) come from a Kotlin language server. There are two, and you choose
one in **Settings → Languages & Frameworks → Language Servers**, under **Server for Kotlin**:

- **Kotlin LSP**, the default: the Kotlin team's
  [kotlin-lsp](https://github.com/Kotlin/kotlin-lsp). It is its vendor's software, downloaded from
  the vendor under the vendor's terms, and partly closed-source; the IDE names the vendor where it
  offers the server.
  It brings its own Java runtime and is about 360 MB to download on macOS. There is no download for
  Windows yet.
- **Kotlin Language Server (fwcd)**: [fwcd's open-source server](https://github.com/fwcd/kotlin-language-server)
  (MIT), about 87 MB, which runs on the project's JDK ([JDKs](06-java.md#jdks)).

The first time a Kotlin file opens, the IDE offers to install the chosen server. **Install**
downloads it from its official place, checks it against the checksum the plug-in names, and keeps it
in the IDE's own folder; nothing is downloaded before you click. Either server reads your Gradle or
Maven project itself, with the project's JDK. The IDE runs the server as a separate program, the
way it runs every language server ([Language servers](17-language-servers.md)).

### Code style

**Settings → Editor → Code Style → Kotlin** offers the options Kotlin LSP reads from the project's
`.editorconfig`, section `[*.{kt,kts}]`: spaces before the parentheses of `if`, `for`, `while`,
`when` and `catch`, around assignment, additive, multiplicative and range operators, at colons and
commas; the hard wrap (`max_line_length`), trailing commas in declarations and calls, `else`,
`catch` and `finally` on a new line; the blank lines kept in declarations, in code and before `}`;
and the import layout and the counts that make an import use `*`. Each is an `ij_kotlin_*`
property; the page names them in `editor.codeStyle.list` for agents.

With the **Project** scheme they are read from and written to the project's `.editorconfig` on
**OK** or **Apply**: only those properties change, under `[*.{kt,kts}]` (added when the file has
no such section), and everything else in the file stays. With **Default (IDE)** they reach the
server once **Code Style → Show Scheme Actions → Export → EditorConfig File** writes them. The fwcd
server formats with ktfmt, which reads only the indent, so it ignores the page. Kotlin has no
arrangement rules: **Rearrange Code** leaves Kotlin files alone.

## Convert Java to Kotlin

The Kotlin plug-in turns Java code into Kotlin, without a language server and without the network:

- **Code → Convert Java File to Kotlin File** (⌥⇧⌘K) with a `.java` file in the editor writes a
  `.kt` file with the same name beside it, opens it and moves the Java file to the Trash (Local
  History keeps it). It does nothing to a file of the same name that exists already.
- **Pasting Java into a Kotlin file**: when you paste into a Kotlin file the text you took with ⌘C or
  ⌘X from a Java file in the IDE, it asks first: **Convert** pastes it as Kotlin, **Paste as It Is** pastes the Java. Members of
  a class and statements are converted as well as whole files.

What is converted: packages and imports; classes, interfaces, enums, records (as data classes),
nested and inner classes and anonymous classes (as `object :`); fields as properties (constants as
`const val` in a companion object), a constructor that only stores its parameters as the class's
primary constructor, other constructors as `constructor(…)`; methods as functions, with `override`,
`open` for classes the file extends, `vararg` and a `main` that still runs; statements (`for` loops over
ranges as `for (i in a until b)`, `switch` as `when`, try-with-resources as `use { }`, multi-catch as
one `catch` per type); expressions (the conditional operator as `if … else`, casts of numbers as
`toInt()` and friends, `instanceof` as `is`, bit operators as `and`, `or`, `shl`…, lambdas, method
references, `X.class` as `X::class.java`, arrays as `IntArray`, `arrayOf`…); Java's collection
interfaces become Kotlin's mutable ones, and a few JDK getters become properties
(`entrySet()` → `entries`, `getMessage()` → `message`). Comments stay where they were.

What to check: the converter reads one file's text, not the project's types. Where Java's meaning
depends on them it takes the common case: `==` between two objects of known types becomes `===`, an
`int` passed where the file declares a `double` is widened, but values from other files are not;
references are not null unless a field starts as `null` or a method returns `null`. What it cannot
carry over — a `case` that falls through to the next, a pattern variable after `instanceof`, a
statement it cannot read — is reported with its line in the Java text, and a statement it cannot read
is kept in a `/* Not converted: … */` comment, so nothing is lost. Build the converted file and look
at what it reports before going on. Agents convert with `file.convert.toLanguage` and
`editor.convert.text` ([AI agents](11-ai-agents.md)).

## Run, test and debug

With the plug-in installed, Kotlin code runs, is tested and is debugged as Java code is, with the
same configurations ([Java](06-java.md#run-a-java-program)); the Kotlin plug-in tells the IDE where
a file's mains and tests are.

- **Mains.** A green ▶ in the gutter next to a top-level `fun main()` or
  `fun main(args: Array<String>)` (also `suspend`) runs it, and **⌃⇧R** does with the caret in it.
  It runs as the file's class: `App.kt` in package `shop` is `shop.AppKt`, or the name that
  `@file:JvmName("…")` gives. A `@JvmStatic fun main(args: Array<String>)` in an `object` runs as
  that object, in a `companion object` as its class, with a ▶ next to both.
- **Tests.** Each test class and test function has a ▶: functions marked `@Test` of JUnit 5,
  JUnit 4 or kotlin.test (whose `@Test` is the framework's), `@ParameterizedTest`,
  `@RepeatedTest` and `@TestFactory`; `@Nested inner` classes as their own classes; and a class
  marked TestNG's `@Test`. They run with Maven or Gradle as Java tests do, with the results tree,
  links from a failure to its line in the `.kt` file, and **Rerun Failed Tests**.
- **Building.** Maven compiles Kotlin with the Kotlin Maven plug-in, Gradle with the Kotlin Gradle
  plug-in, before the program runs. When the IDE was chosen to build (Maven's **Delegate IDE
  build/run actions to Maven** off, or Gradle's **Build and run using: The IDE**), a module with
  Kotlin sources is still built by Maven or Gradle, and the console says so first: the IDE's own
  compiler builds Java only. Kotlin without a `pom.xml` or Gradle build cannot run.
- **Debugging.** Line breakpoints in `.kt` files stop the program and the tests; the Debug tool
  window shows the frames in your Kotlin files, the variables as the JVM has them, and stepping.
  The debugger is the Java one, java-debug ([Java](06-java.md#debug)), which runs inside the Java
  plug-in's language server: the IDE starts that server for the project when a debug session starts,
  even when no Java file is open. What it cannot do for Kotlin:
  - **Evaluate Expression** and watches take Java expressions, which java-debug compiles in a Java
    project; in a Kotlin frame it says so.
  - **Inline functions.** A breakpoint in an inline function's body stops only where the function runs
    as a call, not where the compiler copied it into its caller; **Step Over** on a line that calls
    an inline function goes to the next line of the caller, and frames inside inlined code show
    the caller's line.

### Debugging coroutines

When you **Debug** (never when you Run) a Java or Kotlin application, the IDE starts it with
`-Dkotlinx.coroutines.debug`, the debug mode of kotlinx.coroutines: each thread then carries the name
and number of the coroutine it runs, so the Debug tool window's threads read `main @coroutine#1` or
`DefaultDispatcher-worker-1 @loader#4`, and a `CoroutineName` you gave shows there. The flag is in
the command line the Debug console shows first. It changes the program's thread names while it is
debugged, so a program that reads its own thread names sees them changed: turn it off in
**Settings → Build, Execution, Deployment → Debugger → Kotlin coroutines → Debug mode when
debugging**, or for one configuration with **Coroutine debug mode** in **Edit Configurations**
(**As in Settings**, **On when debugging**, **Off**). A flag you put in the configuration yourself is
kept as it is. It applies to Application configurations; Maven and Gradle runs are not changed.

**Track coroutines** in an Application configuration (off by default) also starts the program with
kotlinx.coroutines' debug agent, so that the debugger can list every coroutine, suspended ones too.
The agent is the `kotlinx-coroutines-debug-….jar` of your project's own dependencies: add
`kotlinx-coroutines-debug` to the project (Maven or Gradle), and the IDE takes it from the classpath
the run resolved, nothing else, and never downloads it. It is used only in a trusted project, and
the `-javaagent:` it adds is in the command line the Debug console shows. When the run's classpath
has no such jar, the console says so and the program starts without it. The agent costs the program
a little speed while it is debugged.

With the agent in the program, the Debug tool window of a Java or Kotlin run has a **Coroutines**
tab: every coroutine with its name, its state (running, suspended or created), its dispatcher and the
frames where it last ran or suspended. The IDE reads them once at each stop by evaluating one fixed,
read-only dump of the coroutine debug agent in the stopped thread, and agents read the same list with
`debug.coroutines.list`. Two limits come from the debugger:

- **Not after Pause.** The Java debugger can run code only in a thread that a breakpoint or a step
  stopped, so after **Pause Program** the tab asks you to step once or stop at a breakpoint.
- **The top 8 frames.** The dump is tried in the stopped thread's frames from the top, and only in
  the first 8; frames of Kotlin's generated classes (such as `invokeSuspend`) cannot run it, so when
  none of the 8 can, the tab says so: step into your own code and look again.

The list shows at most 200 coroutines, each with at most 32 frames.

## Go to Test and Create Test

**Navigate → Test** (**⇧⌘T**) works for Kotlin as for Java
([Java](06-java.md#go-to-test-and-create-test)): `Cart.kt` in `src/main/kotlin/com/acme/` is tested by
`CartTest.kt`, `CartTests.kt`, `CartIT.kt` or `TestCart.kt` in `src/test/kotlin/com/acme/` (also
`src/main/java` with `src/test/java`, `src/commonMain/kotlin` with `src/commonTest/kotlin` and
`src/jvmMain/kotlin` with `src/jvmTest/kotlin`). A function `total`, an extension function
`String.total` among them, is tested by a function whose name starts with `total` or `testTotal`,
names in backticks too (``fun `total of nothing is zero`()``). **Create Test** offers JUnit 5, JUnit 4,
TestNG and kotlin.test, the ones the build names first (`kotlin("test")` or `kotlin-test` for
kotlin.test), and writes a class with `setUp` and `tearDown` (`@BeforeTest` and `@AfterTest` in
kotlin.test) and a `@Test` function for each function checked.

## Kotlin REPL and Kotlin scratch files

The IDE does not ship or download the Kotlin compiler. Two things need it, and use the `kotlinc`
installed on your computer (`KOTLIN_HOME/bin`, PATH, Homebrew or SDKMAN):

- **Tools → Kotlin REPL** opens `kotlinc`'s REPL in a new **Kotlin REPL** tab of the Terminal tool
  window, with the classpath of the active file's module, made as for the
  [JShell console](06-java.md#jshell-console). **Restart Kotlin REPL** and **Stop Kotlin REPL** are
  in **Find Action** (⇧⌘A). Recent Kotlin versions mark this REPL as experimental; what it
  supports is up to the `kotlinc` you have.
- A **Kotlin scratch file** with a `main` gets a ▶ in its gutter. It runs as a **Single-File
  Kotlin Program**: `kotlinc` compiles it with the Kotlin runtime into a JAR in the run's own
  folder, and the project JDK runs its class (`ScratchKt` for `scratch.kt`). Turn on **Use the
  classpath of the module** in the configuration to compile and run it against the project
  ([Running a scratch file](25-scratches.md#running-a-scratch-file)).

Without `kotlinc`, **Kotlin REPL** and the ▶ say what is missing instead of starting.

## New Kotlin projects

With the Kotlin plug-in, **File → New → Project…** has the **Kotlin** type
([New Project](05-project.md#new-project)), with the same fields as
[the Java type](06-java.md#new-java-projects) except **Build system**: **Gradle** (Kotlin DSL
first) or **Maven**. The IDE builds no Kotlin itself, so there is no **IDE** build system. The build
files name Kotlin 2.4.21 and the chosen JDK's release, up to the newest one that version of Kotlin
compiles for. **Use compact project structure**, on by default, keeps the sources directly in `src`
instead of `src/main/kotlin`, and the build files say so.

## New Kotlin files

**New** (⌘N in the Project tool window) lists the following, each starting with the folder's
`package` line when the folder is below a source root (`src/main/kotlin/demo` gives
`package demo`):

- **Kotlin File**
- **Kotlin Class**
- **Kotlin Data Class**
- **Kotlin Interface**
- **Kotlin Enum**
- **Kotlin Object**
- **Kotlin Script**

## Show Kotlin Bytecode

**View → Show Bytecode** (Find Action also finds it as **Show Kotlin Bytecode**) shows what the Kotlin
compiler made of the class at the caret, as for Java ([Show Bytecode](06-java.md#show-bytecode)): the
JDK's `javap -c -p -v` listing of its `.class` file, in a read-only tab in the editor's right split
that follows the caret. In a class, its own class is shown, the innermost one for a nested class
(`Cart$Line`); outside any class (top-level functions and properties) the file's class, such as
`CartKt` for `Cart.kt` (or the name `@file:JvmName` gives).

The classes are taken from what the build made: Gradle's `build/classes/kotlin/main` (and `test`),
Maven's `target/classes`. A class that is not built yet gets the same notification as in Java, with
**Build** (Gradle's `classes` or `testClasses`, Maven's `compile` or `test-compile`). A Kotlin file
outside a Maven or Gradle module is not compiled by the IDE, so its bytecode cannot be shown.

**Decompiling the bytecode back to Java is not available yet**: the listing shows the bytecode only.

## Coverage

Kotlin tests run with coverage as Java tests do: **Run with Coverage** on a JUnit or TestNG
configuration of a Maven or Gradle build measures them with JaCoCo, and the lines are marked in your
`.kt` files, matched by the report's package and file names ([Java](06-java.md#coverage)).
