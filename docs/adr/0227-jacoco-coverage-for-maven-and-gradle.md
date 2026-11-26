# ADR-0227: JaCoCo coverage for Maven and Gradle runs

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0208](0208-code-coverage-model-and-providers.md) (the coverage model and
  providers), [ADR-0152](0152-java-run-configurations-and-the-classpath.md) (Java run
  configurations), [ADR-0153](0153-java-debugging-and-tests.md) (Java tests and the Gradle init script),
  ADR-0195 (network use), AGENTS.md golden rule 10

## Context

ADR-0208 left the JVM half for later: the Java test provider (JUnit and TestNG through Surefire or
Gradle) and the Java program provider (Application and JAR Application) must give a
`CoverageProvider`. JaCoCo is the coverage tool of Java and Kotlin builds. It measures through an
agent in the JVM that runs the code and writes an execution data file (`.exec`); its documented XML
report gives, per source file, each line's missed and covered instructions (`mi`, `ci`) and branches
(`mb`, `cb`). Many builds already use JaCoCo's Maven or Gradle plug-in; many do not, and the IDE
must not download anything without being asked.

## Decision

1. **The build's own plug-in first.** Maven with `jacoco-maven-plugin` in a `pom.xml` of the build:
   `jacoco:prepare-agent` before `test` (left out when a `pom.xml` binds `prepare-agent`), with
   `-Djacoco.destFile=<run folder>/jacoco.exec` and `-Djacoco.append=true`; once the run ended,
   `mvn -B -q jacoco:report -Djacoco.dataFile=<that file>` and every module's
   `target/site/jacoco/jacoco.xml` written since. Gradle with the `jacoco` plug-in: the IDE's
   init script (ADR-0153), with `-Dzen.coverageDir`, points each test task's data into the run's
   folder after the project is evaluated, makes `test` finalized by `jacocoTestReport` (so failing
   tests still give a report) and asks for its XML report in the run's folder.
2. **Otherwise the agent.** Maven: `-DargLine=-javaagent:<agent>=destfile=<exec>,append=true`
   (quoted when a path has spaces); Gradle: `-Dzen.jacocoAgent`, which the init script adds to every
   test task's JVM arguments; Application and JAR Application: the agent before `-cp`/`-jar`. The
   agent jar is copied into the run's folder first. Once the run ended, the JaCoCo command-line tool
   (`java -jar org.jacoco.cli-<v>-nodeps.jar report <exec> --classfiles … --xml …`) turns the data
   into the XML report; the IDE never reads the `.exec` format itself. Classes measured: Maven
   modules' `target/classes`, Gradle projects' `build/classes/*/main`, an Application's classpath
   folders, a JAR Application's JAR.
3. **Where the jars come from.** `org.jacoco.agent-0.8.16-runtime.jar` and
   `org.jacoco.cli-0.8.16-nodeps.jar` in the IDE's data folder (`tools/jacoco/0.8.16/`), the local
   Maven repository or the Gradle cache, only when the file's SHA-512 is the one fixed in the code:
   those folders can be written by any build, and the IDE runs these jars itself, so another version
   or a changed jar is never used. The coverage run's folder is private and randomly named. When
   there is none, Run with Coverage is refused with a message
   that names the command `java.jacoco.download`, which downloads JaCoCo 0.8.16 from Maven
   Central in a background task, only when it runs, and checks each jar's SHA-512 (fixed in the
   code; computed from jars matching the SHA-1 Maven Central publishes for them).
4. **Mapping into the model.** Each `<sourcefile>` of each `<package>` (inside `<group>`s too) is
   found among the project's `.java`, `.kt`, `.groovy` and `.scala` files by `package/file`, a file
   under a `main` folder first; a Kotlin file outside its package's folder is found by its name when
   only one file has it; anything else is left out. A line is `part(hits 1)` when some instructions
   ran, `part(hits 0)` when none did, plus a missed part when it ran with some instructions missed;
   each covered and missed branch is a `branch`. So a line is partly covered when some instructions
   or branches did not run. JaCoCo has no hit counts: a line that ran counts 1.

## Consequences

- No platform change: the providers in `zen-workbench` (`java/jacoco.rs`) answer `coverage()`.
- A `pom.xml` that sets its own `<argLine>` without `@{argLine}` and no JaCoCo plug-in gets no
  agent; a build that writes JaCoCo's data or report somewhere of its own choosing is not read. Run
  with Coverage then says that no data was written.
- Classes in other folders than the conventional ones are not measured by the agent route.
- The Gradle init script changes only test tasks, and only for runs with coverage.
