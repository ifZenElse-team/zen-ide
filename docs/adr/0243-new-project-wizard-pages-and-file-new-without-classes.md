# ADR-0243: Wizard pages instead of a file preview, and File → New without classes

- **Status:** Proposed
- **Date:** 2026-11-05
- **Supersedes:** the file preview of [ADR-0237](0237-new-project-wizard-and-project-generators.md) and
  [ADR-0240](0240-generators-with-a-service.md), "Edit File Templates…" and the folder-dependent
  File → New of [ADR-0155](0155-new-menu-and-file-templates-page.md)
- **Relates to:** [ADR-0242](0242-java-and-kotlin-project-types.md)

## Context

The behaviour to build has no page of generated files in New Project: a service
generator has a second page of fields with a dependency tree. File → New is the same on every folder
and makes no classes; the classes are in the ⌘N popup of the Project tool window, whose list depends
on the folder and has a search field. The owner confirmed that File → New never offers a class.

## Decision

1. **A form has pages.** `Field.page` (0 beside the list of types) makes **Next** and **Previous**;
   a later page is the whole window. The manifest's `preview` and `GeneratorInfo.preview` are gone.
   `project.new.preview` stays for agents: it costs nothing and shows what Create would write.
2. **More form data:** `Field.segmented` (a choice as a row of buttons), `Field.tooltip` (a question
   mark), `GeneratorForm.server` (the service, above Name) and `GeneratorInfo.description` (a line
   above Name). The gear and More via plugins… open Settings over the wizard, which keeps its values
   and loads its form again on return.
3. **Location is the parent folder** in the field; `project.new.*` and the window state keep the
   whole path, and a hint line says where the project goes.
4. **Where a template is offered** stays in the manifest beside `markers`, so that a template the
   person changed keeps it: `sourceFolder` (only in a source folder, never in File → New) and `group`
   (one row for the group, the name dialog chooses the kind).
5. **A source folder** is a folder marked in the project settings, `src/<set>/java` or
   `src/<set>/kotlin` and what is below them, or `src` of a project without `src/main` once it holds
   a `.java` or `.kt` file.  In one, New lists the groups, File, Package, the single source templates
   and the person's own; elsewhere File, Scratch File, Directory, the other templates and
   EditorConfig File.
6. **File → New is `file.new.list` with `menu`,** computed for the project folder, never the selection.
7. **Edit File Templates… leaves the New lists.** The page is in Settings.
8. **Compact Kotlin** keeps sources directly in `src` in every build tool, as public documentation
   shows; `sourceSets` or `sourceDirectory` point the build there.

## Consequences

- Plug-ins that declared `preview` lose nothing else; an unknown key is ignored.
- An IDE-build project with no sample and no Java or Kotlin file has no source folder yet: its first
  class is made with File, or by Add sample code.
- The templates `Java package-info` and `Java module-info` are now named `package-info.java` and
  `module-info.java`; a changed copy under the old name becomes a template of the person's own.

## Alternatives considered

- **Keep the preview as an option:** an extra step on every Spring project.
- **A `sourceFolder` member of `FileTemplate`:** it would be lost in templates saved before it.
