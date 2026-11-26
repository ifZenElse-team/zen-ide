# Spring Boot

The Spring Boot plug-in knows a Spring Boot project's configuration files, beans and endpoints,
and runs its applications. It works in every Zen IDE: JvmJizo comes with it, and the other IDEs offer it in a
notification when a project has `src/main/resources/application.properties`, `application.yml` or
`application.yaml` ([Plug-ins](10-plugins.md)).

## Configuration files

The plug-in takes `application.properties`, `application-<profile>.properties`, `application.yml`,
`application-<profile>.yml` and their `.yaml` forms, wherever they are. A `.properties` file is
edited as the [Properties plug-in](30-properties.md) edits it, a `.yml` file is highlighted as YAML,
with its nested keys folded and listed in the Structure view (**⌘7**).

What a property means comes from the project itself: every jar of the project (the dependencies
Maven or Gradle resolved, as **External Libraries** lists them) may carry
`META-INF/spring-configuration-metadata.json` and
`META-INF/additional-spring-configuration-metadata.json`, written by Spring Boot's configuration
processor. The IDE reads them, and the project's own ones from `target/classes`,
`build/classes/java/main` and `src/main/resources/META-INF`, so the properties always match the
project's Spring Boot version and need no network. They are read in the background when the project
loads and again when its libraries change.

- **Completion** (**⌃Space**, or typing): at the start of a line, the properties the file does not
  set yet, with their type and description. In a `.properties` file the full name is inserted with
  `=`; in a `.yml` file only what is left under the keys above the caret, nested as YAML wants it
  (`servlet:` and `context-path: ` below `server:`). After `=` or `: `, the values the metadata
  names for the property, and `true` and `false` for a Boolean one.
- **Quick Documentation** (**F1**) on a key shows its name, type, description, default value, the
  class that declares it, and why it is deprecated with what to use instead.
- **Problems**: a key no metadata declares is a weak warning, *'…' is not a
  known configuration property*; a deprecated key is struck through and a warning, or an error when Spring Boot no
  longer reads it. Names are matched as Spring Boot binds them: `contextPath`, `context-path` and
  `CONTEXT_PATH` are the same property; a key under a map property (`logging.level.org.hibernate`)
  and an index of a list (`spring.profiles.include[0]`) belong to that property. Keys inside a YAML
  list are not checked. Until the project's libraries are known (the build has not loaded yet),
  nothing is reported.

## Run an application

A Java or Kotlin class marked `@SpringBootApplication` with a `main` gets a run icon in the gutter
that runs it as a **Spring Boot** configuration; **Run Context Configuration** (**⌃⇧R**) inside it
does the same. A **Spring Boot** configuration has the settings of a Java **Application**
([Run a Java program](06-java.md#run-a-java-program)), it builds and runs the same way, and adds:

- **Active profiles**: a list such as `dev, local`, passed as `--spring.profiles.active=dev,local`.
- **Override configuration properties**: names and values, each passed as `--name=value`, which
  Spring Boot takes over every configuration file.

Both come after the program's own arguments. Debugging works as for a Java application.

## Beans and where they are injected

In a Java or Kotlin file of the project, a gutter icon marks each bean and each place a bean is
injected into, with the Spring Boot plug-in on:

- A **bean** (a filled circle) is a class marked `@Component`, `@Service`, `@Repository`,
  `@Controller`, `@RestController` or `@Configuration`, or a `@Bean` method of a configuration.
  Clicking the icon goes to where the bean is injected.
- An **injection point** (a ring) is a constructor parameter of a bean class (a Kotlin primary
  constructor's too), an `@Autowired` (or `@Inject`) field, property or setter, and a `@Bean`
  method's parameter. Clicking it goes to the beans that can fill it.

With one place to go, the click goes there; with several, a list opens to choose from (**Esc**
closes it). Beans are matched to injection points by type: the class's name, the classes and
interfaces it extends or implements, and a `@Bean` method's return type, by simple name. The IDE
reads the project's sources itself, so this works without a language server, and has limits:
names are compared without their package, qualifiers (`@Qualifier`, `@Primary`), profiles and
conditions are not taken into account, beans from libraries are not known, and a type's
supertypes are only the ones its own declaration names. The sources are read again in the
background a few seconds after they change; open files count as they are being edited.

## The Endpoints tool window

**View → Tool Windows → Endpoints** lists the project's request mappings: each method marked
`@GetMapping`, `@PostMapping`, `@PutMapping`, `@DeleteMapping`, `@PatchMapping` or
`@RequestMapping` in a Java or Kotlin class, with its HTTP method (**ANY** for a `@RequestMapping`
without one), its full path (the class's `@RequestMapping` path, then the method's) and its handler.
Double-click a row, or select it and press **⏎**, to open the handler. The list follows changes to
the sources by itself; **Refresh** reads them at once. Paths built from constants are not resolved.

## Spring Boot versions past their support

When `pom.xml` (the `spring-boot-starter-parent` parent, the `spring-boot-dependencies` BOM or a
`spring-boot.version` property), `build.gradle`, `build.gradle.kts` (the `org.springframework.boot`
plug-in or BOM) or `gradle/libs.versions.toml` (a `spring-boot` version) names a Spring Boot version
whose open-source support has ended, the version is underlined with a warning that says when, and
until when commercial support lasts if it still does. The dates come from spring.io
(`https://api.spring.io/projects/spring-boot/generations`), asked at most once a day and kept in the
IDE's cache; offline, the last answer is used, and without one nothing is marked. The IDE does not
contact spring.io until you ask it to, so this is off by default: turn it on in
**Settings → Build, Execution, Deployment → Build Tools**, **Mark Spring Boot versions past their
open-source support**, which names the host it contacts.

## For AI agents

- `springBoot.properties.list` lists the properties the project's metadata declares (`query`,
  `limit`, `waitMs` to wait for the metadata to be read).
- `springBoot.applications.list` lists the project's `@SpringBootApplication` classes, each with the
  configuration that runs it, for `run.configuration.create` or `run.process.start`.
- `editor.lineMarkers.list` lists a file's bean and injection marks with their targets, and
  `editor.lineMarkers.navigate` goes to one (`index` when there are several).
- `springBoot.endpoints.list` lists the request mappings (`waitMs` to wait for the sources),
  `springBoot.endpoints.open` opens a handler, `springBoot.endpoints.refresh` reads the sources
  again, and `view.toolWindow.endpoints` shows the tool window.
- `springBoot.starters.get` gives a module's Spring Boot version, the starters on offer and the ones
  it has (`module` or `file`, `waitMs` to wait for the service, `retry` after a failure);
  `springBoot.starters.apply` adds and removes them by id (`add`, `remove`). `view.startersDialog.open`,
  `.set` (`ticked`), `.apply` and `.close` drive the dialog, and `get_window_state` lists it as `starters`.
- Completion, documentation and problems are the editor's own commands: `editor.completion.start`,
  `editor.documentation.show`, `problems.file.list`.

## New Spring Boot project

**File → New → Project…** lists **Spring Boot** under **Generators**. The plug-in asks the Spring
Initializr service what it offers, so the choices are always current and none is built into the IDE.
The first page has **Server URL** at the top (its gear opens the setting below), then **Name**,
**Location**, **Create Git repository**, **Language**, **Type**, **Group**, **Artifact**, **Package
name**, **JDK**, **Java**, **Packaging**, **Configuration** and any other field the service has. Each
starts with the service's default; **Artifact** follows the project's **Name**, and **Package name**
follows Group and Artifact until you change them.

**Next** leads to the second page: the **Spring Boot** version and **Dependencies**, the starters in a
tree of groups with a search over names and descriptions. The pane beside it describes the chosen
starter, with a link to its guide, and **Added dependencies** lists the ticked ones. A starter that
does not work with the chosen Spring Boot version is disabled and says which versions it needs; one
you ticked earlier leaves the project until you choose a version it works with.

**Create** asks the service for the project, unpacks it into the new folder, opens the project and
imports it through Maven or Gradle.

**Your own service.** Settings → System Settings → **Project Generators** → **Spring Boot: service
URL** points the generator at your company's instance; it is `https://start.spring.io` until you set one.
The address must be `https`, and only you can change it: agents are refused. The IDE takes the project
only from the host the wizard shows, so a service that sends it elsewhere gets an error, naming the address.

**The network.** Selecting Spring Boot in the wizard first shows the address it would ask, with
**Connect**; it asks nothing until you press that. The list of choices is kept in the IDE's data folder: it is reused for as
long as the service allows (two hours on the public service) and checked with the service after that.
Without the network the last saved list is shown, with a line that says so, and **Next** says that
generating needs the network. **Let project generators use the network** in the same settings group
turns all of this off.

## Edit the starters of a project

**Edit Starters…** changes the starters of an open Spring Boot project in the same list as for a new
one. It is in the context menu of a `pom.xml`, `build.gradle` or `build.gradle.kts`, and on the toolbar
of the **Endpoints** tool window. The IDE reads the module's Spring Boot version from the build (the
`spring-boot-starter-parent` parent, the BOM, the `org.springframework.boot` plug-in, or a version in
`gradle/libs.versions.toml`), asks the service what that version offers, and ticks the starters the
module already has. A starter that does not work with this version is disabled and says which
versions it needs. The service knows only the Spring Boot versions it supports: for an older one the
dialog shows its answer and nothing can be edited.

**Apply** edits the build file in place, keeping its formatting, and the project then loads again:

- what you ticked is added with the coordinates and scope the service gives for your version, and
  with the BOMs and repositories it needs: in a `pom.xml` under `dependencyManagement` and
  `repositories`, in a Gradle script through `mavenBom` when the script applies
  `io.spring.dependency-management` and through `platform(...)` when it does not;
- what you unticked is removed. Its BOM and repository stay, because something else may use them.

The whole edit goes in or nothing does: if any part of it fails, the build file stays as it was.

**The network.** The dialog asks the same service as the New Project wizard, the one set in Settings →
System Settings → **Project Generators**. It first shows that address with **Connect** and asks
nothing until you press it. Both answers are saved in the IDE's
data folder for as long as the service allows. If the service cannot be reached, the saved answers
are used, with a line that says so; without them the dialog says that it needs the network and
changes nothing. **Let project generators use the network** turns the requests off here too.
