# ADR-0240: Project generators backed by a web service

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0217](0217-spring-boot-plugin-metadata-from-the-projects-jars.md),
  [ADR-0237](0237-new-project-wizard-and-project-generators.md)

## Context

ADR-0237 lets a plug-in fetch text and left archives for the first generator that needs them. The
Spring Boot generator is that generator: its form comes from the Spring Initializr service, a zip is
its result, a company may run its own instance, and golden rule 10 asks that network use is explicit,
visible and can be turned off. ADR-0237 relied on uninstalling a plug-in for that last part, which
also takes away what the plug-in does offline.

## Decision

1. **The IDE does the network, over https only, redirects too; the plug-in stays without imports.** One more answer of `zen_project`,
   `{"archive": {"url"}}`, has the IDE download a zip and take it as the generator's files, in memory.
   Binary data never crosses into the plug-in.
2. **Unpacking is strict.** The `zip` reader moves from `zen-workbench` to `zen-vfs`, so `zen-plugins`
   can use it. An entry with `..` or an absolute path refuses the whole archive, a link too, and nothing is written into a `.git` folder. Limits:
   16 MiB downloaded and per entry, 4096 entries, 64 MiB unpacked. Unix modes keep `mvnw` and `gradlew`
   executable, empty folders are kept, and one folder holding everything is dropped, so an archive
   with or without a root folder gives the same project.
3. **Saved answers.** A fetch may ask for `"cache": true`: the IDE keeps a `200` answer in
   `generator-cache/<plug-in>/` of the data folder, by address and headers, for its `max-age`, then
   revalidates it with `If-None-Match`. If the service cannot be reached or answers 5xx, the last
   saved answer is used and the form carries a `notice`. A 4xx is an answer about the request and is
   passed on. Only metadata is cached; a generated archive is not.
4. **Preview and Create use the same bytes.** The archive of a preview stays in memory, one at a time
   and by address, until Create takes it, so what was shown is what is written.
5. **A switch, not only the plug-in.** System Settings → **Project Generators** has **Let project
   generators use the network** (`generatorsNetwork`, on by default; the session's
   `project.generators.network`). Off, the IDE asks nothing for any generator: forms come from saved
   answers, and generating says that the network is off. The check is in the host, so a plug-in cannot
   get round it. On by default, the owner's decision, with the address shown first: a type whose form
   asks a service declares `service`, the setting holding its address, and the wizard shows that address
   with **Connect** and asks nothing until the person presses it, once per address per wizard. Off, no
   generator asks anything and a generator's build tool, which needs the network, does not start. An
   agent's explicit `project.new.*` or `springBoot.starters.get` still uses the network, the owner's
   decision: it is explicit, and what it makes is never trusted. It covers generators only. An agent can turn it off, but not on, and cannot change a generator's
   settings: a project the person creates from that service is trusted, so a service an agent chose
   would give it a trusted project.
6. **Generator settings are declared.** A generator lists text `settings` in `plugin.json`; the same
   group shows them, the values are kept in `generatorSettings` by `<plug-in>.<generator>.<id>`, and
   the module gets them in its input. A change of either drops the loaded forms and previews.
7. **`User-Agent` is the host's:** `zen-ide/<version>`, whatever a plug-in sends.
8. **Spring Boot's form is built from the v2.3 metadata, field by field:** `text`, `single-select`,
   `action` and `hierarchical-multi-select` by their type, so a new field of the service shows up
   with its id as the label. Only labels (English and Russian) and the order of the known fields are
   the plug-in's. The types that give a single build file are left out by their `format` tag: a
   project is an archive. The service's `name` is the wizard's **Name**; the sample `artifactId` and
   `packageName` become `${name}` and `${groupId}.${artifactId}`, so `${id}` joined `${name}` in text
   defaults. The address comes from the type's `_links` template (RFC 6570), only on the host the form shows, or
   the generator refuses; and a starter's range
   becomes `enabledWhen` over the service's own Boot versions.
9. **Tests use the service's official stub**, `metadataWithV23AcceptHeader.json` of
   `io.spring.initializr:initializr-web:0.24.0:stubs` (Apache-2.0), kept unchanged in
   `plugins/spring-boot/testdata/`. Its zip answers are not usable: the binary body did not survive the
   stub's text encoding, so the archive tests build their zips.

## Consequences

- A plug-in crate may use `serde_json`: plug-ins still depend on no workspace crate, and this one
  grows by that library. Reading a service's metadata by hand would be worse.
- A generator can show a form offline and still fail at Next, which says why.
- A project with one top folder and nothing beside it loses that folder when it comes from an archive.

## Alternatives considered

- **Bytes in `fetched`, as base64, and unpacking in the plug-in:** a zip library in every such plug-in,
  and path checks the IDE could not enforce.
- **The service URL as a form field:** the form itself comes from that address.
- **Using the plug-in's own on/off as the switch:** it would also take away highlighting and running.
- **Comparing Boot versions in the wizard:** forms are data (ADR-0237); listing the versions that fit
  keeps the wizard free of any one service's rules.
