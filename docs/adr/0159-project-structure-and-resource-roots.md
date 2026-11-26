# ADR-0159: Settings → Project Structure, resource roots and Exclude files write the module file

- **Status:** Proposed
- **Date:** 2026-10-25
- **Relates to:** [ADR-0123](0123-mark-directory-as-excluded.md) (Mark Directory as → Excluded
  writes the module file; extended here), [ADR-0051](0051-jetbrains-keymaps-and-idea-project-settings.md)
  (`.idea` settings),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents)

## Context

ADR-0123 made Mark Directory as → Excluded the one writer of an `.idea` module file
(`<excludeFolder>`) and left Settings → Project Structure and Resource Root to later work. The
requirement: a folder can also be marked as a **Resource
Root** (Mark Directory As → Excluded / Resource Root; no Sources or Test Sources Root), and
**Settings → Project Structure** shows the
content root as a tree of folders with a toolbar "Mark as: Excluded, Resource Root" (pressing the
pressed button again takes the mark off), the **Exclude files** field (name patterns separated by
`;`, with `*` and `?`) under the tree, and on the right "+ Add Content Root", the content root's
path with × and the marked folders, each with ×. The module file keeps
resource roots as `<sourceFolder url="…" type="java-resource" />` and the patterns as
`<excludePattern pattern="…" />` inside `<content>`; the format also has Sources, Test Sources and
Test Resources roots, which GoGarden does not offer. `<sourceFolder>` was not read.

## Decision

1. **`<sourceFolder>` is read.** `ProjectSettings` gains `contentRoots` (every `<content url>`) and
   `sourceFolders` (`{ path, kind }`, kind `sources`, `testSources`, `resources` or
   `testResources`, from `type` and `isTestSource`). Resource roots of both kinds are drawn with a
   purple folder in the Project tool window and on the Project Structure page; source and test roots
   are read for agents and not used otherwise.
2. **Two more writers, on request only.** `SettingsProvider` gets `set_resource_root(root, folder,
   marked)` and `set_exclude_patterns(root, patterns)`, both `Option<Result<ExclusionChange,
   String>>` with a `None` default, as `set_excluded`. `IdeaProject` implements them with the
   method of ADR-0123: one line inserted or removed with its neighbours' indentation and line
   ending, the rest byte for byte, and `.idea/modules.xml` with a module created when the project
   has none. Inside `<content>` the lines keep the order of the `.idea` format: `sourceFolder`,
   then `excludeFolder`, then `excludePattern`.
3. **A folder has one mark.** Marking a folder excluded removes its resource `sourceFolder`;
   marking it as a resource root removes its `excludeFolder` and any other `sourceFolder` of it.
   Unmarking a resource root removes `java-resource` and `java-test-resource` entries alike.
4. **Exclude files are names.** A pattern holding `/` or `\` is refused. Setting the patterns keeps
   every existing `excludePattern` line whose patterns are all still wanted, each once, removes the
   others and adds the missing patterns one per line into the `<content>` of the project's root
   (else the first one). Matching is unchanged (ADR-0123): a file or folder whose name matches is
   excluded, a folder with its content.
5. **Commands (ADR-0033).** `project.directory.markResourceRoot` / `unmarkResourceRoot { path }`,
   `project.structure.setExcludePatterns { patterns }` and `project.structure.get {}`; the folder
   commands answer `excluded`, `resourceRoots` and `excludePatterns` after the change. Every one
   reads the settings again and rebuilds the index in the same call.
6. **The page.** Settings → Project Structure is the first top-level page, before Appearance &
   Behavior (`view.settings.open { page: "projectStructure" }`). Its layout: the toolbar
   toggles, the tree with Exclude files under it, and on the right + Add Content Root and the
   content root's × (both disabled: one content root per project), the root's path abbreviated in
   the middle, then Excluded Folders and Resource Folders with × on each row. The page keeps a
   draft and writes nothing until Apply or OK, which run the commands above for what changed. Its
   tree lists folders only, read when a folder is opened.
7. **⌘; and File → Project Structure…** open the page (`view.projectStructure.open`; an imported
   keymap's `ShowProjectStructureSettings`). Whether GoGarden keeps them is the owner's decision.

## Consequences

- A project shared with a JetBrains IDE sees the same resource roots and Exclude files patterns in
  both IDEs; a project without `.idea` gets one the first time a mark or a pattern is set.
- Resource roots do not yet change completion of paths; they are marks that agents and
  other readers of the module file read.
- Sources Root, Test Sources Root, several content roots per module (Add Content Root) and
  removing the content root are not offered; a module's further content roots are shown as further trees.
