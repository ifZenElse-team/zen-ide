# Maven and Gradle (ADR-0149).

workbench-build-cancelled = Cancelled.
workbench-build-failed = { $program } ended with exit code { $code }:
    { $output }

workbench-maven-kind = Maven
workbench-maven-task = Run Maven Goal
workbench-maven-field-pom = POM file:
workbench-maven-field-goals = Run (goals):
workbench-maven-field-profiles = Profiles:
workbench-maven-field-offline = Work offline
workbench-maven-field-skip-tests = Skip tests
workbench-maven-no-goals = name the goals or phases to run, e.g. clean install
workbench-maven-anything-help = Maven goals and phases: mvn clean install, mvn test
workbench-maven-not-found = Maven is not found: the project has no Maven Wrapper (mvnw), and no mvn or wrapper download is on this computer. Install Maven, or use Download Maven.
workbench-maven-no-pom = The project has no pom.xml at its root.
workbench-maven-no-module = { $pom } is not a pom.xml of the project.
workbench-maven-tree-task = Loading the Maven dependency tree
workbench-maven-no-coordinates = Put the caret on a dependency, plug-in or parent of a pom.xml, or give its coordinates.
workbench-maven-no-version = The version of { $coordinates } is not written in this pom.xml (a parent or a BOM manages it); open it from the dependency tree.
workbench-maven-not-downloaded = { $coordinates } is not in the local Maven repository yet; Reload All Maven Projects downloads it.
workbench-maven-download-task = Downloading Maven { $version }
workbench-maven-download-no-folder = The IDE has no data folder to download Maven into.
workbench-maven-download-checksum = The download of { $url } does not match its SHA-512; nothing was installed.
workbench-maven-download-unavailable = Maven cannot be downloaded here.
workbench-maven-download-done = Maven is installed in { $folder }.

workbench-gradle-kind = Gradle
workbench-gradle-task = Run Gradle Task
workbench-gradle-field-project = Gradle project:
workbench-gradle-field-tasks = Run (tasks and arguments):
workbench-gradle-no-tasks = name the tasks to run, e.g. build or :app:run
workbench-gradle-not-found = Gradle is not found: the build has no Gradle Wrapper (gradlew), and no gradle or wrapper download is on this computer. Add the wrapper to the build, install Gradle, or use Download Gradle; Settings → Build Tools → Gradle chooses which Gradle runs.
workbench-gradle-anything-help = Gradle tasks: gradle build, gradle test, gradle :app:run
workbench-gradle-no-build = The project has no settings.gradle(.kts) or build.gradle(.kts) at its root.
workbench-gradle-tasks-task = Loading the Gradle tasks

workbench-sync-maven-read = Read pom.xml files
workbench-sync-gradle-read = Read Gradle scripts
workbench-sync-import = Import into the Java language server
workbench-sync-no-java-server = No Java language server is available: the Java plug-in is off or not installed.
workbench-sync-java-server-off = { $server } is turned off, so the project is not imported into it.
workbench-sync-java-server-stopped = { $server } is stopped, so the project is not imported into it.
workbench-sync-java-server-missing = { $server } is not installed, so the project is not imported into it.
workbench-sync-import-long = The import is taking long; it goes on in the background.
workbench-sync-fix-install-server = Install { $server }
workbench-sync-fix-restart-server = Restart
workbench-sync-maven-dependencies = Resolve dependencies
workbench-sync-gradle-tasks = Load the build's tasks
workbench-sync-fix-download-maven = Download Maven
workbench-sync-fix-download-gradle = Download Gradle
workbench-sync-check = Check the project
workbench-block-not-trusted = This project is not trusted: its build scripts do not run by themselves, and Java works without Lombok and annotation processors.
workbench-block-no-jdk = No JDK is on this computer.
workbench-block-jdk = The build asks for Java { $wanted }, and it runs on JDK { $major }.
workbench-block-use-jdk = Use JDK { $level }
workbench-sync-fix-jdk = Choose a JDK
workbench-sync-fix-offline = Work Offline
workbench-maven-not-linked = { $pom } is not a linked Maven project; a module goes with its project.
workbench-maven-no-pom-file = There is no pom.xml at { $pom }.
workbench-maven-effective-task = Computing the effective POM
workbench-build-triggers-write = Cannot save the build triggers: { $error }
workbench-build-triggers-not-lines = A task of type { $kind } cannot run around a build.

workbench-dependency-task = Analyzing dependencies
workbench-dependency-no-build = The project has no Maven or Gradle build to analyze.
workbench-dependency-no-module = The build has no module { $module }.
workbench-dependency-not-loaded = The dependencies are not analyzed yet; run Analyze Dependencies first.
workbench-dependency-no-declaration = No build file of the project declares { $dependency }.
workbench-dependency-exclude-direct = { $dependency } does not come with another dependency here; remove it from the build file instead.
workbench-dependency-bad-name = { $dependency } is not a dependency's name.
workbench-dependency-edit-failed = The build file could not be edited: { $error }
workbench-gradle-model-empty = Gradle ended without describing the build.
workbench-gradle-no-build-at = There is no Gradle build in { $folder }.
workbench-gradle-not-linked = { $folder } is not a linked Gradle build.
workbench-build-triggers-sync-gradle = Only Gradle tasks run around a sync.
