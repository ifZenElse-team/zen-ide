# ADR-0041: The recent-projects file is a shared format

- **Status:** Proposed
- **Date:** 2026-10-17

## Context

Zen Toolbox gets a Projects tab: the recent projects of every installed Zen IDE, each
opened in its own IDE. Every IDE already keeps its recent projects in a JSON file in its
application data folder (`zen_workbench::recent`), written by the IDE alone and read back only by
it. Nothing said the file could be read by another program, so its format and location could change
with any release.

The code for the list lived in `zen-workbench`, a feature crate in layer L5. `zen-toolbox` is in L5
too, and ADR-0014 lets a crate depend only on lower layers, so Toolbox could not use it. The options
were:

1. Move the list into a lower-layer crate both can use.
2. Let Toolbox parse the file with its own copy of the types.
3. Ask each IDE for its list at run time (through MCP or a command-line flag).

## Decision

1. **The recent-projects file is a format shared between programs.** It is documented in
   [docs/recent-projects.md](../recent-projects.md): the path on each system, the `ZEN_DATA_DIR`
   override, and the JSON schema with an example that a test parses.
2. **Its code lives in `zen-project` (L1)**, the crate the architecture plans for the workspace
   model, as the module `zen_project::recent`. It depends only on `zen-vfs`. `zen-workbench`
   re-exports it as `zen_workbench::recent`, so the IDEs keep their code. `zen_project::recent::read`
   reads another IDE's list by its application id.
3. **One writer, any number of readers.** Only the IDE that owns a file writes it, atomically.
   Readers such as Zen Toolbox only read it; to open a project they start the owning IDE with the
   project's folder, and the IDE records the opening itself. A missing or unreadable file is an empty
   list.
4. **Changes are additive.** Readers ignore unknown fields; a new field has a default that keeps the
   old meaning. Renaming or removing a field, changing its meaning, or moving the file needs a new ADR
   that supersedes this one, and a new file name.

## Consequences

- Toolbox lists the projects of IDEs that are not running, without any network or process access,
  and stays within the layering rules.
- `zen-project` exists earlier than planned and starts small; the rest of the workspace model
  (roots, modules, run configurations) joins it later.
- The file's format and location are now a compatibility promise: changing them carelessly breaks
  Toolbox for users who update one program before the other. The test of the documented example
  catches accidental changes.
- The IDE still rewrites the whole file and keeps at most 30 entries; Toolbox sees exactly what the
  IDE shows.
- Opening a project from Toolbox starts a new copy of the IDE even when one is running, and each copy
  rewrites the whole file from its own list, so the copy that writes last wins. The file stays valid;
  an IDE that hands the folder to its running copy instead removes this, and needs no format change.

## Alternatives considered

- **A copy of the types in Toolbox.** No layering problem, but two definitions of one format drift
  apart silently; one crate with a test against the documented example keeps them the same.
- **Asking the running IDE.** Works only while the IDE runs, needs automation turned on (ADR-0033 is
  off by default), and costs a process start or a connection for a list that is already on disk.
- **Moving `zen-toolbox` above `zen-workbench`, or letting L5 crates depend on each other.** Breaks
  ADR-0014 for one small module; features must stay independent of one another.
