# ADR-0252: Editing the starters of an existing project

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0240](0240-generators-with-a-service.md) (generators backed by a web service),
  [ADR-0234](0234-dependency-manager-network-repositories-and-providers.md) (dependency providers),
  [ADR-0050](0050-products-are-plugin-bundles.md), AGENTS.md golden rule 10

## Context

The Spring Boot generator knows the service; the dependency providers know the build files. Editing
the starters of an open project needs both, and a third thing neither has: the coordinates, BOMs and
repositories the service names for one Spring Boot version, from `/dependencies?bootVersion=`, which
the generated archive used to carry. A generator had only the steps `form` and `files`.

## Decision

1. **A generator can answer named steps.** `ProjectGenerator::step` runs one more `step` of the
   plug-in's `zen_project` export with free-form values, through the same rounds of `fetch` as a form.
   So the plug-in asks, and the host keeps doing the network: the cache, `If-None-Match`, the saved
   answer, the `User-Agent` and the `generatorsNetwork` switch of ADR-0240 apply without new code.
2. **The plug-in stays the only one who reads the service.** Its step `starters` takes the Boot version
   and the declared `group:artifact` names and gives the select field, the ticked ids and the
   service's `dependencies`, `boms` and `repositories`. A starter outside the project's version is
   disabled by `enabledWhen` over that one version.
3. **The host applies the edit** in `zen_workbench::spring_boot::starters`, as the plug-in's other
   native parts do, because WebAssembly providers wait for ADR-0056: `springBoot.starters.get` and
   `springBoot.starters.apply`. The Boot version is read as the support-end check reads it.
4. **`DependencyChange` grows `AddBom` and `AddRepository`, and edits are planned as one.** A list of
   changes is computed against each other's result in memory, and written only when all of them
   succeeded. Adding a BOM or a repository already there changes nothing.
5. **Scopes are the service's words, mapped by the provider.** Gradle takes `compile`, `runtime`,
   `provided` and `test` as `implementation`, `runtimeOnly`, `compileOnly` and `testImplementation`.
   Maven takes `annotationProcessor` as `<optional>true</optional>`, as the service's own `pom.xml`
   does.
6. **BOMs in Gradle follow the script:** `dependencyManagement { imports { mavenBom ... } }` when it
   applies `io.spring.dependency-management`, `implementation platform(...)` when it does not.
7. **Removing leaves BOMs and repositories.** The IDE cannot tell what else uses them, and a spare
   BOM does no harm.

The coordinates and repository addresses the service gives are checked before they go into a build
file: plain coordinates and an `https` address only, since a build file is code.

The dialog follows ADR-0240: it shows the service's address with **Connect** and asks only after that,
once per address per dialog; a dialog an agent drives connects without the button.

## Consequences

- The official stubs have no successful `/dependencies` answer: the metadata and the refusal of a
  version are the official ones, and the sample of the answer is written after a live one.
- A BOM's version is written literally, not through a property as the service's generated files do.
- The service refuses the versions it no longer supports (on 2026-11-05 the public one answered 400 to
  3.5.0, compatibility range `>=4.0.0`), so an older project gets its message and no list. The IDE
  has nothing better to offer: coordinates are the service's per version.
- With saved answers the dialog works offline; the project's reload is then what may fail.

## Alternatives considered

- **Generating a project and diffing its build file against the open one:** it would need an archive
  download per edit, and it would touch everything the person changed by hand.
- **Native HTTP in the workbench:** it would bypass the cache and the switch of ADR-0240.
