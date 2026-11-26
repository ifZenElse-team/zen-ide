# ADR-0237: The New Project wizard and project generators

- **Status:** Proposed
- **Date:** 2026-11-05
- **Relates to:** [ADR-0007](0007-everything-is-a-command.md), [ADR-0033](0033-ai-access-to-every-api.md),
  [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0205](0205-trusted-projects.md)

## Context

Zen IDEs could only open folders. The requirement: projects are created in a two-pane wizard whose
types come from language support and plug-ins. The types that follow are not all simple: a service-backed
generator has a form that comes from the network, depends on its own fields and has hundreds of
options, shows the generated files first and writes them only then.

## Decision

1. **A project type is a `zen_project::generators::ProjectGenerator`:** its `info` (id, name, icon,
   section, whether it previews and needs the network), its `form`, the `files` it would write, and
   `generate`, which by default writes those files and may run a program instead. Every method may
   block: the IDE calls them off the UI thread.
2. **Forms are data** (`GeneratorForm`): text, path, choice, check and a multi-select with groups and
   search. A field shows, and an option is enabled, while a `Condition` on another field holds, so
   the wizard needs no code of the generator to follow them. Options a condition disables say why.
3. **Who gives them:** the platform gives Empty Project; a language module calls
   `ModuleHost::project_generator`; a plug-in lists `projectGenerators` in its `plugin.json` and
   exports `zen_project`. The wizard lists the modules' types, Empty Project, then the plug-ins' under
   **Generators**, in plug-in order. A product adds nothing of its own.
4. **Plug-ins stay without imports.** A module that needs the network answers `{"fetch": [...]}`; the
   IDE fetches the addresses and calls it again with the answers. Only a generator that declares
   `network` may do so, the wizard says that it uses the network, and it happens only when the person
   selects that generator: plug-ins are installed by the person, and uninstalling or disabling one
   turns its network off. Downloading an archive and unpacking it for the plug-in comes with the
   first generator that needs it, as one more answer of the export.
5. **Everything runs in the background behind commands** (`project.new.*`), which the wizard calls as
   agents do: each frame it reads how a job is doing. Forms, previews and creations are jobs that
   answer `running`, `ready` or `failed`.
6. **A failure leaves nothing.** A generator writes into a hidden folder beside the location, Git
   initialises there, and only then the folder takes the location's place. In a folder the person
   agreed to reuse, the new files are moved in one by one and existing files are kept (`kept`).
7. **Trust.** A project the person creates in a new or empty folder is trusted before it opens, so
   its build is imported at once. A folder that held something is not trusted by creating in it: what
   it held is not the generator's, so opening it asks. An agent's is not, also when it drove the wizard with `run_action` or `press_keys`
   (ADR-0205: agents never trust).
8. **Where it opens.** System Settings → **Open project in** has New window, Current window and Ask,
   and replaces the Appearance setting that had the first two: a saved choice keeps its meaning.
   Current window is a new tab of the main window, as projects share it here; closing the current
   project to make room would throw away what the tabs give.

## Consequences

- A new project type is a data form and a function; Create, Git, previews, errors and trust come with
  the wizard.
- A generator cannot lay out its form freely: what the data cannot say needs a new field kind here.
- `go mod init` runs in the hidden folder, so generators must not record their own folder's path.

## Alternatives considered

- **`zen_modules::form::Form`, the settings pages' form:** each of its controls runs a command as it
  changes; a wizard's fields are one set of values kept until Create.
- **A plug-in generator that is a program run by the IDE:** plug-ins run in the WebAssembly sandbox;
  a generator should not need more to write some files.
- **Writing straight into the location:** a failure in the middle would leave a half-made project.
