# Ant

Ant builds list and run their targets in every Zen IDE with the Ant plug-in; JvmJizo comes with it.
Opening a project with a `build.xml` at its root offers the plug-in in a notification when it is not
installed. Maven and Gradle are on their own page, [Maven and Gradle](26-maven-and-gradle.md).

## The program and the JDK

The IDE looks for `ant` on `PATH`, in the usual tool folders (`/opt/homebrew/bin`,
`/usr/local/bin`…), in `ANT_HOME/bin` and in SDKMAN!. When Ant is found nowhere, the Ant tool
window says so and offers **Download Ant**: Apache Ant 1.10.18 is downloaded from the Apache
archive, checked against its SHA-512, and kept in the IDE's own folder. This is the only time the
IDE downloads Ant, and only when you click it.

Ant runs with the project's JDK (`JAVA_HOME` is set to it and its `bin` comes first on `PATH`),
unless a build file names its own JDK.

## The Ant tool window

**View → Tool Windows → Ant**, at the right of the window, in a project with an Ant build file
while the Ant plug-in is on. It lists the build files, each with its targets; the default target is
marked **(default)**, and a target's description follows its name. At first the project's
`build.xml` is listed.

Double-click a target to run it in its build file's folder; the output is in the Run tool window,
where the files Ant and the compiler name (`build.xml:12:`, `A.java:3:`) are links.

The toolbar:

| Button | What it does |
|---|---|
| **Add Build File** (**+**) | Adds another build file of the project: an XML file whose root element is `<project>`. |
| **Remove Build File** | Takes the selected build file out of the list; the file stays. |
| **Run** | Runs the selected target. |
| **Filter Targets** | Shows only the targets with a description, and the default one. |
| **Collapse All** | Closes every build file. |

The build files are read from their XML, without running Ant, so the tool window fills at once.
The list, the filter and each build file's settings are kept in the project's `.zen/ant.json`.

## A build file's settings

Each build file's runs can use their own:

- **JDK**: a JDK's home folder, instead of the project's;
- **extra classpath**: jars and folders for Ant's own class loader (`-lib`), for tasks that are not
  in Ant itself;
- **maximum memory**: in megabytes, passed to Ant's JVM as `-Xmx` in `ANT_OPTS`;
- **properties**: name and value pairs passed to every run as `-Dname=value`.

They are set with `ant.buildFile.configure` (below); the Properties dialog of the tool window is not
there yet.

## Run configurations

**Run → Edit Configurations…** has the kind **Ant**: the build file and the targets
(`clean jar`; the default target when empty). A target run from the tool window is a temporary
configuration named like `shop [jar]`.

## build.xml

With the Ant plug-in, `build.xml` keeps everything XML has, and completion (⌃Space) knows Ant:

- after `<`, the tasks and types allowed where you are (`target` in `<project>`, the tasks in a
  `<target>`, `classpath` and `src` in `<javac>`);
- the attributes of the common tasks (`srcdir`, `destdir`, `includeantruntime` in `<javac>`);
- target names in `depends`, `extensionOf`, the project's `default` and `<antcall target>`;
- the properties after `${` and in `if` and `unless`, and the ids in `refid` and `classpathref`;
- the values of `true`/`false` attributes and of `level`, `printsummary` and the like.

**Navigate → Declaration** (⌘B or ⌘-click) on a target's name in `depends`, `default` or
`<antcall target>` goes to that target.

## For AI agents

Everything above is a command: `ant.project.get`, `ant.buildFile.add`, `ant.buildFile.remove`,
`ant.buildFile.configure`, `ant.options.set`, `ant.target.run` and `ant.distribution.download`.
The tool window is `view.toolWindow.ant`; `get_window_state` lists its rows (`buildTools.ant.rows`),
and `view.buildTool.select`, `view.buildTool.toggle` and `view.buildTool.run` with
`{ "tool": "ant", "row" }` select, open and run one as a click and a double-click do.
