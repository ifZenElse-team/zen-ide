# Java

JvmJizo is the Zen IDE for Java and the JVM: Java first, with Maven and Gradle, in the same IDE
window as every Zen IDE, with Git, the terminal, Find in Files, the HTTP Client and AI agents.

Everything on this page but the JDKs is plug-ins, and every plug-in works in every Zen IDE: JvmJizo
comes with the Java, Maven and Gradle plug-ins, which the other IDEs install from the Marketplace
([Plug-ins](10-plugins.md)); Kotlin is a plug-in you add when a project needs it. The JDKs are part
of every Zen IDE.

## Install JvmJizo

In Zen Toolbox, under **Available**, click **Install** next to JvmJizo (a preview since 26.10.2); once installed it moves to
**Installed**, and a click starts it ([Zen Toolbox](13-toolbox.md)). Zen Toolbox also puts the
command `jvmjizo` in `~/.local/bin`, so that in a terminal:

```sh
jvmjizo .                                  # the current folder in JvmJizo
jvmjizo src/main/java/App.java:12          # the file at line 12 (:12:5 at column 5 too)
jvmjizo --headless .                       # JvmJizo without a window, for AI agents
jvmjizo mcp --headless .                   # the same as an MCP server on stdio
```

With JvmJizo already running, the command opens the folder or file in it and returns at once.
**Tools → Create Command-line Launcher…** writes the same command from JvmJizo itself.

## Open a Maven or Gradle project

A project is a folder: the one with your `pom.xml` (Maven) or `build.gradle`, `build.gradle.kts`
or `settings.gradle(.kts)` (Gradle). **Open** in the welcome window picks it; recent projects open
with a click. JvmJizo reopens the projects and files that were open when it last quit.

To try it from nothing, a Maven project needs only a `pom.xml` and `src/main/java/App.java`; a Gradle
project a `settings.gradle.kts`, a `build.gradle.kts` with the `java` or `application` plug-in, and
the same source folder. Open the folder, and check its JDK under **Settings → Languages &
Frameworks → JDKs** (below).

What the Java plug-in adds is below; what the Maven and Gradle plug-ins add (the build tool
windows and their run configurations) is described with each plug-in as it lands
([Plug-ins](10-plugins.md)).

## New Java projects

With the Java plug-in, **File → New → Project…** has the **Java** type
([New Project](05-project.md#new-project)):

- **Build system**: **IDE** (sources in `src`, built with `javac`), **Maven**, the default, or
  **Gradle**, with **Gradle DSL** Groovy or Kotlin.
- **JDK**: the JDKs found on the computer, or **Download Eclipse Temurin** with a release; the
  download runs in the background and the list follows. It becomes the project's JDK. With no JDK,
  Create stays disabled and says why.
- **Add sample code**: a `Main` class that prints a greeting.
- **Advanced Settings** (Maven and Gradle): **GroupId** and **ArtifactId**. The sample goes into the
  GroupId's package. Characters a build refuses become `-`, and `_` in the package.

The build files are written without the network. Maven and Gradle download their own plug-ins the
first time the project builds. A new project has no wrapper:
[the IDE finds the program](26-maven-and-gradle.md#the-program-and-the-jdk).

## The Java plug-in

The Java plug-in works in every Zen IDE: JvmJizo comes with it, and the other IDEs offer it in a
banner when you open a `.java` file. Without anything else installed it gives, as you type:

- **Highlighting** of keywords (the contextual ones too: `record`, `sealed`, `non-sealed`,
  `permits`, `var`, `yield`, and `when` in a `switch` pattern; `module`, `requires`, `exports`...
  only in `module-info.java`), types, methods, constants, annotations, strings, characters, text
  blocks (`"""`), numbers and comments, Javadoc comments apart.
- **Folding** of classes, methods, blocks, lambdas, text blocks, comments and the import list.
- **The Structure view** (**⌘7**, and **⌘F12** for the popup): classes, interfaces, enums, records
  and annotation types, nested ones too, with their fields, methods, constructors, enum constants and
  record components.
- **Completion** of keywords and the names in the file, and the Java
  [live templates](02-editor.md#live-templates): `psvm`/`main`, `sout`, `souf` (`printf`), `soutv`
  (a value with its name), `serr`, `fori`, `iter`, `itar` (an array), `itli` (a `List`), `ifn`,
  `inn`, `psf`, `psfs`, `psfi`, `prsf`, `thr`, `lambda` and `sw`. They are listed in Settings →
  Editor → Live Templates under Java, read-only: switch one off, or **Duplicate** it to change it.
- **Postfix completion** after an expression and a dot: `x.var`, `x.val` (`final var`), `ok.if`,
  `ok.else`, `ok.not`, `x.nn` and `x.notnull`, `x.null`, `items.for` and `items.iter`, `n.fori`,
  `n.forr` (counting down), `ok.while`, `x.return`, `e.throw`, `x.sout`, `x.serr`, `call().try`,
  `x.cast`, `Type.new`, `x.par`, `x.lambda`, `x.arg`, `x.switch`, `lock.synchronized` and
  `ok.assert` ([Postfix completion](02-editor.md#postfix-completion)).
- **Surround With** (**⌥⌘T**): **if**, **if / else**, **while**, **do / while**, **for**,
  **try / catch**, **try / finally**, **try / catch / finally**, **synchronized**, **Runnable**, a
  block, and for a selection **(expr)**, **!(expr)** and **((Type) expr)**.
- **Javadoc stubs:** `/**` and **Enter** above a class, method, constructor, record or field puts
  `/** … */` with an `@param` line per parameter, `@return` when the method returns something and an
  `@throws` line per exception after `throws`.
- **Complete Current Statement** (**⇧⌘⏎**) closes the brackets and adds the `;`, or the block of an
  `if`, a loop, a `switch`, `try`, `catch`, `synchronized`, a class or a method declaration, with the
  condition put in parentheses when it has none
  ([Complete Current Statement](02-editor.md#complete-current-statement)).
- **Comment with Line Comment** (**⌘/**) and **Block Comment** (**⌥⌘/**), brackets and quotes typed
  in pairs, and the file's indentation kept.
- **New** (⌘N in the Project tool window, in a source folder): **Java Class**, whose name dialog
  chooses **Java Class**, **Java Interface**, **Java Enum**, **Java Record** or **Java Annotation**,
  **package-info.java** and **module-info.java**. A class created in
  `src/main/java/com/acme` starts with `package com.acme;`: the template variable `${PACKAGE_NAME}`
  is the file's folders below the source root (`java`, `kotlin`, `scala`, `groovy`, or `src`).

### The Eclipse JDT Language Server

Everything that needs the whole project comes from the **Eclipse JDT Language Server**, which runs
on its own, beside the IDE: problems as you type, completion of members and types, **Go to
Declaration** (**⌘B**), **Implementation** (**⌥⌘B**) and **Type Declaration** (**⇧⌘B**), **Find
Usages** (**⌥F7**), **Rename** (**⇧F6**) across files, **Reformat Code** (**⌥⌘L**), **Optimize
Imports** (**⌃⌥O**), quick fixes (**⌥⏎**), Quick Documentation (**F1**) with the Javadoc, and the
**Call** and **Type Hierarchy** (**⌃⌥H**, **⌃H**).
**Go to Declaration** also goes into the JDK's and the libraries' classes: the class opens
read-only, with its sources when there are some, otherwise as the server shows the class file
(declarations without code), and navigation goes on from there. See
[Libraries and their sources](26-maven-and-gradle.md#libraries-and-their-sources).

It is not installed with the plug-in: the first `.java` file you open shows a banner, and
**Install** downloads it (version 1.63.0, about 50 MB, from `download.eclipse.org`) with its
debugger, java-debug 0.53.1 (3 MB, from Maven Central), checks both against their SHA-256 and
unpacks them into the IDE's own folder, `language-servers/jdtls` in the IDE's data folder. Nothing
is downloaded until you click it; **Settings → Languages & Frameworks → Language Servers** shows it
as **installed locally**, and its log.

The server is a Java program and runs on **Java 25 or newer**: the project's JDK when it is new
enough, else the newest JDK 25 or later found (see JDKs below). Your project itself can use any JDK
and language level. When no such JDK is found, the banner says so and offers **Download JDK 25**,
which downloads Eclipse Temurin 25 into the IDE's own folder (as **Add JDK** does); the server
starts once it is there. Agents see the same as `javaNeeded` in `lsp.servers.list`. Its data (the imported projects, the index) is
kept in the IDE's cache, `caches/language-servers/jdtls`, one folder per project, never in the
project.

### Generate code

Inside a class, **Code → Generate…** (**⌘N**) offers what the server can write there, each with a
member chooser ([Generate](02-editor.md#generate)):

- **Constructor**: the superclass's constructors to call (all checked; uncheck the ones you do not
  want) and the fields to initialize.
- **Getter**, **Setter**, **Getter and Setter**: the fields that do not have one yet, all checked.
- **equals() and hashCode()**: the fields to compare. When the class has them already, **Replace
  the existing equals() and hashCode()** (on) writes them anew instead of adding a second pair.
- **toString()**: the fields to show; none is fine too.
- **Override Methods** (**⌃O**): the methods of the superclasses and interfaces that can be
  overridden, under the type that declares them. **Implement Methods** (**⌃I**): the abstract ones
  the class does not implement yet, all checked.
- **Delegate Methods**: the methods of each field's type, under the field, to call through it.

The code is written as the server formats it, with `@Override` where it belongs, and one **⌘Z**
takes it away. How the server writes `toString()`, `equals()` and `hashCode()`, and whether it adds
comments, follows its own `java.codeGeneration` settings. There is no test method generator yet: the
server has none.

The server imports a **Maven** project (`pom.xml`) and a **Gradle** project (`build.gradle`,
`settings.gradle`, with the Gradle wrapper when the project has one) by itself, and imports it again
when those files change. To import, Maven and Gradle resolve the build's plug-ins and dependencies
as they always do, from the repositories the build names.

### Move and Change Signature

Two refactorings of the **Refactor** menu come from the server and open in the shared refactoring
dialog (see [The refactoring dialog](02-editor.md#the-refactoring-dialog)):

- **Move…** (**F6**) moves the class of the file in the editor, or the classes and the package
  selected in the Project tool window, to another package. **To package** lists the packages the
  server knows, starting with the class's own (for a package, its parent). The class's file moves
  into the package's folder, its `package` line changes and every class that uses it gets the
  import it now needs; a package moves as its folder, with the `package` line of each of its
  classes and the imports of the code that uses them. **Preview** lists every change and the
  files moved. Moving into a package that already has a file of the same name is refused. A class
  that is not `public` and is still used from its old package is listed as a conflict before
  anything changes, with **Continue**, **Show Conflicts in View** and **Cancel**.
- **Change Signature…** (**⌘F6**) on a method's name, or on a call of it, asks for its
  **Visibility**, **Return type** and **Name**, and its **Parameters** in a table of **Type**,
  **Name** and **Default value**: **Add** puts a new parameter after the selected one (its default
  value is what the calls pass), **Remove** removes it, **Move Up** and **Move Down** reorder.
  **Keep the old method as a delegate** leaves the old signature calling the new one. **Preview**
  lists the declaration and every call it changes, each changed line apart. A removed parameter
  the method's body still uses is listed as a conflict.

One **⌘Z** undoes either refactoring, the moved files too.

### Class refactorings

These refactorings of the **Refactor** menu (and **Refactor → Extract/Introduce**) also open in the
shared refactoring dialog, with **Preview** and the conflicts listed before anything changes. Put
the caret in the class (or on the member or method) first.

- **Pull Members Up…** moves members of the class into its superclass or one of its interfaces in
  the project. **Pull up members to** chooses where; the **Members** table ticks what moves, and
  **Make abstract** leaves a method's body where it is and declares it abstract above (the
  superclass then becomes abstract, which is listed as a conflict). Into an interface a method is
  declared and only constants can move. A private member that the class still uses becomes
  protected; a moved member that uses one left behind, or a member the destination already has, is
  a conflict.
- **Push Members Down…** moves members into every subclass in the project; **Keep abstract** leaves
  an abstract declaration behind. A use of a moved member outside the subclasses, or a private
  member the moved code uses, is a conflict.
- **Extract/Introduce → Superclass…** creates a new class in the same package between the class and
  its superclass, with the members you tick (**Make abstract** declares a method there and keeps
  its body in the class).
- **Extract/Introduce → Interface…** creates an interface from the public methods you tick, in the
  package you choose, through the language server; the class implements it and its methods get
  `@Override`.
- **Encapsulate Fields…** makes the fields you tick private (or the visibility you choose), adds a
  getter and a setter with the names in the table (an empty setter name adds none) and rewrites
  their uses in other classes: reads become getter calls, assignments, `+=` and `++` written as
  statements become setter calls. A use it cannot rewrite safely, such as an assignment used as a
  value, is listed as a conflict to change by hand.
- **Extract/Introduce → Parameter Object…** on a method or a constructor replaces the parameters
  you tick with one object: a new class with getters, or a record, in the same package. The body
  reads the object, and every call passes `new …(…)`. A method that overrides or is overridden, a
  parameter assigned in the body and a recursive method are refused; a method reference is a
  conflict.

One **⌘Z** undoes each of them, the new files too.

### Code style

**Settings → Editor → Code Style → Java** sets how **Reformat Code** and **Optimize Imports** lay
out Java code. The server formats with the Eclipse formatter, and the page's options are its
settings:

- **Tabs and Indents**: the indent and tab size, the continuation indent (in indents, 2 by
  default), the indent of `case` branches under `switch`, and indents kept on empty lines.
- **Spaces**: before the parentheses of method declarations and calls, `if`, `for`, `while`,
  `switch`, `try`, `catch` and `synchronized`; around assignment, logical, equality and relational,
  bitwise, additive, multiplicative and shift operators; before the braces of classes, methods and
  blocks; after commas and inside the parentheses of calls.
- **Wrapping and Braces**: the hard wrap (120) and the comment line length (80), joining lines
  that are already wrapped, where braces go in class and method declarations and elsewhere (end
  of line, next line, next line shifted, next line if wrapped), how parameter lists, call
  arguments, chained calls, `extends` and `implements` lists, `throws` lists, binary expressions and
  enum constants wrap (do not wrap, wrap if long, chop down if long, wrap always), and `else`,
  `catch`, `finally` and the `while` of `do` on a new line.
- **Blank Lines**: the most blank lines kept in code, and the fewest around the package
  statement, the imports, classes, fields, methods and nested classes.
- **Imports**: the import order, groups separated by `;` (`#` is the group of static imports, an
  empty group holds all the others; `#;java;javax;org;com;` by default), the blank lines between
  import groups, and how many classes of a package (or static names of a class) are imported with
  `*` (99 by default).

With the **Default (IDE)** scheme, a page you changed becomes an Eclipse formatter profile the IDE
writes into its own folder and hands to the server; a page left as it is gives the server no
profile, so it formats as it always has. With the **Project** scheme, the formatter's options are
kept in the project's `.zen/java-formatter.xml` (an Eclipse formatter profile, written on **OK** or
**Apply**), so everyone who opens the project formats the same way; a profile exported from another
tool can be put there, and the settings the page does not show stay as they are. The import order
and the `*` counts are server settings and stay yours in both schemes. Java has no arrangement
rules: **Rearrange Code** leaves Java files alone.

## Run tests

Each test class and test method has a green ▷ in the gutter: JUnit 5 (`@Test`,
`@ParameterizedTest`, `@RepeatedTest`, `@TestFactory`), JUnit 4 and TestNG (a class marked `@Test`
too). **Run** runs the class or the method; **⌃⇧R** runs the test around the caret. The tests run
with the project's build tool: Maven (Surefire) in the folder of the nearest `pom.xml`, or Gradle
from the build with the settings file, the project's `mvnw` or `gradlew` first, with the project's
JDK. Nothing is installed for it, and Gradle gets an init script of the IDE's
(`##zen-test` lines, hidden from the console) that reports each test as it runs; it changes nothing
in your build.

The **Run** tool window shows the tests as a tree, a class with its methods, a parameterized test
with its runs (`positive(int)` › `[1]`): passed, failed and skipped, with their times, and each
test's output. A failure's stack frames link to your sources; selecting a failed test and
double-clicking it opens the line where it failed. **Rerun Failed Tests** runs again only the tests
that failed, and the gutter icons show how each test last went.

A configuration of kind **JUnit** or **TestNG** runs all the tests (`all`), a package (`package`,
with its subpackages), a class, a method or a list of tests (`tests`, as Rerun Failed Tests makes:
`com.acme.CartTest#adds,com.acme.OrderTest#ships`); **Build tool** is `auto` (by the build files),
`maven` or `gradle`. The tool's own arguments (`-Pci`, `--offline`) and environment variables are
passed on.

Agents run the same with `run.process.start` (a gutter target from `run.targets.list`, or a
configuration), follow it with `run.tests.get` (`summary`, `failedOnly` with each failure's file,
line and message, or one test's output) and rerun failures with `run.tests.rerunFailed`.

### Go to Test and Create Test

**Navigate → Test** (**⇧⌘T**) jumps between a class and its tests
([Run and test](15-run-and-test.md#go-to-test-and-create-test)). `Cart` in
`src/main/java/com/acme/Cart.java` is tested by `CartTest`, `CartTests`, `CartIT` or `TestCart` in
`src/test/java/com/acme/` of the same module (the Maven and Gradle folders; a module whose build moves
its source folders elsewhere is not followed yet). A method `total` is tested by a method whose name
starts with `total` or `testTotal`. When a class has no test, **Create New Test…** opens the **Create
Test** dialog: its **Testing library** list starts with the ones the module's or the project's
`pom.xml`, `build.gradle`, `build.gradle.kts` or `gradle/libs.versions.toml` names (JUnit 5, JUnit 4,
TestNG), then the others. The test class it writes has the package line, the library's imports, a
`setUp` and a `tearDown` (`@BeforeEach` and `@AfterEach` in JUnit 5, `@Before` and `@After` in JUnit 4,
`@BeforeMethod` and `@AfterMethod` in TestNG) and a `@Test` method for each method checked.

## Coverage

**Run with Coverage** (Run menu, the ▷ menu in the gutter, the run widget's More Actions) runs a
JUnit or TestNG configuration, an Application or a JAR Application and then marks the lines that ran
in the gutter and fills the **Coverage** tool window ([Run and test](15-run-and-test.md#code-coverage)).
JaCoCo measures it:

- A Maven build that declares `jacoco-maven-plugin` runs its `prepare-agent` goal before `test`
  (unless the build binds it already) and, once the run ended, its `report` goal; the IDE reads
  each module's `target/site/jacoco/jacoco.xml`.
- A Gradle build that applies the `jacoco` plug-in runs `jacocoTestReport` after `test`, with its
  XML report written to the IDE's own folder.
- Any other build, and every Application, gets the JaCoCo agent (`-javaagent`) in the JVM that runs
  the tests or the program: through Surefire's `argLine` for Maven (a `pom.xml` that sets its own
  `<argLine>` must include `@{argLine}`), through the IDE's init script for Gradle. The IDE then
  turns the data into a report with the JaCoCo command-line tool, measuring `target/classes` and
  `build/classes/*/main` (or the classpath's folders, or the JAR).

The agent and the command-line jars are JaCoCo 0.8.16, the IDE's own copy or the same jars in the
local Maven repository (`~/.m2/repository`) or the Gradle cache. Every jar is checked against the
SHA-512 the IDE knows before it runs, so another version or a changed jar is not used. When none is
found, Run with Coverage says so; the command **Download JaCoCo** (Find Action,
`java.jacoco.download`) then downloads JaCoCo 0.8.16 from Maven Central, checks its SHA-512 and
keeps it in the IDE's data folder. Nothing is downloaded unless you run it.

Lines are mapped to your `.java` and `.kt` files by the report's package and file names, so Kotlin
classes show in their Kotlin sources. A line is **covered** when all its instructions ran,
**partly** when some instructions or branches did not, **not covered** when nothing ran; pointing at
a mark shows its branches. JaCoCo does not count how many times a line ran, so the hit count is 1
for every line that ran.

## Debug

Java programs and tests are debugged with **java-debug**, the debugger that runs inside the Eclipse
JDT Language Server (installed with it, above): breakpoints with conditions, exception breakpoints
(caught and uncaught, [Debugging](16-debugging.md#exception-breakpoints)), stepping over, into and
out, the threads and frames, variables, watches and **Evaluate Expression**, in the Debug tool
window as for every language.

- **A test**: **Debug** in the test's gutter menu, or **⌃⇧D** at the caret. Maven runs the tests in
  its own JVM (`-DforkCount=0`), Gradle in its test JVM, which waits for the debugger; the results
  tree fills as in a run.
- **A program**: an **Application** or **JAR Application** configuration is debugged as it runs
  ([Run a Java program](#run-a-java-program)): built, with the same class path, JRE and VM options,
  then its `java` waits for the debugger and java-debug attaches to it.

The debugger talks to the IDE on the loopback interface only.

**Collections and streams.** Lists, sets, maps and arrays open to their elements, map entries as
key → value, with **Show Raw Fields** in a row's menu for what is inside
([Collections, maps and arrays](16-debugging.md#collections-maps-and-arrays)); the IDE turns
java-debug's logical structure on or off through its settings in the language server. **Run →
Debugging Actions → Trace Current Stream Chain** shows the values after each operation of the stream
on the stopped line ([Trace a stream chain](16-debugging.md#trace-a-stream-chain)).

**Hot code replace.** While a Java program is being debugged, change a method body and save: the
language server builds it, and the changed classes are reloaded into the running program at once,
with a notification that names them (the console says so too). **Run → Reload Changed Classes**
does it on request. A changed method takes effect at its next call; the method the program is
stopped in keeps running its old code until it returns. What the JVM cannot replace (a new or
removed method or field, a changed signature) is reported in the notification and leaves the
program as it was; restart it to take such a change. Agents call `debug.classes.reload`, and
`debug.session.get` gives the last reload (`reloaded`).

## JDKs

Every Zen IDE finds the JDKs on your computer, without asking the network:

- `JAVA_HOME`;
- macOS's own list (`/usr/libexec/java_home -V`), `/Library/Java/JavaVirtualMachines` and
  `~/Library/Java/JavaVirtualMachines`; `/usr/lib/jvm` on Linux;
- Homebrew (`openjdk`, `openjdk@21`...), SDKMAN! (`~/.sdkman/candidates/java`), asdf and mise;
- Gradle's toolchains (`~/.gradle/jdks`, `org.gradle.java.installations.paths` in
  `~/.gradle/gradle.properties`) and Maven's (`~/.m2/toolchains.xml`);
- the JDKs you added and the ones the IDE downloaded.

Each JDK is shown with its version and vendor, read from its `release` file, and where it was found.

**Settings → Languages & Frameworks → JDKs** chooses the project's JDK and language level:

- **Project SDK**: a JDK of the list, or **Default**: the one `JAVA_HOME` names, else the newest found.
- **Language level**: **SDK default** (the JDK's own version) or a lower one, e.g. 17 on a JDK 21.
  A level higher than the JDK's is shown as a problem.
- **Add JDK → Add JDK from Disk…** picks a JDK's home folder (or its `.jdk` bundle).
- **Add JDK → Download Eclipse Temurin 25, 21, 17, 11 or 8** downloads the newest build of that
  version from the Adoptium project (`api.adoptium.net`, then GitHub), checks its SHA-256 and unpacks
  it into the IDE's own folder; it becomes the project's JDK when done. Nothing is downloaded until
  you choose this. The download runs in the background, in the status bar's tasks.
- **Remove** forgets a JDK you added, or deletes one the IDE downloaded; JDKs found elsewhere stay
  where they are. **Refresh** looks again after you installed one outside the IDE.

The choice is kept per project, in the IDE's data folder (`jdks.json`). Language servers and run
configurations of a Maven or Gradle project start with its JDK: `JAVA_HOME` names it and its `bin`
comes first on `PATH`.

AI agents read and change the same with the `toolchain.jdk.*` commands: `toolchain.jdk.list`,
`toolchain.jdk.project`, `toolchain.jdk.choose`, `toolchain.jdk.languageLevel`, `toolchain.jdk.add`,
`toolchain.jdk.remove` (those that choose a JDK are refused for agents), `toolchain.jdk.refresh`, `toolchain.jdk.available` (the versions the
Adoptium API offers) and `toolchain.jdk.download` ([AI agents](11-ai-agents.md)).

## Run a Java program

Every Zen IDE runs Java programs with three kinds of run configuration:

- **Application** runs a class's `main`: `public static void main(String[] args)`, or, from Java 21
  on, an instance `void main()` and a file without a class. A ▶ icon in the gutter next to the class
  and its `main` runs it, as **⌃⇧R** does with the caret in the class; **Run → Edit
  Configurations…** sets **Main class**, **Module** (the folder of its `pom.xml` or Gradle build),
  **JRE** (empty for the project's JDK), **Build before run**, the program's arguments, the VM options
  (the tool's arguments field, e.g. `-Xmx512m -Dmode=dev`), the working directory and environment
  variables.
- **JAR Application** runs `java -jar` on a JAR (**Path to JAR**), with the same arguments, VM
  options and JRE.
- **Single-File Java Program** runs one source file with `java <file>.java` (**Source file**,
  **JRE**, and **Use the classpath of the module** with **Module** to compile against the project).
  A Java scratch file runs this way ([Running a scratch file](25-scratches.md#running-a-scratch-file)).

Before an Application runs, the project's own build tool compiles it and gives the classpath:
Maven (`mvn compile dependency:build-classpath`, the module's `target/classes` first), Gradle (the
main source set's runtime classpath, through a small init script of the IDE's in the run's own
folder), or, in a folder without either, `javac` into the run's own folder at the project's language
level. The project's wrapper (`mvnw`, `gradlew`) is used when there is one; Maven and Gradle may
download the project's dependencies, as they do in a terminal. Every program runs with the
configuration's JRE or the project's JDK.

The output is in the Run tool window; a stack trace's frames (`at shop.App.run(App.java:42)`) are
links to the line in the project's sources. Shared JetBrains configurations of the types
Application and JAR Application in `.run/` and `.idea/runConfigurations/` are listed and run as
well ([Run and test](15-run-and-test.md)). **Debug** starts the same configuration under the
debugger ([Debug](#debug)).

A Spring Boot application (a class with `@SpringBootApplication`) runs as a **Spring Boot**
configuration instead, with its active profiles and properties, when the Spring Boot plug-in is on;
the plug-in also completes and checks `application.properties` and `application.yml`
([Spring Boot](31-spring-boot.md)).

## Show Bytecode

**View → Show Bytecode** shows what the compiler made of the class at the caret (in a Kotlin file
too: [Show Kotlin Bytecode](06-kotlin.md#show-kotlin-bytecode)), as the JDK's
`javap -c -p -v` prints it: the constant pool, every member (private ones too) and the bytecode of
each method. The class is found in the module's build output: Maven's `target/classes` (or
`target/test-classes` for a test), Gradle's `build/classes/java/main` and the other languages'
folders under `build/classes`. javap of the project JDK runs in the background, and the listing
opens in a read-only tab in the editor's right split while the caret stays in your file. As the
caret moves into another class (a nested class, or another file), the listing follows it.

A class that is not compiled yet is not guessed at: a notification says so and offers **Build**,
which runs Maven's `compile` (`test-compile` for a test) or Gradle's `classes` (`testClasses`) for
that module; run Show Bytecode again when it is done. A file outside Maven and Gradle (a project
without either, or a [scratch file](25-scratches.md)) is compiled alone with the JDK's `javac`
first, into a temporary folder that is removed afterwards; other classes of its source folder are
compiled along when it uses them.

## JShell console

**Tools → JShell Console…** opens JShell, the JDK's interactive Java console, in a new **JShell**
tab of the Terminal tool window. It runs the project JDK's `jshell` with the classpath of the module
of the active editor's file (or the project's folder): before JShell starts, that module is built
the way an Application run builds it (Maven, Gradle or `javac`, above) as a background task, so
your classes and their dependencies are there to try out (`new shop.App().run()`). Without a
`pom.xml`, Gradle build or Java sources it starts with the JDK alone. Building and loading your
classes runs the project's code, so a project you have not trusted is refused until you trust it;
`java.jshell.open { classpath: false }` starts the JDK alone there.

Type a snippet and press Enter; **↑** and **↓** go through the input history, which JShell keeps
between sessions, and **Tab** completes. `/exit`, or closing the tab, ends it. **JShell Console…**
again opens one more; **Restart JShell** builds the module again and starts the newest console
anew (to see code you changed since), and **Stop JShell** ends it. Both are in **Find Action**
(⇧⌘A).

The Kotlin REPL is the Kotlin plug-in's ([Kotlin](06-kotlin.md#kotlin-repl-and-kotlin-scratch-files)).

## Other languages

Go and Rust, the built-in plug-ins of every Zen IDE, are off in JvmJizo. If a project of yours has
Go or Rust too, switch them on in **Settings → Plugins → Built-in** and restart JvmJizo
([Plug-ins](10-plugins.md)).

## AI agents

JvmJizo's MCP server for AI agents listens on port **63533** when it is on (**Settings → Tools →
MCP Server**); `jvmjizo mcp --headless .` serves a headless JvmJizo on stdio
([AI agents](11-ai-agents.md)). Show Bytecode and the consoles are commands too:
`java.bytecode.show { path?, line?, waitMs? }` answers the class, its `.class` file and, with
`waitMs`, javap's output (or `compiled: false` with the `build` command to run);
`java.jshell.open { module?, classpath?, waitMs? }`, `java.jshell.restart` and `java.jshell.stop`
(and `kotlin.repl.*` for the Kotlin REPL) answer the Terminal `session`, which
`terminal.session.send` types into and `terminal.session.read` reads.
