# ADR-0266: Go scratch files run in the project's module

- **Status:** Proposed
- **Date:** 2026-11-06

## Context

[ADR-0212](0212-scratch-files-run-through-language-providers.md) runs a Go scratch alone, from the
scratches folder, which holds no `go.mod`: it can use only the standard library. A scratch
should import what the current project can, and a new Go scratch should start as a runnable
program.

## Decision

1. `RunProvider` gets an optional `scratch_starter(language)`: the text a new scratch of the language
   starts with and the caret's place in it. `file.scratch.new` uses it when no `text` is given. Go
   gives `package main` with an empty `func main()`, the caret inside.
2. A run of one Go file outside every module of the project still builds alone, unless the file
   imports a package of the project's modules or a path whose first element has a dot. Then it
   builds from that module's folder (the outermost module of the project for an outside import)
   with `go build -overlay=<run folder>/overlay.json ./zen_scratch/<name>`, the overlay mapping
   `<module>/zen_scratch/<name>/<file>` to the scratch, one package per scratch. The folder does not
   exist on disk and nothing is written to the project. It does not start with `_`: gopls resolves a
   file in such a folder but reports none of its errors. The decision is made when the run starts,
   from the saved text, so it follows the imports as they change.
3. Build and run stay two steps, not `go run`, so the run and debug share one build line and the
   binary is in the run's folder like every other Go build.
4. A project that is not trusted gives a run no modules, so its scratch builds alone, as before.
5. gopls sees the scratch at the same path. `RunProvider::scratch_home` answers that path from the
   text being edited, and the workbench maps the scratch to its URI in `zen_lsp::position`, where
   `file_uri` and `uri_path` already look: every request, edit and report then goes through the one
   mapping, and the document keeps its real path in the IDE. When the answer changes the document
   is closed at the old URI and opened at the new one, and a late report of the old URI is dropped.
   A scratch of the standard library alone keeps its own URI.

## Consequences

- A scratch can use the project's packages and `go.mod` dependencies without changing the project.
- gopls works on the scratch as a package of the module while it is open, with the project's
  imports resolved. The package exists only there: it is gone from gopls when the scratch closes.
- A debug build of an overlaid scratch records the mapped path, so a breakpoint set in the scratch
  may not bind; it is not verified.
