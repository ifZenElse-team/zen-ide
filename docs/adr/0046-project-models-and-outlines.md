# ADR-0046: Project models and outlines from language modules

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

GoGarden needs to know what a Go project is made of: the modules of `go.mod`, the workspace of
`go.work`, the Go SDK and the downloaded dependencies, shown under External Libraries in the
Project tool window. It also needs the Structure tool window (⌘7): the types, functions and fields
of the active file as a tree, with the symbol at the caret selected.

Both are language knowledge, which lives in language modules (L5). The platform must not depend on
a language module (ADR-0014), so it needs extension points that a module implements and an IDE
lists, as it already does for highlighting (`zen_syntax::Highlighter`). Everything shown must also
be reachable by agents as a command (ADR-0007, ADR-0033).

The structure can come from two places (ADR-0003): the language server's `textDocument/documentSymbol`,
which is precise but needs the server, and the syntax layer, which is always available. Module files
such as `go.mod` have no language server at all.

## Decision

1. **`zen_project::model` (L1) holds the project model and its extension point.** `ProjectModel`
   has `modules` (`Module`: `name`, `root`, `file`, `languageVersion`, `toolchain`, `dependencies`,
   `excluded`, `details`), an optional `workspace` (`WorkspaceFile`: `file`, `languageVersion`,
   `toolchain`, `uses`, `replacements`, `details`), `libraries` (`Library`: `kind` `sdk` or
   `dependency`, `name`, `version`, `root`, `exists`) and `problems` (`ModelProblem`: `file`, `line`,
   `message`). A `Dependency` has `name`, `version`, `indirect` and an optional `replacement`
   (`name`, `version`, `localPath`). `details` is free JSON for what one language knows beyond the
   common shape. A language module implements `ModelProvider`:

   ```rust
   pub trait ModelProvider: Send + Sync {
       fn id(&self) -> &str;
       fn watches(&self, file_name: &str) -> bool;
       fn load(&self, root: &Path, env: &ModelEnv) -> Option<ProjectModel>;
   }
   ```

   `ModelEnv` says where the toolchain keeps things (`sdk_root`, `sdk_version`, `package_cache`,
   e.g. `GOMODCACHE`); the window fills it from `zen-toolchain`, so a provider does not run tools.
2. **`zen_syntax::Outliner` (L1) is the syntax layer's structure.** Next to `Highlighter`, it has
   `handles(file_name)` and `outline(text) -> Vec<OutlineItem>`; an `OutlineItem` has `name`,
   `detail`, `kind`, `start`, `end`, `selectionStart` (character offsets) and `children`. `kind` is a
   `SymbolKind`: `struct`, `interface`, `type`, `function`, `method`, `field`, `constant`,
   `variable`, `package`, `module` or `other`.
3. **An IDE lists its providers and outliners** in `zen_ui::ide::IdeProduct`
   (`project_models`, `outliners`), as it lists highlighters. The window gives the providers to the
   session state `zen_workbench::project_model::ProjectModels` when it opens a project.
4. **The commands** (`zen-workbench`):

   | Command | Parameters | Result |
   |---|---|---|
   | `project.model.refresh` | `{ paths?: [path] }` | `{ revision, changed }` |
   | `project.module.info` | `{}` | `{ modules, workspace, problems }` |
   | `project.libraries.list` | `{}` | `{ revision, libraries }` |
   | `editor.structure.get` | `{ path?, waitMs?, sort?: "position" \| "name", fields?: bool }` | `StructureState` |

   `project.model.refresh` rebuilds the model when one of `paths` is watched by a provider (or
   always without `paths`); `revision` rises whenever the model changes. `StructureState` is
   `{ path, status: "ready" | "pending" | "unavailable", reason?, source: "server" | "syntax",
   generation, symbols, current }`, where each symbol is `{ name, detail, kind, start, end,
   selectionStart, line, column, children }` and `current` is the index path of the innermost symbol
   at the caret. The language server's document symbols are used when the file has a server; the
   outliner otherwise. While a server that will answer has not answered for the file yet, the
   outliner's symbols come with `status: "pending"` and `source: "syntax"`; after an edit, the last
   server symbols come with `status: "pending"` and `source: "server"` until the new answer, so a
   client does not switch between the two sources on every keystroke. A client reads again while the
   status is `pending`.
5. **The Structure tool window** opens with ⌘7 (`view.toolWindow.structure`) and takes the left
   panel from the Project tool window; whether it is shown is kept in the workspace file
   (`structurePanel`).

## Consequences

- Go modules, and later Cargo for RustRonin, plug into the same model and the same commands.
- The UI draws External Libraries and Structure only from command results, so agents see the same.
- `zen-syntax` and `zen-project` now depend on `serde` and `schemars`, which `zen-text` (L0) already
  uses.
- The model is rebuilt whole on a change of a watched file; incremental updates wait until a project
  is big enough to need them.

## Alternatives considered

- **The model in `zen-workbench` only:** Zen Toolbox and other L1 to L4 code could not read it, and
  `zen-project` is the crate the architecture plans for modules.
- **The structure from the language server only:** nothing for module files, and nothing while the
  server starts or is not installed (ADR-0003 keeps structure in the syntax layer).
- **A language-specific command per IDE (`go.modules.list`):** every client would need to know each
  language; one shape with `details` keeps the clients generic.
