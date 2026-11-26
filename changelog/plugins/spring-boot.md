# Spring Boot plug-in changelog

Every release of the Spring Boot plug-in (`plugins/spring-boot`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-11-04

### Added

- The first release, for Spring Boot projects; JvmJizo comes with it, and every other IDE offers it
  in a notification when a project has `src/main/resources/application.properties`,
  `application.yml` or `application.yaml`. It is also in the Marketplace.
- The **Spring Boot** project generator in New Project, from the Spring Initializr service (ADR-0240):
  a page of fields with **Server URL** and **JDK**, then the Spring Boot version and the dependency
  tree (ADR-0243).
- `application.properties`, `application-<profile>.properties`, `application.yml`,
  `application-<profile>.yml` and the `.yaml` forms highlighted, folded and listed in the Structure
  view, each read as properties or YAML by its text.
- Their properties completed, documented and checked from the configuration metadata in the
  project's jars (needs Zen IDE 26.11 or later).
- Spring Boot run configurations for `@SpringBootApplication` classes, with active profiles and
  overridden properties (needs Zen IDE 26.11 or later).
- Beans and injection points marked in the gutter, the Endpoints tool window, and Spring Boot
  versions past their open-source support marked in build files once turned on in Build Tools (needs
  Zen IDE 26.11 or later).
- Edit Starters: add and remove the starters of an open project (needs Zen IDE 26.11 or later).
