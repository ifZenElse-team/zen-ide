# Maven and Gradle

Maven and Gradle builds run, reload and show their goals, tasks and dependencies in every Zen IDE
with the Maven and Gradle plug-ins; JvmJizo comes with both. Opening a project with a `pom.xml`, or
with a `settings.gradle(.kts)` or `build.gradle(.kts)`, offers the plug-in in a notification when it
is not installed.

## New projects

**File → New → Project…** creates a Maven or Gradle project with the **Java** and **Kotlin** types
([Java](06-java.md#new-java-projects), [Kotlin](06-kotlin.md#new-kotlin-projects)). The build files
are written without the network, and a new project has no wrapper: the program is found as below.


## Maven Archetype

**File → New → Project…** has **Maven Archetype** under Generators, from the Maven plug-in. Choose
the **JDK** Maven runs with, a **Catalog**, an **Archetype** and its **Version**. **Manage catalogs…**
adds catalogs by URL (the Internal catalog comes with the plug-in, and a catalog can be up to 16 MiB,
so the whole Maven Central catalog is too big); **Add…** adds an archetype by its coordinates, with
the repository it comes from. **Additional Properties** are passed to the archetype; **GroupId** and
**ArtifactId** are under **Advanced Settings**.

Maven generates the project and needs the network; its output shows at the bottom of the wizard. If
it fails, the output stays there and nothing is left behind. Maven is the one on your `PATH`, or the
one **Settings → Build, Execution, Deployment → Build Tools → Maven** downloaded. Only a person can
start it this way: an archetype can run a script, so agents are refused, and so is everyone while
the window shows a project that is not trusted.

## The program and the JDK

The IDE runs the project's own wrapper first: `mvnw` or `gradlew`, in the build's folder or a folder
above it in the project. A project with wrapper properties but no script uses the version they name
when that version is on the computer. Otherwise the IDE looks for `mvn` or `gradle` on `PATH`, in
the usual tool folders (`/opt/homebrew/bin`, `/usr/local/bin`…), in SDKMAN! and among the versions
it downloaded, then among the distributions other projects' wrappers left in `~/.m2/wrapper/dists`
and `~/.gradle/wrapper/dists` (`MAVEN_USER_HOME`, or the Gradle user home, moves them): the newest
complete one, a release before a pre-release; a broken or half-unpacked one is skipped. The tool
window shows which tool runs and where it was found. When Maven is found nowhere, the Maven tool window offers **Download
Maven**: Apache Maven 3.9.16 is downloaded from Maven Central, checked, and kept in the IDE's own
folder. This is the only time the IDE downloads Maven, and only when you click it. When Gradle is
found nowhere, the Gradle tool window and the Gradle settings offer **Download Gradle**: the current
release that `services.gradle.org/versions/current` names is downloaded from services.gradle.org,
checked against the SHA-256 published next to it, and kept in the IDE's own folder, again only when
you click it.

Maven and Gradle run with the project's JDK: `JAVA_HOME` is set to it and its `bin` comes first on
`PATH`. The settings below can choose another Maven, another Gradle and another JDK for each.

The project's JDK is the one you chose in **Project Structure**, and a chosen JDK is never replaced.
Without one, the IDE uses the installed JDK of the Java the build asks for: `maven.compiler.release`,
`maven.compiler.source`, the compiler plug-in's settings or `java.version` in a `pom.xml`, the
toolchain, `sourceCompatibility` or `release` in a Gradle script, the highest of all modules. Run
configurations, tests and the Java language server use the same JDK. When no such JDK is installed,
a notification offers **Download JDK** and **Choose a JDK**; nothing is downloaded until you click.

## Maven settings

**Settings → Build, Execution, Deployment → Build Tools → Maven** sets how Maven runs in this
project; the settings are kept per project. Every Maven run uses them: goals from the Maven tool
window and Maven run configurations, Reload All Maven Projects and the dependency tree, tests run
with Maven, and the build of Java applications in Maven modules.

| Setting | What Maven gets |
|---|---|
| **Work offline** | `-o`; the same as **Toggle Offline Mode** in the Maven tool window. |
| **Always update snapshots** | `-U` |
| **Output level** | Quiet `-q`, Info, Debug `-X` |
| **Checksum policy** | Warn `-c`, Fail `-C` |
| **Multiproject build fail policy** | Fail fast `-ff`, Fail at end `-fae`, Never fail `-fn` |
| **Thread count** | `-T 4`, or `1C` for one thread per CPU core |
| **Maven home path** | **Use Maven wrapper** (the project's `mvnw`, else Maven on this computer, also from `~/.m2/wrapper/dists`), **Maven on this computer**, or **Custom**: a Maven folder with `bin/mvn` |
| **User settings file** | `-s <file>` instead of `~/.m2/settings.xml` |
| **Local repository** | `-Dmaven.repo.local=<folder>` instead of the one `settings.xml` names; Go to Dependency POM looks there too |

**Importing** is for Reload All Maven Projects and Show Dependencies: the **JDK for importer** and
the **VM options for importer** (`MAVEN_OPTS`) they run Maven with, and **Automatically download**
**Sources** (`dependency:sources`) and **Documentation** (the `javadoc` jars) of the dependencies,
in background tasks after each reload. The **Generated sources folders** choice is kept for the
project model built from Maven itself, which comes later.

**Runner** is for goals, tests and the build of applications:

- **VM options** (`MAVEN_OPTS`), **JRE** (the project's JDK, `JAVA_HOME` or another JDK),
  **Environment variables** and **Properties** (`-Dname=value`), written as `name=value` pairs
  separated by `;`;
- **Skip tests**: `-DskipTests`, the same as **Toggle 'Skip Tests' Mode**;
- **Delegate IDE build/run actions to Maven**, on by default: a Java application in a Maven module
  is built with `mvn compile`. Turned off, Maven only resolves the classpath and the IDE compiles
  `src/main/java` with `javac` into its own folder, with `src/main/resources` on the classpath.

**Running Tests** chooses which of Surefire's and Failsafe's `argLine`, `systemPropertyVariables`
and `environmentVariables` are passed on. Tests Maven runs in a process of their own get them from
Surefire itself; a test you debug runs inside Maven, so the ticked ones are read from the module's
`pom.xml` and passed to it.

## Gradle settings

**Settings → Build, Execution, Deployment → Build Tools → Gradle** sets how Gradle runs in this
project; the settings are kept per project. Every Gradle run uses them: tasks from the Gradle tool
window and Gradle run configurations, Reload All Gradle Projects and the build model, the classpath of
Java applications in Gradle builds, and tests run in a Gradle build.

| Setting | What Gradle gets |
|---|---|
| **Gradle user home** | `GRADLE_USER_HOME`: where Gradle keeps its caches and the distributions the wrapper downloads; empty for `~/.gradle` |
| **Offline work** | `--offline` |
| **Distribution** | **Wrapper** (the build's `gradlew`, else Gradle on this computer), **Local installation** (Gradle on this computer, also one downloaded by the IDE), or **Specified location**: a Gradle folder with `bin/gradle` |
| **Gradle JVM** | `JAVA_HOME` of Gradle: the project's JDK, `JAVA_HOME`, or another JDK; the application and the tests keep the project's JDK |
| **VM options** | `-Dorg.gradle.jvmargs=<options>` for the Gradle daemon, which Gradle reads before `org.gradle.jvmargs` of `gradle.properties` |
| **Download sources** | after each reload, a background task downloads the sources of the dependencies of every project |

**Build and run using** and **Run tests using** choose who does the work:

- **Gradle**, the default: a Java application is built with Gradle's `classes` before it runs, and
  tests run in Gradle's `test` task.
- **The IDE**: for an application, Gradle only resolves its classpath and the IDE compiles
  `src/main/java` with `javac` into its own folder, with `src/main/resources` on the classpath. For
  tests, Gradle compiles them and writes their classpath, and the IDE runs them on the JUnit
  Platform itself, with the project's JDK; debugging one attaches to that JVM. This needs the JUnit
  Platform (JUnit 5 or later, JUnit 4 through its Vintage engine); the launcher is added when the
  build does not have it.

## Why a project does not load

When something stops the project from loading as it should, one banner at the top of the Maven and
Gradle tool windows says what, with a button that fixes it. It names the first of these, and goes as
soon as the reason is gone, with no restart:

| Reason | What it offers |
|---|---|
| The tool is not found | **Download Maven** or **Download Gradle** |
| The project is not trusted | **Trust Project**; only you can press it, never an AI agent |
| No JDK, or not the Java the build asks for | **Use JDK** when one is installed, else **Download JDK**, and **Choose a JDK** |

A newer JDK you chose yourself shows no warning. A Gradle toolchain finds its own JDK, so it is not
a mismatch. **Reload** shows the same reason in the **Sync** tab of the Build tool window.

## Trusted projects

A build can run code on your computer: Gradle's scripts are programs, and Lombok and annotation
processors run inside the Java language server. So the IDE runs them by itself only in a project you
trust ([ADR-0205](../adr/0205-trusted-projects.md), [ADR-0248](../adr/0248-safe-mode.md)). A project
you have not trusted is asked about when it opens and stays in [safe mode](05-project.md#trust-and-safe-mode)
until you do; a notification asks too when an agent opened it or a run waits for trust.
**Trust Project** trusts it and the folders inside it. Until you do:

- the Java language server does not start, and starts without Lombok, annotation processors and
  Gradle import should it be running from earlier;
- changed build scripts wait for **Load Changes** instead of loading by themselves;
- nothing runs or debugs, its artifacts are not built;
- AI agents cannot trust it for you.

Reloads, goals, tasks and source downloads are refused for you as well, with an offer to trust the project, because a build runs the project's own code whichever Maven or Gradle starts it. Trust is kept in the IDE's data folder, never in the project.

## Lombok and annotation processors

In a trusted project, when it depends on **Lombok** (a `pom.xml` dependency or `annotationProcessorPaths` entry, a
Gradle `org.projectlombok:lombok` dependency, a version catalog entry used as `libs.lombok`, or the
`io.freefair.lombok` plug-in), the Java language server runs with Lombok's jar as its agent: getters,
builders, `log` of `@Slf4j` and the other generated members complete and navigate, and Lombok code shows no false
errors. The jar is the one the build has already downloaded, from the local Maven repository (the
Maven settings' local repository) or the Gradle cache (the Gradle user home); the IDE never
downloads it itself. When the build does not name a version, the newest one there is used. When
the project's Lombok version changes, or it gets downloaded after a reload, the language server
restarts with it, and a notification says so.

**Settings → Build, Execution, Deployment → Compiler → Annotation Processors**, kept per project:

| Setting | What it does |
|---|---|
| **Enable annotation processing** | on by default; off, `javac` gets `-proc:none` and the language server does not run processors of Gradle projects (it restarts to apply it) |
| **Obtain processors from** | **Project build**: Maven's `annotationProcessorPaths` (or Lombok when the project depends on it), Gradle's `annotationProcessor` configuration; or **Processor path**: jars and folders of your own |
| **Production sources directory** | where generated sources go (`-s`), relative to the module; empty for the build's: `target/generated-sources/annotations` or `build/generated/sources/annotationProcessor/java/main` |
| **Options** | processor options as `name=value; other=value`, passed as `-Aname=value` over the build's |

The page shows under each value what the build configures itself: its processors, its
generated sources folder and its `-A` options from `compilerArgs` or `options.compilerArgs`, and
the project's Lombok with the jar the language server uses, or a warning when that jar is not
downloaded yet. The settings apply when the IDE compiles a Java application itself (Maven's Runner
with *Delegate IDE build/run actions to Maven* off, or Gradle's *Build and run using: The IDE*,
or a project without a build), with `-proc:full` on JDK 21 and newer, since JDK 23 and newer run
no processors without it. A Maven or Gradle build of its own follows the build files; so does the
language server for a Maven project.

## The Maven tool window

**View → Tool Windows → Maven**, at the right of the window, in a project with a `pom.xml` while the
Maven plug-in is on. It shows:

- **Profiles** of the build, each with a tick: the ticked ones are passed to every goal you run from
  the tool window (`-P`). At first the profiles active by default are ticked; your ticks are kept
  with the project on this computer, so they are there again after a restart.
- Each project of the build (the root, then its **Modules**, then the projects you added with **Add
  Maven Project**) with:
  - **Lifecycle**: `clean`, `validate`, `compile`, `test`, `package`, `verify`, `install`, `site`,
    `deploy` (every phase of the three lifecycles when **Show Basic Phases Only** is off);
  - **Plugins**: those the packaging uses and those the build declares, with their goals
    (`compiler:compile`, `surefire:test`, `exec:java`…);
  - **Dependencies**: those the `pom.xml` declares, or the resolved tree once it is loaded;
  - **Run Configurations**: the project's run configurations of kind Maven; double-click one to run
    it.

Double-click a phase or a goal to run it in that module; the output is in the Run tool window.
Double-click a dependency to open its POM from the local repository (`~/.m2/repository`). A goal
with a keyboard shortcut shows it beside its name.

Type while the tree has the focus to search it: the first row whose name holds the text is
selected, the text is marked, **↑** and **↓** move, **⏎** runs or opens the row and **⎋** clears the
search.

Right-click a project for:

- **Reload Project**, **Unlink Maven Projects** (the project's own `pom.xml` or one you added; add it
  again with **Add Maven Project**) and **Ignore Projects** / **Unignore Projects**: an ignored
  project is greyed, or hidden while **Show Ignored Projects** is off (only the tool window changes:
  Maven and the language server still see it);
- **Open settings.xml**, or **Create settings.xml** when the user settings file does not exist yet;
- **Download Sources and Documentation** of the dependencies, now, whatever the Importing settings
  say;
- **Show Effective POM**: Maven computes the project's effective POM (`mvn help:effective-pom`, with
  the ticked profiles) in the background and the IDE opens it read-only. It is kept until a
  `pom.xml` of the build changes, so it opens at once the next time;
- **Analyze Dependencies**: opens the [Dependency Analyzer](#the-dependency-analyzer) on that
  project or module. A project added with **Add Maven Project** does not have it, because the
  analyzer reads the build at the project's root.

Right-click a phase or a goal for:

- **Run** and **Debug**: Debug starts Maven's own JVM waiting for the debugger (through
  `MAVEN_OPTS`) and attaches, so breakpoints stop in code that runs inside Maven: `exec:java`,
  plug-ins, tests run without a fork;
- **Create Run Configuration**: saves a Maven run configuration for the goal, with the ticked
  profiles, and selects it;
- **Execute Before Build**, **Execute After Build**, **Execute Before Rebuild** and **Execute After
  Rebuild** (ticked when set): the goal runs in the run of **Build → Build Artifacts** (and Rebuild),
  before anything is compiled or once compilation is done, before the jars are written. They are
  kept in the project's `.zen/build-triggers.json`;
- **Execute Before Run/Debug**: a list of the project's run configurations; tick one to add the goal
  to its Before launch tasks, untick it to remove it;
- **Assign Shortcut…**: press the keys that run the goal from anywhere in the IDE.

The toolbar:

| Button | What it does |
|---|---|
| **Reload All Maven Projects** | Reads the `pom.xml` files again, starts the Java language server when it is installed and enabled, even with no file open, imports the build into it, and loads the dependency tree in the background (this may download). |
| **Run** | Runs the selected phase or goal. |
| **Toggle Offline Mode** | Runs Maven with `-o` (Work offline in the Maven settings). |
| **Toggle 'Skip Tests' Mode** | Runs Maven with `-DskipTests` (Skip tests in the Maven settings' Runner). |
| **Show Dependencies** | Loads the resolved dependency tree (`mvn dependency:tree`). |
| **Add Maven Project** | Adds a `pom.xml` outside the project's folder, or one that is not a module of the project. |
| **Execute Maven Goal** | A field for a command line (`clean install -U`) run in the selected project: it completes phases and plug-in goals (`dependency:tree`) and lists the command lines you ran before. **↑**/**↓** choose, **⇥** or **⏎** takes a suggestion, **⏎** runs. |
| **View Options** | **Group Modules** (off lists every project at the top), **Show Ignored Projects** and **Show Basic Phases Only**; below them **Download Sources and Documentation**, as in the project's menu, and **Collapse All**, which closes every node. |

The project is read from its files, without running Maven, so the tool window fills at once.
Maven runs only when you run something, click Reload or ask for the effective POM. The profiles you
ticked, the projects you added or ignored, the view options and the goals you ran are kept with the
project on this computer.

## The Gradle tool window

**View → Tool Windows → Gradle**, at the right, in a Gradle build while the Gradle plug-in is on.

The window fills by itself once the project is trusted, without Reload: Gradle describes the build
through an init script in a background task (the wrapper may download Gradle the first time, and
Gradle may download plug-ins and libraries). The answer is kept per build file hash, so while the
build scripts, `gradle.properties`, the version catalog and the wrapper's properties are unchanged
the next start reads it and Gradle does not run. Until Gradle answers, the window lists the projects
of the settings script. If Gradle fails, the **Sync** tab of the Build tool window shows its output
and the window keeps the last good model.

Each project has:

- **Tasks**, by Gradle's groups, with `other` last. Double-click a task to run it.
- **Dependencies**, by configuration (`compileClasspath`, `runtimeClasspath`,
  `testCompileClasspath`, annotation processors and so on) as Gradle resolved them, with versions
  chosen over what was requested, unresolved ones marked, and `(*)` on a library listed in full
  earlier in the same configuration.
- **Run Configurations**, at the root of a build: its Gradle run configurations; double-click runs
  one.

Projects included with `includeBuild` and `buildSrc` show as projects of their own at the top of
the window; their tasks run from the composite (`:tools:core:test`). With Gradle older than 8.0,
`buildSrc` shows without tasks.

The toolbar:

| Button | What it does |
|---|---|
| **Reload All Gradle Projects** | Asks Gradle again, whatever the cache holds, and asks the Java language server to import the build |
| **Run** | Runs the selected task |
| **Add Gradle Project** | Links another build beside the project's own; pick its build or settings script. Its folder must be trusted. **Unlink Gradle Project** on its row removes it |
| **Execute Gradle Task** | A command line with the build's tasks, Gradle's common options and the lines you ran before |
| **Toggle Offline Mode** | The **Offline work** setting: every Gradle run gets `--offline` |
| **Build Tool Settings** | Opens the Gradle settings |
| **Collapse All** | Closes the tree |

A right click on a task offers **Run**, **Debug**, **Create Run Configuration**, **Execute Before
Sync**, **Execute After Sync**, **Execute Before Build**, **After Build**, **Before Rebuild**,
**After Rebuild**, **Execute Before Run/Debug** and **Assign Shortcut…**. **Debug** suspends the JVMs
the task forks (`run`, `test`) and attaches the debugger; a task that forks no JVM just runs. Tasks
set to run around a sync run in the same Gradle run that reloads the build, before and after the
model, so one that fails fails the sync; a start that reads the cache runs no Gradle and so none of
them. **Execute Before Build** and the others around a build work as for Maven goals: around Build
Artifacts and Rebuild Artifacts.

Type in the tree to search it: the first row holding the text is selected, ↑ and ↓ move, Enter
runs, Esc clears. When no Gradle is found for the build, the window says so and offers **Download
Gradle**. The linked builds and the lines you ran are kept with the project on this computer.

## The Dependencies tool window

**View → Tool Windows → Dependencies** manages a project's dependencies in one place, for Maven and
Gradle alike. It is on the bottom stripe in a project of a build system whose plug-in is on. The
modules are on the left; on the right are the selected module's declared dependencies with their
group, artifact, version and scope or configuration.

- **Search.** Type a name or `group:artifact` in the search field. The results show the latest
  stable version. Click one, choose the **Version** and the **Scope** (Maven) or configuration
  (Gradle), and click **Add**.
- **Upgrade.** A version shown in colour has something to say: a newer version, or known
  vulnerabilities. Click the dependency: choose a newer version and click **Upgrade**. Pre-releases
  are offered only for a dependency that already uses one; a `-jre` version is offered `-jre`
  versions.
- **Vulnerabilities.** A vulnerable version shows its advisories and **Upgrade to** the nearest
  version free of them; when there is none, the window says so.
- **Remove** deletes the dependency from the build file.
- **Check Again** in the header forgets the answers and asks the servers again.

The build file is edited in place and keeps its formatting and comments, and the project then loads
again, as with Load Changes:

- `pom.xml`: a version a parent or a BOM manages is not written again. Upgrade changes the version
  where it is written: on the dependency, in a property, or in a parent of the project. A version
  that a parent or a BOM from a repository manages cannot be changed here, and the window says what
  manages it.
- Gradle: with a version catalog (`gradle/libs.versions.toml`) a new library and its version go into
  the catalog and the script gets `libs.<alias>`. Without one, a line goes into the `dependencies`
  block in the script's own style: Groovy or Kotlin, its quotes and parentheses. Upgrade changes the
  catalog, the variable, the `gradle.properties` entry or the line that holds the version. Removing
  the last use of a catalog library removes it from the catalog too.

An open build file is edited in its editor and saved, so Undo takes the edit back. Saving includes
what you had typed in that file.

### In a project you have not trusted

Loading a build runs its own code, so in safe mode nothing loads it: **Reload**, **Show Dependencies**,
**Download Sources and Documentation**, **Show Effective POM**, **Analyze Dependencies** and the Gradle
tasks all say the project is not trusted and offer to trust it. The IDE can still read the build files.

### What an agent may not change

An agent may not set what a build runs with: the Gradle distribution, home, JVM, user home and VM options;
the Maven home, user settings, local repository, importer JDK and VM options, and the runner's JRE, VM
options, environment and properties; an Ant build file's JDK, classpath and properties. You can.

### Network use

Search and the checks use the network. They are off by default and each can be turned on in
**Settings → Build, Execution, Deployment → Build Tools**, under **Dependencies**, which names the
servers each one contacts:

| Setting | Contacts |
|---|---|
| **Search for libraries in repositories** (`dependencies.search`) | `search.maven.org` to find artifacts; `maven-metadata.xml` of the module's repositories for their versions |
| **Check for newer versions of dependencies** (`dependencies.checkUpdates`) | `maven-metadata.xml` of the module's repositories (Maven Central, `repo.maven.apache.org`, by default) |
| **Check dependencies for known vulnerabilities** (`dependencies.checkVulnerabilities`) | `api.osv.dev`, and the same `maven-metadata.xml` |

- Versions always come from `maven-metadata.xml`, never from the search index, which lags behind.
- OSV is told only `group:artifact`, and only for what a repository without credentials lists: a
  private library's name is never sent.
- A repository the project names itself is contacted only once the project is trusted. Repositories
  from your Maven `settings.xml`, its mirrors and its credentials always apply; a password is sent
  only to the repository with its id, and an encrypted one is not used. Credentials Gradle keeps are
  not read, so a Gradle repository is asked without them. A plain `http://` repository is not asked.
- A POM of a parent or a BOM that is not in the local repository is downloaded from the same
  repositories, to learn which versions they manage.
- **Work offline** of the Maven or Gradle settings keeps all of this off the network.
- Answers are kept in the IDE's caches for an hour, and asked again with their HTTP validators.
  Offline, the window still lists and edits the declared dependencies, shows what it kept, and says
  that search and checks need the network.

## Load Changes after editing a build script

When you edit `pom.xml`, `build.gradle(.kts)` or `settings.gradle(.kts)`, the project no longer
matches what was loaded, and a **Load Maven Changes** (or **Load Gradle Changes**) button appears
in the editor's top-right corner. Click it, or press **⇧⌘I** (**Load Changes**): the IDE saves the
files and reloads the project as Reload All does. The button hides once the project is loaded.

Changes made outside the IDE (a VCS update, another program) load by themselves. **Settings →
Build, Execution, Deployment → Build Tools → When build scripts change, reload the project** chooses which changes do:

- **External changes** (the default): only changes made outside the IDE; edits in the IDE show the
  button.
- **Any changes**: edits in the IDE too, once they are saved.

## The Build tool window: Sync

**View → Tool Windows → Build**, at the bottom, in a Maven or Gradle project. Its **Sync** tab lists
each load of the project, newest first, with its steps (reading the build files, the import into
the Java language server, resolving the dependencies or loading the tasks), how long each took,
and their errors and warnings. The import step runs until the language server says it finished
importing, and fails with the server's reason. Click an error to open its file at the line. An error with an
obvious fix shows it as a link on its row:

- **Download Maven** or **Download Gradle**, when no tool is found;
- **Install** the Java language server, when it is not installed;
- **Choose a JDK**, when the build needs another Java (it opens Project Structure);
- **Work Offline**, when a repository cannot be reached; the project then loads again offline.

A failed load opens the Build tool window by itself. **Reload** in its header loads the project
again.
download Gradle the first time) and the resolved libraries External Libraries lists, and to import
the build into the Java language server.
Double-click a task to run it.

## The Dependency Analyzer

**Analyze Dependencies**, a button in the Maven and Gradle tool windows and an item of a Maven
project's context menu, shows why each version is on a module's classpath. It opens the
**Dependency Analyzer** tab of the Build tool window and asks
Maven (`dependency:tree -Dverbose`) or Gradle (the resolved graph of each compile and runtime
classpath, through an init script) in a background task, which may download. **Refresh** asks again.

- **Module** chooses the module, **Scope** a Maven scope or a Gradle configuration.
- **List** has each library once, with the version that won; **Tree** shows what brings what. In a
  Gradle module the tree has a folder per configuration.
- A filled dot marks a conflict: this place asked for another version than the one used, shown as
  `1.11 → 1.10`. A hollow dot marks a place that adds nothing, because the same version is already
  there. **Show Conflicts Only** leaves the conflicts, in the tree with what leads to them.
- Selecting a library lists on the right which version won and why, every version asked for and
  every path to it from the module.
- **Go to Declaration**, or a double-click, opens the place in the build file that declares it, or
  for a library that comes with another, the direct dependency that brings it. It looks in the
  module's build file, in parent POMs, in the root build script and in the version catalog.
- **Exclude** adds the exclusion to the build file and loads the project again. In a Maven module it
  is `<exclusions>` under the direct dependency; in a Gradle script `exclude` in that declaration's
  closure, in the script's own Groovy or Kotlin. A row of the tree excludes from the dependency it
  is under; a row of the list from every dependency that brings it. A library only the module itself
  declares cannot be excluded. Comments and formatting of the file stay as they are.

## Run configurations

**Run → Edit Configurations…** has two more kinds:

- **Maven**: the POM file, the goals (`clean install`), the profiles (`ci,!fast`), Work offline and
  Skip tests; the program arguments follow the goals (`-U`, `-Dkey=value`).
- **Gradle**: the Gradle project's folder and the tasks with their options
  (`:app:run --args="x"`).

A goal or task run from a tool window is a temporary configuration named like `core [clean install]`.
Run configurations of JetBrains IDEs' Maven and Gradle types in `.run/` and
`.idea/runConfigurations/` are taken over as they are.

## Libraries and their sources

In a Maven or Gradle project, [External Libraries](05-project.md#external-libraries) in the Project
tool window lists the project's JDK, as `< 21.0.5 >`, and every library, as
`Maven: org.slf4j:slf4j-api:2.0.13` or `Gradle: com.google.guava:guava:33.0.0-jre`. The JDK shows
its sources (`lib/src.zip`, by module); a library shows the packages, classes and other files of
its jar from the local Maven repository (`~/.m2/repository`, or the `localRepository` of
`~/.m2/settings.xml`) or Gradle's cache (`~/.gradle/caches/modules-2`). Maven's list is the
resolved tree once it is loaded (**Reload All Maven Projects** or **Show Dependencies** in the
Maven tool window), else the dependencies the `pom.xml` files declare; Gradle's is the resolved
compile and runtime classpaths, main and test, once **Reload All Gradle Projects** has loaded them,
else the libraries the build scripts declare as `group:artifact:version`. A library not downloaded
yet is greyed out.

Opening a class, from External Libraries or with **Go to Declaration** (**⌘B**) from your code,
shows its sources when the library has them (the `-sources.jar` beside its jar). Otherwise it
shows a decompiled view: the class's declarations without their code, with a banner that gives
the class file's bytecode version and offers **Download Sources**. Library files and decompiled
views are read-only.

**Download Sources** asks the build tool for the library's sources: Maven runs
`mvn dependency:sources` for that library, Gradle a task that resolves its sources artifact. It
uses the network, runs in the background (the Background Tasks window shows it) and never runs on
its own; once it is done, the banner offers **Open Sources**. Agents call
`java.libraries.downloadSources` (`path`: a library file or the decompiled view; every library
without it) and read what an open library document is with `java.libraries.document`.

## Project Structure

[Project Structure](05-project.md#project-structure) (**⌘;**) shows what the build says about the
project, marked **From pom.xml** or **From build.gradle.kts** with an **Open build file** link:

- Maven: every module of the reactor, its source, test and resource folders (`<build>`'s, else
  `src/main/java`, `src/main/resources`, `src/test/java`, `src/test/resources`, and the Kotlin
  folders that exist), `target` excluded, the output folders (`target/classes`,
  `target/test-classes`, or `<outputDirectory>`), the language level (`maven.compiler.release`,
  `maven.compiler.source`, or the compiler plug-in's `release` or `source`) and the declared
  dependencies with their scopes; a version a parent or BOM manages shows as **managed**.
- Gradle: every project of the settings script, the `src/main` and `src/test` folders that exist,
  `build` excluded, the language level (`JavaLanguageVersion.of`, `jvmToolchain`,
  `sourceCompatibility`) and the declared dependencies with their configurations. The output
  folders are Gradle's defaults (`build/classes/java/main`), and the page says they are not read
  from the build.

The project's JDK and language level are the IDE's and are chosen on the Project page. A parent POM
or a build script that moves folders is not read yet.

**Project Structure → Artifacts** offers the build's own packaging first: `mvn package` for Maven,
`gradle jar` for Gradle (`bootJar` when the build applies `org.springframework.boot`), then a jar
the IDE writes from the modules' compiled output and their runtime libraries, inside it or next to
it, with a Main-Class. **Build → Build Artifacts…** builds them; see
[Artifacts](05-project.md#artifacts).

## pom.xml and build scripts

With the Maven plug-in, `pom.xml` keeps everything XML has, and completion (⌃Space) knows the POM:

- after `<`, the elements allowed where you are (`dependency` in `<dependencies>`);
- the values of `<scope>`, `<packaging>` and `<type>`;
- the lifecycle phases in `<phase>`;
- the goals of the well-known plug-ins in `<goal>`;
- the group and artifact ids the file already uses;
- the properties after `${`.

With the Gradle plug-in, Groovy build scripts (`*.gradle`) are highlighted, fold, and list their
blocks (`plugins`, `dependencies`, `tasks.register('hello')`) in the Structure view. Kotlin build
scripts (`*.gradle.kts`) are read by the [Kotlin](06-kotlin.md) plug-in: JvmJizo comes with it, and
the other IDEs offer it when one opens.

### Dependencies in build files

`pom.xml`, Gradle scripts and `gradle/libs.versions.toml` complete dependency coordinates: in a
`<dependency>`'s `<groupId>`, `<artifactId>` and `<version>`, and inside the strings of a
`dependencies` block or a `[libraries]` table. Groups and artifacts come from the search index,
versions from `maven-metadata.xml`.

An outdated version is underlined with **Upgrade to …**, a vulnerable one with its advisories and
**Upgrade to unaffected version …**. The fix writes the build file, which loads again by itself.
They stay quiet until the settings above turn them on, and without the network.

## For AI agents

Everything above is a command:

- Maven: `maven.project.get`, `maven.goal.run`, `maven.options.set`, `maven.project.reload`,
  `maven.dependencies.tree`, `maven.dependency.open` and `maven.distribution.download`.
- Gradle: `gradle.project.get`, `gradle.model.get` (`{ load, waitMs }`: the projects with their
  tasks, source sets and resolved configurations), `gradle.tasks.list`, `gradle.task.run`,
  `gradle.task.debug`, `gradle.options.set` (offline) and `gradle.project.reload`.
- Trust: `project.trust.state` says whether the project is trusted and `project.trust.revoke` stops
  trusting it; only the person trusts a project (`project.trust.grant` and the notification's
  **Trust Project** are refused for agents with `agentNotAllowed`). In an untrusted project the
  commands that reload or run a build are refused for agents with `notTrusted`, and every run for
  everyone. `project.trustedLocations.get` and `.set` read and change Trusted Locations; agents may
  only remove and reorder.
- Load Changes: `build.changes.get` lists the build systems of the project and those whose build
  scripts changed, `view.buildChanges.load` (⇧⌘I, optionally `{ system }`) loads them, and the
  setting `build.reloadAfterChanges` is `external` or `any`.
- Dependency Analyzer: `build.dependencies.analyze` (`{ system, module, scope, conflictsOnly, search,
  load, waitMs }`) gives the modules, the libraries with every usage and the tree;
  `build.dependencies.declaration` (`{ groupId, artifactId, via, noOpen }`) opens or answers where a
  dependency is declared; `build.dependencies.exclude` (`{ groupId, artifactId, via, noReload }`)
  edits the build file and loads again. `view.dependencyAnalyzer.open`, `.set`, `.select`, `.toggle`
  and `.refresh` drive the tab, and `get_window_state` lists it as `dependencyAnalyzer`.
- Sync: `build.sync.get` gives the Sync tab's loads with their steps, messages, places (`offset`
  for `navigate.place.goTo`) and fixes (a command, and whether to load again after it);
  `view.toolWindow.build` shows the tool window, and `get_window_state` lists its rows
  (`buildSync.rows`) and the pending changes (`buildSync.pending`).
  `maven.dependencies.tree`, `maven.dependency.open` and `maven.distribution.download`; the
  settings with `maven.settings.get` and `maven.settings.set` (`{ "key": "runner.vmOptions",
  "value": "-Xmx2g" }`).
- Gradle: `gradle.project.get`, `gradle.tasks.list`, `gradle.task.run`, `gradle.project.reload` and
  `gradle.distribution.download` (`{ "version": "9.2.1" }`, the current release without one); the
  settings with `gradle.settings.get` and `gradle.settings.set` (`{ "key": "runTests", "value":
  "ide" }`).
- Annotation processing: `annotationProcessing.settings.get` (the settings and what the build
  configures, with the project's Lombok) and `annotationProcessing.settings.set` (`{ "key":
  "enabled", "value": false }`).
- Gradle: `gradle.project.get`, `gradle.tasks.list`, `gradle.task.run` and `gradle.project.reload`.
- Project Structure: `project.structure.project`, `project.structure.modules`,
  `project.structure.libraries`, `project.structure.sdks`, `project.structure.problems` and
  `project.structure.openBuildFile`.
- Artifacts: `project.structure.artifacts`, `project.artifact.save`, `project.artifact.delete`,
  `build.artifacts.build`, `build.artifacts.rebuild` and `build.artifacts.clean`.
- Dependencies: `dependencies.project.list`, `dependencies.repository.search`,
  `dependencies.versions.get`, `dependencies.module.add`, `dependencies.version.set`,
  `dependencies.module.remove` and `dependencies.project.refresh`. They never wait for the network
  unless given `waitMs`; `pending` says more is coming. `view.toolWindow.dependencies` shows the
  window, `get_window_state` lists it under `buildTools.dependencies`, and
  `view.dependencies.select` (`{ module, row }`) and `view.dependencies.search` (`{ query }`) drive it.
- The tool windows: `view.toolWindow.maven` and `view.toolWindow.gradle`; `get_window_state`
  lists a shown window's rows (`buildTools.maven.rows`), and `view.buildTool.select`,
  `view.buildTool.toggle` and `view.buildTool.run` (`{ tool, row }`) select, open and run one as a
  click and a double-click do, in a window and in a headless IDE alike. `view.buildTool.menu`
  (`{ tool, row }`) lists a row's context menu as commands with their parameters and ticks,
  `view.buildTool.search` (`{ tool, text }`) is the speed search, `view.maven.executeGoal`
  (`{ goals }`) runs a command line in the selected project and `view.buildTool.assignShortcut`
  opens the shortcut recorder.
- The Maven tool window's actions: `maven.project.add`, `maven.project.unlink`,
  `maven.project.ignore`, `maven.view.set`, `maven.goal.complete` (Execute Maven Goal's
  suggestions), `maven.goal.debug`, `maven.runConfiguration.create`, `maven.settingsFile.open`,
  `maven.sources.download` and `maven.effectivePom.show` (`{ pom, waitMs }`; call it again with
  `start: false` to open a result that was still loading).
- The Gradle tool window's actions: `gradle.project.add`, `gradle.project.unlink`,
  `gradle.task.complete` (Execute Gradle Task's suggestions), `gradle.runConfiguration.create`,
  `view.gradle.executeTask` (`{ tasks, projectPath }`; without `tasks` it opens the popup) and
  `view.gradle.beforeRun`; `get_window_state` lists the rows in `buildTools.gradle.rows`.
- Build triggers: `build.triggers.list` and `build.triggers.set` (`{ when, task, on }`, `when` one
  of `beforeBuild`, `afterBuild`, `beforeRebuild`, `afterRebuild`, and for Gradle tasks
  `beforeSync` and `afterSync`, `task` a before-launch task such as `{ "type": "maven.goal", "pom":
  "pom.xml", "goals": "generate-sources" }`). A goal's shortcut
  is the keymap action `maven.goal.run {"goals":"…","pom":"…"}`, a task's
  `gradle.task.run {"projectPath":".","tasks":"…"}`: an action written as a command id,
  a space and its parameters as JSON runs that command with them (`ui.keymap.setShortcut`).
