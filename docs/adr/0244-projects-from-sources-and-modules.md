# ADR-0244: Clone, existing sources, modules and generators that run a build tool

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0050](0050-products-are-plugin-bundles.md),
  [ADR-0205](0205-trusted-projects.md), [ADR-0237](0237-new-project-wizard-and-project-generators.md),
  [ADR-0242](0242-java-and-kotlin-project-types.md)

## Context

File → New is to have Project from Existing Sources, Project from Version Control, Module
and Module from Existing Sources, and the wizard a Maven Archetype type. Each has a difficulty here.
A clone brings a stranger's code. A project is a folder, not a model the IDE keeps, so an import
wizard has little to ask. A module changes a build file whose format a plug-in knows, not the
platform. A plug-in is WebAssembly and cannot start Maven.

## Decision

1. **Clone (`project.fromVcs.*`, Clone Repository).** One page, Repository URL: rule 10 has no
   accounts, so there are no pages for hosting services. A clone goes, without its submodules, into a
   hidden folder beside the directory, which takes its place only when it worked, so a failure or
   Cancel leaves nothing. The network is used only on the person's Clone or an agent's explicit
   command. `ext::` and other remote-helper addresses are refused and the `ext` transport is off in
   the clone, as an address must not run a program. A clone is never trusted by itself (ADR-0205).
   Submodules wait for trust: `.gitmodules` is the repository's own text, so fetching them reaches the
   hosts a stranger names, recursive clones have run code through crafted submodules before, and the
   person has not seen the project yet. **Initialise Submodule** in
   the Git tool window fetches them after trust, and the person's `submodule.recurse` does not start
   them in the clone.
   The addresses are kept in the IDE's data folder without passwords.
2. **Project from Existing Sources (`project.fromSources.detect`).** The chooser takes a folder or a
   file; the folder, or the file's, is the project. The command says whether it is Maven, Gradle or
   plain sources, and the IDE opens the folder, which loads what is there. The steps of an import
   wizard (import from an external model or create from sources, name and location, source roots,
   libraries, module structure, SDK, frameworks) do not apply: a folder needs none of them, and a
   build's model comes from the build. Nothing is written and nothing is trusted.
3. **New Module is the wizard** in a module mode: its own heading, no Empty Project, no Create Git
   repository, Location at the project's root with the module in `<Location>/<Name>`, and the JDK
   field starting on the project's JDK. `project.module.new` gives a generator `module: true` in
   `values`, and `build` starts on the project's build system.
4. **A build lists its modules in its plug-in.** `project.build.moduleFiles` names the files, and
   the module's `zen_project` export answers the step `addModule`, a pure edit of their text. The
   edit is worked out before the module is published and written after it, so a refusal leaves
   nothing. Maven takes `<modules>` of the project, not of a profile, and makes the parent's
   packaging `pom`, without which Maven refuses it. Gradle adds `include`, and `settings.gradle(.kts)`
   when the build has none. The IDE runs the plug-in's reload command; agents go through the same
   trust check. A build file with unsaved changes refuses the command (`buildFileUnsaved`).
5. **Form additions for the Maven Archetype type.** A field can be a `note` with a link to another
   type or a `table`; `manage` adds a link to a list the IDE keeps per type (Manage catalogs…, Add…),
   given to the form as `lists`; `formDependsOn` names the fields whose values reload a form.
6. **A generator may run a declared build tool.** A plug-in cannot start a program, and it should not
   be able to start any. A declaration names `tool`; the module's `generate` step answers
   `{"run": {"tool", "args", "javaHome", "subfolder"}}`; the IDE refuses another tool, runs this one
   in the hidden folder with the Maven it finds (`PATH` or downloaded) and streams its output into the job, and the job's
   `output` shows it. `subfolder` moves up afterwards, as `archetype:generate` makes one. This is the
   smallest addition that keeps the arguments in the plug-in: the host knows only how to find and
   start Maven.
7. **Only a person runs a generator's tool.** An archetype's post-generation script runs code from
   whatever coordinates were given, so `project.new.create` and `project.module.new` refuse an agent
   with `notTrusted` for a generator with a `tool`, as ADR-0205 does for builds. They refuse a person
   too while the window shows a project not trusted, since Maven reads `.mvn` of the folders above a
   module. A person's Create elsewhere is the person's action, and such a project is trusted as any
   they create. Maven also reads `.mvn` above a project created inside a stranger's folder, which is
   not detected: the location is the person's choice. A list such as Manage catalogs… is the person's
   only too (`project.newLists.set`), as a catalog names where that code comes from.
8. **Command ids have three parts**, so the commands are `project.fromVcs.clone` and
   `project.fromSources.detect`; the actions are `view.project.fromVcs` and
   `view.project.fromSources`.

## Consequences

- A build system adds module support with a manifest line and one pure function.
- A Maven module has no `<parent>`: the wizard has no Parent field yet.
- A catalog is read in one fetch of at most 16 MiB and is offered up to 1000 archetypes; the Maven
  Central catalog, 18 MiB, is too big for now, so the Internal catalog comes built in and others are
  added with Manage catalogs….

## Alternatives considered

- **Module editing in the platform:** it would put `pom.xml` and Gradle syntax below the plug-ins
  (ADR-0050).
- **A plug-in asking for any program:** sandboxed plug-ins would then be as powerful as the host.
- **An archetype written from the plug-in's own copy of its files:** archetypes are many and external;
  Maven is their definition.
- **Trusting a clone the person asked for:** cloning is not reading; ADR-0205 asks for a decision
  after the person can see the project.
