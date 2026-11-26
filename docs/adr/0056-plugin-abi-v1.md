# ADR-0056: Plug-in ABI v1: folding, outline, completion and abbreviation expansion

- **Status:** Proposed
- **Date:** 2026-10-18
- **Extends:** [ADR-0031](0031-plugins.md) (ABI v0), [ADR-0050](0050-products-are-plugin-bundles.md)

## Context

ADR-0050 puts basic web (HTML, CSS, JavaScript, TypeScript) into every IDE, with "highlighting,
folding, structure and basic completion, without a language server". ABI v0 (ADR-0031) lets a
plug-in highlight only. The web plug-ins, and later others, need to tell the IDE where a file can be
folded, what its structure is, what can be typed at the caret and how an abbreviation such as
`ul>li` expands. The plug-ins stay sandboxed modules without imports.

## Decision

1. **ABI v1 extends v0.** A v1 module returns `1` from `zen_abi`; every v0 export stays required. A
   host that knows v1 loads both versions; a v0 module keeps working unchanged. The catalog records
   the ABI a module implements, so an older IDE never installs a v1 module.
2. **Four optional exports**, each `(ptr: i32, len: i32) -> i64`, checked by name when the module
   loads; a missing one means "not supported", one of another type refuses the module. The input is
   written at the pointer `zen_alloc(len)` returns; the result is a pointer in the high 32 bits and
   a length in bytes in the low 32 bits, valid until the next call; a zero length means "nothing".

   | Export | Input | Result |
   |---|---|---|
   | `zen_fold` | the UTF-8 text | records of 8 bytes, little-endian: `start_line: u32`, `end_line: u32`, zero-based, `end_line > start_line` |
   | `zen_outline` | the UTF-8 text | UTF-8 JSON `[{"name", "kind", "start", "end", "children": [...]}]`, byte offsets |
   | `zen_complete` | `caret: u32` (4 bytes, little-endian, a byte offset), then the text | UTF-8 JSON `{"from": u32, "items": [{"label", "kind", "insert", "detail"?}]}`: `from..caret` is replaced by `insert` |
   | `zen_expand` | as `zen_complete` | UTF-8 JSON `{"from": u32, "to": u32, "text": str}`, or a zero length when there is nothing to expand |

   Outline kinds are `element`, `rule`, `property`, `function`, `class`, `method`, `variable`,
   `constant`, `interface`, `type`, `module`, `field`, `key` (unknown kinds become `key`);
   completion kinds add `keyword`, `tag`, `attribute`, `value` and `snippet`. `$0` in `insert` or
   `text` marks where the caret goes (once; the host removes it). Plug-ins write their JSON without
   dependencies: they depend on no workspace crate.
3. **The host trusts nothing.** Every call has a fuel limit, for v0's exports too: 2 billion
   units plus 10 000 per byte of input, at most 20 billion, so that a large file does not stop a
   plug-in that a fixed limit would (the JavaScript analyser uses about 6 000 units per byte); a
   trap, running out of fuel, a result larger than the limit (64 MiB of records, 32 MiB of JSON) or
   JSON that does not parse
   switches the plug-in off for the session, as for highlighting. Ranges that are empty, out of
   bounds, repeated or not on character boundaries are dropped, outline entries outside their parent
   too; at most 100 000 folds, 20 000 outline entries and 2 000 suggestions are taken, names are cut
   to 256 characters, and an expansion must end at the caret. Plug-ins stop their outline at 32
   levels (JSON deeper than 128 levels does not parse) and bound recursion and output themselves,
   since a trap switches them off.
4. **The platform's side.** `zen_syntax::TextFeatures` (with `Outliner` as its supertrait) is the
   extension point; `zen_plugins::WasmHighlighter` implements it. A window reads the current plug-ins
   through `zen_workbench::plugins::PluginFeatures`, so installing or switching off a plug-in takes
   effect at once; for each file the most specific plug-in is used, as for highlighting.
   - Folding: `zen_fold` feeds the editor's existing folding, `editor.fold.*` (list, toggle,
     collapse, expand, collapseAll, expandAll, reveal), whose shapes do not change. In a file whose
     plug-in folds, the plug-in's line ranges take the place of the built-in bracket folds; each
     becomes a character range from the end of its first line to the end of its last, a `block`
     fold. Folded ranges are kept per document and move with its edits, as before. The answer is
     kept by the document as the bracket folds are (`Document::foldable_listed_by`): reused while
     its version is the same, moved along with the edits while typing goes on, and asked for again
     only when the text has settled (`editor.fold.list` with `current: true`), never per keystroke.
   - Structure: `editor.structure.get` uses the plug-in's outline when no language server and no
     built-in outliner answers; for an open file the answer is reused while its version is the same.
   - Completion: `editor.completion.start` takes the plug-in's items at once and merges them after a
     language server's, when the file has one too; a server that is not running does not keep the
     plug-in's items away. The editor starts it as for a server: ⌃Space, typing a word character,
     or `.`. In a plug-in's file a word may hold `-` and `$` too (`background-color`, `$el`), for
     filtering and for Tab, which replaces the rest of the word (Enter inserts). Plug-ins have no
     trigger characters of their own in v1: after `<`, `@`, `:` or `&` the first letter typed, or
     ⌃Space, opens the list. The plug-in filters by the typed prefix, and the host filters again
     as the user types.
   - Expansion: `editor.emmet.expand {path, offset}` → `{"applied": bool}`; `editor.edit.tab`
     expands first when a single caret has no selection, and otherwise does what Tab does.
5. **The base set grows** by `html`, `css`, `javascript` and `typescript` in `BUNDLED_PLUGINS`.

## Consequences

- New `SymbolKind`s (`class`, `element`, `rule`, `property`, `key`) appear in `editor.structure.get`
  and have their own badges in the Structure tool window.
- A release of an IDE needs the four web plug-in crates in `plugins/`, since the bundle builds every
  plug-in in the list.
- A platform crate may use a plug-in crate as a dev-dependency (and only so; `cargo xtask layers`
  checks it): the host's tests run the real plug-ins natively through `zen_plugins::native`, whose
  answers go through the same decoding and checks as a module's.
- Open: an optional per-plug-in list of completion trigger characters in `plugin.json`, carried by
  the catalog, the bundle and the installed manifest, so that `<` alone opens HTML's tag list.
- The ABI is still not stable (ADR-0031); v1 may gain fields before it is declared stable, and new
  capabilities come as new versions.

## Alternatives considered

- **One export answering everything as JSON:** fewer exports, but the host could not tell what a
  plug-in supports without calling it, and every answer would pay for all features.
- **A second folding system with its own commands:** rejected; the editor already has folding with
  its commands, keys and gutter, and a plug-in is only another source of folds.
- **Folding as byte ranges:** finer, but whole lines are what the gutter folds and what survive
  edits best; plug-ins count lines more easily than they keep byte offsets consistent.
- **Host imports (callbacks into the IDE):** rejected by ADR-0031; plug-ins stay without imports.
