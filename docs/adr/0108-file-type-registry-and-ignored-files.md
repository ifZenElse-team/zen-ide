# ADR-0108: One file type registry with user-editable patterns, and the ignored files and folders

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0031](0031-plugins.md) (plug-ins),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents),
  [ADR-0053](0053-fonts-ui-scale-and-code-style.md) (code style), [ADR-0055](0055-language-servers.md)
  (language servers), [ADR-0071](0071-built-in-modules.md) (built-in modules)

## Context

What a file is was decided in several places that did not know of each other: the code style by
`EXTENSIONS` and `FILE_NAMES` in `zen-workbench`, highlighting by each highlighter's `handles`
(plug-ins by their `extensions` and `names`), language servers by the `extensions` and `names` of
their declarations, and icons by a table of their own. A user could change none of them. The names
the IDE hides were hard-coded too, and twice: `HIDDEN` in `zen-vfs` for the tree, `SKIPPED` in
`zen-index` for the index, the two lists not the same (`.idea` and `.gradle` were left out of the
index but shown in the tree, `.hg` and `.svn` the other way round).

The UI specification gives Settings → Editor → File Types two
tabs: Recognized File Types (the types; for the selected one File name patterns and HashBang
patterns, each with + − ✎) and Ignored Files and Folders (`*.pyc *.pyo *.rbc *.yarb *~ .DS_Store
.git .hg .mypy_cache .pytest_cache .ruff_cache .svn CVS __pycache__ _svn vssver.scc vssver2.scc`,
"Ignored files and folders are not visible in the IDE and not indexed"). The patterns are kept per
IDE, as mappings and removed mappings.

## Decision

1. **One registry, `zen_vfs::file_types`.** Built-in modules declare their types through
   `ModuleHost::file_type` (Go: Go, Go Module, Go Checksums; Rust: Rust), every loaded plug-in
   declares one type from its `plugin.json` (its `extensions` as `*.<extension>`, its `names`, and
   the new optional `hashbangs`), and the platform declares Text (`*.txt`, `*.text`). A switched
   off or removed plug-in's type goes away. The registry lives in `zen-vfs` (L0), so that everything
   that lists files can ask it, and is one per process, as the IDE is.
2. **The type of a file** is the type whose current pattern names it best: a whole name
   (`Dockerfile`) beats a pattern with wildcards (`.env.*`, `*.gitlab-ci.yml`), which beats an
   extension (`*.yml`); among equals the pattern with more fixed characters, then the type declared
   first. A file no pattern names takes the type whose hashbang names the interpreter of its first
   line (`#!/usr/bin/env bash` → `bash`; a hashbang `python` also names `python3.12`); only the
   first 512 bytes of a text are looked at, and only regular files are read. The registry keeps the
   whole names and the extensions in hash maps and tries only the other wildcard patterns one by
   one, so the tree can ask it for every row of every frame.
3. **Consumers ask the registry, not their own tables.** `known_name` gives the name a file is known
   by to whatever matches declared patterns: the file's own name while the user's changes leave its
   type as declared (so nothing changes until the user changes something), the first declared
   pattern of its new type made a name (`file.go`) when a moved or added pattern or its hashbang
   gives it another type, and nothing when the user took its type away. Highlighting (built-in and
   plug-in highlighters, folding, the Structure, completion), the code style (`language_of`), the
   language servers (`serves`, `language_for`), the inspectors and the file icons go through it, so
   the existing `handles` of highlighters and the declarations of servers stay as they are.
4. **The user's changes** are added mappings (a pattern given to a type; taken from the type that had
   it), removed mappings of declared patterns, the same for hashbangs, and the ignored names. A
   mapping to a type nobody declares now (a removed plug-in) takes nothing from the type that
   declares the pattern, and comes back with the plug-in. They are kept per IDE in `filetypes.json`
   in the IDE's data folder, written only when something differs from the declarations and the
   default ignored names; a damaged file loses only its damaged entries.
5. **One list of ignored names.** `zen_vfs::is_ignored` decides what the tree lists; the index (and
   with it Find in Files, Search Everywhere, Go to File and the TODO tool window, which search
   through it), the library folders' listing, Local History and the file watcher skip what it names
   too, so writes into `__pycache__` do not refresh the project. `zen_vfs::HIDDEN` (`.git`, `.hg`,
   `.svn`, `.DS_Store`) stays a floor under the list: those are never indexed, watched, recorded or
   written by a bulk change, even when the user removes them from the ignored names and the tree
   shows them. Names are matched case-sensitively, patterns with `*` and `?`. The excluded build
   folders (`EXCLUDED`) stay shown and marked; `.idea` and `.gradle` stay shown and not indexed, now
   from one list in `zen-vfs` (`TOOL_FOLDERS`).
6. **A change applies at once and without stalling the window.** Commands change the registry,
   save the file and index the open project again on a worker thread, as a change of the project's
   excluded folders already did; every other window follows the registry's generation and does the
   same. The tree reads its folders again, the editor asks for its file's highlighter every frame and
   the code style is computed from the type each time, so open files change without being opened
   again.
7. **Commands** (ADR-0033): `editor.fileTypes.list`, `editor.fileTypes.typeOf`,
   `editor.fileTypes.addPattern`, `editor.fileTypes.removePattern`, `editor.fileTypes.addHashbang`,
   `editor.fileTypes.removeHashbang`, `editor.fileTypes.setIgnored` and `editor.fileTypes.reset`.
   The Settings page is a draft that OK and Apply send as these commands.

## Consequences

- Moving `*.yml` to JSON makes every `.yml` file a JSON file for highlighting, code style, language
  servers and icons at once; removing `*.md` from Markdown leaves `.md` files without a type.
- A type made by the user (with keywords of its own), Associate with File
  Type and Override File Type are not part of this decision.
- Code that compared names with `zen_vfs::HIDDEN` to skip version control internals keeps
  compiling and keeps that meaning (open branches use it so); code that decides what the user sees
  calls `zen_vfs::is_ignored`.
- Language servers and file icons ask by the file's name only, not its hashbang: a script without
  an extension is highlighted and indented by its hashbang, but no language server opens it.
- The registry is global state: tests that change it hold a lock of their test module and use
  patterns no other test uses.
