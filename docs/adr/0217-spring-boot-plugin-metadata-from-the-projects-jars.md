# ADR-0217: Spring Boot: a plug-in for its files, the properties from the project's jars, a run kind

- **Status:** Proposed
- **Date:** 2026-11-04
- **Extends:** [ADR-0214](0214-properties-plugin-and-resource-bundles.md) (its decision 4, how a
  framework plug-in completes `application.properties`), [ADR-0197](0197-ant-plugin-and-its-tool-window.md)
  (a plug-in for the files, the platform for what a module cannot do)
- **Relates to:** [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0129](0129-project-plugins-npm-run-and-nuxt.md),
  [ADR-0198](0198-library-classes-and-documents-of-language-servers.md) (the library model)

## Context

JvmJizo should edit and run Spring Boot projects with the framework's knowledge: complete and check
`application.properties` and `application.yml`, show what a property does, and run a
`@SpringBootApplication` class with profiles and property overrides. ADR-0214 expected a framework
plug-in to complete the files from a list of its own keys. The owner decided on 2026-11-04 that the
data come from the official sources so they stay current: the properties a project can set are
those its own jars declare in `META-INF/spring-configuration-metadata.json` and
`META-INF/additional-spring-configuration-metadata.json` (Spring Boot's documented configuration
metadata format), which match the project's Spring Boot version and need no network. A WebAssembly
plug-in sees only the text it is given: it cannot open the project's jars, start a process or add a
run kind.

## Decision

1. **A plug-in, `spring-boot`**, takes `application.properties`, `application-*.properties`,
   `application.yml`, `application-*.yml` and the `.yaml` forms by name, includes the Properties
   plug-in's reader and the YAML highlighter, and tells the two formats apart by the text (a
   mapping key ending a line, an indented key, `---` or a list item is YAML; a `key=value` line is
   properties). It highlights, folds, outlines and, for properties, reports what the Properties
   plug-in reports. Its project markers are those three files under `src/main/resources`, so every
   other IDE offers it for such projects; JvmJizo bundles it.
2. **The platform reads the metadata** (`zen_workbench::spring_boot`), while the plug-in is on: the
   project's jars come from the JVM library model (`JvmLibraries::jars`, the dependencies Maven or
   Gradle resolved), plus the project's own processor output (`target/classes`,
   `build/classes/java/main`, `build/tmp/kapt3/classes/main`) and
   `src/main/resources/META-INF/additional-spring-configuration-metadata.json`. Reading happens on a
   background thread when the project model's revision or those files change; a jar is read once
   while its size and modification time stay. Properties are merged, the first declaration winning
   and later ones filling what it lacks; a hint's `values` join its property (`name.values` for a
   map's values).
3. **Names are matched as Spring Boot binds them**: lower case, without `-` and `_`, list indexes
   dropped and map keys in brackets kept, so `contextPath`, `context-path` and `CONTEXT_PATH` are
   one property; a key under a property whose type is generic or an array (`Map<…>`, `List<…>`)
   belongs to it.
4. **What the metadata gives**, through the editor's own commands: completion of keys (left out
   when the file sets them; in YAML relative to the keys above the caret, inserted nested) and of
   values (the hints' values, `true`/`false` for Boolean ones); Quick Documentation on a key; and
   problems: an unknown key is a weak warning, a deprecated one a warning struck through, or an
   error when its level is `error`. Nothing is reported while no metadata is read, and keys inside
   YAML lists are not checked. The plug-in exports no `zen_complete`, so ADR-0214's decision 4 is
   replaced for Spring Boot: no list of keys is kept in the plug-in.
5. **A run kind `springBoot`** (`SpringBootRun`): the options of a Java application plus
   `activeProfiles` and `overrides`, run as a Java application (`java_run`) whose program arguments
   get `--spring.profiles.active=…` and `--name=value` after the configuration's own. A Java or
   Kotlin file with `@SpringBootApplication` gives it as its gutter target and context; the class's
   `main` (Kotlin's top-level one included) runs.
6. **Two generic run-provider hooks** in `zen_run::RunProvider`: `plugin()`, the plug-in whose
   projects the provider serves (its targets and contexts are offered only while that plug-in is
   on), and `replaces()`, the provider whose targets on the same line its own take the place of
   (Spring Boot's replace Java's on an application class). Kinds stay listed in every IDE, as
   Maven's and Ant's are.
7. **Commands for agents**: `springBoot.properties.list` (the metadata, with `query`, `limit`,
   `waitMs`) and `springBoot.applications.list` (the application classes with their
   configurations).

8. **Beans and injection points from a simple index of the sources**, not from the language
   server (`zen_workbench::spring_boot::source` and `index`): the platform reads the project's
   `.java` and `.kt` files (build output folders left out) with a small tokenizer of its own, in the
   background, again a few seconds after a request when files changed (by size and modification
   time), and open files from their text. A bean is a class with a stereotype annotation
   (`@Component`, `@Service`, `@Repository`, `@Controller`, `@RestController`, `@Configuration`,
   `@SpringBootApplication`) or a `@Bean` method; an injection point is a bean's constructor
   parameter (Kotlin's primary constructor included), an `@Autowired`/`@Inject` field, property or
   setter, or a `@Bean` method's parameter. They match by simple type name, the bean's own or one
   of the supertypes its declaration names. The language server was not used: Eclipse JDT LS and
   kotlin-lsp have no request for "classes with this annotation", and asking for type hierarchies
   per injection point would be slow and only work with a server running. Qualifiers, profiles,
   conditions, library beans and inherited supertypes are therefore not taken into account.
   `editor.lineMarkers.list` and `editor.lineMarkers.navigate` give the marks (the gutter's
   filled circle for a bean, ring for an injection point; one target opens at once, several in a
   list); they are the editor's commands so that other providers can add marks later.
9. **The Endpoints tool window** (`view.toolWindow.endpoints`, bottom, while the plug-in is on)
   lists the request mappings of the same index: `@GetMapping`, `@PostMapping`, `@PutMapping`,
   `@DeleteMapping`, `@PatchMapping` and `@RequestMapping` (its `method` values, or any) on
   methods, each path joined with the class's `@RequestMapping` paths; literal paths only.
   Commands: `springBoot.endpoints.list`, `open` and `refresh`.
10. **Boot versions past their open-source support** are marked by an inspector of build files
    (`pom.xml`, `build.gradle(.kts)`, `libs.versions.toml`), active only while the plug-in is on
    (`Inspector::plugin()`, as for run providers). The version (a parent, a BOM, a property, the
    Gradle plug-in or the catalog's `spring-boot`) is looked up by its `major.minor.x` generation in
    `https://api.spring.io/projects/spring-boot/generations`; past `ossSupportEndDate` it is a
    warning naming the date, and the commercial end while it lasts. Network rules of ADR-0195, and
    off by default (golden rule 10, the owner, 2026-11-04): declared on the Build Tools page as `springBoot.supportEnd.check` naming
    `api.spring.io`, asked at most once a day in the background, the answer kept in the IDE's cache
    with its validators (`If-None-Match`, `If-Modified-Since`; the API sends none today, so the
    daily limit does the work), the last answer used offline, nothing said without one, and a
    failed request not repeated for 15 minutes.

## Consequences

- Completion and checks follow the project's own Spring Boot version and work offline; a project
  whose build has not loaded yet gets nothing until its libraries are known.
- Properties declared without the configuration processor (no metadata) show as unknown;
  adding the processor or `additional-spring-configuration-metadata.json` fixes it.
- Hints with value providers (class references, logger names) and enum values are not offered yet;
  values are not checked against their types.
- Bean navigation and endpoints are text-based: they work without a language server and stay fast,
  but only know what the sources spell out; a project that relies on qualifiers, generics or library
  beans sees fewer or more targets than the container would wire.
- One more host, `api.spring.io`, is listed with its setting and contacted only once the person
  turns the check on.
